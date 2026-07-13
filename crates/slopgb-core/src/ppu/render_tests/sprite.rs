//! `render_tests` — sprite tests (split for file size).

use super::*;

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
    assert_eq!(px(&p, 2, 148), WHITE, "px148 pops 254: blend dot");
    assert_eq!(px(&p, 2, 149), WHITE, "px149 pops 255: committed");
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
    assert_eq!(v0, 269, "paused hunt: discard 2 + stall 5, flip at end - 2");
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
        LIGHT,
        "first tile fetched during the stall with the live SCY row"
    );
    assert_eq!(px(&p, 2, 8), LIGHT, "steady tiles back on SCY=0");
}
