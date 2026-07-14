//! Bitwise logic (`AND`/`ORA`/`EOR`/`BIT`), increments/decrements, the
//! shift/rotate family, and `TSB`/`TRB`. Read-modify-write ops read the operand,
//! spend one internal cycle, then write the result back high byte first. Widths
//! follow `M`. Per the WDC W65C816S datasheet and vectors.

use super::*;

impl Cpu {
    // --- logical (read) -----------------------------------------------------

    pub(crate) fn and(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let v = self.read_data(bus, addr, wide, bank0);
        let r = self.regs.a & v;
        self.set_a(r, wide);
        self.set_nz(self.regs.a, wide);
    }

    pub(crate) fn ora(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let v = self.read_data(bus, addr, wide, bank0);
        let r = self.regs.a | v;
        self.set_a(r, wide);
        self.set_nz(self.regs.a, wide);
    }

    pub(crate) fn eor(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let v = self.read_data(bus, addr, wide, bank0);
        let r = self.regs.a ^ v;
        self.set_a(r, wide);
        self.set_nz(self.regs.a, wide);
    }

    pub(crate) fn and_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let v = self.imm(bus, wide);
        let r = self.regs.a & v;
        self.set_a(r, wide);
        self.set_nz(self.regs.a, wide);
    }

    pub(crate) fn ora_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let v = self.imm(bus, wide);
        let r = self.regs.a | v;
        self.set_a(r, wide);
        self.set_nz(self.regs.a, wide);
    }

    pub(crate) fn eor_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let v = self.imm(bus, wide);
        let r = self.regs.a ^ v;
        self.set_a(r, wide);
        self.set_nz(self.regs.a, wide);
    }

    /// `BIT` (memory): Z from `A & M`; N/V copy the operand's top two bits.
    pub(crate) fn bit(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let (mask, sign) = width_bits(wide);
        let v = self.read_data(bus, addr, wide, bank0) & mask;
        self.set_flag(flag::Z, (self.regs.a & mask) & v == 0);
        self.set_flag(flag::N, v & sign != 0);
        self.set_flag(flag::V, v & (sign >> 1) != 0);
    }

    /// `BIT #`: only Z is affected (N/V unchanged).
    pub(crate) fn bit_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let (mask, _) = width_bits(wide);
        let v = self.imm(bus, wide);
        self.set_flag(flag::Z, (self.regs.a & mask) & v == 0);
    }

    // --- shift / rotate primitives ------------------------------------------

    fn do_asl(&mut self, v: u16, wide: bool) -> u16 {
        let (mask, sign) = width_bits(wide);
        self.set_flag(flag::C, v & sign != 0);
        let r = (v << 1) & mask;
        self.set_nz(r, wide);
        r
    }

    fn do_lsr(&mut self, v: u16, wide: bool) -> u16 {
        self.set_flag(flag::C, v & 1 != 0);
        let r = (v >> 1) & width_bits(wide).0;
        self.set_nz(r, wide);
        r
    }

    fn do_rol(&mut self, v: u16, wide: bool) -> u16 {
        let (mask, sign) = width_bits(wide);
        let carry_in = u16::from(self.regs.p & flag::C != 0);
        self.set_flag(flag::C, v & sign != 0);
        let r = ((v << 1) | carry_in) & mask;
        self.set_nz(r, wide);
        r
    }

    fn do_ror(&mut self, v: u16, wide: bool) -> u16 {
        let (mask, sign) = width_bits(wide);
        let carry_in = if self.regs.p & flag::C != 0 { sign } else { 0 };
        self.set_flag(flag::C, v & 1 != 0);
        let r = ((v >> 1) | carry_in) & mask;
        self.set_nz(r, wide);
        r
    }

    fn do_inc(&mut self, v: u16, wide: bool) -> u16 {
        let r = v.wrapping_add(1) & width_bits(wide).0;
        self.set_nz(r, wide);
        r
    }

    fn do_dec(&mut self, v: u16, wide: bool) -> u16 {
        let r = v.wrapping_sub(1) & width_bits(wide).0;
        self.set_nz(r, wide);
        r
    }

    // --- accumulator-mode shift/rotate/inc/dec (2 cycles) -------------------

    pub(crate) fn asl_a(&mut self) {
        self.acc_rmw(Self::do_asl);
    }
    pub(crate) fn lsr_a(&mut self) {
        self.acc_rmw(Self::do_lsr);
    }
    pub(crate) fn rol_a(&mut self) {
        self.acc_rmw(Self::do_rol);
    }
    pub(crate) fn ror_a(&mut self) {
        self.acc_rmw(Self::do_ror);
    }
    pub(crate) fn inc_a(&mut self) {
        self.acc_rmw(Self::do_inc);
    }
    pub(crate) fn dec_a(&mut self) {
        self.acc_rmw(Self::do_dec);
    }

    fn acc_rmw(&mut self, f: fn(&mut Self, u16, bool) -> u16) {
        let wide = self.regs.acc16();
        let (mask, _) = width_bits(wide);
        let r = f(self, self.regs.a & mask, wide);
        self.set_a(r, wide);
        self.io();
    }

    // --- memory-mode shift/rotate/inc/dec (read-modify-write) ---------------

    pub(crate) fn asl_m(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        self.mem_rmw(bus, addr, bank0, Self::do_asl);
    }
    pub(crate) fn lsr_m(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        self.mem_rmw(bus, addr, bank0, Self::do_lsr);
    }
    pub(crate) fn rol_m(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        self.mem_rmw(bus, addr, bank0, Self::do_rol);
    }
    pub(crate) fn ror_m(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        self.mem_rmw(bus, addr, bank0, Self::do_ror);
    }
    pub(crate) fn inc_m(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        self.mem_rmw(bus, addr, bank0, Self::do_inc);
    }
    pub(crate) fn dec_m(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        self.mem_rmw(bus, addr, bank0, Self::do_dec);
    }

    fn mem_rmw(
        &mut self,
        bus: &mut impl Bus,
        addr: u32,
        bank0: bool,
        f: fn(&mut Self, u16, bool) -> u16,
    ) {
        let wide = self.regs.acc16();
        let v = self.read_data(bus, addr, wide, bank0);
        let r = f(self, v, wide);
        self.rmw_writeback(bus, addr, v, r, wide, bank0);
    }

    /// The modify/store half of a read-modify-write. In native mode this is one
    /// internal cycle then the store; in emulation mode the CPU (6502-style)
    /// dummy-writes the original value first, then the result (vectors show two
    /// writes). Same cycle count either way.
    fn rmw_writeback(
        &mut self,
        bus: &mut impl Bus,
        addr: u32,
        old: u16,
        new: u16,
        wide: bool,
        bank0: bool,
    ) {
        if self.regs.e {
            self.write_data_rmw(bus, addr, old, wide, bank0);
        } else {
            self.io();
        }
        self.write_data_rmw(bus, addr, new, wide, bank0);
    }

    // --- index inc/dec (register) -------------------------------------------

    pub(crate) fn inx(&mut self) {
        let wide = self.regs.idx16();
        self.regs.x = self.do_inc(self.regs.x, wide);
        self.io();
    }
    pub(crate) fn iny(&mut self) {
        let wide = self.regs.idx16();
        self.regs.y = self.do_inc(self.regs.y, wide);
        self.io();
    }
    pub(crate) fn dex(&mut self) {
        let wide = self.regs.idx16();
        self.regs.x = self.do_dec(self.regs.x, wide);
        self.io();
    }
    pub(crate) fn dey(&mut self) {
        let wide = self.regs.idx16();
        self.regs.y = self.do_dec(self.regs.y, wide);
        self.io();
    }

    // --- TSB / TRB ----------------------------------------------------------

    /// `TSB`: Z from `A & M`; then set the bits of `A` in memory.
    pub(crate) fn tsb(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let (mask, _) = width_bits(wide);
        let v = self.read_data(bus, addr, wide, bank0);
        self.set_flag(flag::Z, (self.regs.a & mask) & v == 0);
        let r = (v | self.regs.a) & mask;
        self.rmw_writeback(bus, addr, v, r, wide, bank0);
    }

    /// `TRB`: Z from `A & M`; then clear the bits of `A` in memory.
    pub(crate) fn trb(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let (mask, _) = width_bits(wide);
        let v = self.read_data(bus, addr, wide, bank0);
        self.set_flag(flag::Z, (self.regs.a & mask) & v == 0);
        let r = (v & !self.regs.a) & mask;
        self.rmw_writeback(bus, addr, v, r, wide, bank0);
    }
}

/// The value mask and sign bit for a width.
fn width_bits(wide: bool) -> (u16, u16) {
    if wide {
        (0xFFFF, 0x8000)
    } else {
        (0x00FF, 0x0080)
    }
}
