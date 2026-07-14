//! Opcode decode: map each of the 256 opcodes to its addressing mode and
//! operation. Store/RMW indexed modes resolve with `write = true` so they always
//! spend the indexing dummy cycle. Opcode assignments per the 65C816 matrix.

use super::*;

/// Resolve a read addressing mode and hand `(addr, bank0)` to an op method.
macro_rules! am {
    ($self:ident, $bus:ident, $mode:ident => $op:ident) => {{
        let (addr, bank0) = $self.$mode($bus);
        $self.$op($bus, addr, bank0);
    }};
}

/// Like [`am`], for modes that take a `write` flag (indexed penalty).
macro_rules! amw {
    ($self:ident, $bus:ident, $mode:ident, $write:expr => $op:ident) => {{
        let (addr, bank0) = $self.$mode($bus, $write);
        $self.$op($bus, addr, bank0);
    }};
}

impl Cpu {
    /// Execute the already-fetched `opcode`.
    pub(crate) fn dispatch(&mut self, opcode: u8, bus: &mut impl Bus) {
        match opcode {
            // --- NOP ---
            0xEA => self.io(),

            // --- LDA ---
            0xA9 => self.lda_imm(bus),
            0xA5 => am!(self, bus, am_dp => lda),
            0xB5 => am!(self, bus, am_dp_x => lda),
            0xAD => am!(self, bus, am_abs => lda),
            0xBD => amw!(self, bus, am_abs_x, false => lda),
            0xB9 => amw!(self, bus, am_abs_y, false => lda),
            0xAF => am!(self, bus, am_long => lda),
            0xBF => am!(self, bus, am_long_x => lda),
            0xA1 => am!(self, bus, am_indirect_x => lda),
            0xB1 => amw!(self, bus, am_indirect_y, false => lda),
            0xB2 => am!(self, bus, am_indirect => lda),
            0xA7 => am!(self, bus, am_long_indirect => lda),
            0xB7 => am!(self, bus, am_long_indirect_y => lda),
            0xA3 => am!(self, bus, am_stack_s => lda),
            0xB3 => am!(self, bus, am_stack_s_y => lda),

            // --- LDX ---
            0xA2 => self.ldx_imm(bus),
            0xA6 => am!(self, bus, am_dp => ldx),
            0xB6 => am!(self, bus, am_dp_y => ldx),
            0xAE => am!(self, bus, am_abs => ldx),
            0xBE => amw!(self, bus, am_abs_y, false => ldx),

            // --- LDY ---
            0xA0 => self.ldy_imm(bus),
            0xA4 => am!(self, bus, am_dp => ldy),
            0xB4 => am!(self, bus, am_dp_x => ldy),
            0xAC => am!(self, bus, am_abs => ldy),
            0xBC => amw!(self, bus, am_abs_x, false => ldy),

            // --- STA ---
            0x85 => am!(self, bus, am_dp => sta),
            0x95 => am!(self, bus, am_dp_x => sta),
            0x8D => am!(self, bus, am_abs => sta),
            0x9D => amw!(self, bus, am_abs_x, true => sta),
            0x99 => amw!(self, bus, am_abs_y, true => sta),
            0x8F => am!(self, bus, am_long => sta),
            0x9F => am!(self, bus, am_long_x => sta),
            0x81 => am!(self, bus, am_indirect_x => sta),
            0x91 => amw!(self, bus, am_indirect_y, true => sta),
            0x92 => am!(self, bus, am_indirect => sta),
            0x87 => am!(self, bus, am_long_indirect => sta),
            0x97 => am!(self, bus, am_long_indirect_y => sta),
            0x83 => am!(self, bus, am_stack_s => sta),
            0x93 => am!(self, bus, am_stack_s_y => sta),

            // --- STX / STY ---
            0x86 => am!(self, bus, am_dp => stx),
            0x96 => am!(self, bus, am_dp_y => stx),
            0x8E => am!(self, bus, am_abs => stx),
            0x84 => am!(self, bus, am_dp => sty),
            0x94 => am!(self, bus, am_dp_x => sty),
            0x8C => am!(self, bus, am_abs => sty),

            // --- STZ ---
            0x64 => am!(self, bus, am_dp => stz),
            0x74 => am!(self, bus, am_dp_x => stz),
            0x9C => am!(self, bus, am_abs => stz),
            0x9E => amw!(self, bus, am_abs_x, true => stz),

            // --- transfers ---
            0xAA => self.tax(),
            0xA8 => self.tay(),
            0xBA => self.tsx(),
            0x8A => self.txa(),
            0x9A => self.txs(),
            0x9B => self.txy(),
            0x98 => self.tya(),
            0xBB => self.tyx(),
            0x5B => self.tcd(),
            0x7B => self.tdc(),
            0x1B => self.tcs(),
            0x3B => self.tsc(),

            // --- XBA ---
            0xEB => self.xba(),

            // --- push / pull ---
            0x48 => self.pha(bus),
            0xDA => self.phx(bus),
            0x5A => self.phy(bus),
            0x08 => self.php(bus),
            0x8B => self.phb(bus),
            0x4B => self.phk(bus),
            0x0B => self.phd(bus),
            0x68 => self.pla(bus),
            0xFA => self.plx(bus),
            0x7A => self.ply(bus),
            0x28 => self.plp(bus),
            0xAB => self.plb(bus),
            0x2B => self.pld(bus),
            0xF4 => self.pea(bus),
            0xD4 => self.pei(bus),
            0x62 => self.per(bus),

            // --- flag clears / sets (opcode + one internal cycle) ---
            0x18 => self.flag_op(flag::C, false),
            0x38 => self.flag_op(flag::C, true),
            0x58 => self.flag_op(flag::I, false),
            0x78 => self.flag_op(flag::I, true),
            0xB8 => self.flag_op(flag::V, false),
            0xD8 => self.flag_op(flag::D, false),
            0xF8 => self.flag_op(flag::D, true),

            // --- mode control ---
            0xC2 => self.op_rep(bus),
            0xE2 => self.op_sep(bus),
            0xFB => self.op_xce(),
            0x42 => self.wdm(),

            // --- ORA ---
            0x09 => self.ora_imm(bus),
            0x05 => am!(self, bus, am_dp => ora),
            0x15 => am!(self, bus, am_dp_x => ora),
            0x0D => am!(self, bus, am_abs => ora),
            0x1D => amw!(self, bus, am_abs_x, false => ora),
            0x19 => amw!(self, bus, am_abs_y, false => ora),
            0x0F => am!(self, bus, am_long => ora),
            0x1F => am!(self, bus, am_long_x => ora),
            0x01 => am!(self, bus, am_indirect_x => ora),
            0x11 => amw!(self, bus, am_indirect_y, false => ora),
            0x12 => am!(self, bus, am_indirect => ora),
            0x07 => am!(self, bus, am_long_indirect => ora),
            0x17 => am!(self, bus, am_long_indirect_y => ora),
            0x03 => am!(self, bus, am_stack_s => ora),
            0x13 => am!(self, bus, am_stack_s_y => ora),

            // --- AND ---
            0x29 => self.and_imm(bus),
            0x25 => am!(self, bus, am_dp => and),
            0x35 => am!(self, bus, am_dp_x => and),
            0x2D => am!(self, bus, am_abs => and),
            0x3D => amw!(self, bus, am_abs_x, false => and),
            0x39 => amw!(self, bus, am_abs_y, false => and),
            0x2F => am!(self, bus, am_long => and),
            0x3F => am!(self, bus, am_long_x => and),
            0x21 => am!(self, bus, am_indirect_x => and),
            0x31 => amw!(self, bus, am_indirect_y, false => and),
            0x32 => am!(self, bus, am_indirect => and),
            0x27 => am!(self, bus, am_long_indirect => and),
            0x37 => am!(self, bus, am_long_indirect_y => and),
            0x23 => am!(self, bus, am_stack_s => and),
            0x33 => am!(self, bus, am_stack_s_y => and),

            // --- EOR ---
            0x49 => self.eor_imm(bus),
            0x45 => am!(self, bus, am_dp => eor),
            0x55 => am!(self, bus, am_dp_x => eor),
            0x4D => am!(self, bus, am_abs => eor),
            0x5D => amw!(self, bus, am_abs_x, false => eor),
            0x59 => amw!(self, bus, am_abs_y, false => eor),
            0x4F => am!(self, bus, am_long => eor),
            0x5F => am!(self, bus, am_long_x => eor),
            0x41 => am!(self, bus, am_indirect_x => eor),
            0x51 => amw!(self, bus, am_indirect_y, false => eor),
            0x52 => am!(self, bus, am_indirect => eor),
            0x47 => am!(self, bus, am_long_indirect => eor),
            0x57 => am!(self, bus, am_long_indirect_y => eor),
            0x43 => am!(self, bus, am_stack_s => eor),
            0x53 => am!(self, bus, am_stack_s_y => eor),

            // --- BIT ---
            0x89 => self.bit_imm(bus),
            0x24 => am!(self, bus, am_dp => bit),
            0x34 => am!(self, bus, am_dp_x => bit),
            0x2C => am!(self, bus, am_abs => bit),
            0x3C => amw!(self, bus, am_abs_x, false => bit),

            // --- ASL / LSR / ROL / ROR (accumulator + memory RMW) ---
            0x0A => self.asl_a(),
            0x06 => am!(self, bus, am_dp => asl_m),
            0x16 => am!(self, bus, am_dp_x => asl_m),
            0x0E => am!(self, bus, am_abs => asl_m),
            0x1E => amw!(self, bus, am_abs_x, true => asl_m),
            0x4A => self.lsr_a(),
            0x46 => am!(self, bus, am_dp => lsr_m),
            0x56 => am!(self, bus, am_dp_x => lsr_m),
            0x4E => am!(self, bus, am_abs => lsr_m),
            0x5E => amw!(self, bus, am_abs_x, true => lsr_m),
            0x2A => self.rol_a(),
            0x26 => am!(self, bus, am_dp => rol_m),
            0x36 => am!(self, bus, am_dp_x => rol_m),
            0x2E => am!(self, bus, am_abs => rol_m),
            0x3E => amw!(self, bus, am_abs_x, true => rol_m),
            0x6A => self.ror_a(),
            0x66 => am!(self, bus, am_dp => ror_m),
            0x76 => am!(self, bus, am_dp_x => ror_m),
            0x6E => am!(self, bus, am_abs => ror_m),
            0x7E => amw!(self, bus, am_abs_x, true => ror_m),

            // --- INC / DEC (accumulator + memory RMW) ---
            0x1A => self.inc_a(),
            0xE6 => am!(self, bus, am_dp => inc_m),
            0xF6 => am!(self, bus, am_dp_x => inc_m),
            0xEE => am!(self, bus, am_abs => inc_m),
            0xFE => amw!(self, bus, am_abs_x, true => inc_m),
            0x3A => self.dec_a(),
            0xC6 => am!(self, bus, am_dp => dec_m),
            0xD6 => am!(self, bus, am_dp_x => dec_m),
            0xCE => am!(self, bus, am_abs => dec_m),
            0xDE => amw!(self, bus, am_abs_x, true => dec_m),

            // --- index inc / dec ---
            0xE8 => self.inx(),
            0xC8 => self.iny(),
            0xCA => self.dex(),
            0x88 => self.dey(),

            // --- TSB / TRB ---
            0x04 => am!(self, bus, am_dp => tsb),
            0x0C => am!(self, bus, am_abs => tsb),
            0x14 => am!(self, bus, am_dp => trb),
            0x1C => am!(self, bus, am_abs => trb),

            // --- CMP ---
            0xC9 => self.cmp_imm(bus),
            0xC5 => am!(self, bus, am_dp => cmp),
            0xD5 => am!(self, bus, am_dp_x => cmp),
            0xCD => am!(self, bus, am_abs => cmp),
            0xDD => amw!(self, bus, am_abs_x, false => cmp),
            0xD9 => amw!(self, bus, am_abs_y, false => cmp),
            0xCF => am!(self, bus, am_long => cmp),
            0xDF => am!(self, bus, am_long_x => cmp),
            0xC1 => am!(self, bus, am_indirect_x => cmp),
            0xD1 => amw!(self, bus, am_indirect_y, false => cmp),
            0xD2 => am!(self, bus, am_indirect => cmp),
            0xC7 => am!(self, bus, am_long_indirect => cmp),
            0xD7 => am!(self, bus, am_long_indirect_y => cmp),
            0xC3 => am!(self, bus, am_stack_s => cmp),
            0xD3 => am!(self, bus, am_stack_s_y => cmp),

            // --- CPX / CPY ---
            0xE0 => self.cpx_imm(bus),
            0xE4 => am!(self, bus, am_dp => cpx),
            0xEC => am!(self, bus, am_abs => cpx),
            0xC0 => self.cpy_imm(bus),
            0xC4 => am!(self, bus, am_dp => cpy),
            0xCC => am!(self, bus, am_abs => cpy),

            // --- branches ---
            0x10 => self.branch(bus, self.regs.p & flag::N == 0),
            0x30 => self.branch(bus, self.regs.p & flag::N != 0),
            0x50 => self.branch(bus, self.regs.p & flag::V == 0),
            0x70 => self.branch(bus, self.regs.p & flag::V != 0),
            0x90 => self.branch(bus, self.regs.p & flag::C == 0),
            0xB0 => self.branch(bus, self.regs.p & flag::C != 0),
            0xD0 => self.branch(bus, self.regs.p & flag::Z == 0),
            0xF0 => self.branch(bus, self.regs.p & flag::Z != 0),
            0x80 => self.branch(bus, true),
            0x82 => self.brl(bus),

            other => panic!("unimplemented opcode {other:#04x}"),
        }
    }
}
