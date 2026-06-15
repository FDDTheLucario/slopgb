//! `interconnect_tests` — boot tests (split for file size).

use super::*;

/// The boot ROM leaves its logo graphics in VRAM at hand-off: the
/// header logo decompressed into tiles $01-$18 (even bytes — one
/// bitplane), the (R) trademark tile at $19, and on DMG-family models
/// the two logo tile-map rows (gambatte initstate.cpp setInitialVram
/// hardware dump; the expected bytes below are that dump's prefix for
/// the standard Nintendo logo). mealybug m3_scx_low_3_bits renders
/// the leftover (R) tile.
#[test]
fn post_boot_vram_boot_logo_leftovers() {
    // The fixed logo applies regardless of the cart header (the boot
    // ROM locks up on a mismatch, so hardware VRAM only ever holds
    // the canonical image; gambatte's test carts have no header logo
    // and their references still show it).
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        b.apply_post_boot_state();
        // $CE -> F0 F0 FC FC, $ED -> FC FC F3 F3 (even bytes).
        for (off, want) in [
            (0x00u16, 0xF0u8),
            (0x02, 0xF0),
            (0x04, 0xFC),
            (0x06, 0xFC),
            (0x08, 0xFC),
            (0x0A, 0xFC),
            (0x0C, 0xF3),
            (0x0E, 0xF3),
            // $66 -> 3C 3C 3C 3C twice.
            (0x10, 0x3C),
            (0x16, 0x3C),
            (0x18, 0x3C),
            (0x1E, 0x3C),
        ] {
            assert_eq!(
                b.ppu().vram_read_raw(0x8010 + off),
                want,
                "{model:?} +{off:#x}"
            );
        }
        assert_eq!(b.ppu().vram_read_raw(0x8011), 0, "high bitplane untouched");
        // (R) trademark tile $19.
        assert_eq!(b.ppu().vram_read_raw(0x8190), 0x3C, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x8192), 0x42, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x8194), 0xB9, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x819E), 0x3C, "{model:?}");
        // The logo tile-map rows are deliberately not installed
        // (see install_boot_logo_vram): the pinned gambatte
        // reference PNGs encode a cleared map.
        assert_eq!(b.ppu().vram_read_raw(0x9904), 0x00, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x9910), 0x00, "{model:?}");
    }
}

/// Real DMG-family WRAM powers up in the $00/$FF half-page stripe
/// pattern, mirrored into D000-DFFF (gambatte-core mem_dumps.h
/// `setInitialDmgWram` base pattern; see `install_power_on_wram`).
/// The $DE00 page reading $FF is what the gambatte oamdma_srcFE00_*
/// expectations encode (OAM DMA from $FE00 reads the $DE00 echo).
/// CGB WRAM stays zero-filled.
#[test]
fn post_boot_wram_power_on_pattern() {
    for model in [Model::Dmg0, Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2] {
        let b = booted(model);
        for (addr, want) in [
            (0xC000u16, 0x00u8),
            (0xC0FF, 0x00),
            (0xC100, 0xFF),
            (0xC1FF, 0xFF),
            (0xC2A0, 0x00),
            (0xC700, 0xFF),
            // Polarity inverts across the 2 KiB half...
            (0xC800, 0xFF),
            (0xC900, 0x00),
            (0xCE42, 0xFF),
            (0xCF00, 0x00),
            // ...and D000-DFFF mirrors C000-CFFF.
            (0xD000, 0x00),
            (0xD100, 0xFF),
            (0xDE00, 0xFF),
            (0xDEFF, 0xFF),
            (0xDF00, 0x00),
        ] {
            assert_eq!(b.peek(addr), want, "{model:?} {addr:04X}");
        }
    }
    let b = booted(Model::Cgb);
    for addr in [0xC100u16, 0xC800, 0xDE00] {
        assert_eq!(b.peek(addr), 0x00, "CGB WRAM zero-filled at {addr:04X}");
    }
}

/// The CGB boot ROM hands a CGB-flagged cart off 0x7D8 T-cycles
/// earlier than a DMG cart (the DMG-compat palette tail), shifting
/// DIV and the LCD phase together: DIV $1E9C pinned by gambatte
/// div/start_inc_1/2 (FF04 reads $1E at +96 T immediately before
/// the increment to $1F00) and tima/tc00_start_1/2 (first TIMA
/// increment, DIV bit-9 edge, exactly between rounds at +356), LY
/// $90 by display_startstate ly/stat. The DMG-cart side keeps
/// mooneye misc/boot_div-cgbABCDE's $2674 with the LCD 0x7D8 dots
/// further on (line 148, still in the pandocs#426 LY window).
#[test]
fn post_boot_cgb_cart_hands_off_earlier_than_dmg_cart() {
    let mut dmg_cart = booted(Model::Cgb);
    assert_eq!(dmg_cart.timer.div_counter(), 0x2674);
    assert_eq!(dmg_cart.read(0xFF44), 148);

    let mut cgb_cart = ic_cgb_mode();
    cgb_cart.apply_post_boot_state();
    let div = cgb_cart.timer.div_counter();
    assert_eq!(div, 0x1E9C);
    assert_eq!(div, 0x2674 - 0x7D8);
    // div/start_inc oracle: the read 24 M-cycles in.
    assert_eq!((div + 96) >> 8, 0x1E, "round 1 high byte");
    assert!(
        (div + 96) & 0xFF >= 0xFC,
        "immediately before the increment"
    );
    assert_eq!((div + 100) >> 8, 0x1F, "round 2 high byte");
    // tc00_start oracle: bit-9 falling edge between the rounds.
    assert_eq!((div + 356) % 0x400, 0);
    assert_eq!(cgb_cart.read(0xFF44), 144);
}

#[test]
fn post_boot_io_dmg() {
    let mut b = booted(Model::Dmg);
    assert_eq!(b.read(0xFF00), 0xCF);
    assert_eq!(b.read(0xFF02), 0x7E);
    assert_eq!(b.read(0xFF0F), 0xE1);
    assert_eq!(b.read(0xFF26), 0xF1, "channel 1 beep still on");
    assert_eq!(b.read(0xFF11), 0xBF);
    assert_eq!(b.read(0xFF12), 0xF3);
    assert_eq!(b.read(0xFF24), 0x77);
    assert_eq!(b.read(0xFF25), 0xF3);
    assert_eq!(b.read(0xFF40), 0x91);
    assert_eq!(b.read(0xFF47), 0xFC);
    assert_eq!(b.read(0xFF46), 0xFF);
    assert_eq!(b.read(0xFFFF), 0x00);
}

#[test]
fn post_boot_io_sgb() {
    let mut b = booted(Model::Sgb);
    assert_eq!(b.read(0xFF00), 0xFF, "P1 columns deselected on SGB");
    assert_eq!(b.read(0xFF26), 0xF0, "no boot beep on SGB");
}

#[test]
fn post_boot_io_cgb_dmg_cart() {
    let mut b = booted(Model::Cgb);
    assert_eq!(b.read(0xFF00), 0xFF);
    assert_eq!(b.read(0xFF02), 0x7E, "fast-clock bit absent in DMG mode");
    assert_eq!(b.read(0xFF26), 0xF1);
    assert_eq!(b.read(0xFF46), 0x00);
    assert_eq!(b.read(0xFF4D), 0xFF);
    assert_eq!(b.read(0xFF4F), 0xFE);
    assert_eq!(b.read(0xFF55), 0xFF);
    assert_eq!(b.read(0xFF68), 0xC8, "BCPS boot leftover");
    assert_eq!(b.read(0xFF69), 0xFF, "BCPD unreadable in DMG mode");
    assert_eq!(b.read(0xFF6A), 0xD0, "OCPS boot leftover");
    assert_eq!(b.read(0xFF6C), 0xFF, "OPRI = DMG-style priority");
    assert_eq!(b.read(0xFF70), 0xFF);
    assert_eq!(b.read(0xFF74), 0xFF);
    assert_eq!(b.read(0xFF75), 0x8F);
}

/// For DMG carts whose licensee is not Nintendo (no title-hash lookup),
/// the CGB boot ROM installs the *default* compatibility palette
/// combination — BG palette 0 = $7FFF/$1BEF/$6180/$0000, OBJ palettes 0
/// and 1 = $7FFF/$421F/$1CF2/$0000 (Pan Docs "Compatibility palettes";
/// SameBoy BootROMs/cgb_boot.asm default combination OBJ0=4, OBJ1=4,
/// BG=29). Pins that the BG table differs from the OBJ table and that
/// *both* OBJ slots receive it.
#[test]
fn post_boot_cgb_compat_palettes_are_boot_defaults() {
    fn le_bytes(table: [u16; 4]) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (i, c) in table.into_iter().enumerate() {
            [out[2 * i], out[2 * i + 1]] = c.to_le_bytes();
        }
        out
    }
    for model in [Model::Cgb, Model::Agb] {
        let b = booted(model);
        let (bg, obj) = b.ppu.palette_ram();
        assert_eq!(
            bg[..8],
            le_bytes([0x7FFF, 0x1BEF, 0x6180, 0x0000]),
            "{model:?} BG palette 0"
        );
        let obj_table = le_bytes([0x7FFF, 0x421F, 0x1CF2, 0x0000]);
        assert_eq!(obj[..8], obj_table, "{model:?} OBJ palette 0");
        assert_eq!(obj[8..16], obj_table, "{model:?} OBJ palette 1");
    }
}

#[test]
fn post_boot_io_cgb_mode_cart() {
    let mut rom = test_rom();
    rom[0x143] = 0x80;
    let mut b = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
    b.apply_post_boot_state();
    assert_eq!(b.read(0xFF4D), 0x7E);
    assert_eq!(b.read(0xFF02), 0x7C, "CGB-mode SC has the fast-clock bit");
    assert_eq!(b.read(0xFF6C), 0xFE, "OPRI = OAM index priority");
    assert_eq!(b.read(0xFF70), 0xF8);
    assert_eq!(b.read(0xFF56), 0x3E, "RP idle, not receiving");
}

/// Replicate acceptance/boot_div-dmgABCmgb: DIV reads at M-cycles 14,
/// 78, 141, 205, 269 and 334 after hand-off observe AC AD AD AE AF B1.
#[test]
fn post_boot_div_phase_dmg() {
    let mut b = booted(Model::Dmg);
    let mut cycle = 0u32;
    let mut read_at = |b: &mut Interconnect, m: u32| {
        while cycle + 1 < m {
            b.tick();
            cycle += 1;
        }
        cycle += 1;
        b.read(0xFF04)
    };
    let got = [14, 78, 141, 205, 269, 334].map(|m| read_at(&mut b, m));
    assert_eq!(got, [0xAC, 0xAD, 0xAD, 0xAE, 0xAF, 0xB1]);
}

/// SGB DIV depends on the header bits: an all-zero header yields 731
/// zero bits in the transferred packets -> DIV base + 4*731.
#[test]
fn post_boot_div_sgb_header_dependence() {
    let mut b = booted(Model::Sgb);
    // test_rom() header region 0x104-0x14F is all zeros: payload zeros =
    // 6 * 15 * 8 = 720, command bytes F1/F3/F5/F7/F9/FB add 11.
    assert_eq!(sgb_header_zero_bits(b.cartridge()), 731);
    // div = 0xD170 + 4 * 731 = 0xDCDC; the first read observes +4.
    assert_eq!(b.read(0xFF04), 0xDC);
}

/// Replicate the LY/STAT bytes of boot_hwio-dmgABCmgb: STAT read at
/// M-cycle 1139 is $80 (mode 0, line 9), LY read at 1190 is $0A.
#[test]
fn post_boot_lcd_phase_dmg() {
    let mut b = booted(Model::Dmg);
    ticks(&mut b, 1138);
    assert_eq!(b.read(0xFF41), 0x80);
    let mut b = booted(Model::Dmg);
    ticks(&mut b, 1189);
    assert_eq!(b.read(0xFF44), 0x0A);
}

/// boot_hwio-dmg0: STAT $83 (mode 3, line 1), LY $01.
#[test]
fn post_boot_lcd_phase_dmg0() {
    let mut b = booted(Model::Dmg0);
    ticks(&mut b, 1138);
    assert_eq!(b.read(0xFF41), 0x83);
    let mut b = booted(Model::Dmg0);
    ticks(&mut b, 1189);
    assert_eq!(b.read(0xFF44), 0x01);
}

/// The IF value survives until boot_hwio's read at M-cycle 285 (no
/// stray STAT/vblank bits from the warmed-up PPU).
#[test]
fn post_boot_if_stable() {
    for model in [Model::Dmg0, Model::Dmg, Model::Sgb, Model::Cgb] {
        let mut b = booted(model);
        ticks(&mut b, 284);
        assert_eq!(b.read(0xFF0F), 0xE1, "{model:?}");
    }
}
