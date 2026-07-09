//! Per-voice envelope: ADSR (attack / decay / sustain / release) and the five
//! GAIN modes (direct, linear increase, bent increase, linear decrease,
//! exponential decrease), plus the global rate counter that gates every step.
//!
//! The envelope is an 11-bit value (`0..=0x7FF`); the readable `ENVX` register
//! is `env >> 4`. Timing is driven by a free-running global counter: a step of
//! rate `r` fires when `(counter + OFFSET[r]) % RATE[r] == 0` (rate 0 never
//! fires — the envelope is frozen). This is the classic SPC700 scheme.
//!
//! Sources: nocash **fullsnes** ("SNES APU DSP - ADSR / GAIN") for the modes
//! and the ENVX relationship; **Blargg's SPC_DSP** for the [`RATE`]/[`OFFSET`]
//! counter tables and the exact per-step arithmetic.

/// Envelope phase. `Release` is entered by KOF; `Attack` by KON. GAIN modes do
/// not use these phases (they act directly on `env`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Number of samples between steps for each 5-bit rate (index 0 = "never").
/// The global counter has period 0x7800, an exact multiple of every entry.
/// Source: Blargg SPC_DSP `counter_rates`.
pub(super) const RATE: [u16; 32] = [
    0x7800, // 0 — never fires (frozen)
    2048, 1536, 1280, 1024, 768, 640, 512, 384, 320, 256, 192, 160, 128, 96, 80, 64, 48, 40, 32,
    24, 20, 16, 12, 10, 8, 6, 5, 4, 3, 2, 1,
];

/// Phase offset added to the global counter before the modulo test, per rate.
/// Source: Blargg SPC_DSP `counter_offsets`.
pub(super) const OFFSET: [u16; 32] = [
    1, 0, 1040, 536, 0, 1040, 536, 0, 1040, 536, 0, 1040, 536, 0, 1040, 536, 0, 1040, 536, 0, 1040,
    536, 0, 1040, 536, 0, 1040, 536, 0, 1040, 0, 0,
];

/// The global counter's period. It counts down modulo this each sample.
pub(super) const COUNTER_MAX: u32 = 0x7800;

/// Whether a rate-`rate` step fires at global counter `counter`.
#[inline]
pub(super) fn fires(counter: u32, rate: u8) -> bool {
    let rate = rate as usize & 0x1F;
    if rate == 0 {
        return false; // frozen
    }
    (counter + u32::from(OFFSET[rate])) % u32::from(RATE[rate]) == 0
}

/// One envelope voice's ADSR/GAIN state.
#[derive(Clone, Copy)]
pub(super) struct Env {
    /// 11-bit envelope level (`0..=0x7FF`).
    pub level: i32,
    pub phase: Phase,
}

impl Default for Env {
    fn default() -> Self {
        Env {
            level: 0,
            phase: Phase::Release,
        }
    }
}

impl Env {
    /// Key-on: restart the envelope from zero in the attack phase.
    pub fn key_on(&mut self) {
        self.level = 0;
        self.phase = Phase::Attack;
    }

    /// Key-off: enter the release ramp (env → 0 at a fixed fast rate).
    pub fn key_off(&mut self) {
        self.phase = Phase::Release;
    }

    /// `ENVX` readback (`env >> 4`, 7-bit).
    pub fn envx(&self) -> u8 {
        ((self.level >> 4) & 0x7F) as u8
    }

    pub fn write_state(&self, w: &mut crate::state::Writer) {
        w.u32(self.level as u32);
        w.u8(match self.phase {
            Phase::Attack => 0,
            Phase::Decay => 1,
            Phase::Sustain => 2,
            Phase::Release => 3,
        });
    }

    pub fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::StateError> {
        self.level = r.u32()? as i32;
        self.phase = match r.u8()? {
            0 => Phase::Attack,
            1 => Phase::Decay,
            2 => Phase::Sustain,
            _ => Phase::Release,
        };
        Ok(())
    }

    /// Advance one output sample. `adsr1`/`adsr2`/`gain` are the raw registers,
    /// `counter` the global rate counter. Returns the new 11-bit level.
    pub fn step(&mut self, adsr1: u8, adsr2: u8, gain: u8, counter: u32) -> i32 {
        // Release is unconditional (not rate-gated): a fixed −8 ramp to zero.
        if self.phase == Phase::Release {
            self.level = (self.level - 8).max(0);
            return self.level;
        }

        if adsr1 & 0x80 != 0 {
            self.step_adsr(adsr1, adsr2, counter);
        } else {
            self.step_gain(gain, counter);
        }
        self.level
    }

    fn step_adsr(&mut self, adsr1: u8, adsr2: u8, counter: u32) {
        match self.phase {
            Phase::Attack => {
                let ar = adsr1 & 0x0F;
                let rate = ar * 2 + 1;
                if fires(counter, rate) {
                    // AR = 15 (rate 31) jumps by 0x400, else by 0x20.
                    self.level += if rate < 31 { 0x20 } else { 0x400 };
                    if self.level >= 0x7FF {
                        self.level = 0x7FF;
                        self.phase = Phase::Decay;
                    }
                }
            }
            Phase::Decay => {
                let dr = (adsr1 >> 4) & 0x07;
                let rate = dr * 2 + 0x10;
                if fires(counter, rate) {
                    self.level -= ((self.level - 1) >> 8) + 1;
                    self.level = self.level.max(0);
                    let sl = i32::from(adsr2 >> 5);
                    if (self.level >> 8) <= sl {
                        self.phase = Phase::Sustain;
                    }
                }
            }
            Phase::Sustain => {
                let sr = adsr2 & 0x1F;
                if fires(counter, sr) {
                    self.level -= ((self.level - 1) >> 8) + 1;
                    self.level = self.level.max(0);
                }
            }
            Phase::Release => unreachable!(),
        }
    }

    fn step_gain(&mut self, gain: u8, counter: u32) {
        if gain & 0x80 == 0 {
            // Direct gain: env tracks the register immediately (7-bit << 4).
            self.level = i32::from(gain & 0x7F) << 4;
            return;
        }
        let rate = gain & 0x1F;
        if !fires(counter, rate) {
            return;
        }
        match (gain >> 5) & 0x03 {
            0 => self.level -= 0x20,                        // linear decrease
            1 => self.level -= ((self.level - 1) >> 8) + 1, // exp decrease
            2 => self.level += 0x20,                        // linear increase
            _ => self.level += if self.level < 0x600 { 0x20 } else { 0x08 }, // bent increase
        }
        self.level = self.level.clamp(0, 0x7FF);
    }
}

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod tests;
