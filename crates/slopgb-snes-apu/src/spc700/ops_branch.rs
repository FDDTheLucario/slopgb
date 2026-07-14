//! Branches, jumps, calls/returns, `CBNE`/`DBNZ`.
//!
//! Conditional branches consume the `rel` operand unconditionally and add +2
//! cycles when taken (fullsnes lists the not-taken count in [`CYCLES`]). `CBNE`
//! and `DBNZ` affect **no** flags (their internal compare/decrement is not
//! written to `PSW`).

use super::*;

impl Spc700 {
    /// Relative conditional branch. Returns total cycles (base, +2 if taken).
    pub(super) fn branch(&mut self, base: u32, cond: bool) -> u32 {
        let rel = self.fetch() as i8;
        if cond {
            self.pc = self.pc.wrapping_add(rel as u16);
            base + 2
        } else {
            base
        }
    }

    /// `BRA rel` (`2F`): unconditional relative branch (always taken, 4 cycles).
    pub(super) fn bra(&mut self) {
        let rel = self.fetch() as i8;
        self.pc = self.pc.wrapping_add(rel as u16);
    }

    /// `JMP [!abs+X]` (`1F`): read a pointer word at `abs+X`, jump to it.
    pub(super) fn jmp_absx(&mut self) {
        let base = self.fetch16();
        let addr = base.wrapping_add(self.x as u16);
        self.pc = self.read16(addr);
    }

    /// `PCALL u` (`4F`): call `$FF00 + u`.
    pub(super) fn pcall(&mut self) {
        let u = self.fetch();
        let pc = self.pc;
        self.push16(pc);
        self.pc = 0xFF00 | u as u16;
    }

    /// `TCALL n`: call the vector at `$FFDE - 2n` (n = high nibble of opcode).
    pub(super) fn tcall(&mut self, op: u8) {
        let n = (op >> 4) as u16;
        let vector = self.read16(0xFFDE - 2 * n);
        let pc = self.pc;
        self.push16(pc);
        self.pc = vector;
    }

    /// `RETI` (`7F`): pull PSW, then PC.
    pub(super) fn reti(&mut self) {
        let p = self.pull();
        self.psw = Psw::from_byte(p);
        self.pc = self.pull16();
    }

    /// `BRK` (`0F`): push PC and PSW, jump to `[$FFDE]`, set B, clear I. The
    /// pushed PSW holds the pre-BRK flags (B set only afterwards). (bsnes
    /// `instructionBRK`.)
    pub(super) fn brk(&mut self) {
        let vector = self.read16(0xFFDE);
        let pc = self.pc;
        self.push16(pc);
        let p = self.psw.to_byte();
        self.push(p);
        self.psw.b = true;
        self.psw.i = false;
        self.pc = vector;
    }

    /// `CBNE dp,rel` / `CBNE dp+X,rel`: branch if `[dp(+X)] != A`. No flags.
    pub(super) fn cbne_dp(&mut self, base: u32, indexed: bool) -> u32 {
        let off = self.fetch();
        let addr = if indexed {
            self.dp(off.wrapping_add(self.x))
        } else {
            self.dp(off)
        };
        let v = self.read8(addr);
        let rel = self.fetch() as i8;
        if v != self.a {
            self.pc = self.pc.wrapping_add(rel as u16);
            base + 2
        } else {
            base
        }
    }

    /// `DBNZ dp,rel` (`6E`): decrement `[dp]`, branch if non-zero. No flags.
    pub(super) fn dbnz_dp(&mut self, base: u32) -> u32 {
        let off = self.fetch();
        let addr = self.dp(off);
        let v = self.read8(addr).wrapping_sub(1);
        self.write8(addr, v);
        let rel = self.fetch() as i8;
        if v != 0 {
            self.pc = self.pc.wrapping_add(rel as u16);
            base + 2
        } else {
            base
        }
    }

    /// `DBNZ Y,rel` (`FE`): decrement Y, branch if non-zero. No flags.
    pub(super) fn dbnz_y(&mut self, base: u32) -> u32 {
        self.y = self.y.wrapping_sub(1);
        let rel = self.fetch() as i8;
        if self.y != 0 {
            self.pc = self.pc.wrapping_add(rel as u16);
            base + 2
        } else {
            base
        }
    }
}
