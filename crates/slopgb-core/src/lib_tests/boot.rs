//! `GameBoy` construction: post-boot register/state, opt-in boot ROM.

use super::*;

/// Post-C3-flip default guard: every production `GameBoy::new` must
/// construct on the coherent EAGER-value clock — `leading_edge_reads` ON.
/// Needs no ROM bundle, always runs.
#[test]
fn production_new_is_c3_eager_default() {
    for model in [Model::Dmg, Model::Cgb, Model::Agb] {
        let gb = GameBoy::new(model, rom_with_cgb_flag(0x00)).unwrap();
        assert!(
            gb.reclock_flags(),
            "{model:?}: production GameBoy::new must be C3 eager (leading_edge ON)"
        );
    }
}

/// Pan Docs "CPU registers" (Power-Up Sequence): on CGB/AGB hardware
/// the boot ROM hands a CGB-flagged cart off with DE=$FF56 HL=$000D;
/// a DMG cart gets DE=$0008 HL=$007C (mooneye misc/boot_regs-cgb/-A —
/// every mooneye ROM is DMG-flagged). A/F/B/C are cart-independent:
/// AGB's extra `inc b` gives B=$01/F=$00 for both cart kinds.
#[test]
fn cgb_flagged_cart_boot_regs() {
    for (model, af, bc) in [(Model::Cgb, 0x1180, 0x0000), (Model::Agb, 0x1100, 0x0100)] {
        let gb = GameBoy::new(model, rom_with_cgb_flag(0x80)).unwrap();
        let r = gb.cpu_regs();
        assert_eq!(r.af(), af, "{model:?} CGB cart AF");
        assert_eq!(r.bc(), bc, "{model:?} CGB cart BC");
        assert_eq!(r.de(), 0xFF56, "{model:?} CGB cart DE");
        assert_eq!(r.hl(), 0x000D, "{model:?} CGB cart HL");

        let gb = GameBoy::new(model, rom_with_cgb_flag(0x00)).unwrap();
        let r = gb.cpu_regs();
        assert_eq!(r.af(), af, "{model:?} DMG cart AF");
        assert_eq!(r.bc(), bc, "{model:?} DMG cart BC");
        assert_eq!(r.de(), 0x0008, "{model:?} DMG cart DE");
        assert_eq!(r.hl(), 0x007C, "{model:?} DMG cart HL");
    }
}

/// Boot-ROM task 5: `new_with_boot` runs from the boot ROM in power-on state.
#[test]
fn new_with_boot_starts_at_power_on() {
    let boot: Vec<u8> = (0..0x100u16).map(|i| (i as u8) ^ 0xC3).collect();
    let gb = GameBoy::new_with_boot(Model::Dmg, write_c000_rom(), boot.clone()).unwrap();
    assert_eq!(gb.cpu_regs().pc, 0x0000, "boots from the reset vector");
    assert_eq!(gb.cpu_regs().sp, 0, "power-on SP");
    assert!(gb.boot_active(), "boot ROM mapped");
    assert_eq!(
        gb.debug_read(0x0000),
        boot[0],
        "first instruction is from the boot ROM"
    );
    assert_eq!(
        gb.debug_read(0xFF40),
        0x00,
        "LCD off at power-on (the boot ROM turns it on)"
    );
}

/// A wrong-size boot ROM cannot be mapped: `new_with_boot` ignores it and falls
/// back to the post-boot install (a valid machine, `boot_active` false), rather
/// than running from a half-mapped, broken power-on state.
#[test]
fn new_with_boot_wrong_size_falls_back_to_post_boot() {
    let direct = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    for bad in [0usize, 0x80, 0x200, 0x900] {
        let gb = GameBoy::new_with_boot(Model::Dmg, write_c000_rom(), vec![0u8; bad]).unwrap();
        assert!(!gb.boot_active(), "wrong-size ({bad}) boot ROM not mapped");
        let (r, d) = (gb.cpu_regs(), direct.cpu_regs());
        assert_eq!(
            (r.af(), r.bc(), r.de(), r.hl(), r.sp, r.pc),
            (d.af(), d.bc(), d.de(), d.hl(), d.sp, d.pc),
            "falls back to the exact post-boot register state ({bad})"
        );
    }
    // CGB class wants 2304 B: a 256 B (DMG-size) image is wrong here too.
    let gb = GameBoy::new_with_boot(Model::Cgb, write_c000_rom(), vec![0u8; 0x100]).unwrap();
    assert!(!gb.boot_active(), "256 B boot ROM is wrong for a CGB model");
}

/// Boot-ROM task 6 (golden guard): `new` (no boot ROM) is unchanged — no boot
/// ROM mapped, post-boot entry + registers, exactly as before this feature.
#[test]
fn new_without_boot_is_unchanged() {
    let gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    assert!(!gb.boot_active(), "no boot ROM mapped on the default path");
    let r = gb.cpu_regs();
    let pb = Registers::post_boot(Model::Dmg);
    assert_eq!(r.pc, 0x0100, "starts post-boot at the cart entry");
    assert_eq!(
        (r.af(), r.bc(), r.de(), r.hl(), r.sp, r.pc),
        (pb.af(), pb.bc(), pb.de(), pb.hl(), pb.sp, pb.pc),
        "post-boot register state unchanged"
    );
}
