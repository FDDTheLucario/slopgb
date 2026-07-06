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
fn flip_tile_mirrors_x_and_y() {
    // A single set pixel at (row 0, col 0) maps to the mirrored corner.
    let mut t = [[0u8; 8]; 8];
    t[0][0] = 3;
    assert_eq!(flip_tile(t, false, false), t, "identity when neither flip");
    let xf = flip_tile(t, true, false);
    assert_eq!(xf[0][7], 3, "x-flip mirrors the column");
    assert_eq!(xf[0][0], 0);
    let yf = flip_tile(t, false, true);
    assert_eq!(yf[7][0], 3, "y-flip mirrors the row");
    let both = flip_tile(t, true, true);
    assert_eq!(both[7][7], 3, "both flips mirror to the opposite corner");
}

#[test]
fn viewport_segments_wrap_at_map_edges() {
    // Non-wrapping (top-left): a single segment covering the whole 160×144 box.
    assert_eq!(
        bgmap_viewport_segments(0, 0, 160, 144, 256, 1),
        vec![Rect::new(0, 0, 160, 144)]
    );
    // Wrapping on both axes: scx=200 -> x spans (200,56)+(0,104); scy=200 ->
    // y spans (200,56)+(0,88). The Cartesian product is 4 rects.
    let segs = bgmap_viewport_segments(200, 200, 160, 144, 256, 1);
    assert_eq!(segs.len(), 4);
    assert!(segs.contains(&Rect::new(200, 200, 56, 56)), "near corner");
    assert!(segs.contains(&Rect::new(0, 0, 104, 88)), "wrapped corner");
    // Scale multiplies every coordinate.
    assert_eq!(
        bgmap_viewport_segments(0, 0, 160, 144, 256, 2),
        vec![Rect::new(0, 0, 320, 288)]
    );
}

#[test]
fn window_region_rect_from_wx_wy() {
    // WX=7,WY=0 -> the window fills the whole screen from the map origin.
    assert_eq!(window_region_rect(7, 0, 1), Some(Rect::new(0, 0, 160, 144)));
    // WX=87 -> visible width 167-87=80; WY=40 -> visible height 144-40=104.
    assert_eq!(
        window_region_rect(87, 40, 1),
        Some(Rect::new(0, 0, 80, 104))
    );
    // WX < 7: window starts off the left edge, so the visible slice shifts right
    // to map-x 7-WX and stays the full 160 wide.
    assert_eq!(window_region_rect(0, 0, 1), Some(Rect::new(7, 0, 160, 144)));
    // Fully off-screen -> no rect.
    assert_eq!(window_region_rect(167, 0, 1), None);
    assert_eq!(window_region_rect(7, 144, 1), None);
    // Scale multiplies.
    assert_eq!(window_region_rect(7, 0, 2), Some(Rect::new(0, 0, 320, 288)));
}

#[test]
fn oam_cell_is_ten_pixels_per_scale() {
    // OAM grid pitch is 10px/scale (20px at the default scale 2, as bgb shows);
    // render_oam and the OAM hover hit-test share this so they can't drift.
    assert_eq!(oam_cell(1), 10);
    assert_eq!(oam_cell(2), 20);
    assert_eq!(oam_cell(3), 30);
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
    let (w, h) = (8 * 10, 5 * 10);
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_oam(
            &mut VramRenderCtx {
                c: &mut c,
                rect: Rect::new(0, 0, w as i32, h as i32),
                vram: &vram,
                palettes: &[GREYS],
                cgb: false,
                scale,
            },
            &oam,
            false,
        );
    }
    // Cell 0 shows tile 5 (black); cell 1 (empty sprite) stays untouched.
    assert_eq!(buf[0], GREYS[3], "sprite 0 cell drawn");
    assert_eq!(
        buf[oam_cell(scale) as usize],
        0x0012_3456,
        "empty sprite 1 blank"
    );
}

#[test]
fn render_oam_honors_bank_palette_and_tall() {
    // Bank-1 tile 0 = all index 3 (bank 1 starts at 0x2000).
    let mut vram = vec![0u8; 0x4000];
    for b in &mut vram[0x2000..0x2000 + 16] {
        *b = 0xFF;
    }
    // Sprite 0: tile 0, attr 0x08 = CGB VRAM bank 1, OBJ palette 0.
    let mut oam = vec![0u8; 0xA0];
    oam[0..4].copy_from_slice(&[16, 8, 0, 0x08]);
    // Palette 0: index 0 = white, index 3 = red — so the top (tile 0, all index 3)
    // and lower (tile 1, all index 0) stacked tiles are distinguishable.
    let pal0 = [0x00FF_FFFFu32, 0x00FF_0000, 0x00FF_0000, 0x00FF_0000];
    let scale = 1;
    let (w, h) = (8 * 10, 5 * 18); // tall rows
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_oam(
            &mut VramRenderCtx {
                c: &mut c,
                rect: Rect::new(0, 0, w as i32, h as i32),
                vram: &vram,
                palettes: &[pal0],
                cgb: true, // bank + palette from attr
                scale,
            },
            &oam,
            true, // 8x16
        );
    }
    // Bank-1 tile 0 (all index 3) rendered through CGB OBJ palette 0 at the top.
    assert_eq!(buf[0], 0x00FF_0000, "bank-1 top tile via CGB obj palette");
    // 8x16: the lower stacked tile is the odd tile 1 (zeroed -> index 0 = white).
    assert_eq!(
        buf[15 * w],
        0x00FF_FFFF,
        "8x16 lower stacked tile is tile|1"
    );
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
            &mut VramRenderCtx {
                c: &mut c,
                rect: Rect::new(0, 0, w as i32, h as i32),
                vram: &vram,
                palettes: &[GREYS],
                cgb: false,
                scale,
            },
            0x9800,
            false,
            MapOverlay::Screen { scx: 8, scy: 16 },
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
            &mut VramRenderCtx {
                c: &mut c,
                rect: Rect::new(0, 0, w as i32, h as i32),
                vram: &vram,
                palettes: &[GREYS],
                cgb: false,
                scale: 1,
            },
            0x9800,
            false,
            MapOverlay::None,
            &T,
        );
    }
    // No outline drawn where the viewport edge would be.
    assert_ne!(buf[16 * w + 8], T.breakpoint, "viewport suppressed");
}

#[test]
fn render_bgmap_cgb_per_tile_palette_and_bank() {
    let mut vram = vec![0u8; 0x4000];
    // Bank-1 tile 0 (0x2000) = all index 3; bank-0 tile 0 stays index 0.
    for b in &mut vram[0x2000..0x2000 + 16] {
        *b = 0xFF;
    }
    // Map cell (0,0): tile 0 (at 0x1800), attr 0x0B = palette 3 + VRAM bank 1.
    vram[0x1800] = 0;
    vram[0x3800] = 0x0B;
    // 8 palettes; only index 3 is green so we can tell it was selected.
    let mut pals = [GREYS; 8];
    pals[3] = [0x0000_FF00; 4];
    let (w, h) = (32 * 8usize, 32 * 8usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_bgmap(
            &mut VramRenderCtx {
                c: &mut c,
                rect: Rect::new(0, 0, w as i32, h as i32),
                vram: &vram,
                palettes: &pals,
                cgb: true,
                scale: 1,
            },
            0x9800,
            false,
            MapOverlay::None,
            &T,
        );
    }
    // Bank-1 tile (index 3) through BG palette 3 (green).
    assert_eq!(buf[0], 0x0000_FF00, "per-tile CGB palette + VRAM bank");
}

#[test]
fn render_bgmap_cgb_x_flip_mirrors_the_cell() {
    let mut vram = vec![0u8; 0x4000];
    // Bank-0 tile 0 row 0: only column 0 set to index 3 (both planes bit 7).
    vram[0] = 0x80;
    vram[1] = 0x80;
    vram[0x1800] = 0; // cell (0,0) -> tile 0
    vram[0x3800] = 0x20; // attr: X-flip
    let (w, h) = (32 * 8usize, 32 * 8usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_bgmap(
            &mut VramRenderCtx {
                c: &mut c,
                rect: Rect::new(0, 0, w as i32, h as i32),
                vram: &vram,
                palettes: &[GREYS; 8],
                cgb: true,
                scale: 1,
            },
            0x9800,
            false,
            MapOverlay::None,
            &T,
        );
    }
    // X-flip moves the column-0 pixel to column 7.
    assert_eq!(buf[7], GREYS[3], "x-flip mirrors column 0 -> 7");
    assert_eq!(buf[0], GREYS[0], "original column 0 now background shade");
}

#[test]
fn render_bgmap_screen_viewport_wraps_to_the_opposite_edge() {
    let vram = vec![0u8; 0x4000];
    let (w, h) = (32 * 8usize, 32 * 8usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        // scx=200 -> the 160-wide box wraps; a segment appears at column 0.
        render_bgmap(
            &mut VramRenderCtx {
                c: &mut c,
                rect: Rect::new(0, 0, w as i32, h as i32),
                vram: &vram,
                palettes: &[GREYS],
                cgb: false,
                scale: 1,
            },
            0x9800,
            false,
            MapOverlay::Screen { scx: 200, scy: 0 },
            &T,
        );
    }
    // The wrapped segment outlines the top-left corner — impossible without wrap.
    assert_eq!(buf[0], T.breakpoint, "wrapped viewport segment at column 0");
}

#[test]
fn render_bgmap_window_overlay_draws_visible_region() {
    let vram = vec![0u8; 0x4000];
    let (w, h) = (32 * 8usize, 32 * 8usize);
    let draw = |overlay| {
        let mut buf = vec![0u32; w * h];
        {
            let mut c = Canvas::new(&mut buf, w, h);
            render_bgmap(
                &mut VramRenderCtx {
                    c: &mut c,
                    rect: Rect::new(0, 0, w as i32, h as i32),
                    vram: &vram,
                    palettes: &[GREYS],
                    cgb: false,
                    scale: 1,
                },
                0x9800,
                false,
                overlay,
                &T,
            );
        }
        buf[0]
    };
    // WX=7,WY=0 -> the window fills from the origin; its outline marks (0,0).
    assert_eq!(draw(MapOverlay::Window { wx: 7, wy: 0 }), T.breakpoint);
    // Window fully off-screen -> no outline at the origin.
    assert_ne!(draw(MapOverlay::Window { wx: 200, wy: 0 }), T.breakpoint);
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
    assert!(on_click(&mut s, area, t.x + 1, t.y + 1, false));
    assert_eq!(s.tab, VramTab::Oam);
    // Clicking the same tab again is a no-op (no redraw).
    assert!(!on_click(&mut s, area, t.x + 1, t.y + 1, false));
}

#[test]
fn click_toggles_checkboxes_and_empty_space_does_nothing() {
    let area = Rect::new(0, 0, 520, 440);
    let mut s = VramState::default();
    let l = layout(area);
    assert!(s.grid, "Grid on by default");
    assert!(on_click(
        &mut s,
        area,
        l.grid_box.x + 1,
        l.grid_box.y + 1,
        false
    ));
    assert!(!s.grid, "Grid toggled off");
    assert!(on_click(
        &mut s,
        area,
        l.grid_box.x + 1,
        l.grid_box.y + 1,
        false
    ));
    assert!(s.grid, "Grid toggled back on");
    // A click in dead space (mid-content, no widget) changes nothing.
    assert!(!on_click(
        &mut s,
        area,
        l.content.x + 1,
        l.content.y + 1,
        false
    ));
}

#[test]
fn click_tiles_bank_toggle_is_cgb_only() {
    let area = Rect::new(0, 0, 520, 440);
    let mut s = VramState::default(); // defaults to the Tiles tab
    let l = layout(area);
    let (bx, by) = (l.tile_bank_box.x + 1, l.tile_bank_box.y + 1);
    // On DMG (cgb=false) the bank toggle is inert.
    assert!(!on_click(&mut s, area, bx, by, false));
    assert_eq!(s.tile_bank, 0);
    // On CGB it flips between bank 0 and 1.
    assert!(on_click(&mut s, area, bx, by, true));
    assert_eq!(s.tile_bank, 1);
    assert!(on_click(&mut s, area, bx, by, true));
    assert_eq!(s.tile_bank, 0);
}

#[test]
fn click_source_radio_only_acts_on_bg_map_tab() {
    let area = Rect::new(0, 0, 520, 440);
    let l = layout(area);
    // On the Tiles tab the source radios are inert.
    let mut s = VramState::default();
    let r = l.map_src[2];
    assert!(!on_click(&mut s, area, r.x + 1, r.y + 1, false));
    assert_eq!(s.map_src, 0);
    // On the BG map tab they select.
    s.tab = VramTab::BgMap;
    assert!(on_click(&mut s, area, r.x + 1, r.y + 1, false));
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
