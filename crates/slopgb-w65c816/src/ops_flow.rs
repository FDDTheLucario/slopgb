//! Jumps and subroutine calls: `JMP`/`JML`, `JSR`/`JSL`, `RTS`/`RTL`. The
//! return address pushed is the address of the instruction's last byte (so a
//! return adds one). The 6502-era calls (`JSR`/`RTS`) keep the stack in page 1
//! in emulation; the 65816 long calls (`JSL`/`RTL`) step the full stack. `JMP
//! (abs)` and `JML [abs]` take their pointer from bank 0; `JMP (abs,X)` from the
//! program bank. Per the WDC W65C816S datasheet and vectors.

use super::*;

impl Cpu {
    /// Read a 16-bit little-endian word from bank 0 (second byte wraps at
    /// `$FFFF`).
    fn read16_bank0(&mut self, bus: &mut impl Bus, addr16: u16) -> u16 {
        let lo = self.read8(bus, addr16 as u32) as u16;
        let hi = self.read8(bus, addr16.wrapping_add(1) as u32) as u16;
        lo | (hi << 8)
    }

    // --- jumps --------------------------------------------------------------

    /// `JMP abs`.
    pub(crate) fn jmp_abs(&mut self, bus: &mut impl Bus) {
        self.regs.pc = self.fetch16(bus);
    }

    /// `JMP long` (`JML`): sets the program bank too.
    pub(crate) fn jmp_long(&mut self, bus: &mut impl Bus) {
        let target = self.fetch24(bus);
        self.regs.pbr = (target >> 16) as u8;
        self.regs.pc = target as u16;
    }

    /// `JMP (abs)`: 16-bit pointer read from bank 0.
    pub(crate) fn jmp_indirect(&mut self, bus: &mut impl Bus) {
        let ptr = self.fetch16(bus);
        self.regs.pc = self.read16_bank0(bus, ptr);
    }

    /// `JMP (abs,X)`: pointer `abs + X` read from the program bank.
    pub(crate) fn jmp_indirect_x(&mut self, bus: &mut impl Bus) {
        let base = self.fetch16(bus);
        self.io();
        let ptr = base.wrapping_add(self.regs.x);
        let bank = (self.regs.pbr as u32) << 16;
        let lo = self.read8(bus, bank | ptr as u32) as u16;
        let hi = self.read8(bus, bank | ptr.wrapping_add(1) as u32) as u16;
        self.regs.pc = lo | (hi << 8);
    }

    /// `JML [abs]`: 24-bit pointer read from bank 0.
    pub(crate) fn jmp_long_indirect(&mut self, bus: &mut impl Bus) {
        let ptr = self.fetch16(bus);
        let lo = self.read8(bus, ptr as u32) as u32;
        let mid = self.read8(bus, ptr.wrapping_add(1) as u32) as u32;
        let hi = self.read8(bus, ptr.wrapping_add(2) as u32) as u32;
        self.regs.pc = (lo | (mid << 8)) as u16;
        self.regs.pbr = hi as u8;
    }

    // --- calls / returns ----------------------------------------------------

    /// `JSR abs`: push (PC of last byte), then jump. Stack stays in page 1 in
    /// emulation.
    pub(crate) fn jsr_abs(&mut self, bus: &mut impl Bus) {
        let target = self.fetch16(bus);
        self.io();
        let ret = self.regs.pc.wrapping_sub(1);
        self.push8(bus, (ret >> 8) as u8);
        self.push8(bus, ret as u8);
        self.regs.pc = target;
    }

    /// `JSR (abs,X)`: push the return address before resolving the pointer (in
    /// the program bank).
    pub(crate) fn jsr_indirect_x(&mut self, bus: &mut impl Bus) {
        let lo = self.fetch8(bus) as u16;
        let ret = self.regs.pc; // address of the operand's high byte
        self.push8(bus, (ret >> 8) as u8);
        self.push8(bus, ret as u8);
        let hi = self.fetch8(bus) as u16;
        self.io();
        let ptr = (lo | (hi << 8)).wrapping_add(self.regs.x);
        let bank = (self.regs.pbr as u32) << 16;
        let plo = self.read8(bus, bank | ptr as u32) as u16;
        let phi = self.read8(bus, bank | ptr.wrapping_add(1) as u32) as u16;
        self.regs.pc = plo | (phi << 8);
    }

    /// `JSL long`: push PBR then the return address, and jump to the new bank.
    pub(crate) fn jsl(&mut self, bus: &mut impl Bus) {
        let lo = self.fetch8(bus) as u16;
        let mid = self.fetch8(bus) as u16;
        self.push8_linear(bus, self.regs.pbr);
        self.io();
        let bank = self.fetch8(bus);
        let ret = self.regs.pc.wrapping_sub(1);
        self.push16(bus, ret);
        self.regs.pbr = bank;
        self.regs.pc = lo | (mid << 8);
    }

    /// `RTS`: pull PC and add one. Two internal cycles before, one after.
    pub(crate) fn rts(&mut self, bus: &mut impl Bus) {
        self.io();
        self.io();
        let lo = self.pull8(bus) as u16;
        let hi = self.pull8(bus) as u16;
        self.io();
        self.regs.pc = (lo | (hi << 8)).wrapping_add(1);
    }

    /// `RTL`: pull PC and PBR (linear stack), and add one to PC.
    pub(crate) fn rtl(&mut self, bus: &mut impl Bus) {
        self.io();
        self.io();
        let lo = self.pull8_linear(bus) as u16;
        let hi = self.pull8_linear(bus) as u16;
        let bank = self.pull8_linear(bus);
        self.regs.pc = (lo | (hi << 8)).wrapping_add(1);
        self.regs.pbr = bank;
    }
}
