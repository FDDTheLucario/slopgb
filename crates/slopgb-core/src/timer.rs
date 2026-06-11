//! DIV/TIMA timer (FF04-FF07). Timer work package.
//!
//! Built around the internal 16-bit DIV counter. TIMA increments on falling
//! edges of the mux-selected DIV bit (including edges caused by DIV writes
//! and TAC writes), and reloads from TMA with a 1 M-cycle delay during which
//! writes to TIMA/TMA have the documented quirky effects
//! (mooneye: `tima_reload`, `tima_write_reloading`, `tma_write_reloading`,
//! `div_write`, `rapid_toggle`, `tim*`, `tim*_div_trigger`).

pub struct Timer {
    div: u16,
    // Timer work package owns further state (TIMA/TMA/TAC, reload pipeline).
}

impl Timer {
    pub fn new() -> Self {
        Self { div: 0 }
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

    /// Advance one M-cycle (4 T-cycles). Returns IF bits to request
    /// (bit 2 = timer), 0 if none.
    pub fn tick(&mut self) -> u8 {
        todo!("timer work package")
    }

    /// Read FF04-FF07.
    pub fn read(&self, addr: u16) -> u8 {
        let _ = addr;
        todo!("timer work package")
    }

    /// Write FF04-FF07. Returns IF bits to request immediately (a DIV/TAC
    /// write can clock TIMA via the falling-edge detector), 0 if none.
    pub fn write(&mut self, addr: u16, value: u8) -> u8 {
        let _ = (addr, value);
        todo!("timer work package")
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}
