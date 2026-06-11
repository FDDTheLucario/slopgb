//! APU: 2 pulse channels, wave, noise (FF10-FF3F). APU work package.
//!
//! The frame sequencer (length/envelope/sweep) is clocked by falling edges
//! of DIV bit 4 (bit 5 in CGB double speed) — "DIV-APU". Power-off (NR52)
//! clears registers; length counters are writable while off on DMG only.
//! Emulates obscure behaviors: trigger while sweep negate, wave RAM access
//! during playback, length clocking on enable edge, etc.

pub struct Apu {
    // APU work package owns all state.
}

impl Apu {
    pub fn new(cgb: bool) -> Self {
        let _ = cgb;
        Self {}
    }

    /// Advance one M-cycle (4 T-cycles). `div` is the timer's internal DIV
    /// counter after this cycle; `double_speed` selects the DIV-APU bit.
    pub fn tick(&mut self, div: u16, double_speed: bool) {
        let _ = (div, double_speed);
        todo!("APU work package")
    }

    /// Read FF10-FF3F (unused bits read 1, wave RAM access rules apply).
    pub fn read(&self, addr: u16) -> u8 {
        let _ = addr;
        todo!("APU work package")
    }

    /// Write FF10-FF3F.
    pub fn write(&mut self, addr: u16, value: u8) {
        let _ = (addr, value);
        todo!("APU work package")
    }

    /// Output sample rate for [`Self::drain_samples`]. Default 48000.
    pub fn set_sample_rate(&mut self, hz: u32) {
        let _ = hz;
        todo!("APU work package")
    }

    /// Move all accumulated stereo samples into `out`.
    pub fn drain_samples(&mut self, out: &mut Vec<(f32, f32)>) {
        let _ = out;
        todo!("APU work package")
    }
}
