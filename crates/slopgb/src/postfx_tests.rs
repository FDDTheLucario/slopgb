use super::*;
use crate::windows::options::Settings;

#[test]
fn blend_is_channel_average() {
    assert_eq!(blend_px(0x00FF_FFFF, 0x0000_0000), 0x007F_7F7F);
    assert_eq!(blend_px(0x0010_2040, 0x0010_2040), 0x0010_2040); // same → same
    assert_eq!(blend_px(0x00FF_0000, 0x0000_00FF), 0x007F_007F);
}

#[test]
fn contrast_midpoint_is_identity() {
    let px = 0x0012_3456;
    assert_eq!(contrast_px(px, CONTRAST_NEUTRAL), px);
}

#[test]
fn contrast_above_midpoint_widens_spread() {
    // A dark pixel gets darker, a bright pixel brighter, around mid-grey 128.
    assert!((contrast_px(0x0040_4040, 1.0) & 0xFF) < 0x40);
    assert!((contrast_px(0x00C0_C0C0, 1.0) & 0xFF) > 0xC0);
    // Clamps, never wraps.
    assert_eq!(contrast_px(0x00FF_FFFF, 1.0), 0x00FF_FFFF);
    assert_eq!(contrast_px(0x0000_0000, 1.0), 0x0000_0000);
}

#[test]
fn gbc_lcd_mutes_white_and_keeps_black() {
    assert_eq!(gbc_lcd_px(0x0000_0000), 0x0000_0000); // black stays black
    // White is pulled below full brightness (the muted GBC-panel look).
    let white = gbc_lcd_px(0x00FF_FFFF);
    assert!((white & 0xFF) < 0xFF && ((white >> 16) & 0xFF) < 0xFF);
    assert_eq!((white >> 16) & 0xFF, 240); // 31*32 -> min(960) -> >>2
}

#[test]
fn apply_skips_blend_on_length_mismatch() {
    let s = Settings {
        frame_blend: true,
        ..Settings::default()
    };
    let mut buf = vec![0x00FF_FFFF; 4];
    let prev = vec![0x0000_0000; 2]; // wrong length → blend skipped
    apply(&mut buf, &prev, &s);
    assert!(buf.iter().all(|&p| p == 0x00FF_FFFF));
}

#[test]
fn apply_blends_when_lengths_match() {
    let s = Settings {
        frame_blend: true,
        ..Settings::default()
    };
    let mut buf = vec![0x00FF_FFFF; 4];
    let prev = vec![0x0000_0000; 4];
    apply(&mut buf, &prev, &s);
    assert!(buf.iter().all(|&p| p == 0x007F_7F7F));
}

#[test]
fn any_active_false_on_defaults() {
    assert!(!any_active(&Settings::default()));
}
