//! ALU: arithmetic/logic primitives + the RMW, 16-bit word, `MUL`/`DIV`,
//! `DAA`/`DAS`, and `XCN` handlers.
//!
//! Flag semantics (fullsnes / anomie SPC700 doc):
//! - `ADC`/`SBC` set N V H Z C; `H` is the carry out of bit 3.
//! - `CMP` sets only N Z C (no V, no H).
//! - logic (`OR`/`AND`/`EOR`), shifts, `INC`/`DEC` set N Z (shifts also C).
//! - `CLRV` (elsewhere) is the only op that clears H without touching a value.

use super::*;

impl Spc700 {
    // -- 8-bit arithmetic primitives ---------------------------------------

    /// `A + B + C`. Sets N V H Z C. `H` = carry out of bit 3. `V` = signed
    /// overflow. (anomie SPC700 doc, "ADC".)
    pub(super) fn adc8(&mut self, a: u8, b: u8) -> u8 {
        let cin = self.psw.c as u16;
        let sum = a as u16 + b as u16 + cin;
        let r = sum as u8;
        self.psw.c = sum > 0xFF;
        self.psw.h = ((a & 0x0F) as u16 + (b & 0x0F) as u16 + cin) > 0x0F;
        self.psw.v = ((a ^ r) & (b ^ r) & 0x80) != 0;
        self.set_nz(r);
        r
    }

    /// `A - B - !C`, computed as `ADC(A, ~B)`; `C`=1 means "no borrow". (The
    /// standard identity `A - B - !C == A + (255-B) + C`.)
    pub(super) fn sbc8(&mut self, a: u8, b: u8) -> u8 {
        self.adc8(a, b ^ 0xFF)
    }

    /// `A - B` for comparison: sets N Z C only (C = `A >= B`). No V, no H.
    pub(super) fn cmp8(&mut self, a: u8, b: u8) {
        self.psw.c = a >= b;
        let r = a.wrapping_sub(b);
        self.set_nz(r);
    }

    pub(super) fn op_or(&mut self, a: u8, b: u8) -> u8 {
        let r = a | b;
        self.set_nz(r);
        r
    }
    pub(super) fn op_and(&mut self, a: u8, b: u8) -> u8 {
        let r = a & b;
        self.set_nz(r);
        r
    }
    pub(super) fn op_eor(&mut self, a: u8, b: u8) -> u8 {
        let r = a ^ b;
        self.set_nz(r);
        r
    }

    /// Compute one ALU op; `Cmp` returns `None` (result discarded).
    fn alu(&mut self, op: AluOp, a: u8, b: u8) -> Option<u8> {
        match op {
            AluOp::Or => Some(self.op_or(a, b)),
            AluOp::And => Some(self.op_and(a, b)),
            AluOp::Eor => Some(self.op_eor(a, b)),
            AluOp::Adc => Some(self.adc8(a, b)),
            AluOp::Sbc => Some(self.sbc8(a, b)),
            AluOp::Cmp => {
                self.cmp8(a, b);
                None
            }
        }
    }

    /// `OP dp, dp` (`opcode src dst`): `[dst] = [dst] OP [src]`.
    pub(super) fn alu_dp_dp(&mut self, op: AluOp) {
        let src = self.fetch();
        let dst = self.fetch();
        let sa = self.dp(src);
        let b = self.read8(sa);
        let da = self.dp(dst);
        let a = self.read8(da);
        if let Some(r) = self.alu(op, a, b) {
            self.write8(da, r);
        }
    }

    /// `OP dp, #imm` (`opcode imm dst`): `[dst] = [dst] OP imm`.
    pub(super) fn alu_dp_imm(&mut self, op: AluOp) {
        let imm = self.fetch();
        let dst = self.fetch();
        let da = self.dp(dst);
        let a = self.read8(da);
        if let Some(r) = self.alu(op, a, imm) {
            self.write8(da, r);
        }
    }

    /// `OP (X), (Y)`: `[dp|X] = [dp|X] OP [dp|Y]`, result to `(X)`.
    pub(super) fn alu_xy(&mut self, op: AluOp) {
        let ay = self.dp(self.y);
        let b = self.read8(ay);
        let ax = self.dp(self.x);
        let a = self.read8(ax);
        if let Some(r) = self.alu(op, a, b) {
            self.write8(ax, r);
        }
    }

    // -- shifts / rotates / inc / dec --------------------------------------

    pub(super) fn op_asl(&mut self, v: u8) -> u8 {
        self.psw.c = v & 0x80 != 0;
        let r = v << 1;
        self.set_nz(r);
        r
    }
    pub(super) fn op_lsr(&mut self, v: u8) -> u8 {
        self.psw.c = v & 0x01 != 0;
        let r = v >> 1;
        self.set_nz(r);
        r
    }
    pub(super) fn op_rol(&mut self, v: u8) -> u8 {
        let cin = self.psw.c as u8;
        self.psw.c = v & 0x80 != 0;
        let r = (v << 1) | cin;
        self.set_nz(r);
        r
    }
    pub(super) fn op_ror(&mut self, v: u8) -> u8 {
        let cin = self.psw.c as u8;
        self.psw.c = v & 0x01 != 0;
        let r = (v >> 1) | (cin << 7);
        self.set_nz(r);
        r
    }
    pub(super) fn op_inc(&mut self, v: u8) -> u8 {
        let r = v.wrapping_add(1);
        self.set_nz(r);
        r
    }
    pub(super) fn op_dec(&mut self, v: u8) -> u8 {
        let r = v.wrapping_sub(1);
        self.set_nz(r);
        r
    }

    fn rmw_apply(&mut self, op: Rmw, v: u8) -> u8 {
        match op {
            Rmw::Asl => self.op_asl(v),
            Rmw::Rol => self.op_rol(v),
            Rmw::Lsr => self.op_lsr(v),
            Rmw::Ror => self.op_ror(v),
            Rmw::Inc => self.op_inc(v),
            Rmw::Dec => self.op_dec(v),
        }
    }

    pub(super) fn rmw_dp(&mut self, op: Rmw) {
        let a = self.ea_dp();
        let v = self.read8(a);
        let r = self.rmw_apply(op, v);
        self.write8(a, r);
    }
    pub(super) fn rmw_abs(&mut self, op: Rmw) {
        let a = self.ea_abs();
        let v = self.read8(a);
        let r = self.rmw_apply(op, v);
        self.write8(a, r);
    }
    pub(super) fn rmw_dpx(&mut self, op: Rmw) {
        let a = self.ea_dpx();
        let v = self.read8(a);
        let r = self.rmw_apply(op, v);
        self.write8(a, r);
    }

    // -- 16-bit word ops (operand = a dp byte; word at dp / dp+1) ----------

    /// `ADDW YA, dp`. Two 8-bit adds so `H`/`V` come from the *high* byte add
    /// (H = carry out of bit 11), C = carry out of bit 15, N = bit 15, Z = whole
    /// 16-bit == 0. (bsnes `algorithmADW`.)
    pub(super) fn op_addw(&mut self) {
        let off = self.fetch();
        let m = self.read_word_dp(off);
        let ya = (self.y as u16) << 8 | self.a as u16;
        self.psw.c = false;
        let lo = self.adc8(ya as u8, m as u8);
        let hi = self.adc8((ya >> 8) as u8, (m >> 8) as u8);
        let r = (hi as u16) << 8 | lo as u16;
        self.psw.z = r == 0;
        self.a = lo;
        self.y = hi;
    }

    /// `SUBW YA, dp`. Two 8-bit subtracts; flags mirror `ADDW` (C=1 → no borrow).
    /// (bsnes `algorithmSBW`.)
    pub(super) fn op_subw(&mut self) {
        let off = self.fetch();
        let m = self.read_word_dp(off);
        let ya = (self.y as u16) << 8 | self.a as u16;
        self.psw.c = true;
        let lo = self.sbc8(ya as u8, m as u8);
        let hi = self.sbc8((ya >> 8) as u8, (m >> 8) as u8);
        let r = (hi as u16) << 8 | lo as u16;
        self.psw.z = r == 0;
        self.a = lo;
        self.y = hi;
    }

    /// `CMPW YA, dp`: 16-bit compare, sets N Z C only.
    pub(super) fn op_cmpw(&mut self) {
        let off = self.fetch();
        let m = self.read_word_dp(off);
        let ya = (self.y as u16) << 8 | self.a as u16;
        self.psw.c = ya >= m;
        let r = ya.wrapping_sub(m);
        self.psw.n = r & 0x8000 != 0;
        self.psw.z = r == 0;
    }

    /// `INCW dp`: read word, +1, write back. N = bit 15, Z = word == 0.
    pub(super) fn op_incw(&mut self) {
        let off = self.fetch();
        let r = self.read_word_dp(off).wrapping_add(1);
        self.write_word_dp(off, r);
        self.psw.n = r & 0x8000 != 0;
        self.psw.z = r == 0;
    }

    /// `DECW dp`: read word, -1, write back. N = bit 15, Z = word == 0.
    pub(super) fn op_decw(&mut self) {
        let off = self.fetch();
        let r = self.read_word_dp(off).wrapping_sub(1);
        self.write_word_dp(off, r);
        self.psw.n = r & 0x8000 != 0;
        self.psw.z = r == 0;
    }

    /// `MOVW YA, dp`: load word to YA. N = bit 15, Z = word == 0.
    pub(super) fn op_movw_load(&mut self) {
        let off = self.fetch();
        let w = self.read_word_dp(off);
        self.a = w as u8;
        self.y = (w >> 8) as u8;
        self.psw.n = w & 0x8000 != 0;
        self.psw.z = w == 0;
    }

    /// `MOVW dp, YA`: store YA to a dp word. No flags.
    pub(super) fn op_movw_store(&mut self) {
        let off = self.fetch();
        let w = (self.y as u16) << 8 | self.a as u16;
        self.write_word_dp(off, w);
    }

    // -- MUL / DIV / decimal / nibble --------------------------------------

    /// `MUL YA`: `YA = Y * A`. N,Z from `Y` (the high byte). (anomie, "MUL".)
    pub(super) fn op_mul(&mut self) {
        let r = self.y as u16 * self.a as u16;
        self.a = r as u8;
        self.y = (r >> 8) as u8;
        let y = self.y;
        self.set_nz(y);
    }

    /// `DIV YA, X`: `A = YA / X`, `Y = YA % X`, with the S-SMP's documented
    /// overflow quirk. `V` set when the quotient won't fit in 8 bits; `H` set
    /// when `(Y & 15) >= (X & 15)`; N,Z from the final `A` even on overflow.
    /// Faithful to bsnes `instructionDivide` (which reproduces the hardware's
    /// non-restoring-divider behaviour in the overflow case). Never divides by
    /// zero: `X == 0` routes to the `else` branch (divisor `256 - X = 256`).
    pub(super) fn op_div(&mut self) {
        let ya = (self.y as u16) << 8 | self.a as u16;
        let xw = self.x as u16;
        self.psw.v = self.y >= self.x;
        self.psw.h = (self.y & 0x0F) >= (self.x & 0x0F);
        if (self.y as u16) < (xw << 1) {
            // Quotient fits: ordinary division (x != 0 here, since y < 2x ⇒ x≥1).
            self.a = (ya / xw) as u8;
            self.y = (ya % xw) as u8;
        } else {
            // Overflow case: emulate the divider's wrap. Divisor is 256 - x ≥ 1.
            let ya = ya as i32;
            let x = xw as i32;
            self.a = (255 - (ya - (x << 9)) / (256 - x)) as u8;
            self.y = (x + (ya - (x << 9)) % (256 - x)) as u8;
        }
        let a = self.a;
        self.set_nz(a);
    }

    /// `DAA`: decimal adjust after addition. (bsnes `instructionDecimalAdjustAdd`.)
    pub(super) fn op_daa(&mut self) {
        if self.psw.c || self.a > 0x99 {
            self.a = self.a.wrapping_add(0x60);
            self.psw.c = true;
        }
        if self.psw.h || (self.a & 0x0F) > 0x09 {
            self.a = self.a.wrapping_add(0x06);
        }
        let a = self.a;
        self.set_nz(a);
    }

    /// `DAS`: decimal adjust after subtraction. (bsnes `instructionDecimalAdjustSub`.)
    pub(super) fn op_das(&mut self) {
        if !self.psw.c || self.a > 0x99 {
            self.a = self.a.wrapping_sub(0x60);
            self.psw.c = false;
        }
        if !self.psw.h || (self.a & 0x0F) > 0x09 {
            self.a = self.a.wrapping_sub(0x06);
        }
        let a = self.a;
        self.set_nz(a);
    }

    /// `XCN A`: exchange the nibbles of A (a rotate by 4). N,Z from the result.
    pub(super) fn op_xcn(&mut self) {
        self.a = self.a.rotate_left(4);
        let a = self.a;
        self.set_nz(a);
    }
}
