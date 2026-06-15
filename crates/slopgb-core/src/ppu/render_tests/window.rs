//! `render_tests` — window tests (split for file size).

use super::*;

#[test]
fn window_costs_6_dots() {
    let mut p = dmg_on(0xB1); // window on, map 0x9800 for both
    p.write(0xFF4A, 0); // WY=0
    p.write(0xFF4B, 87); // WX: window from pixel 80
    let v0 = render_line(&mut p, 2);
    // Window-stalled lines flip 1 dot before the pipe end (262).
    assert_eq!(v0, 261);
}

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
