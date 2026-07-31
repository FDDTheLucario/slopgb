//! Unit tests split out of `model.rs` for the file-size rule;
//! compiled as `super::tests` via the `#[path]` attribute.

use super::*;

const ALL: [Model; 7] = [
    Model::Dmg0,
    Model::Dmg,
    Model::Mgb,
    Model::Sgb,
    Model::Sgb2,
    Model::Cgb,
    Model::Agb,
];

#[test]
fn is_cgb_only_for_color_models() {
    for m in ALL {
        assert_eq!(m.is_cgb(), matches!(m, Model::Cgb | Model::Agb), "{m:?}");
    }
}

/// `is_sgb` gates the PPU's SGB view, the coprocessor slot, and the save-state
/// cross-model check — it must name exactly the two Super Game Boy models.
#[test]
fn is_sgb_only_for_super_game_boy_models() {
    for m in ALL {
        assert_eq!(m.is_sgb(), matches!(m, Model::Sgb | Model::Sgb2), "{m:?}");
    }
}

/// Register values straight from the boot_regs-* assertions.
#[test]
fn cpu_registers_match_boot_regs_tests() {
    let cases: [(Model, [u8; 8]); 7] = [
        (
            Model::Dmg0,
            [0x01, 0x00, 0xFF, 0x13, 0x00, 0xC1, 0x84, 0x03],
        ),
        (Model::Dmg, [0x01, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D]),
        (Model::Mgb, [0xFF, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D]),
        (Model::Sgb, [0x01, 0x00, 0x00, 0x14, 0x00, 0x00, 0xC0, 0x60]),
        (
            Model::Sgb2,
            [0xFF, 0x00, 0x00, 0x14, 0x00, 0x00, 0xC0, 0x60],
        ),
        (Model::Cgb, [0x11, 0x80, 0x00, 0x00, 0x00, 0x08, 0x00, 0x7C]),
        (Model::Agb, [0x11, 0x00, 0x01, 0x00, 0x00, 0x08, 0x00, 0x7C]),
    ];
    for (m, [a, f, b, c, d, e, h, l]) in cases {
        let s = m.post_boot_state();
        assert_eq!(
            [s.a, s.f, s.b, s.c, s.d, s.e, s.h, s.l],
            [a, f, b, c, d, e, h, l],
            "{m:?}"
        );
        assert_eq!(s.sp, 0xFFFE, "{m:?}");
        assert_eq!(s.pc, 0x0100, "{m:?}");
    }
}

/// The DIV counter must sit at an M-cycle boundary (≡ 0 mod 4): CPU and
/// DIV share the crystal from reset and DIV writes land on cycle ends.
#[test]
fn div_counter_is_m_cycle_aligned() {
    for m in ALL {
        assert_eq!(m.post_boot_state().div_counter % 4, 0, "{m:?}");
    }
}

/// Encode the boot_div oracle: read k M-cycles after hand-off (each
/// M-cycle ticks DIV by 4 before the access) must land "immediately
/// after an increment" of the expected DIV value.
#[test]
fn div_counter_satisfies_boot_div_read_windows() {
    // (model, M-cycle of the first DIV read, expected DIV high byte)
    // boot_div read cycle = 5 (header NOP+JP) + nops + 3 (LDH).
    let cases = [
        (Model::Dmg0, 5 + 45 + 3, 0x19u16),
        (Model::Dmg, 5 + 6 + 3, 0xAC),
        (Model::Mgb, 5 + 6 + 3, 0xAC),
        (Model::Cgb, 5 + 27 + 3, 0x27),
        (Model::Agb, 5 + 26 + 3, 0x27),
    ];
    for (m, mcycles, hi) in cases {
        let r1 = m.post_boot_state().div_counter.wrapping_add(4 * mcycles);
        assert_eq!(r1 >> 8, hi, "{m:?}: first read high byte");
        assert!(r1 & 0xFF < 4, "{m:?}: read not right after the increment");
        // The third read (M1 + 127, the 56-nop variant) lands
        // "immediately before" an increment and must still read hi+1.
        assert_eq!((r1 + 508) >> 8, hi + 1, "{m:?}: phase probe value");
        assert!(
            (r1 + 508) & 0xFF >= 0xFC,
            "{m:?}: phase probe not right before the increment"
        );
    }
}

/// SGB DIV base: calibrated from boot_div-S (zeros=443, first DIV read
/// at M-cycle 41 must observe $D9 right after an increment → hand-off
/// counter $D85C) and boot_div2-S (zeros=439, read at M-cycle 45 →
/// $D84C); both solve to base $D170.
#[test]
fn sgb_div_base_matches_both_checksum_roms() {
    for m in [Model::Sgb, Model::Sgb2] {
        let base = m.post_boot_state().div_counter;
        assert_eq!(base, 0xD170, "{m:?}");
        assert_eq!(base.wrapping_add(4 * 443), 0xD85C);
        assert_eq!(base.wrapping_add(4 * 439), 0xD84C);
    }
}

/// hwio invariants the interconnect relies on.
#[test]
fn hwio_tables_are_well_formed() {
    for m in ALL {
        let s = m.post_boot_state();
        let addr_of = |a: u16| s.hwio.iter().position(|&(x, _)| x == a);
        let value_of = |a: u16| s.hwio.iter().find(|&&(x, _)| x == a).map(|&(_, v)| v);
        // NR52 power-on must precede every other APU write.
        let nr52 = addr_of(0xFF26).expect("NR52 in table");
        for (i, &(a, _)) in s.hwio.iter().enumerate() {
            if (0xFF10..=0xFF25).contains(&a) || (0xFF30..=0xFF3F).contains(&a) {
                assert!(i > nr52, "{m:?}: APU write {a:04X} before NR52");
            }
        }
        // LCDC must NOT be in the table (the interconnect performs the
        // LCD warmup itself), DMA reg must be.
        assert!(addr_of(0xFF40).is_none(), "{m:?}: LCDC in hwio table");
        assert!(addr_of(0xFF46).is_some(), "{m:?}: FF46 missing");
        assert_eq!(value_of(0xFF0F), Some(0xE1), "{m:?}: IF");
        assert_eq!(value_of(0xFF47), Some(0xFC), "{m:?}: BGP");
        // P1 select bits: DMG-family selects both columns (reads $CF),
        // SGB/CGB deselect both (reads $FF).
        let p1 = value_of(0xFF00).unwrap();
        match m {
            Model::Dmg0 | Model::Dmg | Model::Mgb => assert_eq!(p1, 0x00, "{m:?}"),
            _ => assert_eq!(p1, 0x30, "{m:?}"),
        }
    }
}

/// LCD phase windows derived from the boot_hwio LY/STAT reads.
#[test]
fn lcd_phase_is_inside_the_boot_hwio_window() {
    for m in ALL {
        let p0 = m.post_boot_state().lcd_phase_dots;
        assert!(p0 < 70224, "{m:?}: not a frame position");
        assert_eq!(p0 % 4, 0, "{m:?}: not M-cycle aligned");
    }
    // DMG ABC/MGB/SGB: STAT=$80 at +4556 dots, LY=$0A at +4760.
    for m in [Model::Dmg, Model::Mgb] {
        let p0 = m.post_boot_state().lcd_phase_dots;
        assert!((70028..70224).contains(&p0) || p0 < 8, "{m:?}: {p0}");
    }
    // Inside that window the gbmicrotest poweron_stat/ly/oam/vram
    // comment tables (captured on DMG-CPU-08) pin the hand-off to
    // exactly 60 dots before the start of line 0: 70224 - 60 = 70164.
    // SGB/SGB2 share the DMG boot timing base (no LCD oracle of their
    // own — boot_hwio-S masks LY/STAT).
    for m in [Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2] {
        assert_eq!(m.post_boot_state().lcd_phase_dots, 70164, "{m:?}");
    }
    // DMG0: STAT=$83 (mode 3, line 1) at +4556, LY=$01 at +4760.
    let p0 = Model::Dmg0.post_boot_state().lcd_phase_dots;
    let pos = (p0 + 4556) % 70224;
    assert!((456 + 84..456 + 256).contains(&pos), "DMG0: {p0}");
    // CGB/AGB: no mooneye oracle (boot_hwio masks LY/STAT), but real
    // hardware hands off inside vblank with LY in $90-$94
    // (gbdev/pandocs#426). The table holds the DMG-cart hand-off:
    // the CGB-cart point (gambatte initstate 144*456+164, AGB +4)
    // plus the 0x7D8-dot DMG-compat boot tail (see
    // Interconnect::apply_post_boot_state).
    for m in [Model::Cgb, Model::Agb] {
        let p0 = m.post_boot_state().lcd_phase_dots;
        assert!((144 * 456..149 * 456).contains(&p0), "{m:?}: {p0}");
    }
    assert_eq!(
        Model::Cgb.post_boot_state().lcd_phase_dots,
        144 * 456 + 164 + 0x7D8
    );
    assert_eq!(
        Model::Agb.post_boot_state().lcd_phase_dots,
        144 * 456 + 164 + 4 + 0x7D8
    );
}
