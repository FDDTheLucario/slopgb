//! Architectural register file.

use crate::model::Model;

/// F-register flag bits.
pub mod flags {
    pub const Z: u8 = 0x80;
    pub const N: u8 = 0x40;
    pub const H: u8 = 0x20;
    pub const C: u8 = 0x10;
}

/// CPU register snapshot. Lower 4 bits of F always read zero; the invariant
/// holds by construction — `f` is private and every setter masks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Registers {
    pub a: u8,
    f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
}

impl Registers {
    /// Register values at PC=0x100 for `model` (no boot ROM execution).
    pub fn post_boot(model: Model) -> Self {
        let s = model.post_boot_state();
        Self {
            a: s.a,
            f: s.f & 0xF0,
            b: s.b,
            c: s.c,
            d: s.d,
            e: s.e,
            h: s.h,
            l: s.l,
            sp: s.sp,
            pc: s.pc,
        }
    }

    /// True power-on register state, before any boot ROM runs: every register
    /// is zero and PC is `0x0000` (the boot ROM's reset vector). Used only by
    /// the opt-in boot-ROM path (`GameBoy::new_with_boot`); `new` keeps
    /// [`Self::post_boot`].
    #[must_use]
    pub fn power_on() -> Self {
        Self {
            a: 0,
            f: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            sp: 0,
            pc: 0,
        }
    }

    /// F register. Lower 4 bits always read zero.
    pub fn f(&self) -> u8 {
        self.f
    }

    /// Set F. The lower 4 bits of the written value are discarded: they do
    /// not exist in hardware (gbctr "Flags register").
    pub fn set_f(&mut self, v: u8) {
        self.f = v & 0xF0;
    }

    pub fn af(&self) -> u16 {
        u16::from_be_bytes([self.a, self.f])
    }

    pub fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }

    pub fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }

    pub fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }

    pub fn set_af(&mut self, v: u16) {
        [self.a, self.f] = v.to_be_bytes();
        self.f &= 0xF0;
    }

    pub fn set_bc(&mut self, v: u16) {
        [self.b, self.c] = v.to_be_bytes();
    }

    pub fn set_de(&mut self, v: u16) {
        [self.d, self.e] = v.to_be_bytes();
    }

    pub fn set_hl(&mut self, v: u16) {
        [self.h, self.l] = v.to_be_bytes();
    }
}

// --- Save state (manual serialization; see `crate::state`) ---
impl Registers {
    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        for b in [
            self.a, self.f, self.b, self.c, self.d, self.e, self.h, self.l,
        ] {
            w.u8(b);
        }
        w.u16(self.sp);
        w.u16(self.pc);
    }
    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.a = r.u8()?;
        self.f = r.u8()?;
        self.b = r.u8()?;
        self.c = r.u8()?;
        self.d = r.u8()?;
        self.e = r.u8()?;
        self.h = r.u8()?;
        self.l = r.u8()?;
        self.sp = r.u16()?;
        self.pc = r.u16()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Registers;

    #[test]
    fn power_on_is_all_zero() {
        let r = Registers::power_on();
        assert_eq!(r.af(), 0, "AF zero at power-on");
        assert_eq!(r.bc(), 0);
        assert_eq!(r.de(), 0);
        assert_eq!(r.hl(), 0);
        assert_eq!(r.sp, 0);
        assert_eq!(r.pc, 0x0000, "PC starts at the reset vector");
    }

    #[test]
    fn pair_accessors_round_trip() {
        let mut r = Registers::default();
        r.set_bc(0x1234);
        r.set_de(0x5678);
        r.set_hl(0x9ABC);
        assert_eq!((r.b, r.c), (0x12, 0x34));
        assert_eq!(r.bc(), 0x1234);
        assert_eq!(r.de(), 0x5678);
        assert_eq!(r.hl(), 0x9ABC);
    }

    #[test]
    fn set_af_masks_f_low_nibble() {
        let mut r = Registers::default();
        r.set_af(0x12FF);
        assert_eq!(r.a, 0x12);
        assert_eq!(r.f(), 0xF0);
        assert_eq!(r.af(), 0x12F0);
    }

    #[test]
    fn f_low_nibble_reads_zero_through_every_public_path() {
        // The invariant "lower 4 bits of F always read zero" must hold by
        // construction: every public way of writing F masks the low nibble.
        let mut r = Registers::default();
        r.set_f(0xFF);
        assert_eq!(r.f() & 0x0F, 0);
        assert_eq!(r.f(), 0xF0);
        r.set_af(0xFFFF);
        assert_eq!(r.f() & 0x0F, 0);
        assert_eq!(r.af() & 0x0F, 0);
        // Through the Cpu's mutable register access too.
        let mut cpu = crate::cpu::Cpu::new(crate::model::Model::Dmg);
        cpu.regs_mut().set_f(0xFF);
        assert_eq!(cpu.regs().f() & 0x0F, 0);
    }
}
