//! Architectural register file.

use crate::model::Model;

/// F-register flag bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags;

impl Flags {
    pub const Z: u8 = 0x80;
    pub const N: u8 = 0x40;
    pub const H: u8 = 0x20;
    pub const C: u8 = 0x10;
}

/// CPU register snapshot. Lower 4 bits of F always read zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
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
            f: s.f,
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

#[cfg(test)]
mod tests {
    use super::Registers;

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
        assert_eq!(r.f, 0xF0);
        assert_eq!(r.af(), 0x12F0);
    }
}
