//! Unit tests split out of `registers.rs` for the file-size rule;
//! compiled as `super::tests` via the `#[path]` attribute.

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
