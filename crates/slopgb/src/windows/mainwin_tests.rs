use super::*;
use crate::ui::canvas::Rect;
use crate::ui::menu::menu_rects;

/// The row index of each labelled item, for click coordinates.
fn row_rect(m: &MainMenu, idx: usize) -> Rect {
    menu_rects(m.origin, &m.items)[idx]
}

/// Click point at the centre of row `idx`.
fn at(m: &MainMenu, idx: usize) -> (i32, i32) {
    let r = row_rect(m, idx);
    (r.x + r.w / 2, r.y + r.h / 2)
}

#[test]
fn menu_has_the_fifteen_rc_main_rows_in_order() {
    let m = MainMenu::open((10, 10), true);
    let labels: Vec<&str> = m.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Pause",
            "Load ROM...",
            "Enable sound",
            "Options...",
            "Cheat...",
            "Reset gameboy",
            "Save screenshot",
            "Debugger",
            "State",
            "Other",
            "Sound channel",
            "Window size",
            "Link",
            "Recent ROMs",
            "Exit",
        ]
    );
    assert_eq!(m.actions.len(), m.items.len(), "one action slot per row");
}

#[test]
fn enable_sound_checkmark_tracks_the_sound_state() {
    let on = MainMenu::open((0, 0), true);
    let off = MainMenu::open((0, 0), false);
    assert!(on.items[2].checked, "checked when sound is on");
    assert!(!off.items[2].checked, "unchecked when muted");
    // Either way the row toggles the sound.
    assert_eq!(on.actions[2], Some(Action::ToggleSound));
}

#[test]
fn supported_rows_carry_their_action_the_rest_are_none() {
    let m = MainMenu::open((0, 0), true);
    assert_eq!(m.actions[0], Some(Action::Pause));
    assert_eq!(m.actions[2], Some(Action::ToggleSound));
    assert_eq!(m.actions[5], Some(Action::Reset));
    assert_eq!(m.actions[7], Some(Action::ToggleTool(ToolWindow::Debugger)));
    assert_eq!(m.actions[14], Some(Action::Quit));
    // Greyed stubs + submenu rows have no action.
    for i in [1, 3, 4, 6, 8, 9, 10, 11, 12, 13] {
        assert_eq!(m.actions[i], None, "row {i} is a stub");
    }
}

#[test]
fn submenu_rows_show_the_arrow_but_are_greyed_until_wired() {
    let m = MainMenu::open((0, 0), true);
    for i in 8..=13 {
        assert!(m.items[i].submenu, "row {i} draws the submenu arrow");
        assert!(!m.items[i].enabled, "row {i} greyed until MN2-7");
    }
    // The greyed file-ops rows are not submenus.
    assert!(!m.items[1].submenu, "Load ROM is a plain (greyed) item");
}

#[test]
fn action_at_resolves_only_enabled_wired_rows() {
    let m = MainMenu::open((10, 10), true);
    assert_eq!(m.action_at(at(&m, 0).0, at(&m, 0).1), Some(Action::Pause));
    assert_eq!(m.action_at(at(&m, 14).0, at(&m, 14).1), Some(Action::Quit));
    // A greyed row resolves to nothing (item_at skips disabled rows).
    assert_eq!(
        m.action_at(at(&m, 1).0, at(&m, 1).1),
        None,
        "Load ROM greyed"
    );
    // A submenu row likewise (greyed, no action) until MN2-7.
    assert_eq!(m.action_at(at(&m, 8).0, at(&m, 8).1), None, "State stub");
    // A point outside the box resolves to nothing.
    assert_eq!(m.action_at(-50, -50), None);
}

#[test]
fn hover_at_tracks_the_enabled_row_and_reports_changes() {
    let mut m = MainMenu::open((10, 10), true);
    assert!(
        m.hover_at(at(&m, 0).0, at(&m, 0).1),
        "moving onto Pause changes hover"
    );
    assert_eq!(m.hovered, Some(0));
    assert!(
        !m.hover_at(at(&m, 0).0, at(&m, 0).1),
        "same row → no change"
    );
    // Greyed rows don't take the highlight (item_at skips them).
    assert!(
        m.hover_at(at(&m, 1).0, at(&m, 1).1),
        "leaving Pause changes hover"
    );
    assert_eq!(m.hovered, None, "greyed row is not hovered");
}

#[test]
fn render_draws_ink_including_the_check_and_arrow_columns() {
    use crate::ui::Canvas;
    use crate::ui::menu::{menu_height, menu_width};
    let m = MainMenu::open((0, 0), true);
    let w = menu_width(&m.items).max(1) as usize;
    let h = menu_height(&m.items).max(1) as usize;
    let mut buf = vec![0x00AA_AAAA_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, &m, &Theme::BGB);
    }
    // Background, border, and label ink are all present (not a blank box).
    assert!(buf.contains(&Theme::BGB.bg), "menu background filled");
    assert!(buf.contains(&Theme::BGB.text), "label ink drawn");
}
