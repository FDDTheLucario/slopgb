//! Stack primitives and the push/pull family (including `PEA`/`PEI`/`PER`). The
//! stack lives in bank 0; in emulation mode `S` stays inside page 1. Pushes
//! store high byte first (so the value is little-endian in memory). Per the WDC
//! W65C816S datasheet and vectors.

use super::*;

impl Cpu {
    /// `S` after one decrement (emulation keeps it in page 1).
    fn dec_s(&self) -> u16 {
        if self.regs.e {
            0x0100 | self.regs.s.wrapping_sub(1) & 0x00FF
        } else {
            self.regs.s.wrapping_sub(1)
        }
    }

    /// `S` after one increment (emulation keeps it in page 1).
    fn inc_s(&self) -> u16 {
        if self.regs.e {
            0x0100 | self.regs.s.wrapping_add(1) & 0x00FF
        } else {
            self.regs.s.wrapping_add(1)
        }
    }

    /// Push one byte, then move the stack down.
    pub(crate) fn push8(&mut self, bus: &mut impl Bus, value: u8) {
        self.write8(bus, self.regs.s as u32, value);
        self.regs.s = self.dec_s();
    }

    /// Move the stack up, then pull one byte.
    pub(crate) fn pull8(&mut self, bus: &mut impl Bus) -> u8 {
        self.regs.s = self.inc_s();
        self.read8(bus, self.regs.s as u32)
    }

    /// Push one byte stepping the full 16-bit `S` (no page-1 clamp). Used by the
    /// 65816 long-call ops (`JSL`/`RTL`), which are not clamped to page 1.
    pub(crate) fn push8_linear(&mut self, bus: &mut impl Bus, value: u8) {
        self.write8(bus, self.regs.s as u32, value);
        self.regs.s = self.regs.s.wrapping_sub(1);
    }

    /// Pull one byte stepping the full 16-bit `S` (no page-1 clamp).
    pub(crate) fn pull8_linear(&mut self, bus: &mut impl Bus) -> u8 {
        self.regs.s = self.regs.s.wrapping_add(1);
        self.read8(bus, self.regs.s as u32)
    }

    /// Push a 16-bit value (high byte first). The 65816 stack ops that push a
    /// word — `PHD`/`PEA`/`PEI`/`PER` and native pushes — step the full 16-bit
    /// `S` and are not clamped to page 1 (the emulation-mode SH lock is re-applied
    /// only at the next instruction; datasheet + vectors).
    pub(crate) fn push16(&mut self, bus: &mut impl Bus, value: u16) {
        self.write8(bus, self.regs.s as u32, (value >> 8) as u8);
        self.regs.s = self.regs.s.wrapping_sub(1);
        self.write8(bus, self.regs.s as u32, value as u8);
        self.regs.s = self.regs.s.wrapping_sub(1);
    }

    /// Pull a 16-bit value (low byte first), stepping the full 16-bit `S`.
    pub(crate) fn pull16(&mut self, bus: &mut impl Bus) -> u16 {
        self.regs.s = self.regs.s.wrapping_add(1);
        let lo = self.read8(bus, self.regs.s as u32) as u16;
        self.regs.s = self.regs.s.wrapping_add(1);
        let hi = self.read8(bus, self.regs.s as u32) as u16;
        lo | (hi << 8)
    }

    /// Push a register of `wide` width (accumulator or index). One internal
    /// cycle precedes the store(s).
    fn push_reg(&mut self, bus: &mut impl Bus, value: u16, wide: bool) {
        self.io();
        if wide {
            self.push16(bus, value);
        } else {
            self.push8(bus, value as u8);
        }
    }

    /// Pull a register of `wide` width, set N/Z, and store via `set`. Two
    /// internal cycles precede the load(s).
    fn pull_reg(&mut self, bus: &mut impl Bus, wide: bool, set: impl FnOnce(&mut Self, u16)) {
        self.io();
        self.io();
        let v = if wide {
            self.pull16(bus)
        } else {
            self.pull8(bus) as u16
        };
        set(self, v);
        self.set_nz(v, wide);
    }

    // --- push register ------------------------------------------------------

    pub(crate) fn pha(&mut self, bus: &mut impl Bus) {
        let (v, w) = (self.regs.a, self.regs.acc16());
        self.push_reg(bus, v, w);
    }

    pub(crate) fn phx(&mut self, bus: &mut impl Bus) {
        let (v, w) = (self.regs.x, self.regs.idx16());
        self.push_reg(bus, v, w);
    }

    pub(crate) fn phy(&mut self, bus: &mut impl Bus) {
        let (v, w) = (self.regs.y, self.regs.idx16());
        self.push_reg(bus, v, w);
    }

    /// `PHP`: push the status byte (always 8 bits).
    pub(crate) fn php(&mut self, bus: &mut impl Bus) {
        self.io();
        let p = self.regs.p;
        self.push8(bus, p);
    }

    /// `PHB`: push the data-bank register.
    pub(crate) fn phb(&mut self, bus: &mut impl Bus) {
        self.io();
        let v = self.regs.dbr;
        self.push8(bus, v);
    }

    /// `PHK`: push the program-bank register.
    pub(crate) fn phk(&mut self, bus: &mut impl Bus) {
        self.io();
        let v = self.regs.pbr;
        self.push8(bus, v);
    }

    /// `PHD`: push the 16-bit direct-page register.
    pub(crate) fn phd(&mut self, bus: &mut impl Bus) {
        self.io();
        let v = self.regs.d;
        self.push16(bus, v);
    }

    // --- pull register ------------------------------------------------------

    pub(crate) fn pla(&mut self, bus: &mut impl Bus) {
        let w = self.regs.acc16();
        self.pull_reg(bus, w, |c, v| c.set_a(v, w));
    }

    pub(crate) fn plx(&mut self, bus: &mut impl Bus) {
        let w = self.regs.idx16();
        self.pull_reg(bus, w, |c, v| c.set_x(v, w));
    }

    pub(crate) fn ply(&mut self, bus: &mut impl Bus) {
        let w = self.regs.idx16();
        self.pull_reg(bus, w, |c, v| c.set_y(v, w));
    }

    /// `PLP`: pull the status byte. In emulation mode M and X read back as set.
    pub(crate) fn plp(&mut self, bus: &mut impl Bus) {
        self.io();
        self.io();
        let mut p = self.pull8(bus);
        if self.regs.e {
            p |= flag::M | flag::X;
        }
        self.regs.p = p;
        // An 8-bit index (X set) drops the index high bytes.
        if self.regs.p & flag::X != 0 && !self.regs.e {
            self.regs.x &= 0x00FF;
            self.regs.y &= 0x00FF;
        }
    }

    /// `PLB`: pull the data-bank register (sets N/Z). Like the other 65816-only
    /// stack ops it steps `S` linearly (not clamped to page 1) in emulation.
    pub(crate) fn plb(&mut self, bus: &mut impl Bus) {
        self.io();
        self.io();
        let v = self.pull8_linear(bus);
        self.regs.dbr = v;
        self.set_nz(v as u16, false);
    }

    /// `PLD`: pull the 16-bit direct-page register (sets 16-bit N/Z).
    pub(crate) fn pld(&mut self, bus: &mut impl Bus) {
        self.io();
        self.io();
        let v = self.pull16(bus);
        self.regs.d = v;
        self.set_nz(v, true);
    }

    // --- push effective / immediate -----------------------------------------

    /// `PEA`: push a 16-bit immediate (absolute operand).
    pub(crate) fn pea(&mut self, bus: &mut impl Bus) {
        let v = self.fetch16(bus);
        self.push16(bus, v);
    }

    /// `PEI`: push the 16-bit word at direct-page `(dp)`.
    pub(crate) fn pei(&mut self, bus: &mut impl Bus) {
        let v = self.dp_ptr_word(bus);
        self.push16(bus, v);
    }

    /// `PER`: push `PC + rel16` (a program-counter-relative address).
    pub(crate) fn per(&mut self, bus: &mut impl Bus) {
        let rel = self.fetch16(bus);
        self.io();
        let target = self.regs.pc.wrapping_add(rel);
        self.push16(bus, target);
    }
}
