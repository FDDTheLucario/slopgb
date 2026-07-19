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

#[test]
fn scale2x_doubles_dimensions_and_replicates_flat_areas() {
    // A flat 2×2 block doubles to 4×4 of the same value (no spurious edges).
    let src = vec![0x0011_2233u32; 4];
    let mut dst = Vec::new();
    scale2x(&src, 2, 2, &mut dst);
    assert_eq!(dst.len(), 16, "2×2 -> 4×4");
    assert!(
        dst.iter().all(|&p| p == 0x0011_2233),
        "flat area stays flat"
    );
}

#[test]
fn scale2x_smooths_a_corner_diagonal() {
    // A lone AA at (0,0) with the rest 0: its down (0) and right (0) neighbours
    // match, so scale2x promotes only the inner-diagonal sub-pixel to the
    // background — the classic edge smoothing. The other three stay AA.
    let src = vec![0x00AA_AAAA, 0x0000_0000, 0x0000_0000, 0x0000_0000];
    let mut dst = Vec::new();
    scale2x(&src, 2, 2, &mut dst);
    // Source (0,0) -> dst (0,0),(1,0),(0,1),(1,1) = indices 0,1,4,5 (dst width 4).
    assert_eq!(dst[0], 0x00AA_AAAA, "E0");
    assert_eq!(dst[1], 0x00AA_AAAA, "E1");
    assert_eq!(dst[4], 0x00AA_AAAA, "E2");
    assert_eq!(
        dst[5], 0x0000_0000,
        "E3 promoted to the down/right diagonal"
    );
}

#[test]
fn snes_rgb555_converts_bgr_order_and_expands() {
    assert_eq!(snes_rgb555_px(0x001F), 0x00FF_0000, "red is the low field");
    assert_eq!(snes_rgb555_px(0x03E0), 0x0000_FF00, "green mid");
    assert_eq!(snes_rgb555_px(0x7C00), 0x0000_00FF, "blue high");
    assert_eq!(snes_rgb555_px(0x7FFF), 0x00FF_FFFF, "31 expands to 255");
    assert_eq!(snes_rgb555_px(0x0001), 0x0008_0000, "1 expands to 8");
}
