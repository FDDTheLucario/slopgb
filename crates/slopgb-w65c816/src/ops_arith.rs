//! `ADC` and `SBC`, binary and decimal (BCD), 8- and 16-bit. Binary add/subtract
//! set carry from the unsigned result and overflow from the signed result.
//! Decimal mode adjusts each byte per nibble; N/Z come from the adjusted result,
//! while V (and, for subtract, C) come from the pre-adjust/binary value — the
//! documented 65C816 decimal behaviour (Bruce Clark, "Decimal Mode"). No decimal
//! cycle penalty (confirmed by the vectors). Same addressing modes as `LDA`.

use super::*;

impl Cpu {
    pub(crate) fn adc(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let m = self.read_data(bus, addr, wide, bank0);
        self.do_adc(m, wide);
    }

    pub(crate) fn adc_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let m = self.imm(bus, wide);
        self.do_adc(m, wide);
    }

    pub(crate) fn sbc(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let m = self.read_data(bus, addr, wide, bank0);
        self.do_sbc(m, wide);
    }

    pub(crate) fn sbc_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let m = self.imm(bus, wide);
        self.do_sbc(m, wide);
    }

    fn do_adc(&mut self, m: u16, wide: bool) {
        let (mask, _) = width_bits(wide);
        let a = self.regs.a & mask;
        let (res, carry, v) = if self.regs.p & flag::D != 0 {
            self.adc_decimal(a, m & mask, wide)
        } else {
            self.add_binary(a, m & mask, wide)
        };
        self.finish_add(res, carry, v, wide);
    }

    fn do_sbc(&mut self, m: u16, wide: bool) {
        let (mask, _) = width_bits(wide);
        let a = self.regs.a & mask;
        let (res, carry, v) = if self.regs.p & flag::D != 0 {
            self.sbc_decimal(a, m & mask, wide)
        } else {
            // Subtract is add with the ones-complement operand.
            self.add_binary(a, !m & mask, wide)
        };
        self.finish_add(res, carry, v, wide);
    }

    /// Commit an add/subtract result: store `A` and set N/V/Z/C.
    fn finish_add(&mut self, res: u16, carry: bool, overflow: bool, wide: bool) {
        self.set_flag(flag::C, carry);
        self.set_flag(flag::V, overflow);
        self.set_nz(res, wide);
        self.set_a(res, wide);
    }

    /// Binary add of `A + m + C`; returns (result, carry-out, signed-overflow).
    fn add_binary(&self, a: u16, m: u16, wide: bool) -> (u16, bool, bool) {
        let (mask, sign) = width_bits(wide);
        let cin = u32::from(self.regs.p & flag::C != 0);
        let sum = a as u32 + m as u32 + cin;
        let res = (sum & mask as u32) as u16;
        let carry = sum > mask as u32;
        let overflow = (!(a ^ m) & (a ^ res) & sign) != 0;
        (res, carry, overflow)
    }

    /// Decimal (BCD) add; returns (result, carry-out, overflow). N/Z are taken
    /// from `result` by the caller; V here is the pre-final-adjust overflow.
    fn adc_decimal(&self, a: u16, m: u16, wide: bool) -> (u16, bool, bool) {
        let cin = u16::from(self.regs.p & flag::C != 0);
        let (rlo, c1, a1lo) = bcd_add_byte(a & 0xFF, m & 0xFF, cin);
        if !wide {
            let v = (!(a ^ m) & (a ^ a1lo) & 0x80) != 0;
            return (rlo, c1 != 0, v);
        }
        let (rhi, c2, a1hi) = bcd_add_byte((a >> 8) & 0xFF, (m >> 8) & 0xFF, c1);
        let (ah, mh) = ((a >> 8) & 0xFF, (m >> 8) & 0xFF);
        let v = (!(ah ^ mh) & (ah ^ a1hi) & 0x80) != 0;
        (rlo | (rhi << 8), c2 != 0, v)
    }

    /// Decimal (BCD) subtract. Result nibbles are adjusted per borrow; carry and
    /// overflow come from the plain binary difference (65C816 behaviour).
    fn sbc_decimal(&self, a: u16, m: u16, wide: bool) -> (u16, bool, bool) {
        let (mask, sign) = width_bits(wide);
        let cin = i32::from(self.regs.p & flag::C != 0);
        let bdiff = a as i32 - m as i32 - (1 - cin);
        let carry = bdiff >= 0;
        let overflow = ((a ^ m) & (a ^ (bdiff as u16 & mask)) & sign) != 0;

        let (rlo, borrow) = bcd_sub_byte(a & 0xFF, m & 0xFF, cin);
        let res = if wide {
            let (rhi, _) = bcd_sub_byte((a >> 8) & 0xFF, (m >> 8) & 0xFF, 1 - borrow);
            rlo | (rhi << 8)
        } else {
            rlo
        };
        (res, carry, overflow)
    }
}

/// BCD-add one byte: returns (adjusted result byte, carry-out, pre-adjust sum).
/// The pre-adjust sum carries bit 7 for the V flag.
fn bcd_add_byte(a: u16, m: u16, cin: u16) -> (u16, u16, u16) {
    let mut lo = (a & 0xF) + (m & 0xF) + cin;
    if lo >= 0x0A {
        lo = ((lo + 6) & 0xF) + 0x10;
    }
    let a1 = (a & 0xF0) + (m & 0xF0) + lo;
    let adj = if a1 >= 0xA0 { a1 + 0x60 } else { a1 };
    (adj & 0xFF, u16::from(adj >= 0x100), a1)
}

/// BCD-subtract one byte: `cin` is carry (1 = no borrow in). Returns (result
/// byte, borrow-out as 0/1).
fn bcd_sub_byte(a: u16, m: u16, cin: i32) -> (u16, i32) {
    let mut lo = (a & 0xF) as i32 - (m & 0xF) as i32 - (1 - cin);
    let borrow_lo = lo < 0;
    if borrow_lo {
        lo -= 6;
    }
    let mut hi = (a >> 4) as i32 - (m >> 4) as i32 - i32::from(borrow_lo);
    let borrow_hi = hi < 0;
    if borrow_hi {
        hi -= 6;
    }
    let res = (((hi as u16) & 0xF) << 4) | ((lo as u16) & 0xF);
    (res, i32::from(borrow_hi))
}
