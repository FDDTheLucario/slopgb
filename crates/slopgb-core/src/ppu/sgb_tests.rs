//! Unit tests for the SGB presentation parser ([`SgbView::sgb_command`]).
//! Packet layouts cite Pan Docs "SGB Command $xx".

use super::*;

/// Build a 16-byte command packet: `header` (command × 8 + length) then the
/// data bytes, zero-padded.
fn packet(header: u8, body: &[u8]) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = header;
    p[1..1 + body.len()].copy_from_slice(body);
    p
}

#[test]
fn bgr555_expands_like_cgb() {
    // 5-bit 31 → 0xFF, 5-bit 1 → 0x08 (straight (c<<3)|(c>>2) fill).
    assert_eq!(bgr555(0x1F, 0x00), 0xFF_0000); // R=31
    assert_eq!(bgr555(0xE0, 0x03), 0x00_FF00); // G=31
    assert_eq!(bgr555(0x00, 0x7C), 0x00_00FF); // B=31
    assert_eq!(bgr555(0xFF, 0x7F), 0xFF_FFFF); // all 31
    assert_eq!(bgr555(0x01, 0x00), 0x08_0000); // R=1
}

/// PAL01 ($00): color 0 is the shared entry-0 of all four palettes; colors
/// 1-3 fill palette 0, colors 4-6 fill palette 1; palettes 2/3 keep the DMG
/// default. (Pan Docs "SGB Command $00 — PAL01".)
#[test]
fn pal01_sets_two_palettes_and_shared_bg() {
    let mut s = SgbView::new();
    // 7 BGR555 colors: bg=red, then pal0 = {green, blue, white}, pal1 = R/G/B=1.
    s.sgb_command(&packet(
        0x01,
        &[
            0x1F, 0x00, // color 0 (shared bg) = red
            0xE0, 0x03, // color 1 (pal0 e1) = green
            0x00, 0x7C, // color 2 (pal0 e2) = blue
            0xFF, 0x7F, // color 3 (pal0 e3) = white
            0x01, 0x00, // color 4 (pal1 e1)
            0x20, 0x00, // color 5 (pal1 e2)
            0x00, 0x04, // color 6 (pal1 e3)
        ],
    ));
    assert_eq!(s.pal[0], [0xFF_0000, 0x00_FF00, 0x00_00FF, 0xFF_FFFF]);
    assert_eq!(s.pal[1], [0xFF_0000, 0x08_0000, 0x00_0800, 0x00_0008]);
    // Unnamed palettes: shared bg in entry 0, DMG default in 1-3.
    assert_eq!(s.pal[2], [0xFF_0000, 0xAA_AAAA, 0x55_5555, 0x00_0000]);
    assert_eq!(s.pal[3], [0xFF_0000, 0xAA_AAAA, 0x55_5555, 0x00_0000]);
}

/// PAL12 ($03) writes palettes 1 and 2 (not 0/1): guards the a/b table.
#[test]
fn pal12_targets_palettes_1_and_2() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(
        0x03 * 8 + 1,
        &[0x1F, 0x00, 0xE0, 0x03, 0xE0, 0x03, 0xE0, 0x03, 0x00, 0x7C, 0x00, 0x7C, 0x00, 0x7C],
    ));
    assert_eq!(s.pal[1][1], 0x00_FF00, "pal1 gets colors 1-3 (green)");
    assert_eq!(s.pal[2][1], 0x00_00FF, "pal2 gets colors 4-6 (blue)");
    // Palette 0 untouched except the shared entry 0.
    assert_eq!(s.pal[0], [0xFF_0000, 0xAA_AAAA, 0x55_5555, 0x00_0000]);
}

/// ATTR_BLK ($04): a 5,5..10,10 rect recolors inside/border/outside cells by
/// their control bits. (Pan Docs "SGB Command $04 — ATTR_BLK".)
#[test]
fn attr_blk_fills_inside_border_outside() {
    let mut s = SgbView::new();
    // control = 0b111 (all three), palettes: inside 1, border 2, outside 3.
    let pals = (3 << 4) | (2 << 2) | 1;
    s.sgb_command(&packet(0x04 * 8 + 1, &[1, 0b111, pals, 5, 5, 10, 10]));
    let at = |cx: usize, cy: usize| s.attr[cy * 20 + cx];
    assert_eq!(at(7, 7), 1, "strictly inside");
    assert_eq!(at(5, 7), 2, "on the left border (cx == x1)");
    assert_eq!(at(5, 5), 2, "corner is on-border");
    assert_eq!(at(0, 0), 3, "outside");
    assert_eq!(at(10, 10), 2, "bottom-right corner on-border");
}

/// ATTR_BLK honours only the region control bits that are set.
#[test]
fn attr_blk_skips_regions_without_control_bit() {
    let mut s = SgbView::new();
    // control = 0b001 (inside only); outside/border must stay palette 0.
    let pals = (3 << 4) | (2 << 2) | 1;
    s.sgb_command(&packet(0x04 * 8 + 1, &[1, 0b001, pals, 5, 5, 10, 10]));
    assert_eq!(s.attr[7 * 20 + 7], 1, "inside recolored");
    assert_eq!(s.attr[0], 0, "outside untouched (no control bit)");
    assert_eq!(s.attr[7 * 20 + 5], 0, "border untouched (no control bit)");
}

/// MASK_EN ($17): byte 1 low 2 bits select freeze/black/color-0/cancel.
/// (Pan Docs "SGB Command $17 — MASK_EN".)
#[test]
fn mask_en_modes() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(0x17 * 8 + 1, &[1]));
    assert!(s.holds_frame(), "freeze holds the last frame");
    assert_eq!(s.mask_fill(), None);

    s.sgb_command(&packet(0x17 * 8 + 1, &[2]));
    assert!(!s.holds_frame());
    assert_eq!(s.mask_fill(), Some(0x00_0000), "black fill");

    s.sgb_command(&packet(0x17 * 8 + 1, &[3]));
    assert_eq!(s.mask_fill(), Some(s.pal[0][0]), "palette-0 color-0 fill");

    s.sgb_command(&packet(0x17 * 8 + 1, &[0]));
    assert!(!s.holds_frame());
    assert_eq!(s.mask_fill(), None, "cancel");
}

/// A malformed (short) command is ignored, not a panic.
#[test]
fn short_command_ignored() {
    let mut s = SgbView::new();
    s.sgb_command(&[0x01, 0x1F, 0x00]);
    assert_eq!(s.pal[0], [DMG_SHADES[0], DMG_SHADES[1], DMG_SHADES[2], DMG_SHADES[3]]);
}

/// Palettes, attribute map and mask survive a save-state round-trip.
#[test]
fn state_round_trips() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(
        0x01,
        &[0x1F, 0x00, 0xE0, 0x03, 0x00, 0x7C, 0xFF, 0x7F, 0x01, 0x00, 0x20, 0x00, 0x00, 0x04],
    ));
    let pals = (3 << 4) | (2 << 2) | 1;
    s.sgb_command(&packet(0x04 * 8 + 1, &[1, 0b111, pals, 5, 5, 10, 10]));
    s.sgb_command(&packet(0x17 * 8 + 1, &[1]));

    let mut w = crate::state::Writer::new();
    s.write_state(&mut w);
    let bytes = w.into_vec();
    let mut t = SgbView::new();
    let mut r = crate::state::Reader::new(&bytes);
    t.read_state(&mut r).unwrap();

    assert_eq!(t.pal, s.pal);
    assert_eq!(t.attr, s.attr);
    assert_eq!(t.mask, s.mask);
}
