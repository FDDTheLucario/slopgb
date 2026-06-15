//! Unit tests for the PPU renderer (OAM scan, fetcher, sprites, window,
//! mode-0 grid). Split out of `render.rs` for file size; compiled as
//! `super::tests` via the `#[path]` attribute there.

use super::super::Ppu;
use super::oam_glitch_magic_enable;
use crate::Model;

const WHITE: u32 = 0xFF_FFFF;
const LIGHT: u32 = 0xAA_AAAA;
const DARK: u32 = 0x55_5555;
const BLACK: u32 = 0x00_0000;

fn run_to(p: &mut Ppu, line: u8, dot: u16) {
    let mut guard = 0u32;
    while !(p.line == line && p.dot == dot) {
        p.tick();
        guard += 1;
        assert!(guard < 200_000, "run_to({line},{dot}) never reached");
    }
}

/// Render the given line to completion; returns the dot at which mode 3
/// ended (V0).
fn render_line(p: &mut Ppu, line: u8) -> u16 {
    run_to(p, line, 84);
    finish_line(p)
}

fn px(p: &Ppu, line: usize, x: usize) -> u32 {
    p.back[line * crate::SCREEN_W + x]
}

fn dmg_on(lcdc: u8) -> Ppu {
    let mut p = Ppu::new(Model::Dmg);
    p.write(0xFF47, 0xE4); // identity BGP
    p.write(0xFF48, 0xE4);
    p.write(0xFF49, 0xE4);
    p.write(0xFF40, lcdc);
    p
}

fn set_tile_row(p: &mut Ppu, bank: usize, tile: usize, row: usize, lo: u8, hi: u8) {
    p.vram[bank * 0x2000 + tile * 16 + row * 2] = lo;
    p.vram[bank * 0x2000 + tile * 16 + row * 2 + 1] = hi;
}

fn set_map(p: &mut Ppu, base: usize, row: usize, col: usize, tile: u8) {
    p.vram[base + row * 32 + col] = tile;
}

fn sprite(p: &mut Ppu, i: u8, y: u8, x: u8, tile: u8, flags: u8) {
    p.oam_dma_write(i * 4, y);
    p.oam_dma_write(i * 4 + 1, x);
    p.oam_dma_write(i * 4 + 2, tile);
    p.oam_dma_write(i * 4 + 3, flags);
}

// --- BG rendering ---

#[test]
fn bg_tile_pixels_and_bgp() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
    set_map(&mut p, 0x1800, 0, 0, 1);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), LIGHT);
    assert_eq!(px(&p, 2, 3), LIGHT);
    assert_eq!(px(&p, 2, 4), DARK);
    assert_eq!(px(&p, 2, 7), DARK);
    assert_eq!(px(&p, 2, 8), WHITE); // tile 0 = color 0

    // Remap shades through BGP.
    let mut p = dmg_on(0x91);
    p.write(0xFF47, 0x1B); // 0->3, 1->2, 2->1, 3->0
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
    set_map(&mut p, 0x1800, 0, 0, 1);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), DARK);
    assert_eq!(px(&p, 2, 4), LIGHT);
    assert_eq!(px(&p, 2, 8), BLACK);
}

#[test]
fn bg_scx_fine_scroll_shifts_pixels() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
    set_map(&mut p, 0x1800, 0, 0, 1);
    set_map(&mut p, 0x1800, 0, 1, 1);
    p.write(0xFF43, 3);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), LIGHT); // bg col 3
    assert_eq!(px(&p, 2, 1), DARK); // bg col 4
    assert_eq!(px(&p, 2, 4), DARK); // bg col 7
    assert_eq!(px(&p, 2, 5), LIGHT); // bg col 8 = next tile col 0
}

#[test]
fn bg_scy_selects_row() {
    let mut p = dmg_on(0x91);
    p.write(0xFF42, 5);
    set_tile_row(&mut p, 0, 1, 7, 0xFF, 0xFF); // line 2 + scy 5 = row 7
    set_map(&mut p, 0x1800, 0, 0, 1);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), BLACK);
}

#[test]
fn bg_signed_tile_addressing() {
    let mut p = dmg_on(0x81); // LCDC bit 4 clear: 0x8800 signed mode
    // Tile 0x80 lives at 0x9000 + (-128)*16 = 0x8800.
    p.vram[0x0800 + 2 * 2] = 0xFF;
    p.vram[0x0800 + 2 * 2 + 1] = 0xFF;
    set_map(&mut p, 0x1800, 0, 0, 0x80);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), BLACK);
}

#[test]
fn bg_map_select_bit3() {
    let mut p = dmg_on(0x99); // bit 3: map at 0x9C00
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0xFF);
    set_map(&mut p, 0x1C00, 0, 0, 1);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), BLACK);
}

#[test]
fn dmg_lcdc0_blanks_bg_to_white() {
    let mut p = dmg_on(0x90); // BG disabled
    set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), WHITE);
    assert_eq!(px(&p, 2, 159), WHITE);
}

// --- Mode-3 IO write strobe ---
//
// The CPU drives the data bus during the second half of a write M-cycle
// (gbctr "Memory access timing": the store lands around T3, not after
// T4), so the dot-clocked pixel pipeline observes a rendering-register
// write 2 dots before the tick-then-access commit point. Decoded from
// the mealybug m3_bgp_change references: of the write M-cycle's four
// dots, the pipeline pops dot 1 with the old value, dot 2 with old|new
// on pre-CGB models (mealybug README: "BGP takes the value old OR new
// for one cycle"; CGB-C switches cleanly and still reads old), and
// dots 3-4 with the new value.

/// Mimic the interconnect's write path: stage, tick one M-cycle (4 dots
/// at normal speed), then commit architecturally.
fn mcycle_write(p: &mut Ppu, addr: u16, value: u8) {
    p.stage_write(addr, value, 2);
    for _ in 0..4 {
        p.tick();
    }
    p.write(addr, value);
}

/// Finish the current line's mode 3; returns the dot it ended on (V0).
fn finish_line(p: &mut Ppu) -> u16 {
    let mut flip = None;
    let mut guard = 0u32;
    while !p.line_render_done || p.render.active {
        p.tick();
        if p.line_render_done && flip.is_none() {
            flip = Some(p.dot);
        }
        guard += 1;
        assert!(guard < 2_000, "mode 3 never finished");
    }
    flip.expect("flip dot recorded")
}

#[test]
fn strobe_bgp_write_two_dots_early_with_dmg_blend_pixel() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // solid color 1
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
    }
    // Pixel x pops at dot 97 + x (no SCX/sprites/window): after dot 130
    // pixels 0..=33 have shipped through the old palette.
    run_to(&mut p, 2, 130);
    mcycle_write(&mut p, 0xFF47, 0xE8); // color 1: shade 1 -> shade 2
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 254, "a palette strobe must not move mode-3 end");
    assert_eq!(px(&p, 2, 33), LIGHT, "well before the write: old");
    assert_eq!(px(&p, 2, 34), LIGHT, "write M-cycle dot 1: still old");
    assert_eq!(
        px(&p, 2, 35),
        BLACK,
        "dot 2 transition pixel: BGP reads old|new = 0xEC (color 1 -> 3)"
    );
    assert_eq!(px(&p, 2, 36), DARK, "dot 3: new value, 2 dots early");
    assert_eq!(px(&p, 2, 37), DARK, "dot 4: new");
    assert_eq!(px(&p, 2, 40), DARK, "after the commit: new");
}

#[test]
fn strobe_bgp_write_clean_switch_on_cgb() {
    let mut p = cgb_on(0x91);
    p.set_dmg_compat(true); // BGP remaps into compat palette 0
    p.write(0xFF47, 0xE4);
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // solid color 1
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
    }
    run_to(&mut p, 2, 130);
    mcycle_write(&mut p, 0xFF47, 0xE8);
    finish_line(&mut p);
    let old = p.cgb_color(&p.bg_pal_ram, 0, 1);
    let new = p.cgb_color(&p.bg_pal_ram, 0, 2);
    let blend = p.cgb_color(&p.bg_pal_ram, 0, 3);
    assert_eq!(px(&p, 2, 34), old, "write M-cycle dot 1: old");
    assert_eq!(px(&p, 2, 35), old, "dot 2: still old — no blend on CGB");
    assert_ne!(px(&p, 2, 35), blend, "CGB never blends");
    assert_eq!(px(&p, 2, 36), new, "dot 3: new value, 2 dots early");
    assert_eq!(px(&p, 2, 37), new, "dot 4: new");
}

#[test]
fn strobe_obp0_write_blend_pixel_dmg() {
    let mut p = dmg_on(0x93);
    p.write(0xFF48, 0xE4); // identity OBP0
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // sprite solid color 1
    sprite(&mut p, 0, 18, 8, 4, 0x00); // line 2, screen 0-7, OBP0
    // The X=8 sprite stalls the pipeline 6+5 dots at dot 97, so its
    // pixels 0-7 pop at dots 108-115; dots 110-113 cover x=2..=5.
    run_to(&mut p, 2, 108);
    mcycle_write(&mut p, 0xFF48, 0xE8);
    finish_line(&mut p);
    assert_eq!(px(&p, 2, 0), LIGHT, "before the write: old");
    assert_eq!(px(&p, 2, 1), LIGHT, "write M-cycle dot 1: old");
    assert_eq!(px(&p, 2, 2), BLACK, "dot 2: OBP0 reads old|new");
    assert_eq!(px(&p, 2, 3), DARK, "dot 3: new, 2 dots early");
    assert_eq!(px(&p, 2, 4), DARK, "dot 4: new");
}

/// Double speed: the M-cycle is 2 dots, the strobe lands 1 dot before
/// the commit (second half of the M-cycle, same as normal speed).
#[test]
fn strobe_double_speed_one_dot_early() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00);
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
    }
    run_to(&mut p, 2, 130);
    p.stage_write(0xFF47, 0xE8, 1);
    for _ in 0..2 {
        p.tick();
    }
    p.write(0xFF47, 0xE8);
    finish_line(&mut p);
    assert_eq!(px(&p, 2, 34), BLACK, "ds dot 1: transition (old|new)");
    assert_eq!(px(&p, 2, 35), DARK, "ds dot 2: new, 1 dot early");
}

/// The SCX fine scroll is a live position comparator, not a latched
/// discard count: the comparator hunts through positions 0..7
/// (hardware positions -16..-9) one per dot from mode-3 dot 5,
/// re-reading SCX&7 each step, and the discard schedule is fixed only
/// once it matches (SameBoy render_pixel_if_possible: `(position &
/// 7) == (SCX & 7) -> position = -8`; gambatte scx_during_m3 offset
/// sweeps). A write landing during the hunt changes how many pixels
/// drop and thereby the line's phase and mode-3 length.
#[test]
fn strobe_scx_write_during_hunt_changes_discard_count() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
    set_map(&mut p, 0x1800, 0, 0, 1);
    set_map(&mut p, 0x1800, 0, 1, 1);
    p.write(0xFF43, 7); // hunt would match at dot 96 (index 7)
    // Stage SCX=2 at state(88): the pipeline view commits at dot 91,
    // where the hunt is at index 2 -> match: 2 pixels discard, pixel
    // 0 ships at dot 99 showing bg column 2.
    run_to(&mut p, 2, 88);
    mcycle_write(&mut p, 0xFF43, 2);
    let v0 = finish_line(&mut p);
    assert_eq!(px(&p, 2, 0), LIGHT, "pixel 0 is bg column 2 (color 1)");
    assert_eq!(px(&p, 2, 1), LIGHT, "bg column 3");
    assert_eq!(px(&p, 2, 2), DARK, "bg column 4");
    assert_eq!(v0, 256, "2 discarded pixels: V0 = 254 + 2");
}

/// If an SCX write makes the comparator miss its match (the new value
/// points at an index the hunt already passed), the position counter
/// wraps (-9 -> -16) and re-hunts: the discard grows by 8 and mode 3
/// extends with it (SameBoy: `position_in_line == -9 -> position =
/// -16`; gambatte scx_during_m3 encodes the +8 in its offset sweeps).
#[test]
fn strobe_scx_write_missing_the_match_wraps_the_hunt() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
    }
    p.write(0xFF43, 7);
    // Commit SCX=5 at dot 95: index 5 (dot 94) and earlier compared
    // against 7, indices 6 (dot 95) and 7 (dot 96) miss 5, the
    // counter wraps and re-hunts through the first real tile's pops,
    // matching at index 5 = dot 102 (the 6th pop): 13 pixels discard
    // in total.
    run_to(&mut p, 2, 92);
    mcycle_write(&mut p, 0xFF43, 5);
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 267, "13 discarded pixels: V0 = 254 + 13");
    assert_eq!(
        px(&p, 2, 0),
        DARK,
        "pixel 0 is bg column 13 (col 5: color 2)"
    );
}

/// The BG row is re-evaluated from SCY at each fetcher VRAM access,
/// not latched per fetch: an SCY write landing between a tile's
/// tile-number read and its data reads keeps the old tile number but
/// fetches the new scroll's data rows (mealybug m3_scy_change;
/// gambatte scy/).
#[test]
fn strobe_scy_write_between_tileno_and_data_reads_uses_new_row() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // tile 1 row 2: color 1
    set_tile_row(&mut p, 0, 1, 5, 0xFF, 0xFF); // tile 1 row 5: color 3
    set_tile_row(&mut p, 0, 2, 2, 0x00, 0xFF); // tile 2 (map row 1): color 2
    set_tile_row(&mut p, 0, 2, 5, 0x00, 0xFF);
    set_map(&mut p, 0x1800, 0, 1, 1); // old scroll: map row 0 -> tile 1
    set_map(&mut p, 0x1800, 1, 1, 2); // new scroll: map row 1 -> tile 2
    // Tile column 1 is fetched at dots 97-102: tile number at 98, data
    // low at 100, data high at 102. Staging SCY=11 at state(97) commits
    // the pipeline view at dot 100: the tile number was read with SCY=0
    // (map row 0 -> tile 1), the data reads use the new row
    // (2 + 11) & 7 = 5.
    run_to(&mut p, 2, 97);
    mcycle_write(&mut p, 0xFF42, 11);
    finish_line(&mut p);
    assert_eq!(px(&p, 2, 0), WHITE, "tile col 0 fetched before the write");
    assert_eq!(
        px(&p, 2, 8),
        BLACK,
        "old tile number (tile 1), new data row (5): color 3"
    );
}

/// A staged LCDC value must not enable/disable the LCD early: bit 7 is
/// only honored at the architectural commit (`lcdon_*` mooneye tables
/// were calibrated there).
#[test]
fn strobe_lcdc_bit7_only_at_commit() {
    let mut p = dmg_on(0x91);
    run_to(&mut p, 2, 130);
    p.stage_write(0xFF40, 0x11, 2); // LCD off staged
    for _ in 0..4 {
        p.tick();
    }
    assert!(p.enabled, "staged LCDC.7 must not act before the commit");
    p.write(0xFF40, 0x11);
    assert!(!p.enabled, "the architectural commit disables");
}

// --- Mode 3 length ---

#[test]
fn mode3_length_scx() {
    for scx in 0u8..=8 {
        let mut p = dmg_on(0x91);
        p.write(0xFF43, scx);
        let v0 = render_line(&mut p, 1);
        assert_eq!(v0, 254 + u16::from(scx & 7), "scx {scx}");
    }
}

/// The end-of-line event grid: the mode-0 STAT IRQ source and the
/// externally visible mode-0 flip land together 2 dots before the
/// pipe end — 254+SCX%8 on a bare line (see `m0_flip_events`); the
/// pipe end (the HDMA/palette-blocking anchor, `render_finished`)
/// stays at 256+SCX%8.
#[test]
fn mode0_irq_flip_pipe_end_split() {
    for scx in [0u8, 1, 5, 7] {
        let s = u16::from(scx & 7);
        let mut p = dmg_on(0x91);
        p.write(0xFF41, 0x08); // mode-0 STAT IRQ source enabled
        p.write(0xFF43, scx);
        run_to(&mut p, 2, 84);
        let mut flip = None;
        let mut if_dot = None;
        let mut finished = None;
        while finished.is_none() {
            let iff = p.tick();
            if p.line_render_done && flip.is_none() {
                flip = Some(p.dot);
            }
            if iff & 0x02 != 0 && if_dot.is_none() {
                if_dot = Some(p.dot);
            }
            if p.render_finished && finished.is_none() {
                finished = Some(p.dot);
            }
            assert!(p.dot < 400, "mode 3 never finished (scx {scx})");
        }
        assert_eq!(if_dot, Some(254 + s), "mode-0 STAT IF (scx {scx})");
        assert_eq!(flip, Some(254 + s), "visible flip (scx {scx})");
        assert_eq!(finished, Some(256 + s), "pipe end (scx {scx})");
    }
}

/// Sprite stalls shift the whole event grid: on DMG one sprite at
/// X=0 costs 6 (fetch) + 5 (alignment) dots and the flip leads the
/// pipe end by 3 (see `obj_fetch_base`), so the flip lands at 264 —
/// the top of mooneye intr_2_mode0_timing_sprites' "2 extra cycles"
/// window (260, 264] — and the pipe ends at 267.
#[test]
fn sprite_stall_shifts_event_grid() {
    let mut p = dmg_on(0x93);
    sprite(&mut p, 0, 19, 0, 0, 0); // row 0 on line 3
    run_to(&mut p, 3, 84);
    let mut flip = None;
    let mut finished = None;
    while finished.is_none() {
        p.tick();
        if p.line_render_done && flip.is_none() {
            flip = Some(p.dot);
        }
        if p.render_finished {
            finished = Some(p.dot);
        }
        assert!(p.dot < 400, "mode 3 never finished");
    }
    assert_eq!(flip, Some(264), "flip: 256 + 6 + 5 - 3 (sprite lead)");
    assert_eq!(finished, Some(267), "pipe end: flip + 3");
}

fn penalty(xs: &[u8]) -> i32 {
    let mut p = dmg_on(0x93);
    for (i, &x) in xs.iter().enumerate() {
        sprite(&mut p, i as u8, 19, x, 0, 0); // row 0 on line 3
    }
    i32::from(render_line(&mut p, 3)) - 256
}

/// Mooneye intr_2_mode0_timing_sprites pins each case's flip to the
/// 4-dot window (4e-4, 4e] past its poll anchor at dot 256, where e
/// is the "extra cycles" value — so e = ceil((flip - 256)/4). With
/// the flip at pipe end - 2 and the first fetch costing 5 dots (see
/// `obj_fetch_base`), every sprite case's flip sits exactly where
/// the old end-anchored model put it (the +2 cost and the -2 flip
/// lead cancel), while a sprite-free line flips 2 dots earlier —
/// still inside its e = 0 window.
#[test]
fn sprite_penalty_table() {
    fn e(p: i32) -> i32 {
        assert!(p >= 0, "e() is only defined for real penalties");
        (p + 3) / 4
    }
    // 1-N sprites at X=0 -> extra cycles 2,4,5,7,8,10,11,13,14,16.
    let expect = [2, 4, 5, 7, 8, 10, 11, 13, 14, 16];
    for n in 1..=10usize {
        let dots = penalty(&vec![0u8; n]);
        assert_eq!(dots, 6 * n as i32 + 2, "{n} sprites at x=0");
        assert_eq!(e(dots), expect[n - 1], "{n} sprites at x=0");
    }
    // 10 sprites at X=N.
    for (x, cycles) in [
        (1u8, 16),
        (2, 15),
        (5, 15),
        (7, 15),
        (8, 16),
        (16, 16),
        (160, 16),
        (167, 15),
    ] {
        assert_eq!(e(penalty(&[x; 10])), cycles, "10 sprites at x={x}");
    }
    // Off-screen X >= 168: selected but never fetched — the line
    // flips at the bare 254, i.e. -2 against the poll anchor, inside
    // the e = 0 window (mooneye lists these cases at 0 extra cycles).
    assert_eq!(penalty(&[168; 10]), -2);
    assert_eq!(penalty(&[169; 10]), -2);
    // Two groups on different BG tiles both pay the alignment penalty.
    assert_eq!(e(penalty(&[0, 0, 0, 0, 0, 160, 160, 160, 160, 160])), 17);
    assert_eq!(e(penalty(&[4, 4, 4, 4, 4, 164, 164, 164, 164, 164])), 15);
    // Single sprite at X=N.
    for (x, cycles) in [(0u8, 2), (3, 2), (4, 1), (7, 1), (8, 2), (164, 1)] {
        assert_eq!(e(penalty(&[x])), cycles, "1 sprite at x={x}");
    }
    // Two sprites 8 apart.
    assert_eq!(e(penalty(&[0, 8])), 5);
    assert_eq!(e(penalty(&[4, 12])), 3);
    // 10 sprites 8 apart.
    assert_eq!(e(penalty(&[0, 8, 16, 24, 32, 40, 48, 56, 64, 72])), 27);
    assert_eq!(e(penalty(&[1, 9, 17, 25, 33, 41, 49, 57, 65, 73])), 25);
    assert_eq!(e(penalty(&[4, 12, 20, 28, 36, 44, 52, 60, 68, 76])), 17);
    assert_eq!(e(penalty(&[5, 13, 21, 29, 37, 45, 53, 61, 69, 77])), 15);
    // Reverse OAM order: identical timing.
    assert_eq!(e(penalty(&[72, 64, 56, 48, 40, 32, 24, 16, 8, 0])), 27);
}

#[test]
fn sprites_disabled_no_penalty() {
    let mut p = dmg_on(0x91); // OBJ off
    for i in 0..10 {
        sprite(&mut p, i, 19, 0, 0, 0);
    }
    assert_eq!(render_line(&mut p, 3), 254);
}

#[test]
fn window_costs_6_dots() {
    let mut p = dmg_on(0xB1); // window on, map 0x9800 for both
    p.write(0xFF4A, 0); // WY=0
    p.write(0xFF4B, 87); // WX: window from pixel 80
    let v0 = render_line(&mut p, 2);
    // Window-stalled lines flip 1 dot before the pipe end (262).
    assert_eq!(v0, 261);
}

// --- Window rendering ---

#[test]
fn window_pixels_and_line_counter() {
    let mut p = dmg_on(0xF1); // win map 0x9C00, win on, bg map 0x9800
    p.write(0xFF4A, 1);
    p.write(0xFF4B, 15); // window from pixel 8
    set_map(&mut p, 0x1C00, 0, 0, 2);
    set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF); // window line 0: color 3
    set_tile_row(&mut p, 0, 2, 1, 0x00, 0xFF); // window line 1: color 2
    render_line(&mut p, 1);
    assert_eq!(px(&p, 1, 7), WHITE);
    assert_eq!(px(&p, 1, 8), BLACK, "first window line uses row 0");
    render_line(&mut p, 2);
    assert_eq!(
        px(&p, 2, 8),
        DARK,
        "window line counter advances independently of LY/SCY"
    );
}

#[test]
fn window_wx0_starts_at_left_edge() {
    let mut p = dmg_on(0xB1);
    p.write(0xFF4A, 0);
    p.write(0xFF4B, 0);
    set_map(&mut p, 0x1800, 0, 0, 0); // bg tile 0 (white)
    set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF);
    for col in 0..21 {
        set_map(&mut p, 0x1800, 0, col, 2); // window map = bg map here
    }
    render_line(&mut p, 0);
    // WX=0: the leading 7 window pixels fall off the left edge but the
    // window occupies the whole line.
    assert_eq!(px(&p, 0, 0), BLACK);
}

#[test]
fn window_disabled_by_lcdc5() {
    let mut p = dmg_on(0x91); // bit 5 clear
    p.write(0xFF4A, 0);
    p.write(0xFF4B, 7);
    set_map(&mut p, 0x1C00, 0, 0, 2);
    set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF);
    let v0 = render_line(&mut p, 2);
    assert_eq!(v0, 254, "no window penalty");
    assert_eq!(px(&p, 2, 0), WHITE);
}

/// A WX<=7 value written before mode 3 triggers at its prefill dot
/// even when WX is rewritten twice more mid-line (the m3_wx_5_change
/// per-line pattern): the prefill match wins and the later rewrites
/// find the window already active.
#[test]
fn wx_prefill_trigger_survives_midline_wx_rewrites() {
    let mut p = dmg_on(0xF3);
    p.write(0xFF4A, 4); // WY=4
    p.write(0xFF4B, 80);
    for r in 0..8 {
        set_tile_row(&mut p, 0, 1, r, 0xFF, 0x00); // BG LIGHT
        set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // window BLACK
    }
    for row in 0..32 {
        for col in 0..32 {
            set_map(&mut p, 0x1800, row, col, 1);
            set_map(&mut p, 0x1C00, row, col, 2);
        }
    }
    // Line 10: WX=5 early (dot 58), WX=10 at dot 100, WX=80 at dot 196.
    run_to(&mut p, 10, 56);
    mcycle_write(&mut p, 0xFF4B, 5);
    run_to(&mut p, 10, 98);
    mcycle_write(&mut p, 0xFF4B, 10);
    run_to(&mut p, 10, 194);
    mcycle_write(&mut p, 0xFF4B, 80);
    finish_line(&mut p);
    assert_eq!(px(&p, 10, 0), BLACK, "WX=5 prefill trigger: window from 0");
    assert_eq!(px(&p, 10, 80), BLACK, "window continues");
}
// --- Window machine: LCDC.5 mid-line disable / re-enable ---
//
// mattcurrie's comprehensive-ppu-doc §WIN_EN: "WIN_EN can be disabled
// during mode 3. The disabling will take effect at the end of the
// current window tile being drawn. When the current window tile has
// finished being drawn, the PPU will start drawing background tiles
// again. When the background resumes drawing it is on a tile boundary.
// The low 3 bits of SCX have no effect. [...] If WX has been updated
// correctly and WIN_EN is set again then [...] it will start drawing
// the next row of the window, on the same scanline."

/// Window at WX=15 (pixel 8); WIN_EN staged off so the pipeline view
/// commits at dot 127, mid-way through the window tile covering pixels
/// 24-31. That tile (and the fetch already in flight) finishes; the BG
/// resumes at pixel 32 on a tile boundary at the live map column
/// (gambatte ppu.cpp Tile::f0: `(scx + xpos + 1 - cgb) / 8`), without
/// re-showing the columns the window covered.
#[test]
fn win_en_disable_mid_line_finishes_window_tile_then_bg_resumes() {
    let mut p = dmg_on(0xF1); // win map 9C00, win on, data 8000, bg map 9800
    p.write(0xFF4A, 0); // WY=0
    p.write(0xFF4B, 15); // window from pixel 8
    for r in 0..8 {
        set_tile_row(&mut p, 0, 1, r, 0xFF, 0x00); // tile 1: solid LIGHT
        set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // tile 2: solid BLACK
        set_tile_row(&mut p, 0, 3, r, 0x00, 0xFF); // tile 3: solid DARK
    }
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1); // BG: LIGHT everywhere...
        set_map(&mut p, 0x1C00, 0, col, 2); // window: BLACK everywhere
    }
    set_map(&mut p, 0x1800, 0, 5, 3); // ...except BG col 5: DARK
    // Window triggers at dot 105 (lx==8); window pixel x pops at dot
    // 103+x. Stage the disable at state(124): eff commits at dot 127,
    // while the window tile covering 24-31 pops.
    run_to(&mut p, 2, 124);
    mcycle_write(&mut p, 0xFF40, 0xD1);
    let v0 = finish_line(&mut p);
    assert_eq!(px(&p, 2, 7), LIGHT, "BG before the window");
    assert_eq!(px(&p, 2, 8), BLACK, "window from pixel 8");
    assert_eq!(px(&p, 2, 31), BLACK, "current window tile finishes");
    assert_eq!(
        px(&p, 2, 32),
        LIGHT,
        "BG resumes on the tile boundary at the live column (col 4)"
    );
    assert_eq!(px(&p, 2, 39), LIGHT);
    assert_eq!(px(&p, 2, 40), DARK, "BG col 5 follows: columns 0-3 skipped");
    // DMG aborted-window line: the flip lead drops to 0 (end 262).
    assert_eq!(v0, 262, "the 6-dot window penalty is not refunded");
}

/// After a mid-line disable, re-enabling WIN_EN with WX pointing at a
/// not-yet-drawn pixel retriggers the window — drawing the *next*
/// window row on the same scanline (doc §WIN_EN; gambatte plotPixel
/// increments winYPos on every activation).
#[test]
fn win_en_reenable_same_line_draws_next_window_row() {
    let mut p = dmg_on(0xF1);
    p.write(0xFF4A, 0);
    p.write(0xFF4B, 15);
    for r in 0..8 {
        set_tile_row(&mut p, 0, 1, r, 0xFF, 0x00); // BG tile: LIGHT
    }
    set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // win row 2: BLACK
    set_tile_row(&mut p, 0, 2, 3, 0x00, 0xFF); // win row 3: DARK
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
        set_map(&mut p, 0x1C00, 0, col, 2);
    }
    run_to(&mut p, 2, 124);
    mcycle_write(&mut p, 0xFF40, 0xD1); // window off mid-tile
    p.write(0xFF4B, 107); // new WX: pixel 100, not yet drawn
    mcycle_write(&mut p, 0xFF40, 0xF1); // window back on
    let v0 = finish_line(&mut p);
    assert_eq!(px(&p, 2, 8), BLACK, "first segment: window row 2");
    assert_eq!(px(&p, 2, 99), LIGHT, "BG between the segments");
    assert_eq!(
        px(&p, 2, 100),
        DARK,
        "second segment retriggers at the new WX with the next row (3)"
    );
    assert_eq!(px(&p, 2, 108), DARK, "window column advances normally");
    assert_eq!(p.win_line, 3, "retrigger advanced the line counter");
    // Re-enabled same line: aborted + restarted, DMG lead 0 (end 268).
    assert_eq!(v0, 268, "two window starts: 256 + 6 + 6");
}

/// The window line counter increments at each activation (gambatte
/// plotPixel ++winYPos, init 0xFF at frame start), not at line end:
/// WX=166 activates every line — advancing the counter — even though
/// at most the last pixel can show window output.
#[test]
fn wx_166_advances_window_line_counter_every_line() {
    let mut p = cgb_on(0xB1); // native CGB: no DMG carryover quirk
    p.write(0xFF4A, 0); // WY=0
    p.write(0xFF4B, 166);
    render_line(&mut p, 0);
    assert_eq!(p.win_line, 0, "line 0 activation: 0xFF + 1");
    let v0 = render_line(&mut p, 1);
    assert_eq!(p.win_line, 1);
    assert_eq!(v0, 261, "CGB: the WX=166 start stalls the line end 6 dots");
    p.write(0xFF4B, 15); // normal WX on line 2
    render_line(&mut p, 2);
    assert_eq!(p.win_line, 2, "line 2 draws window row 2: rows 0-1 skipped");
}

/// DMG WX=166 quirk: the start request raised at the match cannot be
/// consumed before the pipeline ends (gambatte handleWinDrawStartReq
/// honors requests at xpos >= 167 only on CGB), so the match line
/// shows no window pixel and only pays a short freeze for the
/// aborted start (m2int_wxA6_m3stat_1/_2 bracket the DMG end between
/// 1 and 4 dots past the unextended end). The request survives to
/// the next line, which starts with the window drawing from the
/// left edge (gambatte M3Start::f0; on_screen/wxA6_wy00), re-arms
/// itself at its own match, and the chain repeats — one window row
/// per line.
#[test]
fn dmg_wx_166_no_window_pixels_counter_advances() {
    let mut p = dmg_on(0xF1);
    p.write(0xFF4A, 1); // WY=1: line 0 (the LCD-enable glitch line) is clean
    p.write(0xFF4B, 166);
    for r in 0..8 {
        set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // window: BLACK
    }
    for col in 0..32 {
        set_map(&mut p, 0x1C00, 0, col, 2);
    }
    let v0 = render_line(&mut p, 1);
    assert_eq!(v0, 257, "DMG: only the aborted-start stall extends mode 3");
    assert_eq!(px(&p, 1, 159), WHITE, "no window pixel on the match line");
    assert_eq!(p.win_line, 0, "the activation still counted a row");
    let v0 = render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), BLACK, "carryover: window from the left edge");
    assert_eq!(px(&p, 2, 159), BLACK);
    assert_eq!(p.win_line, 1, "mode-3 start consumed the request: ++row");
    assert_eq!(v0, 257, "the re-armed match pays the same freeze");
    // The carried-over activation suppresses the line's own match
    // increment but re-arms the request: one row per line.
    render_line(&mut p, 3);
    assert_eq!(px(&p, 3, 0), BLACK);
    assert_eq!(p.win_line, 2);
}

/// The WY condition is sampled at discrete dots (gambatte weMaster
/// checks at line cycles 450/454 and line 0 dot 2), not compared
/// continuously: a WY value that matches LY only *between* the
/// sample points and is gone again by the window's WX match dot
/// must not arm the frame latch. The live comparison against the
/// delayed wy2 copy covers same-line writes instead.
#[test]
fn wy_latch_samples_discretely() {
    let mut p = dmg_on(0xB1);
    p.write(0xFF4B, 87); // window at pixel 80
    p.write(0xFF4A, 200); // WY: no match anywhere
    // Mid-line on line 2 (after dot 2, before dot 451), set WY=2 and
    // move it away again before the dot-451 sample: with continuous
    // latching this would arm the window for the rest of the frame.
    run_to(&mut p, 2, 100);
    p.write(0xFF4A, 2);
    run_to(&mut p, 2, 300);
    p.write(0xFF4A, 200);
    let v0 = render_line(&mut p, 3);
    assert_eq!(v0, 254, "no window: WY matched only between samples");
    // A WY write that holds through the dot-451 sample arms the
    // latch for the rest of the frame.
    run_to(&mut p, 4, 100);
    p.write(0xFF4A, 4);
    run_to(&mut p, 5, 0);
    p.write(0xFF4A, 200);
    let v0 = render_line(&mut p, 6);
    assert_eq!(v0, 261, "the dot-451 sample armed the frame latch");
}

/// On CGB the live WY comparison uses a copy that lags the
/// architectural write by ~6 dots (gambatte video.cpp wyChange:
/// wy2 at cc+6 vs the wx-style commit at cc+2): a WY write landing
/// within 6 dots before the WX match dot is not seen by the
/// comparator on that line.
#[test]
fn cgb_wy2_lags_architectural_wy() {
    let mut p = cgb_on(0xB1);
    p.write(0xFF4B, 87); // window at pixel 80: match dot 170
    p.write(0xFF4A, 200);
    // Commit WY=2 at dot 173 of line 2: arch wy == ly at the match
    // dot 177 (lx == 80), but wy2 catches up only at dot 179.
    run_to(&mut p, 2, 173);
    p.write(0xFF4A, 2);
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 254, "wy2 still held the old value at the match");
    // Same write 5 dots earlier: wy2 caught up before the match.
    let mut p = cgb_on(0xB1);
    p.write(0xFF4B, 87);
    p.write(0xFF4A, 200);
    run_to(&mut p, 3, 168);
    p.write(0xFF4A, 3);
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 261, "wy2 caught up: the live comparison triggers");
}

/// Sprites with OAM X 0-7 are fetched during the 8-dot prefill walk
/// (positions 0-7, before any pixel pops), and the fetch pauses the
/// SCX comparator hunt: an SCX rewrite landing inside the sprite
/// stall is seen by the *paused* comparator when it resumes, not
/// missed (gambatte scx_during_m3 spx0/spx1; the mode-3 length
/// tables of intr_2_mode0_timing_sprites are unchanged because the
/// stall and discard counts are additive either way).
#[test]
fn prefill_sprite_fetch_pauses_scx_hunt() {
    // Baseline: scx=3 + one sprite at X=0 -> discard 3 + stall
    // 3 + (5 - (0+3)) = 5 (Pan Docs OBJ penalty with the
    // first-fetch discount).
    let mut p = dmg_on(0x93);
    p.write(0xFF43, 3);
    sprite(&mut p, 0, 19, 0, 0, 0);
    let v0 = render_line(&mut p, 3);
    assert_eq!(v0, 264, "discard 3 + first-sprite stall 7, flip at end - 2");
    // SCX rewritten from 7 to 2 during the sprite stall (X=0 with
    // SCX=7: stall 3 over dots 89-91, the hunt frozen at position
    // 0): the resumed hunt walks positions 1, 2 against the
    // committed SCX=2 and matches at position 2 -> discard 2. An
    // unpaused hunt would have walked past index 2 before the
    // commit, wrapped, and re-hunted through the pops.
    let mut p = dmg_on(0x93);
    p.write(0xFF43, 7);
    sprite(&mut p, 0, 19, 0, 0, 0);
    run_to(&mut p, 3, 88);
    mcycle_write(&mut p, 0xFF43, 2);
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 261, "paused hunt: discard 2 + stall 5, flip at end - 2");
}

/// WX reaches the pipeline one dot later than the palette strobe
/// (see `stage_write`): a WX=LY rewrite committing at the WX=6
/// prefill comparator dot beats the wx=6 match but not the wx=5 one
/// (mealybug m3_wx_4/5/6_change).
#[test]
fn wx_commit_is_one_dot_later_than_palettes() {
    for (early_wx, hits) in [(5u8, true), (6, false)] {
        let mut p = dmg_on(0xB1);
        p.write(0xFF4A, 0);
        p.write(0xFF4B, early_wx);
        // Stage WX=200 at state(92): the +1 commit lands at dot 96 =
        // prefill position dot for WX=6 (mode-3 dot 12), one past
        // the WX=5 dot (11).
        run_to(&mut p, 2, 92);
        mcycle_write(&mut p, 0xFF4B, 200);
        let v0 = finish_line(&mut p);
        if hits {
            assert_eq!(v0, 261, "wx=5 matched at dot 95, before the commit");
        } else {
            assert_eq!(v0, 254, "wx=6's match dot 96 already saw the rewrite");
        }
    }
}

/// A WX match while the window is already drawing ("reactivation"),
/// landing on the dot that ships the first pixel of a window tile,
/// emits one color-0 pixel and pushes the rest of the line out by a
/// dot; off-boundary matches do nothing (mealybug m3_wx_5_change
/// asm note + reference photos).
#[test]
fn window_reactivation_zero_pixel_on_tile_boundary() {
    let mut p = dmg_on(0xF1);
    p.write(0xFF4A, 0);
    p.write(0xFF4B, 15); // window from pixel 8
    for r in 0..8 {
        set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // window: BLACK
    }
    for col in 0..32 {
        set_map(&mut p, 0x1C00, 0, col, 2);
    }
    // Window tile boundaries at pixels 8, 16, 24...; pixel 16 pops
    // at dot 119 with bg_count == 8. Stage WX=23 so the comparator
    // matches lx==16 exactly there.
    run_to(&mut p, 2, 112);
    mcycle_write(&mut p, 0xFF4B, 23);
    let v0 = finish_line(&mut p);
    assert_eq!(px(&p, 2, 15), BLACK, "window before the reactivation");
    assert_eq!(px(&p, 2, 16), WHITE, "the inserted zero pixel");
    assert_eq!(px(&p, 2, 17), BLACK, "window resumes, shifted one dot");
    // The injected pixel replaces a FIFO pixel at the line's tail:
    // mode-3 length is unchanged.
    assert_eq!(v0, 261, "zero pixel does not extend mode 3");
}

/// LCDC.0 does not gate the window *machine* on DMG: with BG/window
/// display disabled the pixels blank, but the fetch stall and the
/// line-counter advance still happen (gambatte ppu.cpp lcdcWinEn
/// checks only LCDC.5; the bgen bit masks pixels at output).
#[test]
fn dmg_lcdc0_off_window_still_stalls_and_counts() {
    let mut p = dmg_on(0xB0); // window on, BG/window display off
    p.write(0xFF4A, 0);
    p.write(0xFF4B, 87); // window from pixel 80
    let v0 = render_line(&mut p, 2);
    assert_eq!(v0, 261, "window penalty applies with LCDC.0 clear");
    assert_eq!(p.win_line, 2, "line counter advances (lines 0-2)");
    assert_eq!(px(&p, 2, 80), WHITE, "pixels blank through LCDC.0");
}

#[test]
fn cgb_dmg_compat_lcdc0_gates_window() {
    // DMG compatibility mode: LCDC.0 clear blanks BG *and* window
    // pixels (Pan Docs "LCDC.0 — BG and Window enable/priority"), but
    // the window *machine* — trigger, 6-dot stall, line counter — only
    // looks at LCDC.5, exactly as on DMG (gambatte lcdcWinEn).
    let mut p = cgb_on(0xB0); // LCD on, window on, LCDC.0 = 0
    p.set_dmg_compat(true);
    p.write(0xFF4A, 0); // WY = 0
    p.write(0xFF4B, 87); // WX: window from pixel 80
    let v0 = render_line(&mut p, 2);
    assert_eq!(v0, 261, "window stall applies in compat mode, LCDC.0=0");
    assert_eq!(p.win_line, 2, "line counter advances (lines 0-2)");
    assert_eq!(px(&p, 2, 80), CGB_WHITE, "pixels blank through LCDC.0");

    // Native CGB mode: LCDC.0 is only priority — window unaffected.
    let mut p = cgb_on(0xB0);
    p.write(0xFF4A, 0);
    p.write(0xFF4B, 87);
    let v0 = render_line(&mut p, 2);
    assert_eq!(v0, 261, "native CGB: window triggers despite LCDC.0=0");
    assert_eq!(p.win_line, 2, "lines 0, 1 and 2 advanced the counter");
}

// --- Sprite rendering ---

#[test]
fn sprite_pixels_palettes_transparency() {
    let mut p = dmg_on(0x93);
    p.write(0xFF48, 0xE4);
    p.write(0xFF49, 0x1B);
    set_tile_row(&mut p, 0, 4, 0, 0x0F, 0x00); // right half color 1
    sprite(&mut p, 0, 18, 16, 4, 0x00); // line 2, screen 8-15, OBP0
    sprite(&mut p, 1, 18, 40, 4, 0x10); // screen 32-39, OBP1
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 8), WHITE, "transparent sprite pixel shows BG");
    assert_eq!(px(&p, 2, 12), LIGHT, "OBP0 color 1");
    assert_eq!(px(&p, 2, 15), LIGHT);
    assert_eq!(px(&p, 2, 16), WHITE);
    assert_eq!(px(&p, 2, 36), DARK, "OBP1 maps 1 -> 2");
}

#[test]
fn sprite_bg_priority_flag() {
    let mut p = dmg_on(0x93);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00); // bg: cols 0-3 color 1
    set_map(&mut p, 0x1800, 0, 0, 1);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0xFF); // sprite solid color 3
    sprite(&mut p, 0, 18, 8, 4, 0x80); // behind BG, screen 0-7
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), LIGHT, "BG color 1-3 beats OBJ-behind-BG");
    assert_eq!(px(&p, 2, 4), BLACK, "BG color 0 shows the sprite");
}

#[test]
fn sprite_x_flip() {
    let mut p = dmg_on(0x93);
    set_tile_row(&mut p, 0, 4, 0, 0x80, 0x00); // only leftmost pixel
    sprite(&mut p, 0, 18, 16, 4, 0x00);
    sprite(&mut p, 1, 18, 40, 4, 0x20); // X-flipped
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 8), LIGHT);
    assert_eq!(px(&p, 2, 9), WHITE);
    assert_eq!(px(&p, 2, 32), WHITE);
    assert_eq!(px(&p, 2, 39), LIGHT);
}

#[test]
fn sprite_y_flip() {
    let mut p = dmg_on(0x93);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // row 0: color 1
    set_tile_row(&mut p, 0, 4, 7, 0xFF, 0xFF); // row 7: color 3
    sprite(&mut p, 0, 18, 16, 4, 0x40); // Y-flipped: line 2 = row 7
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 8), BLACK);
}

#[test]
fn sprite_8x16_tile_masking() {
    let mut p = dmg_on(0x97); // 8x16
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // top tile row 0: color 1
    set_tile_row(&mut p, 0, 5, 0, 0xFF, 0xFF); // bottom tile row 0: color 3
    // Line 2 hits row 8 of a sprite at y=10 -> bottom tile.
    sprite(&mut p, 0, 10, 16, 5, 0x00); // tile 5: bit 0 ignored -> 4/5
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 8), BLACK, "row 8 comes from tile|1");

    let mut p = dmg_on(0x97);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    set_tile_row(&mut p, 0, 5, 0, 0xFF, 0xFF);
    sprite(&mut p, 0, 18, 16, 5, 0x00); // line 2 = row 0 -> top tile 4
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 8), LIGHT, "row 0 comes from tile&0xFE");
}

/// Sprite selection happens at OAM-scan time (mode 2) with the height
/// LCDC.2 holds *then*; the fetch re-reads LCDC.2. A game clearing
/// LCDC.2 (16 -> 8) mid-mode-3 can hand the Y-flip a scan-time row
/// (>= 8) that exceeds the fetch-time height — `h - 1 - row` must not
/// underflow (panic in debug builds).
#[test]
fn sprite_height_shrunk_between_scan_and_fetch_no_panic() {
    let mut p = dmg_on(0x97); // 8x16 sprites
    sprite(&mut p, 0, 10, 88, 4, 0x40); // line 2 = row 8, Y-flipped
    run_to(&mut p, 2, 90); // scanned during mode 2 (h=16); mode 3 running
    p.write(0xFF40, 0x93); // clear LCDC.2 before the sprite's fetch
    let mut guard = 0u32;
    while !p.line_render_done {
        p.tick();
        guard += 1;
        assert!(guard < 2_000, "mode 3 never finished");
    }
}

#[test]
fn sprite_priority_dmg_lower_x_wins() {
    let mut p = dmg_on(0x93);
    p.write(0xFF49, 0x1B);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
    sprite(&mut p, 0, 18, 20, 4, 0x00); // idx 0, screen 12-19, OBP0
    sprite(&mut p, 1, 18, 18, 4, 0x10); // idx 1, screen 10-17, OBP1
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 10), DARK, "lower-X sprite only");
    assert_eq!(px(&p, 2, 14), DARK, "lower X wins overlap on DMG");
    assert_eq!(px(&p, 2, 18), LIGHT, "higher-X sprite tail");
}

#[test]
fn sprite_priority_clipped_left_edge_lower_x_wins() {
    // Sprites with X <= 8 all trigger at lx == 0, but hardware still
    // fetches them in ascending X order (the OBJ position comparator
    // also runs through the 8-pixel prefill), so the DMG lower-X-wins
    // rule (Pan Docs "Drawing priority") holds even when the OAM order
    // is reversed.
    let mut p = dmg_on(0x93);
    p.write(0xFF49, 0x1B);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
    sprite(&mut p, 0, 18, 8, 4, 0x00); // idx 0, X=8: screen 0-7, OBP0
    sprite(&mut p, 1, 18, 3, 4, 0x10); // idx 1, X=3: screen 0-2, OBP1
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), DARK, "X=3 sprite wins the overlap");
    assert_eq!(px(&p, 2, 2), DARK, "X=3 sprite covers pixels 0-2");
    assert_eq!(px(&p, 2, 3), LIGHT, "X=8 sprite resumes at pixel 3");
    assert_eq!(px(&p, 2, 7), LIGHT);
}

#[test]
fn sprite_penalty_clipped_group_pays_in_x_order() {
    // X=0 and X=4 share the trigger (lx == 0) *and* the BG tile: the
    // leftmost sprite pays the first-per-tile alignment penalty
    // (5 - 0 = 5 dots) whichever OAM slot it sits in, so OAM order
    // [4, 0] costs the same as [0, 4]: 3 + 5 + 6 + 0 dots.
    assert_eq!(penalty(&[0, 4]), 14);
    assert_eq!(penalty(&[4, 0]), 14, "OAM order must not change timing");
}

#[test]
fn sprite_priority_same_x_oam_order() {
    let mut p = dmg_on(0x93);
    p.write(0xFF49, 0x1B);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    sprite(&mut p, 0, 18, 20, 4, 0x00); // idx 0, OBP0
    sprite(&mut p, 1, 18, 20, 4, 0x10); // idx 1, OBP1, same X
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 14), LIGHT, "lower OAM index wins at equal X");
}

// --- MGB frozen-OAM-DMA sprite glitch (madness/mgb_oam_dma_halt_sprites.s) ---

fn mgb_on(lcdc: u8) -> Ppu {
    let mut p = Ppu::new(Model::Mgb);
    p.write(0xFF47, 0xE4);
    p.write(0xFF40, lcdc);
    p
}

/// The exact scenario of the test ROM: old=$30/next=$40 in OAM, in-flight
/// byte $1A, magic-enable entry present. The glitch sprite must render at
/// Y=$38/X=$5A, tile $38, flags $5A (OBP1, Y flip, above BG, no X flip).
#[test]
fn mgb_frozen_dma_glitch_sprite_renders() {
    let mut p = mgb_on(0x93);
    p.write(0xFF48, 0x00); // OBP0 all white: proves OBP1 is selected
    p.write(0xFF49, 0xE4); // identity OBP1
    p.oam_dma_write(2, 0x30); // old
    p.oam_dma_write(3, 0x40); // next
    sprite(&mut p, 1, 0x9F, 0xA7, 0x9F, 0xA7); // magic enable entry
    set_tile_row(&mut p, 0, 0x38, 0, 0xFF, 0xFF); // solid color 3
    set_tile_row(&mut p, 0, 0x38, 7, 0x80, 0x80); // leftmost pixel only
    p.set_oam_dma_freeze(Some((2, 0x1A)));
    // Sprite Y=$38=56: first line 40. Flags Y flip: line 40 = tile row 7.
    render_line(&mut p, 40);
    assert_eq!(p.render.n_sprites, 10, "all slots hold the glitch sprite");
    assert_eq!(px(&p, 40, 81), WHITE);
    assert_eq!(px(&p, 40, 82), BLACK, "X=$5A: left edge at 82, OBP1");
    assert_eq!(px(&p, 40, 83), WHITE, "flags $5A: no X flip");
    // Last line 47 = tile row 0 (flipped): solid 8 pixels.
    render_line(&mut p, 47);
    for x in 82..90 {
        assert_eq!(px(&p, 47, x), BLACK, "x={x}");
    }
    assert_eq!(px(&p, 47, 90), WHITE);
    // Off the glitch sprite's Y range: nothing renders.
    render_line(&mut p, 48);
    assert_eq!(p.render.n_sprites, 0);
    assert_eq!(px(&p, 48, 82), WHITE);
}

/// The glitched entry formulas: Y = C = (old | new) & $FC,
/// X = F = next | new; selection by the glitched Y as usual.
#[test]
fn mgb_glitch_formulas_and_selection() {
    let mut p = mgb_on(0x93);
    sprite(&mut p, 1, 0x98, 0x00, 0x09, 0x00); // minimal magic entry
    p.oam[8] = 0x21; // old
    p.oam[9] = 0x05; // next
    p.set_oam_dma_freeze(Some((8, 0x18)));
    // (0x21|0x18) & 0xFC = 0x38; 0x05|0x18 = 0x1D.
    p.ly = 40; // row 56 = Y exactly
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 10);
    for (i, s) in p.render.sprites.iter().enumerate() {
        assert_eq!(s.y, 0x38, "slot {i}");
        assert_eq!(s.x, 0x1D, "slot {i}");
        assert_eq!(s.tile, 0x38, "slot {i}");
        assert_eq!(s.flags, 0x1D, "slot {i}");
        assert_eq!(s.idx, i as u8, "slot {i}");
    }
    p.ly = 39; // row 55: above the sprite
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 0);
    p.ly = 47; // row 63: last 8x8 line
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 10);
    p.ly = 48; // row 64: below
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 0);
    // 8x16 mode extends the match window like a normal sprite.
    p.write(0xFF40, 0x97);
    p.ly = 55; // row 71 < 56+16
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 10);
    // Clearing the freeze restores the normal scan (real OAM: nothing
    // on this line).
    p.set_oam_dma_freeze(None);
    p.ly = 40;
    p.write(0xFF40, 0x93);
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 0);
}

/// Magic-enable ranges [$98-$9F, $00-$A7, $09-$9F, $00-$A7]: each byte
/// position checked just inside and just outside its range; position in
/// OAM is irrelevant but 4-byte alignment is required.
#[test]
fn mgb_glitch_magic_enable_ranges() {
    let mut oam = [0u8; 0xA0];
    assert!(!oam_glitch_magic_enable(&oam), "all-zero OAM: no enable");
    for (entry, ok) in [
        ([0x98, 0x00, 0x09, 0x00], true),  // every byte at its low bound
        ([0x9F, 0xA7, 0x9F, 0xA7], true),  // every byte at its high bound
        ([0x97, 0x00, 0x09, 0x00], false), // byte 0 below $98
        ([0xA0, 0x00, 0x09, 0x00], false), // byte 0 above $9F
        ([0x98, 0xA8, 0x09, 0x00], false), // byte 1 above $A7
        ([0x98, 0x00, 0x08, 0x00], false), // byte 2 below $09
        ([0x98, 0x00, 0xA0, 0x00], false), // byte 2 above $9F
        ([0x98, 0x00, 0x09, 0xA8], false), // byte 3 above $A7
    ] {
        let mut oam = [0u8; 0xA0];
        oam[12..16].copy_from_slice(&entry);
        assert_eq!(oam_glitch_magic_enable(&oam), ok, "{entry:02X?}");
    }
    // "The position in OAM does not matter": last entry works too.
    oam[156..160].copy_from_slice(&[0x9F, 0xA7, 0x9F, 0xA7]);
    assert!(oam_glitch_magic_enable(&oam));
    // Misaligned in-range bytes straddling two entries do not count.
    let mut oam = [0u8; 0xA0];
    oam[14..18].copy_from_slice(&[0x98, 0x00, 0x09, 0x00]);
    assert!(!oam_glitch_magic_enable(&oam));
}

/// Without a magic-enable entry the MGB scan selects nothing at all
/// while frozen, even on a line the glitched Y would match.
#[test]
fn mgb_glitch_needs_magic_enable() {
    let mut p = mgb_on(0x93);
    p.oam[2] = 0x30;
    p.oam[3] = 0x40;
    p.set_oam_dma_freeze(Some((2, 0x1A)));
    p.ly = 40;
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 0);
    // Adding the magic entry enables it.
    sprite(&mut p, 5, 0x9F, 0xA7, 0x9F, 0xA7);
    p.oam_scan();
    assert_eq!(p.render.n_sprites, 10);
}

/// The interconnect caps the in-flight DMA index at 159, but the pub
/// `set_oam_dma_freeze` API accepts any u8: an out-of-range index must
/// degrade like the no-successor case (undriven bus reads 0xFF), not
/// panic during the next scan.
#[test]
fn mgb_glitch_freeze_index_out_of_range_no_panic() {
    let mut p = mgb_on(0x93);
    sprite(&mut p, 1, 0x9F, 0xA7, 0x9F, 0xA7); // magic enable entry
    p.set_oam_dma_freeze(Some((0xA0, 0x1A)));
    p.ly = 40;
    p.oam_scan();
    // old = next = 0xFF -> glitched Y = 0xFC: matches no visible line.
    assert_eq!(p.render.n_sprites, 0);
}

/// The glitch is MGB-only: the asm documents different (unreferenced)
/// results for DMG/CGB/AGB. With no disconnect level set those models
/// fall back to the plain scan of the frozen OAM contents (in the
/// integrated machine a freeze always coincides with the DMA owning
/// OAM, so their scans latch $FF instead — the dmg08-verified
/// gambatte oamdma_late_halt_stat rows pin that selection).
#[test]
fn frozen_dma_glitch_is_mgb_only() {
    for model in [Model::Dmg, Model::Cgb, Model::Agb] {
        let mut p = Ppu::new(model);
        p.write(0xFF40, 0x93);
        p.oam_dma_write(2, 0x30);
        p.oam_dma_write(3, 0x40);
        sprite(&mut p, 1, 0x9F, 0xA7, 0x9F, 0xA7); // magic entry
        p.set_oam_dma_freeze(Some((2, 0x1A)));
        p.ly = 40; // glitched Y would match here on MGB
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 0, "{model:?}");
        // Plain scan still sees the real (frozen) OAM: the $9F entry
        // covers rows 159-166, i.e. visible line 143 only.
        p.ly = 143;
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 1, "{model:?}");
        assert_eq!(p.render.sprites[0].y, 0x9F, "{model:?}");
    }
}

// --- dot-serial OAM scan (gbctr "OAM scan": one entry per 2 dots;
// --- gambatte sprite_mapper.cpp OamReader; SameBoy display.c mode-2
// --- loop) ---

/// The scan consumes one OAM entry per 2 dots across mode 2: an OAM
/// mutation landing mid-scan must not affect entries the scan already
/// consumed, and must reach entries it has not.
#[test]
fn oam_scan_consumes_entries_serially() {
    let mut p = dmg_on(0x93);
    sprite(&mut p, 0, 18, 20, 4, 0x00); // covers line 2
    sprite(&mut p, 30, 18, 40, 4, 0x00); // covers line 2
    run_to(&mut p, 2, 40); // mid-scan: entry 0 consumed, entry 30 not
    p.oam[0] = 0; // move both entries off every line
    p.oam[120] = 0;
    run_to(&mut p, 2, 83);
    assert_eq!(
        p.render.n_sprites, 1,
        "entry 0 was latched before the write, entry 30 after"
    );
    assert_eq!(p.render.sprites[0].idx, 0);
    // An undisturbed line selects both again (and in OAM order).
    run_to(&mut p, 3, 83);
    assert_eq!(p.render.n_sprites, 0, "post-write contents: none match");
    p.oam[0] = 18;
    p.oam[120] = 18;
    run_to(&mut p, 4, 83);
    assert_eq!(p.render.n_sprites, 2);
    assert_eq!(p.render.sprites[0].idx, 0);
    assert_eq!(p.render.sprites[1].idx, 30);
}

/// While the OAM DMA controller owns OAM, the scan's reads are
/// disconnected from it and latch $FF — a disabled sprite (gambatte
/// memory.cpp startOamDma: the OamReader's source switches to
/// rdisabledRam, all $FF, until endOamDma).
#[test]
fn oam_scan_reads_disabled_while_dma_owns_oam() {
    let mut p = dmg_on(0x93);
    sprite(&mut p, 0, 18, 20, 4, 0x00);
    sprite(&mut p, 30, 18, 40, 4, 0x00);
    run_to(&mut p, 2, 40); // entry 0 latched, entry 30 not yet
    p.set_oam_dma_active(true);
    run_to(&mut p, 2, 83);
    assert_eq!(p.render.n_sprites, 1, "entry 30's slot read $FF");
    assert_eq!(p.render.sprites[0].idx, 0);
    // A fully covered scan selects nothing.
    run_to(&mut p, 3, 83);
    assert_eq!(p.render.n_sprites, 0);
    // Reconnect mid-scan: entries scanned after it read real OAM.
    run_to(&mut p, 4, 40);
    p.set_oam_dma_active(false);
    run_to(&mut p, 4, 83);
    assert_eq!(p.render.n_sprites, 1);
    assert_eq!(p.render.sprites[0].idx, 30, "entry 0's slot read $FF");
    // Fully reconnected: both select again.
    run_to(&mut p, 5, 83);
    assert_eq!(p.render.n_sprites, 2);
}

#[test]
fn ten_sprite_limit_by_oam_order() {
    let mut p = dmg_on(0x93);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    // 11 sprites on the line; the 11th (highest OAM index) is dropped.
    for i in 0..11u8 {
        sprite(&mut p, i, 18, 8 + i * 12, 4, 0);
    }
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 9 * 12), LIGHT, "10th sprite renders");
    assert_eq!(px(&p, 2, 10 * 12), WHITE, "11th sprite dropped");
}

// --- CGB ---

fn cgb_on(lcdc: u8) -> Ppu {
    let mut p = Ppu::new(Model::Cgb);
    // BG palette 0 color 0 = white, identity-ish grayscale for colors.
    for pal in 0..2usize {
        for (c, raw) in [(0usize, 0x7FFFu16), (1, 0x294A), (2, 0x14A5), (3, 0x0000)] {
            p.bg_pal_ram[pal * 8 + c * 2] = raw as u8;
            p.bg_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
            p.obj_pal_ram[pal * 8 + c * 2] = raw as u8;
            p.obj_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
        }
    }
    // Make palette 1 color 1 pure red, obj palette 1 color 1 pure blue.
    p.bg_pal_ram[8 + 2] = 0x1F;
    p.bg_pal_ram[8 + 3] = 0x00;
    p.obj_pal_ram[8 + 2] = 0x00;
    p.obj_pal_ram[8 + 3] = 0x7C;
    p.write(0xFF40, lcdc);
    p
}

const CGB_WHITE: u32 = 0xFF_FFFF;
const RED: u32 = 0xFF_0000;
const BLUE: u32 = 0x00_00FF;

#[test]
fn cgb_color_expansion() {
    let p = cgb_on(0x91);
    assert_eq!(p.cgb_color(&p.bg_pal_ram, 0, 0), CGB_WHITE);
    assert_eq!(p.cgb_color(&p.bg_pal_ram, 1, 1), RED);
    // 5->8 bit expansion: (c << 3) | (c >> 2).
    let mut q = cgb_on(0x91);
    q.bg_pal_ram[0] = 0x10; // red = 16
    q.bg_pal_ram[1] = 0x00;
    assert_eq!(q.cgb_color(&q.bg_pal_ram, 0, 0), 0x84_0000);
}

#[test]
fn cgb_bg_attributes_palette_bank_flips() {
    let mut p = cgb_on(0x91);
    // Tile 1 data in bank 1 only; bank 0 left zero.
    set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00); // leftmost pixel color 1
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x09; // palette 1, bank 1
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), RED, "bank 1 data, palette 1");
    assert_eq!(px(&p, 2, 1), CGB_WHITE);

    // X flip.
    let mut p = cgb_on(0x91);
    set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00);
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x29; // + X flip
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), CGB_WHITE);
    assert_eq!(px(&p, 2, 7), RED);

    // Y flip: line 2 fetches tile row 5.
    let mut p = cgb_on(0x91);
    set_tile_row(&mut p, 1, 1, 5, 0x80, 0x00);
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x49; // + Y flip
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), RED);
}

#[test]
fn cgb_sprite_priority_by_oam_index() {
    let mut p = cgb_on(0x93);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
    sprite(&mut p, 0, 18, 20, 4, 0x01); // idx 0, obj palette 1 (blue)
    sprite(&mut p, 1, 18, 18, 4, 0x00); // idx 1, palette 0, lower X
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 14), BLUE, "CGB: lower OAM index wins overlap");
    // OPRI bit 0 set: DMG-style X priority.
    let mut p = cgb_on(0x93);
    p.write(0xFF6C, 1);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    sprite(&mut p, 0, 18, 20, 4, 0x01);
    sprite(&mut p, 1, 18, 18, 4, 0x00);
    render_line(&mut p, 2);
    assert_ne!(px(&p, 2, 14), BLUE, "OPRI=1: lower X wins");
}

#[test]
fn cgb_bg_priority_and_master_priority() {
    // BG attr bit 7 set, BG color nonzero: BG wins...
    let mut p = cgb_on(0x93);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00); // bg cols 0-3 color 1
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x81; // priority + palette 1
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    sprite(&mut p, 0, 18, 8, 4, 0x01); // obj palette 1 (blue)
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), RED, "BG attr priority beats sprite");
    assert_eq!(px(&p, 2, 4), BLUE, "BG color 0 always loses");

    // ...unless LCDC bit 0 is clear: master priority off.
    let mut p = cgb_on(0x92);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00);
    set_map(&mut p, 0x1800, 0, 0, 1);
    set_map(&mut p, 0x1800, 0, 2, 1);
    p.vram[0x2000 + 0x1800] = 0x81;
    p.vram[0x2000 + 0x1802] = 0x81;
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    sprite(&mut p, 0, 18, 8, 4, 0x81); // even OAM bit 7 set
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), BLUE, "LCDC0=0 strips all BG priority");
    // And the BG itself still renders (not blanked like DMG).
    assert_eq!(px(&p, 2, 9), CGB_WHITE);
    assert_eq!(px(&p, 2, 16), RED, "BG drawn where no sprite covers it");
}

#[test]
fn cgb_vbk_banks() {
    let mut p = cgb_on(0x91);
    run_to(&mut p, 145, 0); // vblank: VRAM accessible
    assert_eq!(p.read(0xFF4F), 0xFE);
    p.write(0x8000, 0x11);
    p.write(0xFF4F, 1);
    assert_eq!(p.read(0xFF4F), 0xFF);
    assert_eq!(p.read(0x8000), 0);
    p.write(0x8000, 0x22);
    assert_eq!(p.read(0x8000), 0x22);
    assert_eq!(p.vram_read_raw(0x8000), 0x22);
    p.vram_write_raw(0x9FFF, 0x33);
    assert_eq!(p.vram[0x3FFF], 0x33);
    p.write(0xFF4F, 0xFE); // only bit 0 counts
    assert_eq!(p.read(0x8000), 0x11);
    assert_eq!(p.vram_read_raw(0x8000), 0x11);
}

#[test]
fn cgb_palette_registers() {
    let mut p = cgb_on(0x91);
    run_to(&mut p, 145, 0);
    p.write(0xFF68, 0x80); // index 0, auto-increment
    p.write(0xFF69, 0x1F);
    p.write(0xFF69, 0x00);
    assert_eq!(p.read(0xFF68), 0x40 | 0x82);
    assert_eq!(p.bg_pal_ram[0], 0x1F);
    assert_eq!(p.bg_pal_ram[1], 0x00);
    p.write(0xFF68, 0x00);
    assert_eq!(p.read(0xFF69), 0x1F, "read back without increment");
    assert_eq!(p.read(0xFF68), 0x40, "reads have bit 6 set");

    p.write(0xFF6A, 0x80 | 0x10);
    p.write(0xFF6B, 0xAA);
    assert_eq!(p.obj_pal_ram[0x10], 0xAA);
    assert_eq!(p.read(0xFF6A), 0x40 | 0x91);
}

#[test]
fn cgb_palette_ram_blocked_in_mode3() {
    let mut p = cgb_on(0x91);
    p.bg_pal_ram[0] = 0x12;
    run_to(&mut p, 1, 100); // mode 3
    assert_eq!(p.read(0xFF41) & 3, 3);
    p.write(0xFF68, 0x80);
    assert_eq!(p.read(0xFF69), 0xFF, "reads blocked during mode 3");
    p.write(0xFF69, 0x77);
    assert_eq!(p.bg_pal_ram[0], 0x12, "write dropped during mode 3");
    assert_eq!(
        p.read(0xFF68) & 0x3F,
        1,
        "auto-increment still happens on a blocked write (Pan Docs)"
    );
}

#[test]
fn dmg_cgb_registers_unmapped() {
    let mut p = dmg_on(0x91);
    assert_eq!(p.read(0xFF4F), 0xFF);
    assert_eq!(p.read(0xFF68), 0xFF);
    assert_eq!(p.read(0xFF69), 0xFF);
    assert_eq!(p.read(0xFF6C), 0xFF);
    p.write(0xFF4F, 1); // ignored
    p.write(0x9000, 0x55);
    run_to(&mut p, 150, 0);
    assert_eq!(p.read(0x9000), 0x55);
}

#[test]
fn set_dmg_palette_applies() {
    let mut p = dmg_on(0x91);
    p.set_dmg_palette([0x11, 0x22, 0x33, 0x44]);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
    set_map(&mut p, 0x1800, 0, 0, 1);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), 0x22);
    assert_eq!(px(&p, 2, 4), 0x33);
    assert_eq!(px(&p, 2, 8), 0x11);
}

/// End-to-end DMG-compat rendering through the CGB boot ROM's *default*
/// compatibility palettes (Pan Docs "Compatibility palettes"; SameBoy
/// cgb_boot.asm combination OBJ0=4, OBJ1=4, BG=29): BG pixels remap
/// through BGP into the BG table, OBJ pixels through OBP0/OBP1 into the
/// distinct OBJ table. Expected XRGB values follow the c-sp collection's
/// `(X << 3) | (X >> 2)` channel expansion (dmg-acid2 README).
#[test]
fn cgb_compat_default_palette_render() {
    let mut p = Ppu::new(Model::Cgb);
    p.set_dmg_compat(true);
    // Install the boot defaults through the palette ports (LCD off — no
    // mode-3 blocking), exactly as `apply_post_boot_state` does.
    p.write(0xFF68, 0x80);
    for c in [0x7FFFu16, 0x1BEF, 0x6180, 0x0000] {
        p.write(0xFF69, c as u8);
        p.write(0xFF69, (c >> 8) as u8);
    }
    p.write(0xFF6A, 0x80);
    for _ in 0..2 {
        for c in [0x7FFFu16, 0x421F, 0x1CF2, 0x0000] {
            p.write(0xFF6B, c as u8);
            p.write(0xFF6B, (c >> 8) as u8);
        }
    }
    p.write(0xFF47, 0xE4); // identity BGP
    p.write(0xFF48, 0xE4); // identity OBP0
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
    set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // shade 3
    set_map(&mut p, 0x1800, 0, 0, 1);
    set_map(&mut p, 0x1800, 0, 1, 2);
    set_tile_row(&mut p, 0, 3, 0, 0xF0, 0x0F); // sprite: 1s then 2s
    sprite(&mut p, 0, 18, 48, 3, 0); // line 2 row 0, screen x 40-47, OBP0
    p.write(0xFF40, 0x93); // LCD + BG + OBJ on
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), 0x7BFF31, "BG shade 1");
    assert_eq!(px(&p, 2, 4), 0x0063C6, "BG shade 2");
    assert_eq!(px(&p, 2, 8), 0x00_0000, "BG shade 3");
    assert_eq!(px(&p, 2, 16), 0xFF_FFFF, "BG shade 0");
    assert_eq!(px(&p, 2, 40), 0xFF8484, "OBJ shade 1");
    assert_eq!(px(&p, 2, 44), 0x943939, "OBJ shade 2");
}

#[test]
fn frame_buffer_double_buffering() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 0, 0xFF, 0xFF);
    set_map(&mut p, 0x1800, 0, 0, 1);
    // The frame right after the LCD enable is presented blank (see
    // `first_frame_after_lcd_enable_is_blank`); double buffering is
    // observable from the second frame on.
    run_to(&mut p, 144, 0);
    run_to(&mut p, 143, 455);
    assert_eq!(p.frame()[0], WHITE, "frame() is the completed frame");
    p.tick(); // 144:0 -> swap
    assert_eq!(p.frame()[0], BLACK);
}

// --- Fetch-grid register sampling (mealybug mode-3 fetch cluster) ---
//
// On the DMG blob, every BG fetch VRAM access samples the plain eff
// view at its read dot — the same strobe the pop-anchored palette
// photographs pin (write visible from the dot after the transition
// dot). Decoded from m3_lcdc_tile_sel_change/bg_map_change blob bands,
// whose sprite-stepped stalls bracket each stage's sampling dot.
//
// Steady no-sprite grid on a bare line: tile col c's NO/LO/HI reads
// sit at dots 98/100/102 + 8*(c-1); pixel x pops at dot 97 + x.

#[test]
fn fetch_lo_read_samples_eff_at_the_read_dot() {
    let mut p = dmg_on(0x91); // bit 4: unsigned $8000 tile data
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // $8000 tile 1: color 1
    // The $8800-region alias of tile 1 ($9010) keeps row 2 = 00/00.
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
    }
    // Stage at dot 105: eff commits for reads from dot 108 = tile
    // col 2's LO read.
    run_to(&mut p, 2, 105);
    mcycle_write(&mut p, 0xFF40, 0x81); // clear LCDC.4 mid-line
    finish_line(&mut p);
    assert_eq!(px(&p, 2, 8), LIGHT, "tile 1: reads done before the write");
    assert_eq!(
        px(&p, 2, 16),
        WHITE,
        "tile 2: the LO read at dot 108 sees the committed LCDC.4"
    );
    assert_eq!(px(&p, 2, 24), WHITE, "tile 3: fully new");
}

#[test]
fn fetch_lo_read_one_dot_before_commit_keeps_old_value() {
    // Same shape staged one dot later: the LO read at dot 108 now
    // sits one dot before the eff commit (109) and must keep the old
    // data (only the HI read sees the new bank, whose row is 00/00
    // either way, so tile 2 stays color 1).
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00);
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
    }
    run_to(&mut p, 2, 106);
    mcycle_write(&mut p, 0xFF40, 0x81);
    finish_line(&mut p);
    assert_eq!(px(&p, 2, 16), LIGHT, "LO one dot before the commit: old");
}

#[test]
fn fetch_tile_no_read_samples_eff_at_the_read_dot() {
    // The tile-number read samples the same eff view
    // (m3_lcdc_bg_map_change blob bands 2/3 bracket it): a write
    // committing at dot 106 is seen by tile col 2's NO read at 106.
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // color 1 (LIGHT)
    set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // color 3 (BLACK)
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1); // $9800: tile 1
        set_map(&mut p, 0x1C00, 0, col, 2); // $9C00: tile 2
    }
    run_to(&mut p, 2, 103);
    mcycle_write(&mut p, 0xFF40, 0x99); // BG map -> $9C00
    finish_line(&mut p);
    assert_eq!(px(&p, 2, 8), LIGHT, "tile 1: NO read at 98, old map");
    assert_eq!(
        px(&p, 2, 16),
        BLACK,
        "tile 2: NO read at the commit dot 106 reads $9C00"
    );
}

#[test]
fn bg_fetcher_free_runs_during_sprite_stall() {
    let mut p = dmg_on(0x83); // BG + OBJ on, $8800-signed tile data
    set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF); // $8000 tile 0: black
    // $9000 tile 0 row 2 stays 00/00 (white); sprite tile 2 stays
    // all-zero = transparent, the stall is what matters.
    sprite(&mut p, 0, 18, 17, 2, 0);
    // LCDC.4 set for dots [106, 113] of the fetch view: stage at 104,
    // restore staged 8 dots later (the mealybug ld [hl],c / ld [hl],b
    // cadence).
    run_to(&mut p, 2, 104);
    mcycle_write(&mut p, 0xFF40, 0x93);
    for _ in 0..4 {
        p.tick();
    }
    mcycle_write(&mut p, 0xFF40, 0x83);
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 263, "10-dot stall, flip at pipe end - 3: mooneye dot");
    assert_eq!(
        px(&p, 2, 16),
        BLACK,
        "in-flight tile col 2: NO/LO/HI on stall dots 107/109/111 all \
             see the toggled tile-data bank"
    );
    assert_eq!(px(&p, 2, 24), WHITE, "tile col 3 fetched after restore");
}

#[test]
fn bg_fetcher_stall_reads_before_window_stay_old() {
    // Band-8 bracket: sprite X=8 triggers at dot 97; the free-running
    // reads (98/100/102) all precede the write window, so the
    // in-flight tile keeps the old bank even though the stall overlaps
    // the write.
    let mut p = dmg_on(0x83);
    set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF);
    sprite(&mut p, 0, 18, 8, 2, 0);
    run_to(&mut p, 2, 104);
    mcycle_write(&mut p, 0xFF40, 0x93);
    for _ in 0..4 {
        p.tick();
    }
    mcycle_write(&mut p, 0xFF40, 0x83);
    finish_line(&mut p);
    assert_eq!(
        px(&p, 2, 8),
        WHITE,
        "tile col 1 in flight at the trigger: reads 98/100/102 are old"
    );
}

#[test]
fn prefill_stall_refetch_reads_complete_before_the_walk() {
    // Prefill (X=0) sprite stall: the free-running refetch completes
    // its LO/HI reads on stall dots 94/96, well before a write
    // landing around the old frozen-refetch dots — the tile keeps
    // the old bank (m3_lcdc_tile_sel_change blob band 0).
    let mut p = dmg_on(0x83);
    set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF); // $8000 tile 0: black
    sprite(&mut p, 0, 18, 0, 2, 0); // X=0 prefill sprite, transparent
    run_to(&mut p, 2, 104);
    mcycle_write(&mut p, 0xFF40, 0x93);
    for _ in 0..4 {
        p.tick();
    }
    mcycle_write(&mut p, 0xFF40, 0x83);
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 264, "X=0 sprite: 11-dot stall, flip on its mooneye dot");
    assert_eq!(
        px(&p, 2, 0),
        WHITE,
        "tile 0 refetch: LO at 104 (before the lead) and HI on the \
             transition dot 106 (eff still old) both fetch $9000"
    );
}

#[test]
fn fetch_during_stall_samples_eff_at_the_read_dot() {
    // In-stall (free-running) fetch reads sample eff exactly like the
    // steady grid. m3_lcdc_bg_map_change blob bands 16/17: the
    // in-flight tile's NO read lands one dot before the eff commit
    // during the stall and reads the old map.
    let mut p = dmg_on(0x93); // BG + OBJ on, $8000 tiles, map $9800
    set_tile_row(&mut p, 0, 1, 2, 0x00, 0x00); // tile 1: white
    set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // tile 2: black
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
        set_map(&mut p, 0x1C00, 0, col, 2);
    }
    sprite(&mut p, 0, 18, 16, 3, 0); // X=16: trigger dot 105, stall 11
    // BG map -> $9C00 for eff dots [107, 114].
    run_to(&mut p, 2, 104);
    mcycle_write(&mut p, 0xFF40, 0x9B);
    for _ in 0..4 {
        p.tick();
    }
    mcycle_write(&mut p, 0xFF40, 0x93);
    finish_line(&mut p);
    assert_eq!(
        px(&p, 2, 16),
        WHITE,
        "in-stall NO read on the transition dot samples eff: old map"
    );
}

#[test]
fn window_start_preempts_same_dot_sprite_trigger() {
    // m3_lcdc_win_map_change band 8 (sprite X=8, WX=7): the WX match
    // and the sprite trigger land on the same dot (97), and the
    // reference shows the window's first tile fetched *before* the
    // sprite stall — its NO read sits at dot 99, ahead of a write
    // whose eff commit lands at 107 — so the window start preempts
    // the sprite fetch on the shared dot.
    let mut p = dmg_on(0xF3); // LCD + WIN ($9C00) + $8000 tiles + OBJ + BG
    set_tile_row(&mut p, 0, 0, 2, 0x00, 0x00); // tile 0: white
    set_tile_row(&mut p, 0, 1, 2, 0xFF, 0xFF); // tile 1: black
    // Window map $9C00: tile 0 (white); the toggled map $9800: tile 1
    // (black).
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1);
        set_map(&mut p, 0x1C00, 0, col, 0);
    }
    p.write(0xFF4A, 0);
    p.write(0xFF4B, 7);
    sprite(&mut p, 0, 18, 8, 2, 0); // X=8: triggers at lx 0 (dot 97)
    run_to(&mut p, 0, 2); // latch WY
    run_to(&mut p, 2, 104);
    mcycle_write(&mut p, 0xFF40, 0xB3); // win map -> $9800 (black)
    for _ in 0..4 {
        p.tick();
    }
    mcycle_write(&mut p, 0xFF40, 0xF3);
    finish_line(&mut p);
    assert_eq!(
        px(&p, 2, 0),
        WHITE,
        "window col 0 NO read at dot 99 precedes the toggle: old map"
    );
}

#[test]
fn prefill_sprite_stall_free_runs_fetcher_with_eff_sampling() {
    // m3_scy_change line 0 (sprite X=0): the refetched first tile's
    // LO/HI reads land on stall dots 94/96 sampling the live eff SCY
    // (the row written ~dot 91), while the push waits for the
    // pause-aware startup walk (first pixel stays at dot 107 — the
    // mooneye X=0 cost-10 anchor).
    let mut p = dmg_on(0x93);
    set_tile_row(&mut p, 0, 0, 2, 0xFF, 0x00); // ly2+scy0: color 1
    set_tile_row(&mut p, 0, 0, 5, 0xFF, 0xFF); // ly2+scy3: color 3
    sprite(&mut p, 0, 18, 0, 2, 0); // X=0 prefill sprite
    // SCY=3 drives eff reads from dot 93 and SCY=0 again from dot 97:
    // the in-stall LO/HI reads (dots 94/96) see 3 and fetch row 5,
    // while a frozen-prefill refetch (reads at 104/106) would see the
    // restored 0 and fetch row 2.
    run_to(&mut p, 2, 90);
    mcycle_write(&mut p, 0xFF42, 3);
    for _ in 0..4 {
        p.tick();
    }
    mcycle_write(&mut p, 0xFF42, 0);
    let v0 = finish_line(&mut p);
    assert_eq!(v0, 264, "X=0 sprite: 11-dot stall, flip on its mooneye dot");
    assert_eq!(
        px(&p, 2, 0),
        BLACK,
        "first tile fetched during the stall with the live SCY row"
    );
    assert_eq!(px(&p, 2, 8), LIGHT, "steady tiles back on SCY=0");
}

#[test]
fn obj_disable_suppresses_sprite_pixels_at_the_mix() {
    // LCDC.1 gates sprite pixels at the pixel mix, not just the
    // fetch trigger: a sprite fetched while enabled stops showing on
    // the dots where the eff view reads OBJ off
    // (m3_lcdc_obj_en_change: each band's sprite is fetched during
    // the prefill, yet the columns shipping inside the disable
    // window show background).
    let mut p = dmg_on(0x93);
    p.write(0xFF48, 0xFF); // OBP0: all black
    set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF); // sprite tile: solid c3
    sprite(&mut p, 0, 18, 10, 2, 0); // screen x 2-9, fetched at lx 2
    // Disable OBJ with eff commit at dot 109: pixels x2..3 (dots
    // 107/108 after the 9-dot stall) still show, x4+ (dots 109+) are
    // suppressed mid-sprite.
    run_to(&mut p, 2, 106);
    mcycle_write(&mut p, 0xFF40, 0x91);
    finish_line(&mut p);
    assert_eq!(px(&p, 2, 2), BLACK, "shipped before the disable");
    assert_eq!(
        px(&p, 2, 7),
        WHITE,
        "sprite pixel mixed while eff OBJ-enable is low: background"
    );
}

#[test]
fn dmg_sprite_stall_shifts_palette_boundary_one_pixel() {
    // The blob's 6-dot first OBJ fetch (see `obj_fetch_base`) puts a
    // sprite-stalled line's pop grid one dot later than the old
    // 5-dot model: the same BGP write boundary lands one pixel left
    // (m3_lcdc_obj_en_change_variant's late BGP pulse and the
    // m3_bgp_change_sprites photos pin these columns exactly).
    let mut p = dmg_on(0x93);
    sprite(&mut p, 0, 18, 2, 2, 0); // X=2 prefill, stall 6+3
    run_to(&mut p, 2, 252);
    mcycle_write(&mut p, 0xFF47, 0xFF);
    finish_line(&mut p);
    // Pop start 106: px148 pops at 254 (the blend dot), px149 at 255.
    assert_eq!(px(&p, 2, 147), WHITE, "px147 pops 253: old bgp");
    assert_eq!(px(&p, 2, 148), BLACK, "px148 pops 254: blend dot");
    assert_eq!(px(&p, 2, 149), BLACK, "px149 pops 255: committed");
}

// --- WX 0-7 trigger is pause-aware (m3_lcdc_win_map_change family) ---
//
// The WX comparator runs against the position counter, which freezes
// during sprite fetch stalls: a prefill (OAM X < 8) sprite stall
// shifts a WX<=7 match later by the stall length instead of skipping
// it. The m3_lcdc_win_map_change2 reference (WX=7 with X=1/X=5
// sprites on every line) shows the window drawn on all sprite lines.

#[test]
fn wx7_window_trigger_survives_prefill_sprite_stall() {
    // LCD + WIN (map $9C00) + unsigned tiles + OBJ + BG.
    let mut p = dmg_on(0xF3);
    // Window line counter reaches 2 on line 2 (one activation per
    // line from line 0): the window fetch reads tile row 2.
    set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF); // tile 0: black (window)
    for col in 0..32 {
        set_map(&mut p, 0x1800, 0, col, 1); // BG: tile 1 = white
        set_map(&mut p, 0x1C00, 0, col, 0); // window: tile 0 = black
    }
    p.write(0xFF4A, 0); // WY = 0
    p.write(0xFF4B, 7); // WX = 7: window from lx 0
    sprite(&mut p, 0, 18, 1, 2, 0); // X=1 prefill sprite, transparent
    run_to(&mut p, 0, 2); // latch WY at line 0 dot 2
    render_line(&mut p, 2);
    assert_eq!(
        px(&p, 2, 0),
        BLACK,
        "window starts: the WX=7 match shifted by the 10-dot stall"
    );
    assert_eq!(px(&p, 2, 100), BLACK, "window holds to the right edge");
}
