//! `Settings` value-logic tests (defaults, model resolution, the exception
//! mask, palette math, formats), split out of `options_tests.rs` to keep it
//! under the 1000-line cap.

use super::*;

// --- Task 1: Settings defaults ----------------------------------------------

#[test]
fn settings_default_matches_spec() {
    let d = Settings::default();
    assert_eq!(d.model, ModelChoice::Auto);
    assert_eq!(d.volume, 1.0);
    assert_eq!(d.ff_speed, 10);
    assert_eq!(d.framerate_limit, 0);
    assert!(!d.show_framerate);
    assert!(d.lowercase_disasm);
    assert!(!d.lowercase_hex);
    assert!(d.show_clocks);
    assert!(!d.freeze_recent);
    assert!(!d.pause_on_focus_loss);
    // bgb shows the debugger on Esc by default (BUG-1) — the slopgb-faithful default.
    assert!(d.esc_shows_debugger);
    assert_eq!(d.scheme, 0);
    assert_eq!(d.dmg_palette, SCHEMES[0].colors);
    // The default scheme is bgb's pale-green LCD ("BGB 0.3"), decoded from
    // bgb.ini Color0..3 (stored BGR) — so a fresh slopgb (and its no-ROM blank
    // screen) looks like bgb. The lightest shade is the captured #E8FCCC.
    assert_eq!(SCHEMES[0].name, "BGB 0.3");
    assert_eq!(d.dmg_palette[0], 0x00E8_FCCC);
    // Exceptions: bgb ships with "break on invalid opcode" checked, the rest off.
    assert!(d.break_invalid_op);
    assert!(!d.break_ld_b_b);
    assert!(!d.break_echo_ram);
    assert!(!d.break_lcd_off_vblank);
    // Boot ROMs: off + no paths by default (golden-safe — post-boot install).
    assert!(!d.bootroms_enabled);
    assert!(d.bootrom_dmg.is_empty() && d.bootrom_gbc.is_empty() && d.bootrom_sgb.is_empty());
}

#[test]
fn exception_mask_maps_settings_to_core_bits() {
    use slopgb_core::{EXC_ECHO_RAM, EXC_INVALID_OPCODE, EXC_LCD_OFF_VBLANK, EXC_LD_B_B};
    // Default = invalid-opcode only.
    assert_eq!(Settings::default().exception_mask(), EXC_INVALID_OPCODE);
    // Nothing armed → 0 (golden-safe / inert).
    let none = Settings {
        break_invalid_op: false,
        ..Settings::default()
    };
    assert_eq!(none.exception_mask(), 0);
    // All four armed → all four bits.
    let all = Settings {
        break_ld_b_b: true,
        break_invalid_op: true,
        break_echo_ram: true,
        break_lcd_off_vblank: true,
        ..Settings::default()
    };
    assert_eq!(
        all.exception_mask(),
        EXC_LD_B_B | EXC_INVALID_OPCODE | EXC_ECHO_RAM | EXC_LCD_OFF_VBLANK
    );
}

#[test]
fn model_choice_from_option_maps_preference() {
    use slopgb_core::Model;
    // The persistent --model preference seeds the dialog: None → Auto (bgb
    // default; never force-switches on Apply), explicit models → their radio.
    assert_eq!(ModelChoice::from_option(None), ModelChoice::Auto);
    assert_eq!(ModelChoice::from_option(Some(Model::Dmg)), ModelChoice::Dmg);
    assert_eq!(ModelChoice::from_option(Some(Model::Cgb)), ModelChoice::Cgb);
    assert_eq!(
        ModelChoice::from_option(Some(Model::Agb)),
        ModelChoice::Cgb,
        "AGB is CGB-family"
    );
}

#[test]
fn model_choice_resolve_maps_policies() {
    use slopgb_core::Model;
    // Forcing choices ignore the ROM header.
    let none = &[0u8; 0][..];
    assert_eq!(ModelChoice::Dmg.resolve(none), (Model::Dmg, false));
    assert_eq!(ModelChoice::Sgb.resolve(none), (Model::Sgb, false));
    assert_eq!(ModelChoice::Sgb2.resolve(none), (Model::Sgb2, false));
    // "GBC + initial SGB border" = a CGB machine plus the border-overlay flag.
    assert_eq!(ModelChoice::CgbBorder.resolve(none), (Model::Cgb, true));

    // A CGB-flagged, SGB-capable header.
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = 0xC0; // CGB only
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33; // SGB unlock (both bytes)
    // "prefer SGB" picks SGB when the header unlocks it...
    assert_eq!(ModelChoice::AutoSgb.resolve(&rom), (Model::Sgb, false));
    // ...while "prefer GBC" / "Gameboy or GBC" ignore SGB → CGB here.
    assert_eq!(ModelChoice::Auto.resolve(&rom), (Model::Cgb, false));
    assert_eq!(ModelChoice::AutoNoSgb.resolve(&rom), (Model::Cgb, false));
    // Without the SGB unlock bytes, prefer-SGB falls back to auto (DMG here).
    assert_eq!(
        ModelChoice::AutoSgb.resolve(&vec![0u8; 0x8000]),
        (Model::Dmg, false)
    );
}

#[test]
fn screenshot_format_ext_next_and_key_roundtrip() {
    use crate::windows::options::ScreenshotFormat;
    assert_eq!(ScreenshotFormat::Bmp.ext(), "bmp");
    assert_eq!(ScreenshotFormat::Png.ext(), "png");
    assert_eq!(ScreenshotFormat::Bmp.next(), ScreenshotFormat::Png);
    assert_eq!(ScreenshotFormat::Png.next(), ScreenshotFormat::Bmp);
    assert_eq!(ScreenshotFormat::from_key("png"), ScreenshotFormat::Png);
    assert_eq!(ScreenshotFormat::from_key("garbage"), ScreenshotFormat::Bmp);
}

#[test]
fn palette_0_31_display_matches_captured_bgb() {
    // Captured from real bgb (docs/bgb-reference/options/options-gbcolors-031.png):
    // the lightest BGB-0.3 colour 232/252/204 reads 29/31/25 with "0-31 numbers"
    // on — i.e. v8 >> 3.
    let mut s = Settings::default();
    s.select_scheme(0); // BGB 0.3: colour 0 = 0x00E8FCCC = 232,252,204
    s.palette_edit_shade = 0;
    s.palette_0_31 = false;
    assert_eq!(
        [
            s.palette_channel_display(0),
            s.palette_channel_display(1),
            s.palette_channel_display(2)
        ],
        [232, 252, 204]
    );
    s.palette_0_31 = true;
    assert_eq!(
        [
            s.palette_channel_display(0),
            s.palette_channel_display(1),
            s.palette_channel_display(2)
        ],
        [29, 31, 25],
        "0-31 numbers must show bgb's v8>>3 readout"
    );
}

#[test]
fn set_palette_channel_edits_the_selected_shade_and_snaps_in_5bit() {
    let mut s = Settings::default();
    s.select_scheme(0);
    s.palette_edit_shade = 2; // a mid shade
    // 8-bit mode: max frac -> 255 in the green channel, others untouched.
    s.palette_0_31 = false;
    let before = s.dmg_palette;
    s.set_palette_channel(1, 1.0);
    assert_eq!((s.dmg_palette[2] >> 8) & 0xFF, 255, "green set to 255");
    assert_eq!(s.dmg_palette[2] & 0xFF, before[2] & 0xFF, "blue untouched");
    assert_eq!(s.dmg_palette[0], before[0], "other shades untouched");
    // 5-bit mode: setting level 15 snaps to v5<<3 = 120 (bgb's readout inverse).
    s.palette_0_31 = true;
    s.set_palette_channel(0, 15.0 / 31.0);
    assert_eq!((s.dmg_palette[2] >> 16) & 0xFF, 120, "red snaps to 15<<3");
    assert_eq!(
        s.palette_channel_display(0),
        15,
        "reads back as 15 in 5-bit"
    );
}

// --- slider helper -----------------------------------------------------------

#[test]
fn slider_frac_maps_position() {
    let track = Rect::new(10, 0, 100, 10);
    assert_eq!(slider_frac(track, 10), 0.0);
    assert_eq!(slider_frac(track, 110), 1.0);
    assert!((slider_frac(track, 60) - 0.5).abs() < 0.01);
    assert_eq!(slider_frac(track, -5), 0.0, "clamped");
}
