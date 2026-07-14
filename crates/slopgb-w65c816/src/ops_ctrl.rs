//! Flag and mode control: `REP`/`SEP` mask the status byte, `XCE` swaps carry
//! and emulation, `WDM` is a two-byte no-op. The single-bit flag clears/sets
//! (`CLC`, `SEC`, ...) are decoded inline in `dispatch`. Per the WDC W65C816S
//! datasheet and vectors.

use super::*;

impl Cpu {
    /// A single-flag clear/set (`CLC`/`SEC`/...); one internal cycle.
    pub(crate) fn flag_op(&mut self, mask: u8, on: bool) {
        self.set_flag(mask, on);
        self.io();
    }

    /// `REP #`: clear the masked status bits (M/X held in emulation).
    pub(crate) fn op_rep(&mut self, bus: &mut impl Bus) {
        let mask = self.fetch8(bus);
        self.io();
        self.regs.rep(mask);
    }

    /// `SEP #`: set the masked status bits (setting X drops index high bytes).
    pub(crate) fn op_sep(&mut self, bus: &mut impl Bus) {
        let mask = self.fetch8(bus);
        self.io();
        self.regs.sep(mask);
    }

    /// `XCE`: exchange carry and emulation.
    pub(crate) fn op_xce(&mut self) {
        self.io();
        self.regs.xce();
    }

    /// `WDM #`: reserved two-byte no-op (consumes its signature byte).
    pub(crate) fn wdm(&mut self, bus: &mut impl Bus) {
        self.fetch8(bus);
    }
}
