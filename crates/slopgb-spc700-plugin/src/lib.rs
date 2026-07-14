//! Coprocessor plugin wrapping the SNES SPC700 (S-SMP) + S-DSP audio subsystem
//! (`slopgb-snes-apu`) — the exact same code the `slopgb-core` built-in SGB
//! audio path runs, compiled to wasm and driven by the host through the tier-3
//! [`Coprocessor`] interface.
//!
//! ## Comm ports
//!
//! Ports `0-3` are the SNES↔APU comm latches (`$2140-$2143` on the SNES side,
//! `$F4-$F7` on the APU side) — the real channel the SNES CPU uses to boot and
//! command the SPC700, including the IPL upload handshake. Ports `4-5` are
//! read-only observability: the low and high bytes of the running S-DSP sample
//! count, so a host can confirm the DSP synthesized while clocked.
//!
//! ## Clocking
//!
//! `run_until(target)` advances the SPC700 in its own 1.024 MHz cycle domain and
//! drives one S-DSP sample every 32 SPC cycles (→ 32 kHz), mirroring the
//! built-in `SgbApu` wiring exactly (the DSP shares the SPC700's APU RAM). The
//! synthesized stereo PCM is buffered and drained to the host over the tier-3
//! `drain_pcm` path; ports `4-5` keep surfacing the running sample count for
//! quick observability.

#![deny(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use slopgb_plugin_api::{Coprocessor, slopgb_coprocessor_plugin};
use slopgb_snes_apu::dsp::SDsp;
use slopgb_snes_apu::spc700::{Dsp, Spc700};
use slopgb_snes_apu::state::{Reader, StateError, Writer};

/// The S-DSP emits one stereo sample every 32 SPC700 cycles (→ 32 kHz).
const DSP_PERIOD: u32 = 32;

/// Cap on the undrained PCM buffer (~2 s at 32 kHz). The host drains every
/// frame, so this only bounds memory if it never does; excess is dropped.
const PCM_CAP: usize = 1 << 16;

/// Forwards the SPC700's `$F2`/`$F3` DSP-register accesses to the shared
/// [`SDsp`]; synthesis (which needs APU RAM) is driven by the plugin, not here.
struct DspLink(Rc<RefCell<SDsp>>);

impl Dsp for DspLink {
    fn read(&mut self, addr: u8) -> u8 {
        self.0.borrow_mut().read(addr)
    }
    fn write(&mut self, addr: u8, val: u8) {
        self.0.borrow_mut().write(addr, val);
    }
}

/// The SPC700 + S-DSP coprocessor, clocked by the host.
struct Spc700Cop {
    spc: Spc700,
    /// The S-DSP, shared with the [`DspLink`] attached to `spc`.
    dsp: Rc<RefCell<SDsp>>,
    /// SPC cycles executed since reset (the chip's own cycle domain).
    cycles: u64,
    /// SPC cycles accumulated toward the next 32 kHz DSP sample.
    dsp_div: u32,
    /// Total S-DSP samples produced since reset (host-observable on ports 4-5).
    samples: u64,
    /// Stereo PCM synthesized but not yet drained by the host (oldest first).
    pcm: Vec<(i16, i16)>,
}

impl Spc700Cop {
    /// A power-on SPC700 (IPL ROM enabled) with a fresh S-DSP attached.
    fn power_on() -> Self {
        let dsp = Rc::new(RefCell::new(SDsp::new()));
        let mut spc = Spc700::new();
        spc.attach_dsp(Box::new(DspLink(Rc::clone(&dsp))));
        Spc700Cop {
            spc,
            dsp,
            cycles: 0,
            dsp_div: 0,
            samples: 0,
            pcm: Vec::new(),
        }
    }
}

impl Coprocessor for Spc700Cop {
    fn new() -> Self {
        Self::power_on()
    }

    fn reset(&mut self) {
        *self = Self::power_on();
    }

    fn run_until(&mut self, target_cycle: u64) -> u64 {
        while self.cycles < target_cycle {
            let cyc = self.spc.step();
            self.cycles += u64::from(cyc);
            self.dsp_div += cyc;
            while self.dsp_div >= DSP_PERIOD {
                self.dsp_div -= DSP_PERIOD;
                let s = self.dsp.borrow_mut().sample(self.spc.apu_ram_mut());
                if self.pcm.len() < PCM_CAP {
                    self.pcm.push(s);
                }
                self.samples += 1;
            }
        }
        self.cycles
    }

    fn port_write(&mut self, port: u8, val: u8) {
        if (port as usize) < 4 {
            self.spc.snes_write_port(port as usize, val);
        }
    }

    fn port_read(&mut self, port: u8) -> u8 {
        match port {
            0..=3 => self.spc.snes_read_port(port as usize),
            4 => (self.samples & 0xFF) as u8,
            5 => ((self.samples >> 8) & 0xFF) as u8,
            _ => 0,
        }
    }

    fn drain_pcm(&mut self) -> Vec<(i16, i16)> {
        std::mem::take(&mut self.pcm)
    }

    fn set_pc(&mut self, addr: u32) {
        self.spc.set_pc(addr as u16);
    }

    /// Poke `bytes` into APU RAM at `addr` (wrapping the 64 KB space) — how the
    /// host installs a resident SPC700 driver or lands a SOU_TRN upload.
    fn write_ram(&mut self, addr: u32, bytes: &[u8]) {
        let ram = self.spc.apu_ram_mut();
        for (i, &b) in bytes.iter().enumerate() {
            ram[(addr.wrapping_add(i as u32) & 0xFFFF) as usize] = b;
        }
    }

    fn read_ram(&mut self, addr: u32, len: usize) -> Vec<u8> {
        let ram = self.spc.apu_ram_mut();
        (0..len)
            .map(|i| ram[(addr.wrapping_add(i as u32) & 0xFFFF) as usize])
            .collect()
    }

    /// Serialize the SPC700 + S-DSP + the host-side cycle/sample accumulators.
    /// The transient undrained `pcm` is not persisted (audio the host has yet to
    /// pull), matching the built-in path that clears it on restore.
    fn save_state(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.spc.write_state(&mut w);
        self.dsp.borrow().write_state(&mut w);
        w.u64(self.cycles);
        w.u32(self.dsp_div);
        w.u64(self.samples);
        w.into_vec()
    }

    fn load_state(&mut self, bytes: &[u8]) {
        let mut r = Reader::new(bytes);
        let restored = (|| -> Result<(), StateError> {
            self.spc.read_state(&mut r)?;
            self.dsp.borrow_mut().read_state(&mut r)?;
            self.cycles = r.u64()?;
            self.dsp_div = r.u32()?;
            self.samples = r.u64()?;
            Ok(())
        })();
        // A truncated/foreign buffer leaves the chip partially restored but
        // usable; drop the transient audio buffer either way.
        let _ = restored;
        self.pcm.clear();
    }
}

slopgb_coprocessor_plugin!(Spc700Cop);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
