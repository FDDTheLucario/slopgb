//! DIV/TIMA timer (FF04-FF07). Timer work package.
//!
//! Built around the internal 16-bit DIV counter. TIMA increments on falling
//! edges of the mux-selected DIV bit (including edges caused by DIV writes
//! and TAC writes), and reloads from TMA with a 1 M-cycle delay during which
//! writes to TIMA/TMA have the documented quirky effects
//! (mooneye: `tima_reload`, `tima_write_reloading`, `tma_write_reloading`,
//! `div_write`, `rapid_toggle`, `tim*`, `tim*_div_trigger`).
//!
//! Hardware model (gbctr "Timer" chapter, Pan Docs "Timer obscure behaviour"):
//! the timer circuit feeds `DIV bit (selected by TAC freq) AND TAC.enable`
//! into a falling-edge detector that clocks TIMA. Anything that drives that
//! signal from 1 to 0 increments TIMA: the DIV counter incrementing past the
//! selected bit, a DIV write (counter reset), or a TAC write that disables
//! the timer / switches to a frequency whose bit is currently 0.

/// Result of one M-cycle [`Timer::tick`].
pub struct TimerTick {
    /// IF bits to request (bit 2 = timer), 0 if none.
    pub iff: u8,
    /// The reload + IF commit fired in the *second half* of this M-cycle
    /// (T-substep 2 or 3). The SM83's halt-exit logic samples IE & IF
    /// mid-cycle — after 2 of the 4 T-cycles (SameBoy sm83_cpu.c,
    /// `GB_cpu_run`'s halted path) — not at the end-of-cycle point the
    /// running CPU's prefetch sampling models, so it misses such a commit
    /// until the next cycle (gambatte tima/tc*_irq_*; wilbertpol
    /// acceptance/timer/timer_if rounds 5/6 vs 3/4); see
    /// `Bus::pending_halt_wake`. IF *reads* in the commit cycle do see the
    /// bit (mooneye `tima_reload`-derived sequences).
    pub late: bool,
}

#[derive(Clone)]
pub struct Timer {
    div: u16,
    tima: u8,
    tma: u8,
    tac: u8,
    /// T-cycles until a pending TIMA overflow reload fires (0 = none).
    ///
    /// When TIMA overflows it reads 0x00 for 4 T-cycles, then TIMA := TMA and
    /// the timer interrupt is requested (mooneye `tima_reload`).
    reload_in: u8,
    /// TIMA was reloaded from TMA during the current M-cycle's tick. During
    /// this cycle a TIMA write is ignored and a TMA write is forwarded to
    /// TIMA (mooneye `tima_write_reloading`, `tma_write_reloading`).
    reloaded: bool,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            div: 0,
            tima: 0,
            tma: 0,
            tac: 0,
            reload_in: 0,
            reloaded: false,
        }
    }

    /// Set the internal 16-bit DIV counter (post-boot init only).
    pub fn set_div(&mut self, div: u16) {
        self.div = div;
    }

    /// Internal 16-bit DIV counter; other peripherals (APU frame sequencer,
    /// serial clock) derive their clocks from bits of this.
    pub fn div_counter(&self) -> u16 {
        self.div
    }

    /// DIV counter bit selected by the TAC frequency bits.
    /// 00 -> bit 9 (4096 Hz), 01 -> bit 3 (262144 Hz),
    /// 10 -> bit 5 (65536 Hz), 11 -> bit 7 (16384 Hz).
    fn selected_bit(&self) -> u16 {
        match self.tac & 0x03 {
            0 => 1 << 9,
            1 => 1 << 3,
            2 => 1 << 5,
            _ => 1 << 7,
        }
    }

    /// The edge-detector input: selected DIV bit AND timer enable.
    fn mux_out(&self) -> bool {
        self.tac & 0x04 != 0 && self.div & self.selected_bit() != 0
    }

    /// Clock TIMA once (a falling edge arrived at the edge detector).
    fn clock_tima(&mut self) {
        self.tima = self.tima.wrapping_add(1);
        if self.tima == 0 {
            // Overflow: TIMA stays 0x00 for 4 T-cycles before the TMA reload
            // and interrupt request (mooneye `tima_reload`).
            self.reload_in = 4;
        }
    }

    /// Advance one M-cycle (4 T-cycles). Returns the IF bits to request
    /// (bit 2 = timer) and on which part of the cycle they were committed.
    pub fn tick(&mut self) -> TimerTick {
        self.reloaded = false;
        let mut iff = 0;
        let mut late = false;
        for substep in 0..4 {
            let (i, l) = self.tick_substep(substep);
            iff |= i;
            late |= l;
        }
        TimerTick { iff, late }
    }

    /// Advance one T-cycle (one of the 4 substeps of an M-cycle), returning the
    /// IF bit set this T (bit 2 = timer) and whether it committed in the
    /// M-cycle's second half (`substep >= 2`, the `late` halt-wake mask). The
    /// per-substep primitive [`Self::tick`] composes from; `reloaded` is reset
    /// once per M-cycle by the caller (`tick` inlines it).
    pub fn tick_substep(&mut self, substep: u8) -> (u8, bool) {
        let mut iff = 0;
        let mut late = false;
        // The reload pipeline runs first so the T-cycle that caused the
        // overflow does not consume one of the 4 delay T-cycles.
        if self.reload_in > 0 {
            self.reload_in -= 1;
            if self.reload_in == 0 {
                self.tima = self.tma;
                self.reloaded = true;
                iff |= 0x04;
                late = substep >= 2;
            }
        }
        let before = self.mux_out();
        self.div = self.div.wrapping_add(1);
        if before && !self.mux_out() {
            self.clock_tima();
        }
        (iff, late)
    }

    /// Reset the per-M-cycle `reloaded` latch. Test-only (the `tick_substep`
    /// composition unit test drives the substeps manually; production `tick`
    /// inlines the reset).
    #[cfg(test)]
    pub fn begin_mcycle(&mut self) {
        self.reloaded = false;
    }

    /// Read FF04-FF07.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.div >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            // TAC upper 5 bits are unimplemented and read 1.
            0xFF07 => 0xF8 | self.tac,
            _ => 0xFF,
        }
    }

    /// Write FF04-FF07. A DIV/TAC write can clock TIMA via the falling-edge
    /// detector, but never requests IF directly: even a write-induced TIMA
    /// overflow raises the interrupt only at the reload 4 T-cycles later
    /// (from `tick`).
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF04 => {
                // Writing any value resets the whole 16-bit counter; if the
                // selected bit was 1 this is a falling edge and clocks TIMA
                // (mooneye `tim*_div_trigger`, `div_write`).
                let before = self.mux_out();
                self.div = 0;
                if before && !self.mux_out() {
                    self.clock_tima();
                }
            }
            0xFF05 => {
                // A write in the same M-cycle as the TMA reload is ignored;
                // otherwise it takes effect, and a write during the 4 T-cycle
                // overflow window also cancels the pending reload and
                // interrupt (mooneye `tima_write_reloading`).
                if !self.reloaded {
                    self.tima = value;
                    self.reload_in = 0;
                }
            }
            0xFF06 => {
                self.tma = value;
                if self.reloaded {
                    // TIMA is being loaded from TMA this cycle, so the new
                    // value propagates (mooneye `tma_write_reloading`).
                    self.tima = value;
                }
            }
            0xFF07 => {
                // Disabling the timer or switching to a frequency whose bit
                // is currently 0 while the old selected bit is 1 produces a
                // falling edge (mooneye `rapid_toggle`).
                let before = self.mux_out();
                self.tac = value & 0x07;
                if before && !self.mux_out() {
                    self.clock_tima();
                }
            }
            _ => {}
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

// --- Save state (manual serialization; see `crate::state`) ---
impl Timer {
    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        w.u16(self.div);
        w.u8(self.tima);
        w.u8(self.tma);
        w.u8(self.tac);
        w.u8(self.reload_in);
        w.bool(self.reloaded);
    }
    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.div = r.u16()?;
        self.tima = r.u8()?;
        self.tma = r.u8()?;
        self.tac = r.u8()?;
        self.reload_in = r.u8()?;
        self.reloaded = r.bool()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "timer_tests.rs"]
mod tests;
