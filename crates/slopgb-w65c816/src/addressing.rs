//! Effective-address resolvers for every 65C816 addressing mode. Each returns
//! the 24-bit address plus a `bank0` flag: `true` when a 16-bit data access must
//! keep its high byte in bank 0 (direct-page and stack data wrap at `$FFFF`).
//! Operand/pointer fetches and the internal/dummy cycles each mode spends are
//! charged here so the total matches the vectors. Addresses pinned to the WDC
//! W65C816S datasheet (§ addressing modes) and the SingleStepTests vectors.

use super::*;

impl Cpu {
    /// Charge the direct-page penalty cycle taken whenever the low byte of `D`
    /// is non-zero (datasheet cycle-count footnote: "add 1 if DL != 0").
    fn dp_penalty(&mut self) {
        if self.regs.d & 0x00FF != 0 {
            self.io();
        }
    }

    /// The bank-0 offset of a direct-page access for base offset `off` plus
    /// index `idx`. In emulation mode with `DL == 0` the address wraps inside
    /// the direct page (high byte fixed to `DH`); otherwise it is the plain
    /// 16-bit sum `D + off + idx`.
    fn dp_effective(&self, off: u8, idx: u16) -> u16 {
        let d = self.regs.d;
        if self.regs.e && d & 0x00FF == 0 {
            (d & 0xFF00) | ((off as u16).wrapping_add(idx) & 0x00FF)
        } else {
            d.wrapping_add(off as u16).wrapping_add(idx)
        }
    }

    /// The address of the second byte of a direct-page pointer, honouring the
    /// same emulation `DL == 0` page wrap as [`dp_effective`].
    fn dp_ptr_next(&self, base16: u16) -> u16 {
        if self.regs.e && self.regs.d & 0x00FF == 0 {
            (base16 & 0xFF00) | (base16.wrapping_add(1) & 0x00FF)
        } else {
            base16.wrapping_add(1)
        }
    }

    /// Read a 16-bit pointer from bank 0 at `base16` (second byte per
    /// [`dp_ptr_next`]).
    fn read_ptr16(&mut self, bus: &mut impl Bus, base16: u16) -> u16 {
        let lo = self.read8(bus, base16 as u32) as u16;
        let hi = self.read8(bus, self.dp_ptr_next(base16) as u32) as u16;
        lo | (hi << 8)
    }

    /// Read a 16-bit pointer from bank 0 at `base16`, second byte at `base+1`
    /// with a plain 16-bit wrap (no direct-page page-wrap). Used by stack-
    /// relative indirect, whose pointer is not a direct-page access.
    fn read_ptr16_linear(&mut self, bus: &mut impl Bus, base16: u16) -> u16 {
        let lo = self.read8(bus, base16 as u32) as u16;
        let hi = self.read8(bus, base16.wrapping_add(1) as u32) as u16;
        lo | (hi << 8)
    }

    /// Read a 24-bit pointer from bank 0 at `base16` (bytes at `base`, `base+1`,
    /// `base+2`, wrapping in bank 0).
    fn read_ptr24(&mut self, bus: &mut impl Bus, base16: u16) -> u32 {
        let b0 = self.read8(bus, base16 as u32) as u32;
        let b1 = self.read8(bus, base16.wrapping_add(1) as u32) as u32;
        let b2 = self.read8(bus, base16.wrapping_add(2) as u32) as u32;
        b0 | (b1 << 8) | (b2 << 16)
    }

    /// Whether an indexed access spends its extra (dummy-read) cycle: always for
    /// writes/RMW and for 16-bit index; for 8-bit index only when the low-byte
    /// add crosses a page (datasheet: absolute/`(dp)` indexed penalty).
    fn indexed_penalty(&self, base16: u16, index: u16, write: bool) -> bool {
        write || self.regs.idx16() || (base16 & 0xFF00) != (base16.wrapping_add(index) & 0xFF00)
    }

    /// Direct page: `d,dp`.
    pub(crate) fn am_dp(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        (self.dp_effective(off, 0) as u32, true)
    }

    /// Direct page indexed by X: `dp,X`.
    pub(crate) fn am_dp_x(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        self.io();
        (self.dp_effective(off, self.regs.x) as u32, true)
    }

    /// Direct page indexed by Y: `dp,Y`.
    pub(crate) fn am_dp_y(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        self.io();
        (self.dp_effective(off, self.regs.y) as u32, true)
    }

    /// Absolute (data bank): `dbr:abs`.
    pub(crate) fn am_abs(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let a = self.fetch16(bus);
        (((self.regs.dbr as u32) << 16) | a as u32, false)
    }

    /// Absolute indexed by X: `dbr:abs + X`.
    pub(crate) fn am_abs_x(&mut self, bus: &mut impl Bus, write: bool) -> (u32, bool) {
        let a = self.fetch16(bus);
        if self.indexed_penalty(a, self.regs.x, write) {
            self.io();
        }
        let eff = ((self.regs.dbr as u32) << 16)
            .wrapping_add(a as u32)
            .wrapping_add(self.regs.x as u32);
        (eff & 0x00FF_FFFF, false)
    }

    /// Absolute indexed by Y: `dbr:abs + Y`.
    pub(crate) fn am_abs_y(&mut self, bus: &mut impl Bus, write: bool) -> (u32, bool) {
        let a = self.fetch16(bus);
        if self.indexed_penalty(a, self.regs.y, write) {
            self.io();
        }
        let eff = ((self.regs.dbr as u32) << 16)
            .wrapping_add(a as u32)
            .wrapping_add(self.regs.y as u32);
        (eff & 0x00FF_FFFF, false)
    }

    /// Absolute long: `al`.
    pub(crate) fn am_long(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        (self.fetch24(bus), false)
    }

    /// Absolute long indexed by X: `al + X` (carries into the bank; no penalty).
    pub(crate) fn am_long_x(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let a = self.fetch24(bus);
        (a.wrapping_add(self.regs.x as u32) & 0x00FF_FFFF, false)
    }

    /// Direct-page indirect: `(dp)` -> `dbr:[dp]`.
    pub(crate) fn am_indirect(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        let base = self.dp_effective(off, 0);
        let ptr = self.read_ptr16(bus, base);
        (((self.regs.dbr as u32) << 16) | ptr as u32, false)
    }

    /// Direct-page indexed indirect: `(dp,X)` -> `dbr:[dp+X]`.
    pub(crate) fn am_indirect_x(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        self.io();
        let base = self.dp_effective(off, self.regs.x);
        let ptr = self.read_ptr16(bus, base);
        (((self.regs.dbr as u32) << 16) | ptr as u32, false)
    }

    /// Direct-page indirect indexed by Y: `(dp),Y` -> `dbr:[dp] + Y`.
    pub(crate) fn am_indirect_y(&mut self, bus: &mut impl Bus, write: bool) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        let base = self.dp_effective(off, 0);
        let ptr = self.read_ptr16(bus, base);
        if self.indexed_penalty(ptr, self.regs.y, write) {
            self.io();
        }
        let eff = ((self.regs.dbr as u32) << 16)
            .wrapping_add(ptr as u32)
            .wrapping_add(self.regs.y as u32);
        (eff & 0x00FF_FFFF, false)
    }

    /// Direct-page indirect long: `[dp]` -> `[dp]` (24-bit pointer).
    pub(crate) fn am_long_indirect(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        let base = self.dp_effective(off, 0);
        (self.read_ptr24(bus, base), false)
    }

    /// Direct-page indirect long indexed by Y: `[dp],Y` -> `[dp] + Y`.
    pub(crate) fn am_long_indirect_y(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.dp_penalty();
        let base = self.dp_effective(off, 0);
        let ptr = self.read_ptr24(bus, base);
        (ptr.wrapping_add(self.regs.y as u32) & 0x00FF_FFFF, false)
    }

    /// Read the 16-bit word a direct-page operand points at (used by `PEI`):
    /// fetch the `dp` offset, take the DP penalty, then read the pointer.
    pub(crate) fn dp_ptr_word(&mut self, bus: &mut impl Bus) -> u16 {
        let off = self.fetch8(bus);
        self.dp_penalty();
        let base = self.dp_effective(off, 0);
        self.read_ptr16(bus, base)
    }

    /// Stack relative: `sr,S` -> bank 0 `S + sr`.
    pub(crate) fn am_stack_s(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.io();
        (self.regs.s.wrapping_add(off as u16) as u32, true)
    }

    /// Stack-relative indirect indexed by Y: `(sr,S),Y` -> `dbr:[S+sr] + Y`.
    pub(crate) fn am_stack_s_y(&mut self, bus: &mut impl Bus) -> (u32, bool) {
        let off = self.fetch8(bus);
        self.io();
        let base = self.regs.s.wrapping_add(off as u16);
        let ptr = self.read_ptr16_linear(bus, base);
        self.io();
        let eff = ((self.regs.dbr as u32) << 16)
            .wrapping_add(ptr as u32)
            .wrapping_add(self.regs.y as u32);
        (eff & 0x00FF_FFFF, false)
    }
}
