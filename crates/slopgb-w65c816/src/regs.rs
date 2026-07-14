//! The 65C816 register file + processor-status flags, and the mode/width rules
//! that govern every instruction (WDC W65C816S datasheet; Eyes & Lichty
//! "Programming the 65816"). Clean-room: modeled from the datasheet, never from
//! an emulator.

/// Processor-status (`P`) flag bits. In native mode `M`/`X` select the
/// accumulator/index width; in emulation mode both are forced set (8-bit) and
/// bit 4 doubles as the 6502 break flag.
pub mod flag {
    pub const N: u8 = 0x80;
    pub const V: u8 = 0x40;
    /// Accumulator/memory width: 1 = 8-bit, 0 = 16-bit (native only).
    pub const M: u8 = 0x20;
    /// Index width: 1 = 8-bit, 0 = 16-bit (native only). Bit 4 = break in
    /// emulation.
    pub const X: u8 = 0x10;
    pub const D: u8 = 0x08;
    pub const I: u8 = 0x04;
    pub const Z: u8 = 0x02;
    pub const C: u8 = 0x01;
}

/// The 65C816 programmer-visible registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regs {
    /// Accumulator (`C` = full 16 bits; `A` = low 8 in 8-bit mode).
    pub a: u16,
    /// Index X.
    pub x: u16,
    /// Index Y.
    pub y: u16,
    /// Stack pointer (high byte pinned to `01` in emulation).
    pub s: u16,
    /// Direct-page register.
    pub d: u16,
    /// Program counter.
    pub pc: u16,
    /// Program-bank register.
    pub pbr: u8,
    /// Data-bank register.
    pub dbr: u8,
    /// Processor status (`NVMXDIZC`).
    pub p: u8,
    /// Emulation mode (the pseudo-flag swapped with carry by `XCE`).
    pub e: bool,
}

impl Default for Regs {
    fn default() -> Self {
        Self::at_reset()
    }
}

impl Regs {
    /// The register state a RESET leaves (bar `PC`, which the CPU loads from the
    /// reset vector): emulation mode, 8-bit A/X/Y, decimal off, IRQs masked,
    /// direct page 0, banks 0, stack in page 1.
    #[must_use]
    pub fn at_reset() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            s: 0x0100,
            d: 0,
            pc: 0,
            pbr: 0,
            dbr: 0,
            p: flag::M | flag::X | flag::I,
            e: true,
        }
    }

    /// Whether the accumulator/memory is 16-bit (native + `M` clear).
    #[must_use]
    pub fn acc16(&self) -> bool {
        !self.e && self.p & flag::M == 0
    }

    /// Whether the index registers are 16-bit (native + `X` clear).
    #[must_use]
    pub fn idx16(&self) -> bool {
        !self.e && self.p & flag::X == 0
    }

    /// Exchange carry and emulation (`XCE`). Entering emulation forces 8-bit
    /// A/X/Y, clears the index high bytes, and pins the stack to page 1.
    pub fn xce(&mut self) {
        let carry = self.p & flag::C != 0;
        if self.e {
            self.p |= flag::C;
        } else {
            self.p &= !flag::C;
        }
        self.e = carry;
        if self.e {
            self.enter_emulation();
        }
    }

    /// Clear the `P` bits set in `mask` (`REP`). `M`/`X` cannot be cleared in
    /// emulation mode.
    pub fn rep(&mut self, mask: u8) {
        let mask = if self.e {
            mask & !(flag::M | flag::X)
        } else {
            mask
        };
        self.p &= !mask;
    }

    /// Set the `P` bits set in `mask` (`SEP`). Setting `X` (8-bit index) drops
    /// the index high bytes.
    pub fn sep(&mut self, mask: u8) {
        self.p |= mask;
        if self.p & flag::X != 0 {
            self.x &= 0x00FF;
            self.y &= 0x00FF;
        }
    }

    fn enter_emulation(&mut self) {
        self.p |= flag::M | flag::X;
        self.x &= 0x00FF;
        self.y &= 0x00FF;
        self.s = 0x0100 | (self.s & 0x00FF);
    }
}

#[cfg(test)]
#[path = "regs_tests.rs"]
mod tests;
