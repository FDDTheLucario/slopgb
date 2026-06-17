use super::*;

const T: Theme = Theme::BGB;

#[test]
fn tab_labels_and_render() {
    assert_eq!(
        VramTab::ALL.map(VramTab::label),
        ["BG map", "Tiles", "OAM", "Palettes"]
    );
    let (w, h) = (260usize, 20usize);
    let mut buf = vec![0u32; w * h];
    let mut c = Canvas::new(&mut buf, w, h);
    let rects = render_tabs(&mut c, 0, 0, VramTab::Oam, &T);
    assert_eq!(rects.len(), 4);
}

#[test]
fn fit_scale_is_floored_and_at_least_one() {
    assert_eq!(
        fit_scale(100, 100, 128, 192),
        1,
        "smaller than natural -> 1"
    );
    assert_eq!(fit_scale(256, 384, 128, 192), 2, "exact 2x");
    assert_eq!(fit_scale(320, 480, 128, 192), 2, "2.5x floors to 2");
    assert_eq!(fit_scale(384, 576, 128, 192), 3, "exact 3x");
    assert_eq!(fit_scale(0, 0, 128, 192), 1, "degenerate content -> 1");
    assert_eq!(
        fit_scale(1000, 100, 128, 192),
        1,
        "limited by the shorter dim"
    );
}

#[test]
fn dmg_palette_rows_map_register_shades_through_grey_ramp() {
    // BGP 0xE4 = 11_10_01_00 -> color IDs 0..3 map to shades [0,1,2,3].
    // OBP0 0x1B = 00_01_10_11 -> [3,2,1,0]. OBP1 0xFF -> [3,3,3,3].
    let rows = dmg_palette_rows(0xE4, 0x1B, 0xFF);
    assert_eq!(rows.len(), 3);
    assert_eq!((rows[0].name, rows[0].reg), ("BGP", 0xE4));
    assert_eq!(rows[0].colors, [GREYS[0], GREYS[1], GREYS[2], GREYS[3]]);
    assert_eq!((rows[1].name, rows[1].reg), ("OBP0", 0x1B));
    assert_eq!(rows[1].colors, [GREYS[3], GREYS[2], GREYS[1], GREYS[0]]);
    assert_eq!((rows[2].name, rows[2].reg), ("OBP1", 0xFF));
    assert_eq!(rows[2].colors, [GREYS[3]; 4]);
}

#[test]
fn render_tiles_blits_tile_zero_into_the_top_left_cell() {
    let mut vram = vec![0u8; 0x4000];
    for b in &mut vram[0..16] {
        *b = 0xFF; // tile 0: every pixel index 3
    }
    let scale = 2;
    let (w, h) = (16 * 8 * scale as usize, 24 * 8 * scale as usize);
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_tiles(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            &vram,
            0,
            &GREYS,
            scale,
        );
    }
    // Tile 0's top-left scaled block is palette index 3 = GREYS[3] (black).
    assert_eq!(buf[0], GREYS[3]);
    // Tile 1 (next 16 bytes are zero -> index 0 = white) sits at x = 8*scale.
    assert_eq!(buf[8 * scale as usize], GREYS[0]);
}

#[test]
fn render_oam_draws_present_sprites_and_skips_empty() {
    let mut vram = vec![0u8; 0x4000];
    // tile 5 = all index 3.
    for b in &mut vram[5 * 16..5 * 16 + 16] {
        *b = 0xFF;
    }
    let mut oam = vec![0u8; 0xA0];
    oam[0..4].copy_from_slice(&[16, 8, 5, 0]); // sprite 0: present, tile 5
    // sprite 1 left all-zero -> empty.
    let scale = 1;
    let (w, h) = (8 * (8 * scale as usize + 4), 5 * (8 * scale as usize + 4));
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_oam(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            &oam,
            &vram,
            &GREYS,
            scale,
        );
    }
    // Cell 0 shows tile 5 (black); cell 1 (empty sprite) stays untouched.
    assert_eq!(buf[0], GREYS[3], "sprite 0 cell drawn");
    let cell = 8 * scale as usize + 4;
    assert_eq!(buf[cell], 0x0012_3456, "empty sprite 1 cell blank");
}

#[test]
fn tile_index_resolves_signed_and_unsigned_addressing() {
    assert_eq!(tile_index(0, false), 0);
    assert_eq!(tile_index(255, false), 255);
    // Signed (0x8800): n is i8 relative to tile 256.
    assert_eq!(tile_index(0, true), 256);
    assert_eq!(tile_index(127, true), 383);
    assert_eq!(tile_index(0x80, true), 128); // -128
    assert_eq!(tile_index(0xFF, true), 255); // -1
}

#[test]
fn render_bgmap_draws_cells_and_the_viewport_outline() {
    let mut vram = vec![0u8; 0x4000];
    // tile 0 = all index 3; map cell (0,0) at 0x9800 (offset 0x1800) -> tile 0.
    for b in &mut vram[0..16] {
        *b = 0xFF;
    }
    let scale = 1;
    let (w, h) = (32 * 8usize, 32 * 8usize);
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_bgmap(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            &vram,
            0x9800,
            false,
            8,
            16,
            &GREYS,
            scale,
            true,
            &T,
        );
    }
    // Cell (0,0) drew tile 0 (black) at the top-left.
    assert_eq!(buf[0], GREYS[3]);
    // The viewport outline (theme.breakpoint = red) sits at (scx=8, scy=16).
    assert_eq!(buf[16 * w + 8], T.breakpoint, "viewport top edge at (8,16)");
}

#[test]
fn render_bgmap_omits_the_viewport_when_disabled() {
    let mut vram = vec![0u8; 0x4000];
    for b in &mut vram[0..16] {
        *b = 0xFF;
    }
    let (w, h) = (32 * 8usize, 32 * 8usize);
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_bgmap(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            &vram,
            0x9800,
            false,
            8,
            16,
            &GREYS,
            1,
            false,
            &T,
        );
    }
    // No outline drawn where the viewport edge would be.
    assert_ne!(buf[16 * w + 8], T.breakpoint, "viewport suppressed");
}

#[test]
fn vram_state_defaults_match_bgb() {
    let s = VramState::default();
    assert_eq!(s.tab, VramTab::Tiles);
    assert!(s.grid);
    assert!(s.show_paletted);
    assert!(s.scxy);
    assert_eq!((s.map_src, s.tile_src), (0, 0));
    assert_eq!(s.hover, None);
}

#[test]
fn layout_partitions_the_window_without_overlap() {
    let area = Rect::new(0, 0, 520, 440);
    let l = layout(area);
    assert_eq!(l.tabs.len(), 4);
    assert!(l.tabs[0].x < l.tabs[1].x, "tabs left-to-right");
    // Content sits below the tabs; details sits to its right; they don't overlap.
    assert!(l.content.y >= l.tabs[0].bottom());
    assert!(l.details.x >= l.content.right());
    assert!(l.content.intersect(&l.details).w == 0);
    // Controls live inside the details column.
    for r in [l.grid_box, l.paletted_box, l.scxy_box] {
        assert!(r.x >= l.details.x, "control inside details panel");
        assert!(r.bottom() <= area.bottom());
    }
}

#[test]
fn click_on_a_tab_switches_the_active_tab() {
    let area = Rect::new(0, 0, 520, 440);
    let mut s = VramState::default();
    let l = layout(area);
    // Click the OAM tab (index 2).
    let t = l.tabs[2];
    assert!(on_click(&mut s, area, t.x + 1, t.y + 1));
    assert_eq!(s.tab, VramTab::Oam);
    // Clicking the same tab again is a no-op (no redraw).
    assert!(!on_click(&mut s, area, t.x + 1, t.y + 1));
}

#[test]
fn click_toggles_checkboxes_and_empty_space_does_nothing() {
    let area = Rect::new(0, 0, 520, 440);
    let mut s = VramState::default();
    let l = layout(area);
    assert!(s.grid, "Grid on by default");
    assert!(on_click(&mut s, area, l.grid_box.x + 1, l.grid_box.y + 1));
    assert!(!s.grid, "Grid toggled off");
    assert!(on_click(&mut s, area, l.grid_box.x + 1, l.grid_box.y + 1));
    assert!(s.grid, "Grid toggled back on");
    // A click in dead space (mid-content, no widget) changes nothing.
    assert!(!on_click(&mut s, area, l.content.x + 1, l.content.y + 1));
}

#[test]
fn click_source_radio_only_acts_on_bg_map_tab() {
    let area = Rect::new(0, 0, 520, 440);
    let l = layout(area);
    // On the Tiles tab the source radios are inert.
    let mut s = VramState::default();
    let r = l.map_src[2];
    assert!(!on_click(&mut s, area, r.x + 1, r.y + 1));
    assert_eq!(s.map_src, 0);
    // On the BG map tab they select.
    s.tab = VramTab::BgMap;
    assert!(on_click(&mut s, area, r.x + 1, r.y + 1));
    assert_eq!(s.map_src, 2);
}

#[test]
fn hover_tracks_only_the_content_area() {
    let area = Rect::new(0, 0, 520, 440);
    let mut s = VramState::default();
    let l = layout(area);
    let (cx, cy) = (l.content.x + 5, l.content.y + 5);
    assert!(on_hover(&mut s, area, cx, cy));
    assert_eq!(s.hover, Some((cx, cy)));
    // Same spot again: no change.
    assert!(!on_hover(&mut s, area, cx, cy));
    // Moving into the details panel clears the hover.
    assert!(on_hover(&mut s, area, l.details.x + 2, l.details.y + 2));
    assert_eq!(s.hover, None);
}

#[test]
fn render_palettes_expands_cgb_colour_words() {
    let mut bg = vec![0u8; 64];
    bg[0] = 0xFF; // palette 0, colour 0 = 0x7FFF -> white
    bg[1] = 0x7F;
    let obj = vec![0u8; 64];
    let (w, h) = (200usize, 160usize);
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_palettes(&mut c, Rect::new(0, 0, w as i32, h as i32), &bg, &obj, &T);
    }
    // First swatch's interior is white (0x7FFF expanded).
    assert_eq!(buf[2 * w + 2], 0x00FF_FFFF);
}
