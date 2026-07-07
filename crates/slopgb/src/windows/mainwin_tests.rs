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
fn popup_size_unions_main_menu_and_submenu() {
    let menu = MainMenu::open((0, 0), true, false);
    let main_only = popup_content_size(&menu, None);
    let mb = crate::ui::menu::menu_bounds(menu.origin, &menu.items);
    assert_eq!(main_only, (mb.right(), mb.bottom()));
    // Hang the Window size submenu off its row: the popup widens to cover it.
    let row = menu
        .row_rect(MenuEffect::Submenu(SubKind::WindowSize))
        .unwrap();
    let sub = SubMenu::window_size(row, WindowSizeChoice::Scale(2));
    let with_sub = popup_content_size(&menu, Some(&sub));
    let sb = crate::ui::menu::menu_bounds(sub.origin, &sub.items);
    assert!(with_sub.0 > main_only.0, "submenu widens the popup");
    assert_eq!(with_sub.0, mb.right().max(sb.right()));
    assert_eq!(with_sub.1, mb.bottom().max(sb.bottom()));
}

#[test]
fn popup_origin_clamps_to_monitor() {
    let cursor = (900, 700);
    let window = (10, 20); // game-window outer position
    let popup = (200, 300);
    // No monitor info: raw window + cursor, unclamped.
    assert_eq!(popup_screen_origin(cursor, window, popup, None), (910, 720));
    // 1024x768 monitor at the origin: the popup would overflow both edges, so it
    // shifts left/up to fit entirely on-screen.
    let (x, y) = popup_screen_origin(cursor, window, popup, Some((0, 0, 1024, 768)));
    assert_eq!((x, y), (824, 468));
    assert!(x + popup.0 <= 1024 && y + popup.1 <= 768);
}

#[test]
fn popup_gap_around_submenu_is_transparent() {
    // The popup window is sized to the union of the main menu + the open submenu,
    // so a short submenu leaves an L-shaped gap (right of the main menu, above /
    // below the submenu box). bgb's submenu is a separate floating box — that gap
    // must be transparent (desktop shows through), NOT a filled background.
    let menu = MainMenu::open((0, 0), true, false);
    let row = menu
        .row_rect(MenuEffect::Submenu(SubKind::State))
        .expect("State row exists");
    let sub = SubMenu::state(row);
    let (main_box, sub_box) = popup_menu_boxes(&menu, Some(&sub));
    let sub_box = sub_box.expect("submenu box present");
    // Inside the main menu box → opaque.
    assert!(main_box.contains(2, 2), "main box opaque");
    // Inside the submenu box → opaque.
    assert!(
        sub_box.contains(sub.origin.0 + 2, sub.origin.1 + 2),
        "sub box opaque"
    );
    // The L-gap: right of the main menu (x past its right edge) and above the
    // submenu's top → inside NEITHER box → transparent (the bug: it was filled).
    let (gx, gy) = (sub.origin.0 + 2, 2);
    assert!(
        !main_box.contains(gx, gy) && !sub_box.contains(gx, gy),
        "gap right-of-main / above-sub is transparent"
    );
    // With no submenu open, there is no second box and anything past the main
    // box is transparent.
    let (m2, s2) = popup_menu_boxes(&menu, None);
    assert!(s2.is_none(), "no submenu → no second opaque box");
    assert!(
        !m2.contains(m2.right() + 2, 2),
        "no-sub: past main is transparent"
    );
}

#[test]
fn menu_has_the_fifteen_rc_main_rows_in_order() {
    let m = MainMenu::open((10, 10), true, false);
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
            "MCP",
            "Recent ROMs",
            "Exit",
        ]
    );
    assert_eq!(m.effects.len(), m.items.len(), "one effect slot per row");
}

#[test]
fn pause_row_checks_when_paused() {
    // BUG-3: bgb check-marks the Pause row while paused; slopgb left it blank.
    let paused = MainMenu::open((0, 0), true, true);
    assert!(paused.items[0].checked, "Pause checked while paused");
    let running = MainMenu::open((0, 0), true, false);
    assert!(!running.items[0].checked, "Pause unchecked while running");
}

#[test]
fn enable_sound_checkmark_tracks_the_sound_state() {
    let on = MainMenu::open((0, 0), true, false);
    let off = MainMenu::open((0, 0), false, false);
    assert!(on.items[2].checked, "checked when sound is on");
    assert!(!off.items[2].checked, "unchecked when muted");
    assert_eq!(on.effects[2], MenuEffect::Run(Action::ToggleSound));
}

#[test]
fn supported_rows_run_their_action_window_size_opens_a_submenu_rest_none() {
    let m = MainMenu::open((0, 0), true, false);
    assert_eq!(m.effects[0], MenuEffect::Run(Action::Pause));
    assert_eq!(m.effects[2], MenuEffect::Run(Action::ToggleSound));
    assert_eq!(m.effects[5], MenuEffect::Run(Action::Reset));
    assert_eq!(
        m.effects[7],
        MenuEffect::Run(Action::ToggleTool(ToolWindow::Debugger))
    );
    assert_eq!(m.effects[15], MenuEffect::Run(Action::Quit));
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
    assert_eq!(m.effects[14], MenuEffect::Submenu(SubKind::RecentRoms));
    // Link opens its submenu (rows grey by connection state — see
    // link_submenu_greys_rows_by_state).
    assert_eq!(m.effects[12], MenuEffect::Submenu(SubKind::Link));
    // MCP opens its submenu (Start/Stop, greyed by server state).
    assert_eq!(m.effects[13], MenuEffect::Submenu(SubKind::Mcp));
}

#[test]
fn submenu_rows_show_the_arrow_window_size_enabled_others_greyed() {
    let m = MainMenu::open((0, 0), true, false);
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
    assert!(m.items[12].enabled, "Link is wired");
    assert!(m.items[1].enabled, "Load ROM is wired (MN4)");
    assert!(
        !m.items[1].submenu,
        "Load ROM is a plain item (opens a modal)"
    );
}

#[test]
fn effect_at_resolves_only_enabled_rows() {
    let m = MainMenu::open((10, 10), true, false);
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
    // Link (row 12) now opens its submenu; a point outside the box resolves to
    // None (greyed-row → None is exercised by the submenu tests).
    assert_eq!(
        m.effect_at(at(&m, 12).0, at(&m, 12).1),
        MenuEffect::Submenu(SubKind::Link)
    );
    assert_eq!(m.effect_at(-50, -50), MenuEffect::None);
}

#[test]
fn row_rect_locates_the_window_size_row_for_its_submenu() {
    let m = MainMenu::open((10, 10), true, false);
    let r = m
        .row_rect(MenuEffect::Submenu(SubKind::WindowSize))
        .expect("window size row exists");
    assert_eq!(r, row_rect(&m, WINDOW_SIZE_ROW), "matches the 12th row");
}

#[test]
fn hover_at_tracks_the_enabled_row_and_reports_changes() {
    let mut m = MainMenu::open((10, 10), true, false);
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
        m.hover_at(-50, -50),
        "leaving Pause for empty space changes hover"
    );
    assert_eq!(m.hovered, None, "an off-row point is not hovered");
}

#[test]
fn render_draws_ink_including_the_check_and_arrow_columns() {
    use crate::ui::Canvas;
    use crate::ui::menu::{menu_height, menu_width};
    let m = MainMenu::open((0, 0), true, false);
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
            "Import bgb.ini...",
            "Export bgb.ini...",
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
    assert_eq!(s.choices[3], Some(SubChoice::ImportBgb));
    assert_eq!(s.choices[4], Some(SubChoice::ExportBgb));
    assert_eq!(s.choices[10], Some(SubChoice::About));
    // The not-built rows are greyed with no choice.
    for i in [5, 6, 7, 8, 9] {
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
    let (xi, yi) = centre(3); // "Import bgb.ini..." (live)
    assert_eq!(s.choice_at(xi, yi), Some(SubChoice::ImportBgb));
    let (xg, yg) = centre(5); // "cheat searcher" (greyed)
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
    // BUG-2: Quick Save/Load/Select carry NO accelerator label — bgb's F2/F4/F3
    // collide with slopgb's game-window F2/F3/F4 (open debugger/VRAM/iomap), so
    // the menu rows are click-only rather than advertising a dead/wrong hotkey.
    for i in [0, 1, 2] {
        assert_eq!(
            s.items[i].shortcut, None,
            "row {i} has no accelerator label"
        );
    }
    // Load state... is now live (on-disk save states); Select / Load recovery
    // stay greyed (those subsystems aren't built).
    assert_eq!(s.choices[4], Some(SubChoice::LoadState));
    assert!(s.items[4].enabled, "Load state... is live");
    for i in [2, 3] {
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

// --- Link submenu ----------------------------------------------------------

#[test]
fn link_submenu_greys_rows_by_state() {
    // `SubMenu::link(active, listening)`. Order matches main-sub-link.png:
    // Listen / Connect / Disconnect / Cancel listen.
    let labels = |s: &SubMenu| -> Vec<String> { s.items.iter().map(|i| i.label.clone()).collect() };
    let idle = SubMenu::link(PARENT, false, false);
    assert_eq!(idle.kind, SubKind::Link);
    assert_eq!(
        labels(&idle),
        ["Listen", "Connect", "Disconnect", "Cancel listen"]
    );
    // Idle (no socket): Listen + Connect enabled; Disconnect + Cancel greyed.
    assert_eq!(idle.choices[0], Some(SubChoice::LinkListen));
    assert_eq!(idle.choices[1], Some(SubChoice::LinkConnect));
    assert_eq!(idle.choices[2], None, "Disconnect greyed when idle");
    assert_eq!(idle.choices[3], None, "Cancel listen greyed when idle");

    // Active + not listening (dialing or connected): only Disconnect is live —
    // it both aborts a pending dial and tears down a live connection.
    let conn = SubMenu::link(PARENT, true, false);
    assert_eq!(conn.choices[0], None, "Listen greyed while active");
    assert_eq!(conn.choices[1], None, "Connect greyed while active");
    assert_eq!(conn.choices[2], Some(SubChoice::LinkDisconnect));
    assert_eq!(conn.choices[3], None);

    // Listening (active, waiting for a peer): only Cancel listen is live.
    let listen = SubMenu::link(PARENT, true, true);
    assert_eq!(listen.choices[0], None, "Listen greyed while listening");
    assert_eq!(listen.choices[1], None, "Connect greyed while listening");
    assert_eq!(listen.choices[2], None, "Disconnect greyed while listening");
    assert_eq!(listen.choices[3], Some(SubChoice::LinkCancelListen));
}

#[test]
fn mcp_submenu_greys_rows_by_state() {
    let labels = |s: &SubMenu| -> Vec<String> { s.items.iter().map(|i| i.label.clone()).collect() };
    let idle = SubMenu::mcp(PARENT, false);
    assert_eq!(idle.kind, SubKind::Mcp);
    assert_eq!(labels(&idle), ["Start server...", "Stop server"]);
    // Idle: Start enabled, Stop greyed.
    assert_eq!(idle.choices[0], Some(SubChoice::McpStart));
    assert_eq!(idle.choices[1], None, "Stop greyed when idle");
    // Running: Start greyed, Stop enabled.
    let running = SubMenu::mcp(PARENT, true);
    assert_eq!(running.choices[0], None, "Start greyed while running");
    assert_eq!(running.choices[1], Some(SubChoice::McpStop));
}
