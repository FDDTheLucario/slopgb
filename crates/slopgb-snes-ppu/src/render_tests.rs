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

/// A frozen copy of the per-pixel `bg_line` (the pre-run-decode shape),
/// kept as the fuzz oracle: the production renderer must stay
/// slot-identical to it over randomized states.
fn bg_line_ref(ppu: &SnesPpu, bg: usize, y: u16, out: &mut [Option<(u16, bool)>; 256]) {
    out.fill(None);
    let bpp = match (ppu.bgmode & 7, bg) {
        (0, 0..=3) => 2u16,
        (1, 0 | 1) => 4,
        (1, 2) => 2,
        _ => return,
    };
    let tile16 = ppu.bgmode & 1 << (4 + bg) != 0;
    let map_base = usize::from(ppu.bgsc[bg] >> 2) << 10;
    let size = ppu.bgsc[bg] & 3;
    let char_base = usize::from(ppu.nba[bg / 2] >> (bg % 2 * 4) & 0xF) << 12;
    let words_per_tile = usize::from(bpp) * 4;
    let fine_mask = if tile16 { 15u16 } else { 7 };
    let shift = if tile16 { 4 } else { 3 };
    let vy = y.wrapping_add(ppu.vofs[bg]) & 0x3FF;
    for (x, slot) in out.iter_mut().enumerate() {
        let vx = (x as u16).wrapping_add(ppu.hofs[bg]) & 0x3FF;
        let (tx, ty) = (vx >> shift, vy >> shift);
        let mut map = usize::from(ty & 31) << 5 | usize::from(tx & 31);
        if size & 1 != 0 && tx & 32 != 0 {
            map += 0x400;
        }
        if size & 2 != 0 && ty & 32 != 0 {
            map += 0x400 << (size & 1);
        }
        let entry = ppu.vram[(map_base + map) & 0x7FFF];
        let mut ch = usize::from(entry & 0x3FF);
        let pal = usize::from(entry >> 10 & 7);
        let prio = entry & 0x2000 != 0;
        let mut fx = vx & fine_mask;
        let mut fy = vy & fine_mask;
        if entry & 0x4000 != 0 {
            fx = fine_mask - fx;
        }
        if entry & 0x8000 != 0 {
            fy = fine_mask - fy;
        }
        if tile16 {
            if fx >= 8 {
                ch = (ch + 1) & 0x3FF;
                fx -= 8;
            }
            if fy >= 8 {
                ch = (ch + 0x10) & 0x3FF;
                fy -= 8;
            }
        }
        let row = char_base + ch * words_per_tile + usize::from(fy);
        let bit = 7 - fx;
        let w0 = ppu.vram[row & 0x7FFF];
        let mut idx = usize::from(w0 >> bit & 1 | (w0 >> 8 >> bit & 1) << 1);
        if bpp == 4 {
            let w1 = ppu.vram[(row + 8) & 0x7FFF];
            idx |= usize::from((w1 >> bit & 1) << 2 | (w1 >> 8 >> bit & 1) << 3);
        }
        if idx == 0 {
            continue;
        }
        let cg = if ppu.bgmode & 7 == 0 {
            bg * 0x20 + pal * 4 + idx
        } else {
            pal * usize::from(1u16 << bpp) + idx
        };
        *slot = Some((ppu.cgram[cg & 0xFF] & 0x7FFF, prio));
    }
}

/// xorshift32 — deterministic in-test randomness (no deps).
fn xs(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// The production `bg_line` stays slot-identical to the frozen per-pixel
/// reference over fuzzed states: both modes, 8/16-px tiles, every screen
/// size, flips, scrolls including the 10-bit wrap seam, all BGs.
#[test]
fn bg_line_matches_reference_over_fuzzed_states() {
    let mut s = 0x1234_5678u32;
    for case in 0..200 {
        let mut ppu = SnesPpu::new();
        // Sparse-random VRAM: enough structure for maps + tiles everywhere.
        for _ in 0..4096 {
            let i = xs(&mut s) as usize & 0x7FFF;
            ppu.vram[i] = xs(&mut s) as u16;
        }
        for c in ppu.cgram.iter_mut() {
            *c = xs(&mut s) as u16;
        }
        ppu.bgmode = (xs(&mut s) as u8 & 1) | (xs(&mut s) as u8 & 0xF8);
        for i in 0..4 {
            ppu.bgsc[i] = xs(&mut s) as u8;
        }
        ppu.nba = [xs(&mut s) as u8, xs(&mut s) as u8];
        for i in 0..4 {
            // Bias half the scrolls onto the wrap seam.
            ppu.hofs[i] = if xs(&mut s) & 1 == 0 {
                (0x3F8 + (xs(&mut s) & 0xF)) as u16 & 0x3FF
            } else {
                xs(&mut s) as u16 & 0x3FF
            };
            ppu.vofs[i] = xs(&mut s) as u16 & 0x3FF;
        }
        for &y in &[0u16, 1, 7, 8, 100, 223] {
            for bg in 0..4 {
                let mut want = [None; 256];
                bg_line_ref(&ppu, bg, y, &mut want);
                let mut got = [None; 256];
                ppu.bg_line(bg, y, &mut got);
                assert_eq!(got[..], want[..], "case {case} bg {bg} y {y}");
            }
        }
    }
}

/// A run that straddles the 10-bit playfield wrap while X-flipped: the
/// seam splits mid-run (vx 0x3FE,0x3FF,0x000...) and each side samples its
/// own map entry with the mirror applied inside that entry's tile.
#[test]
fn run_straddling_the_wrap_seam_with_xflip() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x05, 0x01); // mode 1
    ppu.write(0x07, 0x04); // BG1 map at word $400, 32x32 (wraps at 256px)
    ppu.write(0x0B, 0x01); // tiles at word $1000
    // vx 0x3FE-0x3FF live in map tile 31 of a wrapped row (playfield x
    // wraps mod 1024 but the 32x32 map repeats every 256px -> tile 31).
    ppu.vram[0x400 + 31] = 2 | 1 << 14; // char 2, X-flip
    ppu.vram[0x400] = 3; // char 3, no flip (post-wrap tile 0)
    let t2 = 0x1000 + 2 * 16;
    // Screen cols 6,7 X-flip onto tile cols 1,0 = plane bits 6,7 ($C0).
    ppu.vram[t2] = 0x00C0;
    let t3 = 0x1000 + 3 * 16;
    ppu.vram[t3] = 0x0080; // pixel 0 = color 1
    ppu.cgram[1] = 0x2222;
    ppu.write(0x0D, 0xFE); // BG1HOFS = 0x3FE
    ppu.write(0x0D, 0x03);

    let out = line(&ppu, 0, 0);
    // Screen x0 -> vx 0x3FE = tile col 6, X-flipped -> sampled col 1 = set.
    assert_eq!(out[0], Some((0x2222, false)), "pre-seam flipped pixel");
    assert_eq!(out[1], Some((0x2222, false)), "pre-seam flipped pixel 2");
    // Screen x2 -> vx 0x000 = the post-wrap tile's pixel 0.
    assert_eq!(out[2], Some((0x2222, false)), "post-seam pixel 0");
    assert_eq!(out[3], None, "post-seam pixel 1 empty");
}
