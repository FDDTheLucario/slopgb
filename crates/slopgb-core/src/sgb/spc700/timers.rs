//! The three SPC700 timers.
//!
//! T0/T1 are clocked at 8 kHz (1.024 MHz ÷ 128), T2 at 64 kHz (÷ 16). Each timer
//! has an 8-bit stage that counts up to its target (`$FA-$FC`, where 0 means
//! 256); on match the stage resets and the visible **4-bit** output counter
//! (`$FD-$FF`) increments. Reading the output register clears it to 0.
//! Enabling a timer (`$F1` bit 0→1) resets both the stage and the output.
//! (fullsnes, "SNES APU Timers".)
//!
//! Timers are driven at whole-instruction granularity: [`Spc700::tick_timers`]
//! accumulates each instruction's cycle count into the prescalers. That is
//! ample for a driver that polls the counters (upgrade to per-cycle only if a
//! test ever needs sub-instruction timer phase).

use super::*;

#[derive(Clone, Copy, Default)]
pub(super) struct Timer {
    pub enabled: bool,
    /// Target divisor (`$FA-$FC`); 0 means 256.
    pub target: u8,
    /// Internal 8-bit stage counter (counts prescaler ticks).
    stage: u16,
    /// Visible 4-bit output counter (`$FD-$FF`).
    out: u8,
}

impl Timer {
    /// One prescaler tick. Increments the stage; on reaching the target, resets
    /// the stage and bumps the 4-bit output.
    fn tick(&mut self) {
        if !self.enabled {
            return;
        }
        self.stage += 1;
        let period = if self.target == 0 { 256 } else { self.target as u16 };
        if self.stage >= period {
            self.stage = 0;
            self.out = self.out.wrapping_add(1) & 0x0F;
        }
    }

    /// Read the 4-bit output counter and clear it (read-and-clear).
    pub fn read_out(&mut self) -> u8 {
        let v = self.out;
        self.out = 0;
        v
    }

    /// Reset stage + output (on an enable rising edge).
    pub fn reset_counters(&mut self) {
        self.stage = 0;
        self.out = 0;
    }
}

impl Spc700 {
    /// Advance the timers (and any attached DSP) by `cyc` SPC700 cycles.
    pub(super) fn tick_timers(&mut self, cyc: u32) {
        self.presc_8k += cyc;
        while self.presc_8k >= 128 {
            self.presc_8k -= 128;
            self.timer[0].tick();
            self.timer[1].tick();
        }
        self.presc_64k += cyc;
        while self.presc_64k >= 16 {
            self.presc_64k -= 16;
            self.timer[2].tick();
        }
    }
}
