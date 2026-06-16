use super::*;
use crate::ui::canvas::Rect;
use crate::ui::menu::menu_rects;

/// The row index of each labelled item, for click coordinates.
fn row_rect(m: &MainMenu, idx: usize) -> Rect {
    menu_rects(m.origin, &m.items)[idx]
}

/// Click point at the centre of main-menu row `idx`.
fn at(m: &MainMenu, idx: usize) -> (i32, i32) {
    let r = row_rect(m, idx);
    (r.x + r.w / 2, r.y + r.h / 2)
}

/// Index of the "Window size" row (carries the submenu opener).
const WINDOW_SIZE_ROW: usize = 11;
/// Index of the "Sound channel" row (MN3 submenu opener).
const SOUND_CHANNEL_ROW: usize = 10;

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
    assert_eq!(m.effects.len(), m.items.len(), "one effect slot per row");
}

#[test]
fn enable_sound_checkmark_tracks_the_sound_state() {
    let on = MainMenu::open((0, 0), true);
    let off = MainMenu::open((0, 0), false);
    assert!(on.items[2].checked, "checked when sound is on");
    assert!(!off.items[2].checked, "unchecked when muted");
    assert_eq!(on.effects[2], MenuEffect::Run(Action::ToggleSound));
}

#[test]
fn supported_rows_run_their_action_window_size_opens_a_submenu_rest_none() {
    let m = MainMenu::open((0, 0), true);
    assert_eq!(m.effects[0], MenuEffect::Run(Action::Pause));
    assert_eq!(m.effects[2], MenuEffect::Run(Action::ToggleSound));
    assert_eq!(m.effects[5], MenuEffect::Run(Action::Reset));
    assert_eq!(
        m.effects[7],
        MenuEffect::Run(Action::ToggleTool(ToolWindow::Debugger))
    );
    assert_eq!(m.effects[14], MenuEffect::Run(Action::Quit));
    assert_eq!(
        m.effects[WINDOW_SIZE_ROW],
        MenuEffect::Submenu(SubKind::WindowSize)
    );
    assert_eq!(
        m.effects[SOUND_CHANNEL_ROW],
        MenuEffect::Submenu(SubKind::SoundChannel),
        "Sound channel opens its submenu (MN3)"
    );
    assert_eq!(
        m.effects[6],
        MenuEffect::Run(Action::SaveScreenshot),
        "Save screenshot is wired (MN4)"
    );
    assert_eq!(
        m.effects[9],
        MenuEffect::Submenu(SubKind::Other),
        "Other opens its submenu (MN5)"
    );
    // Options / Cheat open their info-box stubs (MN7).
    assert_eq!(m.effects[3], MenuEffect::Run(Action::MainOptions));
    assert_eq!(m.effects[4], MenuEffect::Run(Action::MainCheats));
    // State opens its submenu (MN6 Quick Save/Load).
    assert_eq!(m.effects[8], MenuEffect::Submenu(SubKind::State));
    // Load ROM (MN4) opens the path modal; Recent ROMs (MN4) opens its submenu.
    assert_eq!(m.effects[1], MenuEffect::Run(Action::MainLoadRom));
    assert_eq!(m.effects[13], MenuEffect::Submenu(SubKind::RecentRoms));
    // Only the Link row stays a not-yet-wired stub.
    assert_eq!(m.effects[12], MenuEffect::None, "Link is a stub");
}

#[test]
fn submenu_rows_show_the_arrow_window_size_enabled_others_greyed() {
    let m = MainMenu::open((0, 0), true);
    // All six submenu rows draw the arrow.
    for i in [8, 9, 10, 11, 12, 13] {
        assert!(m.items[i].submenu, "row {i} draws the submenu arrow");
    }
    // Window size + Sound channel are live; the rest stay greyed until MN4-7.
    assert!(
        m.items[WINDOW_SIZE_ROW].enabled,
        "Window size is wired (MN2)"
    );
    assert!(
        m.items[SOUND_CHANNEL_ROW].enabled,
        "Sound channel is wired (MN3)"
    );
    assert!(m.items[9].enabled, "Other is wired (MN5)");
    assert!(m.items[8].enabled, "State is wired (MN6)");
    assert!(m.items[13].enabled, "Recent ROMs is wired (MN4)");
    assert!(
        !m.items[12].enabled,
        "Link stays greyed until its milestone"
    );
    assert!(m.items[1].enabled, "Load ROM is wired (MN4)");
    assert!(
        !m.items[1].submenu,
        "Load ROM is a plain item (opens a modal)"
    );
}

#[test]
fn effect_at_resolves_only_enabled_rows() {
    let m = MainMenu::open((10, 10), true);
    assert_eq!(
        m.effect_at(at(&m, 0).0, at(&m, 0).1),
        MenuEffect::Run(Action::Pause)
    );
    assert_eq!(
        m.effect_at(at(&m, WINDOW_SIZE_ROW).0, at(&m, WINDOW_SIZE_ROW).1),
        MenuEffect::Submenu(SubKind::WindowSize)
    );
    // Load ROM (row 1) now opens the path modal.
    assert_eq!(
        m.effect_at(at(&m, 1).0, at(&m, 1).1),
        MenuEffect::Run(Action::MainLoadRom)
    );
    // A still-greyed row (Link) + a point outside the box resolve to None.
    assert_eq!(m.effect_at(at(&m, 12).0, at(&m, 12).1), MenuEffect::None);
    assert_eq!(m.effect_at(-50, -50), MenuEffect::None);
}

#[test]
fn row_rect_locates_the_window_size_row_for_its_submenu() {
    let m = MainMenu::open((10, 10), true);
    let r = m
        .row_rect(MenuEffect::Submenu(SubKind::WindowSize))
        .expect("window size row exists");
    assert_eq!(r, row_rect(&m, WINDOW_SIZE_ROW), "matches the 12th row");
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
    assert!(
        m.hover_at(at(&m, 12).0, at(&m, 12).1),
        "leaving Pause changes hover"
    );
    assert_eq!(m.hovered, None, "greyed row (Link) is not hovered");
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
    assert!(buf.contains(&Theme::BGB.bg), "menu background filled");
    assert!(buf.contains(&Theme::BGB.text), "label ink drawn");
}

// --- Window size submenu (MN2) ---------------------------------------------

/// A parent row rect to anchor the submenu against.
const PARENT: Rect = Rect::new(20, 40, 100, 15);

#[test]
fn window_size_submenu_has_the_eight_captured_rows() {
    let s = SubMenu::window_size(PARENT, WindowSizeChoice::Scale(2));
    let labels: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "1x1",
            "2x2",
            "3x3",
            "4x4",
            "5x5",
            "6x6",
            "Full screen",
            "Fullscreen stretched",
        ]
    );
    assert_eq!(s.kind, SubKind::WindowSize);
    assert_eq!(
        s.choices[2],
        Some(SubChoice::WindowSize(WindowSizeChoice::Scale(3)))
    );
    assert_eq!(
        s.choices[6],
        Some(SubChoice::WindowSize(WindowSizeChoice::Fullscreen))
    );
    assert_eq!(
        s.choices[7],
        Some(SubChoice::WindowSize(WindowSizeChoice::FullscreenStretched))
    );
}

#[test]
fn the_active_size_is_the_only_checked_row() {
    let s = SubMenu::window_size(PARENT, WindowSizeChoice::Scale(2));
    assert!(s.items[1].checked, "2x2 checked when active is Scale(2)");
    for i in [0, 2, 3, 4, 5, 6, 7] {
        assert!(!s.items[i].checked, "row {i} unchecked");
    }
    // Fullscreen active → only "Full screen" checked.
    let f = SubMenu::window_size(PARENT, WindowSizeChoice::Fullscreen);
    assert!(f.items[6].checked, "Full screen checked");
    assert!(!f.items[1].checked, "no integer size checked in fullscreen");
}

#[test]
fn submenu_opens_to_the_right_of_its_parent_row() {
    let s = SubMenu::window_size(PARENT, WindowSizeChoice::Scale(3));
    assert_eq!(s.origin.0, PARENT.right(), "hangs off the row's right edge");
    assert_eq!(s.origin.1, PARENT.y, "top-aligned to the row");
}

#[test]
fn choice_at_resolves_the_clicked_size() {
    let s = SubMenu::window_size(PARENT, WindowSizeChoice::Scale(2));
    let rects = menu_rects(s.origin, &s.items);
    let centre = |i: usize| (rects[i].x + rects[i].w / 2, rects[i].y + rects[i].h / 2);
    let (x4, y4) = centre(3); // "4x4"
    assert_eq!(
        s.choice_at(x4, y4),
        Some(SubChoice::WindowSize(WindowSizeChoice::Scale(4)))
    );
    let (xf, yf) = centre(6); // "Full screen"
    assert_eq!(
        s.choice_at(xf, yf),
        Some(SubChoice::WindowSize(WindowSizeChoice::Fullscreen))
    );
    assert_eq!(s.choice_at(-99, -99), None, "outside the box");
}

// --- Sound channel submenu (MN3) -------------------------------------------

#[test]
fn sound_channel_submenu_has_the_four_captured_rows() {
    let s = SubMenu::sound_channel(PARENT, [false; 4]);
    let labels: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["1", "2", "3", "4"]);
    let shortcuts: Vec<Option<&str>> = s.items.iter().map(|i| i.shortcut.as_deref()).collect();
    assert_eq!(
        shortcuts,
        [Some("F5"), Some("F6"), Some("F7"), Some("F8")],
        "each channel carries its bgb hotkey"
    );
    assert_eq!(s.kind, SubKind::SoundChannel);
    for (i, ch) in (1..=4u8).enumerate() {
        assert_eq!(s.choices[i], Some(SubChoice::SoundChannel(ch)));
    }
}

#[test]
fn sound_channel_checks_track_the_audible_channels() {
    // A row is checked when its channel is *audible* (not muted): muting
    // channel 2 un-checks only row 2.
    let s = SubMenu::sound_channel(PARENT, [false, true, false, false]);
    assert!(s.items[0].checked, "ch1 audible");
    assert!(!s.items[1].checked, "ch2 muted -> unchecked");
    assert!(s.items[2].checked && s.items[3].checked, "ch3/4 audible");
    // All audible -> every row checked.
    let all = SubMenu::sound_channel(PARENT, [false; 4]);
    assert!(all.items.iter().all(|i| i.checked));
}

#[test]
fn sound_channel_choice_at_resolves_the_clicked_channel() {
    let s = SubMenu::sound_channel(PARENT, [false; 4]);
    let rects = menu_rects(s.origin, &s.items);
    let centre = |i: usize| (rects[i].x + rects[i].w / 2, rects[i].y + rects[i].h / 2);
    let (x3, y3) = centre(2); // row "3"
    assert_eq!(s.choice_at(x3, y3), Some(SubChoice::SoundChannel(3)));
    assert_eq!(s.choice_at(-99, -99), None, "outside the box");
}

// --- Other submenu + info box (MN5) ----------------------------------------

#[test]
fn other_submenu_has_the_captured_rows_live_and_greyed() {
    let s = SubMenu::other(PARENT);
    assert_eq!(s.kind, SubKind::Other);
    let labels: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Cart info",
            "System info",
            "VRAM viewer",
            "cheat searcher",
            "Camera control...",
            "clear recent roms list",
            "debug mode enabled: *",
            "Close screen",
            "About...",
        ]
    );
    assert_eq!(s.choices[0], Some(SubChoice::CartInfo));
    assert_eq!(s.choices[1], Some(SubChoice::SystemInfo));
    assert_eq!(s.choices[2], Some(SubChoice::OpenVram));
    assert_eq!(s.choices[8], Some(SubChoice::About));
    // The not-built rows are greyed with no choice.
    for i in [3, 4, 5, 6, 7] {
        assert!(!s.items[i].enabled, "row {i} greyed");
        assert_eq!(s.choices[i], None, "row {i} has no choice");
    }
}

#[test]
fn other_choice_at_resolves_only_enabled_rows() {
    let s = SubMenu::other(PARENT);
    let rects = menu_rects(s.origin, &s.items);
    let centre = |i: usize| (rects[i].x + rects[i].w / 2, rects[i].y + rects[i].h / 2);
    let (xc, yc) = centre(0); // "Cart info"
    assert_eq!(s.choice_at(xc, yc), Some(SubChoice::CartInfo));
    let (xg, yg) = centre(3); // "cheat searcher" (greyed)
    assert_eq!(s.choice_at(xg, yg), None, "greyed row resolves to None");
}

#[test]
fn render_info_draws_the_box_and_text() {
    use crate::ui::Canvas;
    let info = InfoBox::new(
        "Cart info",
        vec!["title: TEST".into(), "rom: 32 KiB".into()],
    );
    let (w, h) = (220usize, 130usize);
    let mut buf = vec![0x00AA_AAAA_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_info(&mut c, &info, &Theme::BGB);
    }
    assert!(buf.contains(&Theme::BGB.bg), "info box background filled");
    assert!(buf.contains(&Theme::BGB.text), "info text ink drawn");
}

// --- State submenu (MN6 Quick Save/Load) -----------------------------------

#[test]
fn state_submenu_has_quick_save_load_live_rest_greyed() {
    let s = SubMenu::state(PARENT);
    assert_eq!(s.kind, SubKind::State);
    let labels: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Quick Save",
            "Quick Load",
            "Select",
            "Load recovery state",
            "Load state...",
        ]
    );
    assert_eq!(s.choices[0], Some(SubChoice::QuickSave));
    assert_eq!(s.choices[1], Some(SubChoice::QuickLoad));
    // The on-disk-format rows stay greyed (MN6 deferred).
    for i in [2, 3, 4] {
        assert!(!s.items[i].enabled, "row {i} greyed");
        assert_eq!(s.choices[i], None);
    }
}

// --- Recent ROMs submenu (MN4) ---------------------------------------------

#[test]
fn recent_roms_submenu_lists_entries_or_a_greyed_placeholder() {
    // Empty: a single greyed "(no recent ROMs)" row.
    let empty = SubMenu::recent_roms(PARENT, &[]);
    assert_eq!(empty.kind, SubKind::RecentRoms);
    assert_eq!(empty.items.len(), 1);
    assert!(!empty.items[0].enabled);
    assert_eq!(empty.choices[0], None);

    // Non-empty: one live row per name, each loading its index.
    let names = vec!["crystal.gbc".to_owned(), "tetris.gb".to_owned()];
    let s = SubMenu::recent_roms(PARENT, &names);
    let labels: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["crystal.gbc", "tetris.gb"]);
    assert_eq!(s.choices[0], Some(SubChoice::LoadRecent(0)));
    assert_eq!(s.choices[1], Some(SubChoice::LoadRecent(1)));
    assert!(s.items.iter().all(|i| i.enabled));
}
