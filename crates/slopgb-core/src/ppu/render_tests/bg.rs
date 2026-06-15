//! `render_tests` — bg tests (split for file size).

use super::*;

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
