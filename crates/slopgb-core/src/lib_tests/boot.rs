//! `GameBoy` construction: post-boot register/state, opt-in boot ROM.

use super::*;

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

/// Hand-off palette RAM splits on the cart's CGB flag, not on the hardware.
/// A DMG-flagged cart takes the compatibility palettes (BCPS $C8 / OCPS $D0,
/// mooneye `misc/boot_hwio-C`, itself a DMG-flagged cart); a CGB-flagged cart
/// skips that code entirely and inherits the boot logo's own palette state
/// (BCPS $C8 / OCPS $C1), byte-for-byte what `bootroms/cgb_boot.bin` leaves.
#[test]
fn cgb_flagged_cart_keeps_the_boot_logo_palettes() {
    let words = |ram: &[u8; 64], i: usize| u16::from(ram[i * 2]) | (u16::from(ram[i * 2 + 1]) << 8);

    let gb = GameBoy::new(Model::Cgb, rom_with_cgb_flag(0x80)).unwrap();
    let (bg, obj) = gb.cgb_palette_ram();
    assert_eq!(
        [0, 1, 2, 3].map(|i| words(bg, i)),
        [0x0000, 0x5294, 0x2108, 0xFFFF],
        "CGB cart BG palette 0"
    );
    assert!(
        (4..32).all(|i| words(bg, i) == 0x7FFF),
        "CGB cart BG palettes 1-7 are white"
    );
    assert_eq!(words(obj, 0), 0xFF00, "CGB cart OBJ palette 0 colour 0");
    assert!(
        (1..32).all(|i| words(obj, i) == 0xFFFF),
        "CGB cart OBJ RAM keeps its power-on fill past the one cleared byte"
    );
    assert_eq!((gb.debug_read(0xFF68), gb.debug_read(0xFF6A)), (0xC8, 0xC1));

    let gb = GameBoy::new(Model::Cgb, rom_with_cgb_flag(0x00)).unwrap();
    let (bg, obj) = gb.cgb_palette_ram();
    assert_eq!(
        [0, 1, 2, 3].map(|i| words(bg, i)),
        [0x7FFF, 0x1BEF, 0x6180, 0x0000],
        "DMG cart BG compat palette"
    );
    assert_eq!(
        [0, 1, 2, 3].map(|i| words(obj, i)),
        [0x7FFF, 0x421F, 0x1CF2, 0x0000],
        "DMG cart OBJ compat palette"
    );
    assert_eq!((gb.debug_read(0xFF68), gb.debug_read(0xFF6A)), (0xC8, 0xD0));
}
