//! Loads, stores, register transfers and `XBA`. Load/store width follows the
//! accumulator (`M`) or index (`X`) flag; transfers take the destination
//! register's width. Behaviour per the WDC W65C816S datasheet and vectors.

use super::*;

impl Cpu {
    // --- register width helpers ---------------------------------------------

    /// Write the accumulator honouring `M`: 16-bit replaces `C`, 8-bit keeps the
    /// high byte (`B`).
    pub(crate) fn set_a(&mut self, value: u16, wide: bool) {
        self.regs.a = if wide {
            value
        } else {
            (self.regs.a & 0xFF00) | (value & 0x00FF)
        };
    }

    /// Write index X; an 8-bit index clears the high byte.
    pub(crate) fn set_x(&mut self, value: u16, wide: bool) {
        self.regs.x = if wide { value } else { value & 0x00FF };
    }

    /// Write index Y; an 8-bit index clears the high byte.
    pub(crate) fn set_y(&mut self, value: u16, wide: bool) {
        self.regs.y = if wide { value } else { value & 0x00FF };
    }

    /// Fetch an immediate operand of accumulator or index width.
    pub(crate) fn imm(&mut self, bus: &mut impl Bus, wide: bool) -> u16 {
        if wide {
            self.fetch16(bus)
        } else {
            self.fetch8(bus) as u16
        }
    }

    // --- loads --------------------------------------------------------------

    /// `LDA` from a resolved address.
    pub(crate) fn lda(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let v = self.read_data(bus, addr, wide, bank0);
        self.set_a(v, wide);
        self.set_nz(v, wide);
    }

    /// `LDX` from a resolved address.
    pub(crate) fn ldx(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.idx16();
        let v = self.read_data(bus, addr, wide, bank0);
        self.set_x(v, wide);
        self.set_nz(v, wide);
    }

    /// `LDY` from a resolved address.
    pub(crate) fn ldy(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.idx16();
        let v = self.read_data(bus, addr, wide, bank0);
        self.set_y(v, wide);
        self.set_nz(v, wide);
    }

    /// `LDA #`.
    pub(crate) fn lda_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.acc16();
        let v = self.imm(bus, wide);
        self.set_a(v, wide);
        self.set_nz(v, wide);
    }

    /// `LDX #`.
    pub(crate) fn ldx_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.idx16();
        let v = self.imm(bus, wide);
        self.set_x(v, wide);
        self.set_nz(v, wide);
    }

    /// `LDY #`.
    pub(crate) fn ldy_imm(&mut self, bus: &mut impl Bus) {
        let wide = self.regs.idx16();
        let v = self.imm(bus, wide);
        self.set_y(v, wide);
        self.set_nz(v, wide);
    }

    // --- stores -------------------------------------------------------------

    /// `STA` to a resolved address.
    pub(crate) fn sta(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        let v = self.regs.a;
        self.write_data(bus, addr, v, wide, bank0);
    }

    /// `STX` to a resolved address.
    pub(crate) fn stx(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.idx16();
        let v = self.regs.x;
        self.write_data(bus, addr, v, wide, bank0);
    }

    /// `STY` to a resolved address.
    pub(crate) fn sty(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.idx16();
        let v = self.regs.y;
        self.write_data(bus, addr, v, wide, bank0);
    }

    /// `STZ` (store zero, accumulator width).
    pub(crate) fn stz(&mut self, bus: &mut impl Bus, addr: u32, bank0: bool) {
        let wide = self.regs.acc16();
        self.write_data(bus, addr, 0, wide, bank0);
    }

    // --- transfers ----------------------------------------------------------

    /// Register-to-register transfers spend one internal cycle.
    pub(crate) fn tax(&mut self) {
        let wide = self.regs.idx16();
        let v = self.regs.a;
        self.set_x(v, wide);
        self.set_nz(self.regs.x, wide);
        self.io();
    }

    pub(crate) fn tay(&mut self) {
        let wide = self.regs.idx16();
        let v = self.regs.a;
        self.set_y(v, wide);
        self.set_nz(self.regs.y, wide);
        self.io();
    }

    pub(crate) fn txa(&mut self) {
        let wide = self.regs.acc16();
        let v = self.regs.x;
        self.set_a(v, wide);
        self.set_nz(self.regs.a, wide);
        self.io();
    }

    pub(crate) fn tya(&mut self) {
        let wide = self.regs.acc16();
        let v = self.regs.y;
        self.set_a(v, wide);
        self.set_nz(self.regs.a, wide);
        self.io();
    }

    pub(crate) fn tsx(&mut self) {
        let wide = self.regs.idx16();
        let v = self.regs.s;
        self.set_x(v, wide);
        self.set_nz(self.regs.x, wide);
        self.io();
    }

    pub(crate) fn txs(&mut self) {
        // TXS moves X into S with no flags. Emulation pins SH to 01.
        self.regs.s = if self.regs.e {
            0x0100 | (self.regs.x & 0x00FF)
        } else {
            self.regs.x
        };
        self.io();
    }

    pub(crate) fn txy(&mut self) {
        let wide = self.regs.idx16();
        let v = self.regs.x;
        self.set_y(v, wide);
        self.set_nz(self.regs.y, wide);
        self.io();
    }

    pub(crate) fn tyx(&mut self) {
        let wide = self.regs.idx16();
        let v = self.regs.y;
        self.set_x(v, wide);
        self.set_nz(self.regs.x, wide);
        self.io();
    }

    /// `TCD`: full 16-bit `C` -> `D`, 16-bit flags.
    pub(crate) fn tcd(&mut self) {
        self.regs.d = self.regs.a;
        self.set_nz(self.regs.d, true);
        self.io();
    }

    /// `TDC`: `D` -> full 16-bit `C`, 16-bit flags.
    pub(crate) fn tdc(&mut self) {
        self.regs.a = self.regs.d;
        self.set_nz(self.regs.a, true);
        self.io();
    }

    /// `TCS`: full 16-bit `C` -> `S`, no flags. Emulation pins SH to 01.
    pub(crate) fn tcs(&mut self) {
        self.regs.s = if self.regs.e {
            0x0100 | (self.regs.a & 0x00FF)
        } else {
            self.regs.a
        };
        self.io();
    }

    /// `TSC`: `S` -> full 16-bit `C`, 16-bit flags.
    pub(crate) fn tsc(&mut self) {
        self.regs.a = self.regs.s;
        self.set_nz(self.regs.a, true);
        self.io();
    }

    /// `XBA`: swap the accumulator's two bytes; flags on the new low byte (8-bit).
    pub(crate) fn xba(&mut self) {
        self.regs.a = self.regs.a.rotate_right(8);
        self.set_nz(self.regs.a & 0x00FF, false);
        self.io();
        self.io();
    }
}
