//! Bit instructions.
//!
//! - `SET1`/`CLR1 dp.bit` and `BBS`/`BBC dp.bit,rel`: the tested bit is the
//!   opcode's top three bits (`op >> 5`). No flags.
//! - `TSET1`/`TCLR1 !abs`: set/clear the bits of A in memory; N,Z come from
//!   `A - mem` (a comparison), not from the written value. (anomie SPC700 doc.)
//! - `MOV1`/`AND1`/`OR1`/`EOR1`/`NOT1`: carry-bit ops on a **13-bit** address
//!   with the bit index in the top 3 bits: `addr = word & 0x1FFF`,
//!   `bit = word >> 13`. Only `C` (and `NOT1`'s memory bit) changes.

use super::*;

impl Spc700 {
    /// `SET1 dp.bit` (`x2`): set bit `op>>5` in `[dp]`. No flags.
    pub(super) fn set1(&mut self, op: u8) {
        let bit = op >> 5;
        let a = self.ea_dp();
        let v = self.read8(a) | (1 << bit);
        self.write8(a, v);
    }

    /// `CLR1 dp.bit` (`x2`): clear bit `op>>5` in `[dp]`. No flags.
    pub(super) fn clr1(&mut self, op: u8) {
        let bit = op >> 5;
        let a = self.ea_dp();
        let v = self.read8(a) & !(1 << bit);
        self.write8(a, v);
    }

    /// `BBS dp.bit, rel` (`x3`): branch if the bit is set. No flags.
    pub(super) fn bbs(&mut self, base: u32, op: u8) -> u32 {
        let bit = op >> 5;
        let a = self.ea_dp();
        let v = self.read8(a);
        let rel = self.fetch() as i8;
        if (v >> bit) & 1 != 0 {
            self.pc = self.pc.wrapping_add(rel as u16);
            base + 2
        } else {
            base
        }
    }

    /// `BBC dp.bit, rel` (`x3`): branch if the bit is clear. No flags.
    pub(super) fn bbc(&mut self, base: u32, op: u8) -> u32 {
        let bit = op >> 5;
        let a = self.ea_dp();
        let v = self.read8(a);
        let rel = self.fetch() as i8;
        if (v >> bit) & 1 == 0 {
            self.pc = self.pc.wrapping_add(rel as u16);
            base + 2
        } else {
            base
        }
    }

    /// `TSET1 !abs` (`0E`): `mem |= A`; N,Z from `A - mem_original`.
    pub(super) fn tset1(&mut self) {
        let addr = self.fetch16();
        let m = self.read8(addr);
        let t = self.a.wrapping_sub(m);
        self.set_nz(t);
        let v = m | self.a;
        self.write8(addr, v);
    }

    /// `TCLR1 !abs` (`4E`): `mem &= ~A`; N,Z from `A - mem_original`.
    pub(super) fn tclr1(&mut self) {
        let addr = self.fetch16();
        let m = self.read8(addr);
        let t = self.a.wrapping_sub(m);
        self.set_nz(t);
        let v = m & !self.a;
        self.write8(addr, v);
    }

    /// Fetch the 16-bit membit operand → `(addr, bit)` with `addr = w & 0x1FFF`,
    /// `bit = w >> 13`.
    fn membit_operand(&mut self) -> (u16, u8) {
        let w = self.fetch16();
        (w & 0x1FFF, (w >> 13) as u8)
    }

    /// `MOV1 C, mem.bit` (`AA`): `C = bit`.
    pub(super) fn mov1_c_m(&mut self) {
        let (addr, bit) = self.membit_operand();
        let v = self.read8(addr);
        self.psw.c = (v >> bit) & 1 != 0;
    }

    /// `MOV1 mem.bit, C` (`CA`): write `C` into the memory bit. No other flags.
    pub(super) fn mov1_m_c(&mut self) {
        let (addr, bit) = self.membit_operand();
        let mut v = self.read8(addr);
        if self.psw.c {
            v |= 1 << bit;
        } else {
            v &= !(1 << bit);
        }
        self.write8(addr, v);
    }

    /// `AND1 C, mem.bit` (`4A`) / `AND1 C, /mem.bit` (`6A`, `negate`).
    pub(super) fn and1(&mut self, negate: bool) {
        let (addr, bit) = self.membit_operand();
        let mut b = (self.read8(addr) >> bit) & 1 != 0;
        if negate {
            b = !b;
        }
        self.psw.c = self.psw.c && b;
    }

    /// `OR1 C, mem.bit` (`0A`) / `OR1 C, /mem.bit` (`2A`, `negate`).
    pub(super) fn or1(&mut self, negate: bool) {
        let (addr, bit) = self.membit_operand();
        let mut b = (self.read8(addr) >> bit) & 1 != 0;
        if negate {
            b = !b;
        }
        self.psw.c = self.psw.c || b;
    }

    /// `EOR1 C, mem.bit` (`8A`): `C ^= bit`.
    pub(super) fn eor1(&mut self) {
        let (addr, bit) = self.membit_operand();
        let b = (self.read8(addr) >> bit) & 1 != 0;
        self.psw.c ^= b;
    }

    /// `NOT1 mem.bit` (`EA`): flip the memory bit. No flags.
    pub(super) fn not1(&mut self) {
        let (addr, bit) = self.membit_operand();
        let v = self.read8(addr) ^ (1 << bit);
        self.write8(addr, v);
    }
}
