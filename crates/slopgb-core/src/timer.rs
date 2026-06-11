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

    /// Advance one M-cycle (4 T-cycles). Returns IF bits to request
    /// (bit 2 = timer), 0 if none.
    pub fn tick(&mut self) -> u8 {
        self.reloaded = false;
        let mut iff = 0;
        for _ in 0..4 {
            // The reload pipeline runs first so the T-cycle that caused the
            // overflow does not consume one of the 4 delay T-cycles.
            if self.reload_in > 0 {
                self.reload_in -= 1;
                if self.reload_in == 0 {
                    self.tima = self.tma;
                    self.reloaded = true;
                    iff |= 0x04;
                }
            }
            let before = self.mux_out();
            self.div = self.div.wrapping_add(1);
            if before && !self.mux_out() {
                self.clock_tima();
            }
        }
        iff
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

    /// Write FF04-FF07. Returns IF bits to request immediately (a DIV/TAC
    /// write can clock TIMA via the falling-edge detector), 0 if none.
    ///
    /// Note: even a write-induced TIMA overflow raises IF only at the reload
    /// 4 T-cycles later (from `tick`), so this currently always returns 0;
    /// the return type is kept for the interconnect contract.
    pub fn write(&mut self, addr: u16, value: u8) -> u8 {
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
        0
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a timer with div = 0 and the given registers, with no edges
    /// produced (matches the mooneye tests' state right after their final
    /// `ldh (DIV),a` reference reset).
    fn timer_with(tac: u8, tima: u8, tma: u8) -> Timer {
        let mut t = Timer::new();
        t.write(0xFF07, tac);
        t.write(0xFF06, tma);
        t.write(0xFF05, tima);
        t
    }

    /// Run `n` M-cycles, OR-ing the produced IF bits. Accesses in the
    /// mooneye sequences happen *after* the final tick of their M-cycle.
    fn ticks(t: &mut Timer, n: u32) -> u8 {
        let mut iff = 0;
        for _ in 0..n {
            iff |= t.tick();
        }
        iff
    }

    #[test]
    fn div_reads_high_byte_of_internal_counter() {
        let mut t = Timer::new();
        t.set_div(0xABCD);
        assert_eq!(t.div_counter(), 0xABCD);
        assert_eq!(t.read(0xFF04), 0xAB);
    }

    #[test]
    fn div_increments_four_per_m_cycle() {
        let mut t = Timer::new();
        ticks(&mut t, 3);
        assert_eq!(t.div_counter(), 12);
        ticks(&mut t, 61);
        assert_eq!(t.read(0xFF04), 1); // 64 M-cycles = 256 T-cycles
    }

    #[test]
    fn div_write_resets_counter() {
        let mut t = Timer::new();
        t.set_div(0x1234);
        assert_eq!(t.write(0xFF04, 0x99), 0); // written value is irrelevant
        assert_eq!(t.div_counter(), 0);
        assert_eq!(t.read(0xFF04), 0);
    }

    #[test]
    fn div_counter_wraps() {
        let mut t = Timer::new();
        t.set_div(0xFFFE);
        t.tick();
        assert_eq!(t.div_counter(), 2);
    }

    #[test]
    fn register_readback_and_unused_bits() {
        let mut t = Timer::new();
        t.write(0xFF05, 0x12);
        t.write(0xFF06, 0x34);
        t.write(0xFF07, 0x05);
        assert_eq!(t.read(0xFF05), 0x12);
        assert_eq!(t.read(0xFF06), 0x34);
        // TAC upper 5 bits read 1.
        assert_eq!(t.read(0xFF07), 0xFD);
        t.write(0xFF07, 0xF8); // unused bits written are dropped
        assert_eq!(t.read(0xFF07), 0xF8);
    }

    /// mooneye tim00/tim01/tim10/tim11: TIMA increments exactly every
    /// 1024/16/64/256 T-cycles after a DIV reset.
    #[test]
    fn tima_increment_periods() {
        for (tac, period_mcycles) in [(0x04u8, 256u32), (0x05, 4), (0x06, 16), (0x07, 64)] {
            let mut t = timer_with(tac, 4, 4);
            ticks(&mut t, period_mcycles - 1);
            assert_eq!(t.read(0xFF05), 4, "tac {tac:#04x}: one cycle early");
            t.tick();
            assert_eq!(t.read(0xFF05), 5, "tac {tac:#04x}: on the boundary");
            ticks(&mut t, period_mcycles - 1);
            assert_eq!(t.read(0xFF05), 5, "tac {tac:#04x}: second period early");
            t.tick();
            assert_eq!(t.read(0xFF05), 6, "tac {tac:#04x}: second boundary");
        }
    }

    /// mooneye tim00_div_trigger etc.: a DIV write while the selected bit is
    /// high produces a falling edge and clocks TIMA; while low it does not.
    #[test]
    fn div_write_triggers_increment_when_selected_bit_high() {
        // M-cycles after which the selected bit has just gone high
        // (half a period after reset).
        for (tac, half_period) in [(0x04u8, 128u32), (0x05, 2), (0x06, 8), (0x07, 32)] {
            let mut t = timer_with(tac, 4, 4);
            ticks(&mut t, half_period / 2); // selected bit still 0
            assert_eq!(t.write(0xFF04, 0), 0);
            assert_eq!(t.read(0xFF05), 4, "tac {tac:#04x}: bit low, no edge");

            let mut t = timer_with(tac, 4, 4);
            ticks(&mut t, half_period); // selected bit now 1
            assert_eq!(t.write(0xFF04, 0), 0);
            assert_eq!(t.read(0xFF05), 5, "tac {tac:#04x}: bit high, edge");
        }
    }

    /// mooneye rapid_toggle: disabling the timer while the selected bit is
    /// high clocks TIMA; re-enabling (rising edge) does not, and the internal
    /// counter is not reset by TAC writes.
    #[test]
    fn tac_disable_with_selected_bit_high_increments() {
        let mut t = timer_with(0x04, 4, 4);
        ticks(&mut t, 128); // div = 512, bit 9 high
        assert_eq!(t.write(0xFF07, 0x00), 0);
        assert_eq!(t.read(0xFF05), 5);
        t.write(0xFF07, 0x04); // rising edge: no increment
        assert_eq!(t.read(0xFF05), 5);
    }

    #[test]
    fn tac_disable_with_selected_bit_low_does_not_increment() {
        let mut t = timer_with(0x04, 4, 4);
        ticks(&mut t, 64); // div = 256, bit 9 low
        t.write(0xFF07, 0x00);
        assert_eq!(t.read(0xFF05), 4);
    }

    #[test]
    fn disabled_timer_does_not_count() {
        let mut t = timer_with(0x00, 4, 4);
        assert_eq!(ticks(&mut t, 1024), 0);
        assert_eq!(t.read(0xFF05), 4);
    }

    /// A TAC frequency switch from a high selected bit to a low one is a
    /// falling edge too (same edge detector as enable).
    #[test]
    fn tac_frequency_change_can_increment() {
        let mut t = timer_with(0x07, 4, 4); // bit 7
        ticks(&mut t, 32); // div = 128: bit 7 high, bit 9 low
        t.write(0xFF07, 0x04); // switch to bit 9
        assert_eq!(t.read(0xFF05), 5);
    }

    /// mooneye tima_reload: after overflow TIMA reads 0x00 for 4 T-cycles
    /// (one M-cycle at the observable access points), then TMA. Increments
    /// keep their 64-T-cycle phase, no extra delay.
    ///
    /// Reference state: div = 0, TIMA = TMA = 0xFE, TAC = freq 10 (bit 5,
    /// 64 T-cycles). Reads happen after the tick of M-cycle:
    ///   28 nops + 3  -> div 124 -> 0xFF   (d)
    ///   29 nops + 3  -> div 128 -> 0x00   (e, overflow this cycle)
    ///   30 nops + 3  -> div 132 -> 0xFE   (c, reload this cycle)
    ///   60 nops + 3  -> div 252 -> 0xFF   (h)
    ///   61 nops + 3  -> div 256 -> 0x00   (l, second overflow)
    ///   62 nops + 3  -> div 260 -> 0xFE   (b)
    #[test]
    fn tima_reload_sequence() {
        for (mcycles, expected) in [
            (31u32, 0xFFu8),
            (32, 0x00),
            (33, 0xFE),
            (63, 0xFF),
            (64, 0x00),
            (65, 0xFE),
        ] {
            let mut t = timer_with(0x06, 0xFE, 0xFE);
            ticks(&mut t, mcycles);
            assert_eq!(t.read(0xFF05), expected, "after {mcycles} M-cycles");
        }
    }

    /// The timer interrupt is requested in the reload M-cycle, not in the
    /// overflow M-cycle.
    #[test]
    fn tima_reload_irq_timing() {
        let mut t = timer_with(0x06, 0xFE, 0xFE);
        assert_eq!(ticks(&mut t, 32), 0); // includes the overflow cycle
        assert_eq!(t.read(0xFF05), 0x00);
        assert_eq!(t.tick(), 0x04); // reload cycle raises IF bit 2
        assert_eq!(t.read(0xFF05), 0xFE);
    }

    /// mooneye tima_write_reloading. Writes of 0x7F to TIMA at the access
    /// point of M-cycle W (reference state as in `tima_reload_sequence`),
    /// then a read 3 M-cycles later:
    ///   W=31 (div 124, before overflow): normal write, +1 at div 128 -> 0x80
    ///   W=32 (div 128, overflow cycle):  write wins, reload cancelled -> 0x7F
    ///   W=33 (div 132, reload cycle):    write ignored, TMA wins      -> 0xFE
    ///   W=34 (div 136, after reload):    normal write                 -> 0x7F
    #[test]
    fn tima_write_reloading_cases() {
        for (w, expected) in [(31u32, 0x80u8), (32, 0x7F), (33, 0xFE), (34, 0x7F)] {
            let mut t = timer_with(0x06, 0xFE, 0xFE);
            ticks(&mut t, w);
            t.write(0xFF05, 0x7F);
            let iff = ticks(&mut t, 3);
            assert_eq!(t.read(0xFF05), expected, "write at M-cycle {w}");
            assert_eq!(iff, 0, "no IF after the write at M-cycle {w}");
        }
    }

    /// A TIMA write in the overflow window cancels both the reload and the
    /// interrupt; counting continues from the written value in phase.
    #[test]
    fn tima_write_in_overflow_window_cancels_reload_and_irq() {
        let mut t = timer_with(0x06, 0xFE, 0xFE);
        ticks(&mut t, 32); // overflow at div 128
        t.write(0xFF05, 0x7F);
        // No reload, no IRQ; next increment still at div 192 (16 cycles on).
        assert_eq!(ticks(&mut t, 15), 0);
        assert_eq!(t.read(0xFF05), 0x7F);
        assert_eq!(t.tick(), 0);
        assert_eq!(t.read(0xFF05), 0x80);
    }

    /// mooneye tma_write_reloading. Writes of 0x7F to TMA at M-cycle W:
    ///   W=32 (overflow cycle): reload one cycle later picks up new TMA -> 0x7F
    ///   W=33 (reload cycle):   forwarded to TIMA as well               -> 0x7F
    ///   W=34, W=35 (after):    too late, TIMA keeps old TMA            -> 0xFE
    #[test]
    fn tma_write_reloading_cases() {
        for (w, expected) in [(32u32, 0x7Fu8), (33, 0x7F), (34, 0xFE), (35, 0xFE)] {
            let mut t = timer_with(0x06, 0xFE, 0xFE);
            ticks(&mut t, w);
            t.write(0xFF06, 0x7F);
            ticks(&mut t, 3);
            assert_eq!(t.read(0xFF05), expected, "write at M-cycle {w}");
            assert_eq!(t.read(0xFF06), 0x7F, "TMA itself always updated");
        }
    }

    /// A DIV-write-induced increment that overflows TIMA also delays the
    /// reload + IRQ by 4 T-cycles (one observable M-cycle).
    #[test]
    fn div_write_overflow_delays_reload() {
        let mut t = timer_with(0x04, 0xFF, 0x42);
        ticks(&mut t, 128); // div = 512, bit 9 high, no edge yet
        assert_eq!(t.read(0xFF05), 0xFF);
        assert_eq!(t.write(0xFF04, 0), 0); // edge -> overflow, IF delayed
        assert_eq!(t.read(0xFF05), 0x00);
        assert_eq!(t.tick(), 0x04);
        assert_eq!(t.read(0xFF05), 0x42);
    }

    /// Same as above via TAC disable, and the reload window write rules
    /// apply to write-induced overflows too.
    #[test]
    fn tac_write_overflow_delays_reload_and_reload_cycle_write_ignored() {
        let mut t = timer_with(0x04, 0xFF, 0x10);
        ticks(&mut t, 128); // div = 512, bit 9 high
        assert_eq!(t.write(0xFF07, 0x00), 0); // disable -> edge -> overflow
        assert_eq!(t.read(0xFF05), 0x00);
        assert_eq!(t.tick(), 0x04); // reload still completes when disabled
        assert_eq!(t.read(0xFF05), 0x10);
        t.write(0xFF05, 0x99); // same M-cycle as the reload: ignored
        assert_eq!(t.read(0xFF05), 0x10);
    }

    /// Edges are detected at T-cycle granularity inside a tick, so a DIV
    /// phase that is not a multiple of 4 still clocks TIMA correctly.
    #[test]
    fn edge_mid_m_cycle_is_detected() {
        let mut t = Timer::new();
        t.set_div(14);
        t.write(0xFF07, 0x05); // select bit 3 (currently 1; enabling is a rising edge)
        t.tick(); // div 14 -> 18, falling edge at 16 on the 2nd T-cycle
        assert_eq!(t.read(0xFF05), 1);
    }
}
