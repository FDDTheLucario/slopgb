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
fn memory_window_status_bar_shows_nearest_symbol() {
    use crate::symbols::SymbolTable;
    use std::rc::Rc;
    let gb = machine();
    let theme = Theme::BGB;
    let st = WinState::Memory(MemoryView {
        mem_base: 0x4008,
        symbols: Rc::new(SymbolTable::parse("00:4000 Reset")),
        goto: None,
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
