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
//! built-in `SgbApu` wiring exactly (the DSP shares the SPC700's APU RAM). PCM
//! draining is not part of the tier-3 ABI yet, so the plugin surfaces the sample
//! count rather than the stream; wiring a bulk PCM path is the SGB-integration
//! follow-up.

#![deny(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use slopgb_plugin_api::{Coprocessor, slopgb_coprocessor_plugin};
use slopgb_snes_apu::dsp::SDsp;
use slopgb_snes_apu::spc700::{Dsp, Spc700};

/// The S-DSP emits one stereo sample every 32 SPC700 cycles (→ 32 kHz).
const DSP_PERIOD: u32 = 32;

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
    /// Total S-DSP samples produced since reset (host-observable proof of
    /// synthesis; the tier-3 ABI has no PCM-drain path yet).
    samples: u64,
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
                let _ = self.dsp.borrow_mut().sample(self.spc.apu_ram_mut());
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
}

slopgb_coprocessor_plugin!(Spc700Cop);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
