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
            0x42 => self.wdm(bus),

            other => panic!("unimplemented opcode {other:#04x}"),
        }
    }
}
