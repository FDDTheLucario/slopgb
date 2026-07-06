use super::*;
use slopgb_core::{GameBoy, Model};

fn machine() -> GameBoy {
    GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).expect("zeroed rom loads")
}

#[test]
fn render_each_tool_window_fills_background_and_draws_content() {
    let theme = Theme::BGB;
    let gb = machine();
    for kind in [
        ToolWindow::Debugger,
        ToolWindow::Vram,
        ToolWindow::IoMap,
        ToolWindow::MemoryViewer,
    ] {
        let (w, h) = (640usize, 480usize);
        let mut buf = vec![0xDEAD_BEEF_u32; w * h];
        {
            let mut c = Canvas::new(&mut buf, w, h);
            render(
                kind,
                &gb,
                &mut c,
                &theme,
                &WinState::new(kind),
                &Breakpoints::default(),
            );
        }
        // The whole surface was painted (no leftover sentinel) and the window
        // background + some text ink are present.
        assert!(
            !buf.contains(&0xDEAD_BEEF),
            "{kind:?}: surface fully painted"
        );
        assert!(buf.contains(&theme.bg), "{kind:?}: background filled");
        assert!(buf.contains(&theme.text), "{kind:?}: content drawn");
    }
}

#[test]
fn memory_view_scroll_wraps_by_rows() {
    let mut m = MemoryView::default();
    assert_eq!(m.mem_base, 0xFF00);
    m.scroll(-1);
    assert_eq!(m.mem_base, 0xFEF0);
    m.scroll(2);
    assert_eq!(m.mem_base, 0xFF10);
    m.mem_base = 0xFFF0;
    m.scroll(1);
    assert_eq!(m.mem_base, 0x0000, "wraps past the top");
}

#[test]
fn memory_view_goto_resolves_hex_symbol_and_ignores_junk() {
    use crate::symbols::SymbolTable;
    use std::rc::Rc;
    let mut v = MemoryView::default();
    assert!(v.apply_goto("C000"));
    assert_eq!(v.mem_base, 0xC000);
    assert!(v.apply_goto("$8000"), "accepts $ prefix");
    assert_eq!(v.mem_base, 0x8000);
    assert!(!v.apply_goto("zzz"), "garbage rejected");
    assert_eq!(v.mem_base, 0x8000, "junk leaves base unchanged");
    // A loaded symbol name resolves to its address.
    v.symbols = Rc::new(SymbolTable::parse("00:1234 Foo"));
    assert!(v.apply_goto("Foo"));
    assert_eq!(v.mem_base, 0x1234);
}

#[test]
fn memory_view_edit_two_nibbles_commit_a_byte_and_advance() {
    let mut v = MemoryView {
        cursor: 0xC000,
        ..Default::default()
    };
    // First nibble is held, no write yet.
    assert_eq!(v.edit_hex_digit(0xA), None);
    assert_eq!(v.edit_hi, Some(0xA));
    // Second nibble completes 0xA5, returns the write, advances the cursor.
    assert_eq!(v.edit_hex_digit(0x5), Some((0xC000, 0xA5)));
    assert_eq!(v.edit_hi, None);
    assert_eq!(v.cursor, 0xC001, "cursor advanced to the next byte");
}

#[test]
fn memory_view_cancel_edit_discards_pending_nibble() {
    let mut v = MemoryView::default();
    assert!(!v.cancel_edit(), "nothing to cancel when idle");
    v.edit_hex_digit(0xF);
    assert!(v.cancel_edit(), "a pending edit is cancelled");
    assert_eq!(v.edit_hi, None);
    assert!(!v.cancel_edit(), "already cancelled");
}

#[test]
fn memory_view_cursor_move_autoscrolls_and_cancels_edit() {
    let mut v = MemoryView::default(); // mem_base = cursor = 0xFF00
    v.edit_hex_digit(0xC); // start an edit
    v.move_cursor(-16, 8); // up one row cancels the edit and scrolls the view
    assert_eq!(v.edit_hi, None, "moving cancels a pending edit");
    assert_eq!(v.cursor, 0xFEF0);
    assert_eq!(v.mem_base, 0xFEF0, "scrolled up so the cursor stays visible");
    // Moving within the visible window does not scroll.
    v.move_cursor(16, 8);
    assert_eq!(v.cursor, 0xFF00);
    assert_eq!(v.mem_base, 0xFEF0, "cursor still visible, no scroll");
}

#[test]
fn memory_window_tints_cdl_flagged_bytes() {
    let mut gb = machine();
    // ROM low area (bank 0): the physical CDL index equals the GB address, so
    // the fixture index is unambiguous under the bank-aware layout.
    let base = 0x0100u16;
    gb.set_cdl(true);
    let mut fixture = gb.cdl_flags().unwrap().to_vec();
    fixture[base as usize] = 4; // X → red at column 0
    assert!(gb.load_cdl(&fixture));
    // Cursor left at the default 0xFF00 (off-screen from base) so its overlay
    // can't cover the tinted cell.
    let st = WinState::Memory(MemoryView {
        mem_base: base,
        ..MemoryView::default()
    });
    let (w, h) = (430usize, 360usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(
            ToolWindow::MemoryViewer,
            &gb,
            &mut c,
            &Theme::BGB,
            &st,
            &Breakpoints::default(),
        );
    }
    let gw = crate::ui::font::GLYPH_W;
    let lh = crate::ui::text::line_height() as usize;
    let want = crate::cdl::cdl_color(4).unwrap();
    let cell_has = |cx: usize, color: u32| {
        (0..lh).any(|y| (cx..cx + 2 * gw).any(|x| buf[y * w + x] == color))
    };
    // Column 0 hex starts at char 10; the flagged byte's cell is tinted.
    assert!(cell_has(10 * gw, want), "flagged byte cell tinted");
    // Column 1 (char 13) is unflagged → not tinted.
    assert!(!cell_has(13 * gw, want), "unflagged byte not tinted");
}

#[test]
fn mem_bank_label_names_the_live_banked_region() {
    let gb = machine(); // DMG, ROM-only (no MBC, no external RAM)
    assert_eq!(mem_bank_label(&gb, 0x0100), None, "fixed ROM bank 0");
    assert_eq!(mem_bank_label(&gb, 0x4000).as_deref(), Some("ROM01"), "None-mapper high bank");
    assert_eq!(mem_bank_label(&gb, 0x8000).as_deref(), Some("VRM0"));
    assert_eq!(mem_bank_label(&gb, 0xA000), None, "no RAM chip");
    assert_eq!(mem_bank_label(&gb, 0xC000).as_deref(), Some("WRM0"));
    assert_eq!(mem_bank_label(&gb, 0xD000).as_deref(), Some("WRM1"), "DMG WRAM bank 1");
    assert_eq!(mem_bank_label(&gb, 0xFF80), None, "HRAM unbanked");
}

#[test]
fn mem_bank_label_follows_cgb_wram_and_mbc_rom_banks() {
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x143] = 0x80; // CGB
    rom[0x147] = 0x19; // MBC5
    rom[0x148] = 0x03; // 8 banks
    let mut gb = GameBoy::new(Model::Cgb, rom).unwrap();
    gb.debug_write(0x2000, 5); // MBC5 ROMB0 = 5
    assert_eq!(mem_bank_label(&gb, 0x4000).as_deref(), Some("ROM05"));
    gb.debug_write(0xFF70, 3); // SVBK = 3
    assert_eq!(mem_bank_label(&gb, 0xD000).as_deref(), Some("WRM3"));
    assert_eq!(mem_bank_label(&gb, 0x8000).as_deref(), Some("VRM0"));
}

#[test]
fn memory_window_status_bar_shows_nearest_symbol() {
    use crate::symbols::SymbolTable;
    use std::rc::Rc;
    let gb = machine();
    let theme = Theme::BGB;
    let st = WinState::Memory(MemoryView {
        mem_base: 0x4008,
        symbols: Rc::new(SymbolTable::parse("00:4000 Reset")),
        goto: None,
        cursor: 0x4008,
        edit_hi: None,
    });
    let (w, h) = (430usize, 360usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(
            ToolWindow::MemoryViewer,
            &gb,
            &mut c,
            &theme,
            &st,
            &Breakpoints::default(),
        );
    }
    // The status bar text is rendered (some ink in the bottom line).
    let lh = crate::ui::text::line_height() as usize;
    let bar_row = (h - lh) * w;
    assert!(
        buf[bar_row..].contains(&theme.text),
        "status bar drawn in the bottom row"
    );
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
    assert!(!tile_details(0, 0, 2).is_empty(), "col 0 -> tile 0");
    assert!(!tile_details(255, 0, 2).is_empty(), "col 15 still in grid");
    assert!(tile_details(256, 0, 2).is_empty(), "col 16 is blank space");
    assert!(tile_details(400, 0, 2).is_empty(), "far right is blank");
    // Below the 24-row bank-0 grid there is no tile either.
    assert!(tile_details(0, 384, 2).is_empty(), "row 24 past tile 383");
}

#[test]
fn tiles_two_col_splits_content_into_nonoverlapping_left_right() {
    let content = Rect::new(10, 20, 400, 400);
    let (left, right, s) = tiles_two_col(content);
    assert!(s >= 1);
    assert_eq!((left.x, left.y), (10, 20), "left grid at content origin");
    assert_eq!((left.w, left.h), (16 * 8 * s, 24 * 8 * s), "left is a 16x24 grid");
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
    assert_eq!((left.w, left.h), (32 * 8 * s, 32 * 8 * s), "left is a 32x32 grid");
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
    let d0 = bgmap_details_two(&gb, &s, 4, 4, left, right, sc);
    assert!(d0[0].starts_with("BG"), "{d0:?}");
    // Hover inside the right grid -> Window map.
    let rx = (right.x - left.x) + 4;
    let d1 = bgmap_details_two(&gb, &s, rx, 4, left, right, sc);
    assert!(d1[0].starts_with("Window"), "{d1:?}");
    // Hover in the gutter -> no cell.
    assert!(bgmap_details_two(&gb, &s, left.w + 1, 4, left, right, sc).is_empty());
}

#[test]
fn tile_details_two_maps_hover_to_bank_and_prints_real_bank() {
    let content = Rect::new(0, 0, 400, 400);
    let (left, right, s) = tiles_two_col(content);
    // Hover inside the left grid -> bank 0.
    let d0 = tile_details_two(4, 4, left, right, s);
    assert!(d0[1].starts_with("Tile Address 0:"), "{d0:?}");
    // Hover inside the right grid -> bank 1 (real bank in the label).
    let rx = (right.x - left.x) + 4; // content-relative x just inside right grid
    let d1 = tile_details_two(rx, 4, left, right, s);
    assert!(d1[1].starts_with("Tile Address 1:"), "{d1:?}");
    // Hover in the gutter between the grids -> no tile.
    assert!(tile_details_two(left.w + 1, 4, left, right, s).is_empty());
}

#[test]
fn tile_details_track_the_live_scale() {
    // The same hover pixel resolves to a different tile at a different scale, so
    // the details hit-test must use the live (fitted) scale, not a fixed one.
    assert_eq!(
        tile_details(32, 0, 2)[0],
        "Tile No. 2",
        "16px/tile at scale 2"
    );
    assert_eq!(
        tile_details(32, 0, 3)[0],
        "Tile No. 1",
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

#[test]
fn render_is_side_effect_free_on_the_machine() {
    // Rendering must not advance or mutate emulation (it takes &GameBoy).
    let gb = machine();
    let before = (gb.cycles(), gb.frame_count(), gb.cpu_regs().pc);
    let (w, h) = (320usize, 240usize);
    let mut buf = vec![0u32; w * h];
    let mut c = Canvas::new(&mut buf, w, h);
    render(
        ToolWindow::Debugger,
        &gb,
        &mut c,
        &Theme::BGB,
        &WinState::Stateless,
        &Breakpoints::default(),
    );
    assert_eq!((gb.cycles(), gb.frame_count(), gb.cpu_regs().pc), before);
}
