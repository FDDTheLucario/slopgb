//! The SNES **S-DSP** — the 8-voice sample/echo synthesizer that turns the
//! SPC700's register writes + APU RAM into a 32 kHz stereo stream.
//!
//! The DSP produces one stereo sample every 32 SPC700 cycles (1.024 MHz ÷ 32 =
//! 32 kHz). Each sample it: latches key-on (1-sample delayed) and key-off;
//! clocks the noise LFSR; runs all 8 [`voice`]s (BRR decode → pitch/Gaussian →
//! envelope), applying pitch modulation (`PMON`) and the noise source (`NON`);
//! sums them into the master mix and the echo bus (`EON`); runs the [`echo`]
//! unit; and applies master volume (`MVOL`) and the `FLG` mute.
//!
//! Register map (`$00-$7F`, per nocash **fullsnes** "SNES APU DSP Registers"):
//! per voice `v` (base `v<<4`): `VOLL/VOLR/PL/PH/SRCN/ADSR1/ADSR2/GAIN` at
//! `+0..+7`, `ENVX/OUTX` (read-only) at `+8/+9`, `FIR` at `+F`. Globals:
//! `MVOLL=0C MVOLR=1C EVOLL=2C EVOLR=3C KON=4C KOF=5C FLG=6C ENDX=7C EFB=0D
//! PMON=2D NON=3D EON=4D DIR=5D ESA=6D EDL=7D`.
//!
//! The register file is written through the SPC700's `$F2`/`$F3` port (wired by
//! the SGB APU seam in `slopgb-core`); the DSP shares the SPC700's 64 KB APU RAM
//! for BRR sample data and the echo buffer.
//!
//! Sources: nocash **fullsnes**, **Blargg SPC_DSP**, **bsnes** `dsp` — cited at
//! each submodule.

mod brr;
mod echo;
mod envelope;
mod gaussian;
mod voice;

use echo::{Echo, EchoRegs};
use voice::Voice;

// Global register addresses.
const MVOLL: usize = 0x0C;
const MVOLR: usize = 0x1C;
const EVOLL: usize = 0x2C;
const EVOLR: usize = 0x3C;
const KON: usize = 0x4C;
const KOF: usize = 0x5C;
const FLG: usize = 0x6C;
const ENDX: usize = 0x7C;
const EFB: usize = 0x0D;
const PMON: usize = 0x2D;
const NON: usize = 0x3D;
const EON: usize = 0x4D;
const DIR: usize = 0x5D;
const ESA: usize = 0x6D;
const EDL: usize = 0x7D;

/// The complete S-DSP: register file, 8 voices, echo unit, and the shared
/// noise/rate state.
#[derive(Clone)]
pub struct SDsp {
    /// The 128-byte register file (`$00-$7F`).
    regs: [u8; 128],
    voices: [Voice; 8],
    echo: Echo,
    /// Free-running rate counter (period 0x7800) that gates every envelope /
    /// noise step.
    counter: u32,
    /// 15-bit noise LFSR.
    noise: i32,
    /// KON edge pending this sample and the value delayed one sample (the DSP's
    /// 1-sample key-on latency).
    kon_edge: u8,
    kon_edge_delayed: u8,
    /// Previous KON register value, for 0→1 edge detection on write.
    kon_prev: u8,
    /// `ENDX` accumulator (voices that hit an END block).
    endx: u8,
}

impl Default for SDsp {
    fn default() -> Self {
        Self::new()
    }
}

impl SDsp {
    pub fn new() -> Self {
        SDsp {
            regs: [0; 128],
            voices: Default::default(),
            echo: Echo::default(),
            counter: 0,
            noise: 0x4000,
            kon_edge: 0,
            kon_edge_delayed: 0,
            kon_prev: 0,
            endx: 0,
        }
    }

    // -- Register access (forwarded from the SPC700 $F2/$F3 port) -----------

    /// Read a DSP register. `addr` is the raw `$F2` value; bit 7 selects the
    /// read-only mirror (same data). `ENVX`/`OUTX`/`ENDX` return live state.
    pub fn read(&self, addr: u8) -> u8 {
        let reg = (addr & 0x7F) as usize;
        match reg & 0x0F {
            0x08 => self.voices[reg >> 4].env.envx(),
            0x09 => self.voices[reg >> 4].outx as u8,
            _ if reg == ENDX => self.endx,
            _ => self.regs[reg],
        }
    }

    /// Write a DSP register. Writes to the `$80-$FF` mirror are ignored (the
    /// mirror is read-only, matching the SPC700 shadow behaviour).
    pub fn write(&mut self, addr: u8, val: u8) {
        if addr & 0x80 != 0 {
            return;
        }
        let reg = addr as usize;
        self.regs[reg] = val;
        match reg {
            KON => {
                // Key-on is edge-triggered (0→1): "write 0 then 1 to restart".
                self.kon_edge |= val & !self.kon_prev;
                self.kon_prev = val;
            }
            ENDX => {
                // Writing ENDX clears the end flags.
                self.endx = 0;
                self.regs[ENDX] = 0;
            }
            // Soft reset (bit 7): silence + key-off every voice.
            FLG if val & 0x80 != 0 => {
                for v in &mut self.voices {
                    v.env.level = 0;
                    v.env.key_off();
                }
            }
            _ => {}
        }
    }

    #[inline]
    fn reg_i8(&self, i: usize) -> i32 {
        self.regs[i] as i8 as i32
    }

    // -- Synthesis ----------------------------------------------------------

    /// Produce one 32 kHz stereo sample. Reads BRR data + the sample directory
    /// from `ram` and reads/writes the echo buffer there.
    pub fn sample(&mut self, ram: &mut [u8; 0x1_0000]) -> (i16, i16) {
        // Global rate counter (counts down, wraps at the period).
        self.counter = if self.counter == 0 {
            envelope::COUNTER_MAX - 1
        } else {
            self.counter - 1
        };

        let flg = self.regs[FLG];

        // Noise LFSR, clocked at the FLG noise rate (bits 0-4).
        if envelope::fires(self.counter, flg & 0x1F) {
            let bit = (self.noise ^ (self.noise >> 1)) & 1;
            self.noise = ((self.noise >> 1) & 0x3FFF) | (bit << 14);
        }
        let noise_sample = i32::from((self.noise << 1) as i16);

        // Key-on (1-sample delayed) and key-off (level-sensitive).
        let kon_now = self.kon_edge_delayed;
        self.kon_edge_delayed = self.kon_edge;
        self.kon_edge = 0;
        let dir = self.regs[DIR];
        for v in 0..8 {
            if kon_now & (1 << v) != 0 {
                let srcn = self.regs[(v << 4) + 4];
                self.voices[v].key_on(ram, dir, srcn);
                self.endx &= !(1 << v);
            }
        }
        let kof = self.regs[KOF];
        for v in 0..8 {
            if kof & (1 << v) != 0 {
                self.voices[v].env.key_off();
            }
        }

        // Per-voice synthesis + mix.
        let pmon = self.regs[PMON];
        let non = self.regs[NON];
        let eon = self.regs[EON];
        let (mut main_l, mut main_r) = (0i32, 0i32);
        let (mut echo_l, mut echo_r) = (0i32, 0i32);
        let mut prev_out = 0i32;
        let mut endx = self.endx;
        for v in 0..8 {
            let base = v << 4;
            let mut pitch =
                (u16::from(self.regs[base + 2]) | (u16::from(self.regs[base + 3]) << 8)) & 0x3FFF;
            // Pitch modulation from the previous voice (voice 0 is never
            // modulated).
            if v > 0 && pmon & (1 << v) != 0 {
                let p = i32::from(pitch);
                pitch = (p + (((prev_out >> 5) * p) >> 10)).clamp(0, 0x3FFF) as u16;
            }
            let out = self.voices[v].step(
                ram,
                pitch,
                self.regs[base + 5],
                self.regs[base + 6],
                self.regs[base + 7],
                self.counter,
                noise_sample,
                non & (1 << v) != 0,
                &mut endx,
                v,
            );
            prev_out = out;

            let l = (out * self.reg_i8(base)) >> 7;
            let r = (out * self.reg_i8(base + 1)) >> 7;
            main_l += l;
            main_r += r;
            if eon & (1 << v) != 0 {
                echo_l += l;
                echo_r += r;
            }
        }
        self.endx = endx;

        // Echo unit.
        let echo_regs = EchoRegs {
            esa: self.regs[ESA],
            edl: self.regs[EDL],
            efb: self.regs[EFB] as i8,
            evol_l: self.regs[EVOLL] as i8,
            evol_r: self.regs[EVOLR] as i8,
            fir: [
                self.regs[0x0F] as i8,
                self.regs[0x1F] as i8,
                self.regs[0x2F] as i8,
                self.regs[0x3F] as i8,
                self.regs[0x4F] as i8,
                self.regs[0x5F] as i8,
                self.regs[0x6F] as i8,
                self.regs[0x7F] as i8,
            ],
            write_disabled: flg & 0x20 != 0,
        };
        let echo_in = (echo_l.clamp(-32768, 32767), echo_r.clamp(-32768, 32767));
        let (add_l, add_r) = self.echo.process(ram, &echo_regs, echo_in);

        // Master volume + echo, then the FLG mute (bit 6) / reset (bit 7).
        let mut l = ((main_l * self.reg_i8(MVOLL)) >> 7) + add_l;
        let mut r = ((main_r * self.reg_i8(MVOLR)) >> 7) + add_r;
        if flg & 0xC0 != 0 {
            l = 0;
            r = 0;
        }
        (l.clamp(-32768, 32767) as i16, r.clamp(-32768, 32767) as i16)
    }

    // -- Save state ---------------------------------------------------------

    pub fn write_state(&self, w: &mut crate::state::Writer) {
        w.bytes(&self.regs);
        w.u32(self.counter);
        w.u32(self.noise as u32);
        w.u8(self.kon_edge);
        w.u8(self.kon_edge_delayed);
        w.u8(self.kon_prev);
        w.u8(self.endx);
        for v in &self.voices {
            v.write_state(w);
        }
        self.echo.write_state(w);
    }

    pub fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::StateError> {
        r.bytes_into(&mut self.regs)?;
        self.counter = r.u32()?;
        self.noise = r.u32()? as i32;
        self.kon_edge = r.u8()?;
        self.kon_edge_delayed = r.u8()?;
        self.kon_prev = r.u8()?;
        self.endx = r.u8()?;
        for v in &mut self.voices {
            v.read_state(r)?;
        }
        self.echo.read_state(r)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "dsp_tests.rs"]
mod tests;
