use super::*;
use slopgb_core::{GameBoy, Model};

fn blank_rom() -> Vec<u8> {
    vec![0u8; 0x8000]
}

#[test]
fn each_view_has_expected_dimensions() {
    let gb = GameBoy::new(Model::Cgb, {
        let mut r = blank_rom();
        r[0x143] = 0x80; // CGB flag
        r
    })
    .unwrap();
    for (view, w, h) in [
        ("tile0", 128, 192),
        ("tile1", 128, 192),
        ("bg", 256, 256),
        ("win", 256, 256),
        ("oam", 64, 40),      // 8x5 of 8x8 (LCDC bit2 off after reset)
        ("palette", 64, 256), // CGB: 16 rows × 4 × 16px
    ] {
        let bmp = capture(&gb, view, false).unwrap();
        assert_eq!((bmp.w, bmp.h), (w, h), "{view} dims");
        assert_eq!(bmp.px.len(), w * h);
    }
}

#[test]
fn unknown_view_errors() {
    let gb = GameBoy::new(Model::Dmg, blank_rom()).unwrap();
    assert!(capture(&gb, "sprites", false).is_err());
    assert!(capture(&gb, "", false).is_err());
}

#[test]
fn dmg_palette_view_is_three_rows() {
    let gb = GameBoy::new(Model::Dmg, blank_rom()).unwrap();
    let bmp = capture(&gb, "palette", false).unwrap();
    assert_eq!((bmp.w, bmp.h), (64, 48)); // BGP/OBP0/OBP1 × 4 × 16px
}

#[test]
fn written_tile_shows_through() {
    let mut gb = GameBoy::new(Model::Dmg, blank_rom()).unwrap();
    let before = capture(&gb, "tile0", false).unwrap().px[0];
    // Tile 0, row 0: both bitplanes set → every pixel becomes colour index 3
    // (black in the grey ramp), so the top-left pixel changes.
    gb.debug_write(0x8000, 0xFF);
    gb.debug_write(0x8001, 0xFF);
    let bmp = capture(&gb, "tile0", false).unwrap();
    assert_eq!(bmp.px[0], 0x0000_0000, "index-3 pixel is black");
    assert_ne!(bmp.px[0], before, "the write shows through");
}

#[test]
fn no_palette_forces_the_grey_ramp() {
    let mut gb = GameBoy::new(Model::Dmg, blank_rom()).unwrap();
    // BGP maps colour id 0 → shade 3 (black); the BG map is all-zero (tile 0,
    // colour id 0), so the paletted bg is black but the unpaletted one is white.
    gb.debug_write(0xFF47, 0b1111_1111);
    let paletted = capture(&gb, "bg", false).unwrap().px[0];
    let raw = capture(&gb, "bg", true).unwrap().px[0];
    assert_eq!(paletted, 0x0000_0000, "id 0 → shade 3 → black");
    assert_eq!(raw, 0x00FF_FFFF, "no_palette → grey ramp shade 0 → white");
}

#[test]
fn palreg_dumps_dmg_registers() {
    let mut gb = GameBoy::new(Model::Dmg, blank_rom()).unwrap();
    gb.debug_write(0xFF47, 0xE4); // BGP: identity map 0,1,2,3
    let text = palreg(&gb);
    assert!(text.contains("DMG palettes"), "{text}");
    assert!(text.contains("BGP  $E4  0 1 2 3"), "{text}");
}

#[test]
fn palreg_dumps_cgb_palette_words() {
    let gb = GameBoy::new(Model::Cgb, {
        let mut r = blank_rom();
        r[0x143] = 0x80;
        r
    })
    .unwrap();
    let text = palreg(&gb);
    assert!(text.contains("CGB palettes"), "{text}");
    assert!(text.contains("BG0"), "{text}");
    assert!(text.contains("OB7"), "{text}");
}
