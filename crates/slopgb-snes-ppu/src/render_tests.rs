//! BG renderer tests, pinned to the fullsnes tile/map/scroll laws.

use super::*;

fn line(ppu: &SnesPpu, bg: usize, y: u16) -> [Option<(u16, bool)>; 256] {
    let mut out = [None; 256];
    ppu.bg_line(bg, y, &mut out);
    out
}

/// Mode 1 BG1 (16-color): a 4bpp tile with plane bits split across the two
/// plane-pair words renders its row — bit 7 is the leftmost pixel, plane 0
/// is the color LSB, color 0 is transparent, and the map entry's palette +
/// priority ride along (CGRAM index = palette*16 + color).
#[test]
fn mode1_bg1_renders_a_4bpp_tile_row() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x01); // mode 1
    ppu.write(0x07, 0x04); // BG1 map base = word $400, 32x32
    ppu.write(0x0B, 0x01); // BG1 tiles at word $1000
    // Map (0,0): char 2, palette 3, priority set.
    ppu.vram[0x400] = 2 | 3 << 10 | 1 << 13;
    // Char 2 row 0: pixel 0 = color 5 (planes 0+2), pixel 7 = color 1.
    let t = 0x1000 + 2 * 16;
    ppu.vram[t] = 0x0081; // plane 0 = $81 (pixels 0,7), plane 1 = 0
    ppu.vram[t + 8] = 0x0080; // plane 2 = $80 (pixel 0), plane 3 = 0
    ppu.cgram[3 * 16 + 5] = 0x7C1F;
    ppu.cgram[3 * 16 + 1] = 0x03E0;

    let out = line(&ppu, 0, 0);
    assert_eq!(out[0], Some((0x7C1F, true)), "pixel 0: color 5, priority");
    assert_eq!(out[7], Some((0x03E0, true)), "pixel 7: color 1");
    assert_eq!(out[1], None, "color 0 is transparent");
    assert_eq!(
        out[8], None,
        "the neighbouring all-zero tile is transparent"
    );
}

/// The write-twice scroll pair lands `hi<<8|lo` (the shared-latch formula
/// self-cancels for a normal pair), pixels sample `x + HOFS` with 10-bit
/// wraparound, and VOFS moves the sampled row.
#[test]
fn scroll_applies_with_ten_bit_wrap() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x01);
    ppu.write(0x07, 0x04);
    ppu.write(0x0B, 0x01);
    ppu.vram[0x400] = 2;
    let t = 0x1000 + 2 * 16;
    ppu.vram[t] = 0x0001; // row 0, pixel 7 = color 1
    ppu.vram[t + 1] = 0x0080; // row 1, pixel 0 = color 1
    ppu.cgram[1] = 0x1111;

    ppu.write(0x0D, 0x03); // BG1HOFS = 3
    ppu.write(0x0D, 0x00);
    let out = line(&ppu, 0, 0);
    assert_eq!(out[4], Some((0x1111, false)), "vx = 4+3 = 7 hits pixel 7");
    assert_eq!(out[7], None, "vx = 10 leaves the tile");

    ppu.write(0x0D, 0xFE); // BG1HOFS = $3FE: vx wraps 1022,1023,0,1,...
    ppu.write(0x0D, 0x03);
    ppu.write(0x0E, 0x01); // BG1VOFS = 1: sample tile row 1
    ppu.write(0x0E, 0x00);
    let out = line(&ppu, 0, 0);
    assert_eq!(out[2], Some((0x1111, false)), "vx wrapped to 0, row 1");
}

/// Map-entry flips mirror the tile fetch (fullsnes BG-map entry bits
/// 14/15): X-flip swaps pixel 0/7, Y-flip samples the opposite row.
#[test]
fn map_entry_flips_mirror_the_tile() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x01);
    ppu.write(0x07, 0x04);
    ppu.write(0x0B, 0x01);
    ppu.vram[0x400] = 2 | 1 << 14 | 1 << 15; // char 2, X-flip + Y-flip
    let t = 0x1000 + 2 * 16;
    ppu.vram[t + 7] = 0x0080; // row 7, pixel 0 = color 1
    ppu.cgram[1] = 0x2222;

    let out = line(&ppu, 0, 0); // Y-flip: screen row 0 samples tile row 7
    assert_eq!(out[7], Some((0x2222, false)), "X-flip: pixel 0 lands at 7");
    assert_eq!(out[0], None);
}

/// Mode 0 gives each BG its own CGRAM slice (fullsnes CGRAM content:
/// BG1/2/3/4 palettes at 00h/20h/40h/60h).
#[test]
fn mode0_offsets_each_bg_palette_base() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x00); // mode 0 (all BGs 2bpp)
    ppu.write(0x08, 0x04); // BG2 map base = word $400
    ppu.write(0x0B, 0x10); // BG2 tiles at word $1000
    ppu.vram[0x400] = 2 | 1 << 10; // char 2, palette 1
    let t = 0x1000 + 2 * 8;
    ppu.vram[t] = 0x8080; // row 0 pixel 0: planes 0+1 -> color 3
    ppu.cgram[0x20 + 4 + 3] = 0x5555; // BG2 base $20 + palette 1 + color 3

    let out = line(&ppu, 1, 0);
    assert_eq!(out[0], Some((0x5555, false)), "BG2 palettes start at $20");
}

/// Screen size bit 0 (64-tile-wide maps): tile column 32 fetches from the
/// second 32x32 screen at map base + $400 (fullsnes 2107h layout chart).
#[test]
fn wide_screen_uses_the_second_screen_block() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x01);
    ppu.write(0x07, 0x05); // map base $400, size 1 (64x32)
    ppu.write(0x0B, 0x01);
    ppu.vram[0x400 + 0x400] = 2; // SC1 entry (0,0) = char 2
    let t = 0x1000 + 2 * 16;
    ppu.vram[t] = 0x0080; // pixel 0 = color 1
    ppu.cgram[1] = 0x3333;

    ppu.write(0x0D, 0x00); // BG1HOFS = $100 = 256: tile column 32
    ppu.write(0x0D, 0x01);
    let out = line(&ppu, 0, 0);
    assert_eq!(out[0], Some((0x3333, false)), "column 32 -> SC1");
}

/// 16x16 tiles: the entry names the upper-left 8x8 char; right is N+1 and
/// below is N+10h, with BG tiles carrying across the 10-bit char space
/// (fullsnes "16x16 (and bigger) Tiles": BG char $1FF's right half is
/// $200).
#[test]
fn tile16_quadrants_and_char_carry() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x11); // mode 1 + BG1 16x16 tiles
    ppu.write(0x07, 0x04);
    ppu.write(0x0B, 0x00); // tiles at word 0
    ppu.vram[0x400] = 0x1FF; // char $1FF
    ppu.vram[0x200 * 16] = 0x0080; // char $200 row 0, pixel 0 = color 1
    ppu.vram[0x20F * 16] = 0x0040; // char $20F ($1FF+$10) row 0, pixel 1
    ppu.cgram[1] = 0x4444;

    let out = line(&ppu, 0, 0);
    assert_eq!(
        out[8],
        Some((0x4444, false)),
        "right half: char carries to $200"
    );
    let out = line(&ppu, 0, 8);
    assert_eq!(out[1], Some((0x4444, false)), "bottom half: char $1FF+$10");
}

/// Mode 1 BG3 is 2bpp with plain palette*4 CGRAM indexing (no mode-0 BG
/// offset), and BG4 doesn't exist in mode 1.
#[test]
fn mode1_bg3_is_2bpp_and_bg4_absent() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x01);
    ppu.write(0x09, 0x04); // BG3 map base $400
    ppu.write(0x0C, 0x01); // BG3 tiles at word $1000
    ppu.vram[0x400] = 2 | 2 << 10; // palette 2
    let t = 0x1000 + 2 * 8;
    ppu.vram[t] = 0x0080;
    ppu.cgram[2 * 4 + 1] = 0x6666;

    let out = line(&ppu, 2, 0);
    assert_eq!(out[0], Some((0x6666, false)), "palette*4 indexing");
    let out = line(&ppu, 3, 0);
    assert!(out.iter().all(Option::is_none), "no BG4 in mode 1");
}
