//! Compares (`CMP`/`CPX`/`CPY`) and the branch family. A compare subtracts
//! without storing and sets N/Z/C (C = no borrow). A taken branch spends one
//! extra cycle, and a second in emulation mode when the target crosses a page.
//! Per the WDC W65C816S datasheet and vectors.

use super::*;

impl Cpu {
    /// Set N/Z/C from `reg - value` at the given width (compare semantics).
    fn compare(&mut self, reg: u16, value: u16, wide: bool) {
        let (mask, _) = width_bits(wide);
        let reg = reg & mask;
        let value = value & mask;
        self.set_flag(flag::C, reg >= value);
        self.set_nz(reg.wrapping_sub(value) & mask, wide);
    }

    pub(crate) fn cmp(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let v = self.read_data(bus, addr, wide, bank0);
        self.compare(self.regs.a, v, wide);
    }

    pub(crate) fn cmp_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let v = self.imm(bus, wide);
        self.compare(self.regs.a, v, wide);
    }

    pub(crate) fn cpx(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.idx16();
        let v = self.read_data(bus, addr, wide, bank0);
        self.compare(self.regs.x, v, wide);
    }

    pub(crate) fn cpx_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.idx16();
        let v = self.imm(bus, wide);
        self.compare(self.regs.x, v, wide);
    }

    pub(crate) fn cpy(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.idx16();
        let v = self.read_data(bus, addr, wide, bank0);
        self.compare(self.regs.y, v, wide);
    }

    pub(crate) fn cpy_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.idx16();
        let v = self.imm(bus, wide);
        self.compare(self.regs.y, v, wide);
    }

    // --- branches -----------------------------------------------------------

    /// A conditional (or, for `BRA`, unconditional) 8-bit relative branch.
    pub(crate) fn branch(&mut self, bus: &mut impl Bus, taken: bool) {
        let off = self.fetch8(bus) as i8 as i16 as u16;
        if !taken {
            return;
        }
        self.io();
        let from = self.regs.pc;
        let target = from.wrapping_add(off);
        // A page crossing costs one more cycle only in emulation mode.
        if self.regs.e && (from & 0xFF00) != (target & 0xFF00) {
            self.io();
        }
        self.regs.pc = target;
    }

    /// `BRL`: unconditional 16-bit relative branch (always taken, no page cost).
    pub(crate) fn brl(&mut self, bus: &mut impl Bus) {
        let off = self.fetch16(bus);
        self.io();
        self.regs.pc = self.regs.pc.wrapping_add(off);
    }
}
