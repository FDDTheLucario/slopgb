//! Tests for the VRAM viewer rendering (moved with the code out of
//! `windows_tests.rs`).

use super::*;
use slopgb_core::{GameBoy, Model};

fn machine() -> GameBoy {
    GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).expect("zeroed rom loads")
}

#[test]
fn vram_render_routes_each_tab_and_shows_hover_details() {
    use vram::{VramState, VramTab};
    let gb = machine();
    let theme = Theme::BGB;
    let (w, h) = (560usize, 470usize);
    let render_tab = |tab, hover| {
        let mut buf = vec![0u32; w * h];
        let st = WinState::Vram(VramState {
            tab,
            hover,
            ..VramState::default()
        });
        let mut c = Canvas::new(&mut buf, w, h);
        render(
            ToolWindow::Vram,
            &gb,
            &mut c,
            &theme,
            &st,
            &Breakpoints::default(),
        );
        buf
    };
    // Each tab routes to a different renderer, so their buffers differ.
    let tiles = render_tab(VramTab::Tiles, None);
    let palettes = render_tab(VramTab::Palettes, None);
    assert_ne!(tiles, palettes, "tabs route to distinct content");
    // Hovering a content cell draws the details field list (extra ink).
    let l = vram::layout(Rect::new(0, 0, w as i32, h as i32));
    let hovered = render_tab(VramTab::Tiles, Some((l.content.x + 5, l.content.y + 5)));
    assert_ne!(hovered, tiles, "hover adds the details panel");
}

#[test]
fn tile_details_has_no_phantom_tile_right_of_the_grid() {
    // At scale 2 the 16-col grid spans 256 px; the content area is wider. A
    // hover left of the edge resolves a tile; at/beyond column 16 there is none.
    assert!(!tile_details(0, 0, 2, false).is_empty(), "col 0 -> tile 0");
    assert!(
        !tile_details(255, 0, 2, false).is_empty(),
        "col 15 still in grid"
    );
    assert!(
        tile_details(256, 0, 2, false).is_empty(),
        "col 16 is blank space"
    );
    assert!(
        tile_details(400, 0, 2, false).is_empty(),
        "far right is blank"
    );
    // Below the 24-row bank-0 grid there is no tile either.
    assert!(
        tile_details(0, 384, 2, false).is_empty(),
        "row 24 past tile 383"
    );
}

#[test]
fn dec_hex_shows_decimal_and_uppercase_hex() {
    assert_eq!(dec_hex(10, false), "10 ($0A)");
    assert_eq!(dec_hex(0, false), "0 ($00)");
    assert_eq!(dec_hex(255, false), "255 ($FF)");
    assert_eq!(
        dec_hex(383, false),
        "383 ($17F)",
        "widens past two hex digits"
    );
    // mask8 (Options "8-bit tile hex") wraps the hex to the low byte.
    assert_eq!(dec_hex(383, true), "383 ($7F)", "$17F & 0xFF -> $7F");
    assert_eq!(dec_hex(256, true), "256 ($00)");
    assert_eq!(
        dec_hex(10, true),
        "10 ($0A)",
        "sub-256 unchanged by the mask"
    );
}

#[test]
fn tile_details_appends_hex_to_the_tile_number() {
    // scale 2 -> 16 px/cell. Top-left tile 0.
    assert_eq!(tile_details(0, 0, 2, false)[0], "Tile No. 0 ($00)");
    // col 15, row 23 -> tile 23*16+15 = 383 -> three hex digits.
    assert_eq!(
        tile_details(15 * 16, 23 * 16, 2, false)[0],
        "Tile No. 383 ($17F)"
    );
    // With the 8-bit-hex option the same tile wraps to $7F.
    assert_eq!(
        tile_details(15 * 16, 23 * 16, 2, true)[0],
        "Tile No. 383 ($7F)"
    );
}

#[test]
fn tiles_two_col_splits_content_into_nonoverlapping_left_right() {
    let content = Rect::new(10, 20, 400, 400);
    let (left, right, s) = tiles_two_col(content);
    assert!(s >= 1);
    assert_eq!((left.x, left.y), (10, 20), "left grid at content origin");
    assert_eq!(
        (left.w, left.h),
        (16 * 8 * s, 24 * 8 * s),
        "left is a 16x24 grid"
    );
    assert_eq!((right.w, right.h), (left.w, left.h), "same size grids");
    assert!(right.x >= left.x + left.w, "no horizontal overlap");
    assert!(
        right.x + right.w <= content.x + content.w,
        "right grid fits inside content"
    );
}

#[test]
fn bgmap_two_col_splits_content_into_nonoverlapping_left_right() {
    let content = Rect::new(10, 20, 600, 300);
    let (left, right, s) = bgmap_two_col(content);
    assert!(s >= 1);
    assert_eq!((left.x, left.y), (10, 20), "left grid at content origin");
    assert_eq!(
        (left.w, left.h),
        (32 * 8 * s, 32 * 8 * s),
        "left is a 32x32 grid"
    );
    assert_eq!((right.w, right.h), (left.w, left.h), "same size grids");
    assert!(right.x >= left.x + left.w, "no horizontal overlap");
    assert!(
        right.x + right.w <= content.x + content.w,
        "right grid fits inside content"
    );
}

#[test]
fn bgmap_bases_derive_bg_from_bit3_and_window_from_bit6() {
    let mut gb = machine();
    // Auto: LCDC bit3 (BG select) and bit6 (window select) pick each grid's base.
    // Zeroed ROM leaves LCDC=0 → both auto to 0x9800; flip bit6 → window to 0x9C00.
    gb.debug_write(0xFF40, 0x40); // window tilemap select on, BG select off
    let s = VramState {
        tab: VramTab::BgMap,
        ..VramState::default()
    };
    let (bg, win, _signed) = bgmap_bases(&gb, &s);
    assert_eq!(bg, 0x9800, "BG grid uses LCDC bit3 (off)");
    assert_eq!(win, 0x9C00, "window grid uses LCDC bit6 (on)");
}

#[test]
fn bgmap_details_two_maps_hover_to_bg_or_window_grid() {
    let gb = machine();
    let s = VramState {
        tab: VramTab::BgMap,
        ..VramState::default()
    };
    let content = Rect::new(0, 0, 600, 300);
    let (left, right, sc) = bgmap_two_col(content);
    // Hover inside the left grid -> BG map.
    let d0 = bgmap_details_two(&gb, &s, 4, 4, left, right, sc, false);
    assert!(d0[0].starts_with("BG"), "{d0:?}");
    // Hover inside the right grid -> Window map.
    let rx = (right.x - left.x) + 4;
    let d1 = bgmap_details_two(&gb, &s, rx, 4, left, right, sc, false);
    assert!(d1[0].starts_with("Window"), "{d1:?}");
    // Hover in the gutter -> no cell.
    assert!(bgmap_details_two(&gb, &s, left.w + 1, 4, left, right, sc, false).is_empty());
}

#[test]
fn tile_details_two_maps_hover_to_bank_and_prints_real_bank() {
    let content = Rect::new(0, 0, 400, 400);
    let (left, right, s) = tiles_two_col(content);
    // Hover inside the left grid -> bank 0.
    let d0 = tile_details_two(4, 4, left, right, s, false);
    assert!(d0[1].starts_with("Tile Address 0:"), "{d0:?}");
    // Hover inside the right grid -> bank 1 (real bank in the label).
    let rx = (right.x - left.x) + 4; // content-relative x just inside right grid
    let d1 = tile_details_two(rx, 4, left, right, s, false);
    assert!(d1[1].starts_with("Tile Address 1:"), "{d1:?}");
    // Hover in the gutter between the grids -> no tile.
    assert!(tile_details_two(left.w + 1, 4, left, right, s, false).is_empty());
}

#[test]
fn tile_details_track_the_live_scale() {
    // The same hover pixel resolves to a different tile at a different scale, so
    // the details hit-test must use the live (fitted) scale, not a fixed one.
    assert_eq!(
        tile_details(32, 0, 2, false)[0],
        "Tile No. 2 ($02)",
        "16px/tile at scale 2"
    );
    assert_eq!(
        tile_details(32, 0, 3, false)[0],
        "Tile No. 1 ($01)",
        "24px/tile at scale 3"
    );
}

#[test]
fn vram_geom_bounds_the_extent_within_a_large_content_area() {
    // A content area larger than the natural map: the drawn extent hugs the
    // bounded map (cols*8*scale), not the whole content rect — QA "bg map should
    // be bounded". 600/256 -> scale 2 -> 512 square, inside 600.
    let content = Rect::new(0, 0, 600, 600);
    let bg = vram_geom(VramTab::BgMap, content, false);
    assert_eq!(bg.scale, 2);
    assert_eq!((bg.extent.w, bg.extent.h), (32 * 8 * 2, 32 * 8 * 2));
    assert!(
        bg.extent.w < content.w && bg.extent.h < content.h,
        "bounded"
    );
    // Tiles: 128x192 natural; 600/128=4, 600/192=3 -> scale 3.
    let tiles = vram_geom(VramTab::Tiles, content, false);
    assert_eq!(tiles.scale, 3);
    assert_eq!((tiles.extent.w, tiles.extent.h), (16 * 8 * 3, 24 * 8 * 3));
    // Palettes has no grid; frames the whole content.
    let pal = vram_geom(VramTab::Palettes, content, false);
    assert!(!pal.grid);
    assert_eq!(pal.extent, content);
}
