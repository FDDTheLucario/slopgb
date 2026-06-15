use super::*;
use slopgb_core::{GameBoy, Model};

fn machine() -> GameBoy {
    GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).expect("zeroed rom loads")
}

#[test]
fn render_each_tool_window_fills_background_and_draws_content() {
    let theme = Theme::BGB;
    let gb = machine();
    for kind in [ToolWindow::Debugger, ToolWindow::Vram, ToolWindow::IoMap] {
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
    // The 16-col grid spans 256 px; the content area is wider. A hover left of
    // the edge resolves a tile; at/beyond column 16 there is none.
    assert!(!tile_details(0, 0).is_empty(), "col 0 -> tile 0");
    assert!(!tile_details(255, 0).is_empty(), "col 15 still in grid");
    assert!(tile_details(256, 0).is_empty(), "col 16 is blank space");
    assert!(tile_details(400, 0).is_empty(), "far right is blank");
    // Below the 24-row bank-0 grid there is no tile either.
    assert!(tile_details(0, 384).is_empty(), "row 24 past tile 383");
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
