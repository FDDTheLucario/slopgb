//! Menu bar + dropdown wiring tests (MB1).

use super::super::*;
use super::{AREA, NOPS, regs0};
use crate::ui::canvas::Canvas;
use crate::ui::menu::menu_rects;

#[test]
fn menubar_rects_tile_the_bar_left_to_right() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let rects = menubar_rects(l.menu);
    assert_eq!(rects.len(), 6);
    for w in rects.windows(2) {
        assert_eq!(w[0].right(), w[1].x, "labels abut without gaps");
    }
    assert_eq!(rects[0].x, l.menu.x);
    // Each label resolves to its own index.
    for (i, r) in rects.iter().enumerate() {
        assert_eq!(menubar_at(l.menu, r.x + 1, r.y + 1), Some(i));
    }
    assert_eq!(
        menubar_at(l.menu, l.menu.right() + 5, 1),
        None,
        "past the bar"
    );
}

#[test]
fn each_bar_menu_has_the_captured_item_count() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let st = DebuggerState::default();
    let counts = [16, 5, 18, 11, 11, 5]; // File, Search, Run, Debug, Window, Profiler
    for (idx, &n) in counts.iter().enumerate() {
        let m = menubar_menu(idx, l.menu, &st, 0x0100);
        assert_eq!(m.items.len(), n, "menu {idx} item count");
        assert_eq!(m.bar, Some(idx), "dropdown knows its bar label");
    }
}

#[test]
fn clicking_a_bar_label_opens_its_dropdown() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let rects = menubar_rects(l.menu);
    let mut st = DebuggerState::default();
    // Click the "Run" label (index 2).
    let r = rects[2];
    let action = on_left_click(NOPS, AREA, &mut st, regs0(), r.x + 2, r.y + 2, |_| 0);
    assert_eq!(action, None);
    let m = st.menu.as_ref().expect("Run dropdown opened");
    assert_eq!(m.bar, Some(2));
    assert_eq!(m.items.len(), 18);
}

#[test]
fn debug_menu_toggle_breakpoint_acts_on_the_cursor() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let rects = menubar_rects(l.menu);
    let mut st = DebuggerState {
        cursor: Some(0x0150),
        ..DebuggerState::default()
    };
    // Open the Debug menu (index 3).
    let r = rects[3];
    on_left_click(NOPS, AREA, &mut st, regs0(), r.x + 2, r.y + 2, |_| 0);
    // "Toggle breakpoint" is the first item; clicking it toggles bp at the cursor.
    let item_rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let ir = item_rects[0];
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        ir.x + ir.w / 2,
        ir.y + ir.h / 2,
        |_| 0,
    );
    assert_eq!(
        action,
        Some(MenuOutcome::Act(DebugAction::ToggleBreakpoint(0x0150)))
    );
    assert!(st.menu.is_none(), "selecting closes the dropdown");
}

#[test]
fn run_menu_run_to_cursor_falls_back_to_pc_without_a_cursor() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let rects = menubar_rects(l.menu);
    let mut st = DebuggerState::default(); // no cursor selected
    on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        rects[2].x + 2,
        rects[2].y + 2,
        |_| 0,
    );
    // "Run to Cursor" is index 9 in the Run menu.
    let item_rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let ir = item_rects[9];
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        ir.x + ir.w / 2,
        ir.y + ir.h / 2,
        |_| 0,
    );
    assert_eq!(
        action,
        Some(MenuOutcome::Act(DebugAction::RunToCursor(0x0100))),
        "defaults to PC"
    );
}

/// Open menu-bar dropdown `bar_idx`, click its item `item_idx`, return the
/// outcome. Each click closes the menu, so callers reopen per item.
fn click_menubar_item(bar_idx: usize, item_idx: usize) -> Option<MenuOutcome> {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let rects = menubar_rects(l.menu);
    let mut st = DebuggerState::default();
    let br = rects[bar_idx];
    on_left_click(NOPS, AREA, &mut st, regs0(), br.x + 2, br.y + 2, |_| 0);
    let item_rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let ir = item_rects[item_idx];
    on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        ir.x + ir.w / 2,
        ir.y + ir.h / 2,
        |_| 0,
    )
}

#[test]
fn run_menu_wires_forward_execution_commands() {
    use crate::input::Action;
    // (item index in run_menu, expected command).
    let cases = [
        (0, Action::DbgBreak),    // Run (resume)
        (3, Action::Reset),       // Reset (numpad *)
        (4, Action::DbgStep),     // Trace
        (6, Action::DbgStepOver), // Step Over
        (14, Action::DbgStepOut), // Step out
    ];
    for (item, act) in cases {
        assert_eq!(
            click_menubar_item(2, item),
            Some(MenuOutcome::Command(act)),
            "Run menu item {item} fires {act:?}"
        );
    }
    // Jump to cursor (F6) + Call cursor are now wired (cursor falls back to PC).
    assert_eq!(
        click_menubar_item(2, 12),
        Some(MenuOutcome::Command(Action::DbgJumpToCursor)),
        "Jump to cursor"
    );
    assert_eq!(
        click_menubar_item(2, 13),
        Some(MenuOutcome::Act(DebugAction::Call(0x0100))),
        "Call cursor acts on the cursor-or-PC"
    );
    // The reverse / not-yet-built variants stay greyed (no outcome).
    assert_eq!(click_menubar_item(2, 1), None, "Run no break greyed");
    assert_eq!(click_menubar_item(2, 8), None, "Animate greyed");
}

#[test]
fn search_menu_wires_go_to_pc() {
    use crate::input::Action;
    // "go to PC" is the last Search item (index 4).
    assert_eq!(
        click_menubar_item(1, 4),
        Some(MenuOutcome::Command(Action::DbgGoToPc)),
        "go to PC re-centers the disasm"
    );
    // Search string + bookmark rows are live now (MB3); see debugger_search_tests.
    assert_eq!(
        click_menubar_item(1, 0),
        Some(MenuOutcome::Command(Action::DbgSearch)),
        "Search string now opens the prompt"
    );
}

#[test]
fn file_menu_wires_the_export_commands() {
    use crate::input::Action;
    assert_eq!(
        click_menubar_item(0, 10),
        Some(MenuOutcome::Command(Action::SaveScreenshot)),
        "save screenshot"
    );
    assert_eq!(
        click_menubar_item(0, 11),
        Some(MenuOutcome::Command(Action::DbgSaveMemDump)),
        "save memory_dump"
    );
    assert_eq!(
        click_menubar_item(0, 12),
        Some(MenuOutcome::Command(Action::DbgSaveAsm)),
        "save asm"
    );
    // On-disk save states are now live (via the shared path modal).
    assert_eq!(
        click_menubar_item(0, 7),
        Some(MenuOutcome::Command(Action::DbgLoadState)),
        "Load state"
    );
    assert_eq!(
        click_menubar_item(0, 8),
        Some(MenuOutcome::Command(Action::DbgSaveState)),
        "Save state"
    );
    // Load ROM stays greyed in the debugger File menu (needs a picker).
    assert_eq!(click_menubar_item(0, 0), None, "Load ROM greyed");
}

#[test]
fn window_menu_wires_the_viewer_toggles() {
    use crate::input::Action;
    use crate::ui::ToolWindow;
    assert_eq!(
        click_menubar_item(4, 0),
        Some(MenuOutcome::Command(Action::ToggleTool(ToolWindow::Vram))),
        "VRAM viewer toggles its window"
    );
    assert_eq!(
        click_menubar_item(4, 1),
        Some(MenuOutcome::Command(Action::ToggleTool(
            ToolWindow::MemoryViewer
        ))),
        "Memory viewer toggles its window"
    );
    assert_eq!(
        click_menubar_item(4, 7),
        Some(MenuOutcome::Command(Action::ToggleTool(ToolWindow::IoMap))),
        "IO map toggles its window"
    );
    // Options (F11) is not built yet — stays greyed.
    assert_eq!(click_menubar_item(4, 4), None, "Options greyed");
}

#[test]
fn render_menubar_draws_labels_and_highlights_the_open_one() {
    use crate::ui::Theme;
    let t = Theme::BGB;
    let bar = Rect::new(0, 0, 760, 18);
    let (w, h) = (760usize, 18usize);
    let mut buf = vec![0x00AA_AAAA_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_menubar(&mut c, bar, Some(2), &t); // "Run" open
    }
    // Some label ink is present (the bar isn't blank).
    assert!(buf.contains(&t.text), "labels drawn");
    // The open label (index 2) is flooded with the highlight colour.
    let r2 = menubar_rects(bar)[2];
    let mid = (r2.y as usize + r2.h as usize / 2) * w + (r2.x as usize + 1);
    assert_eq!(buf[mid], t.current, "open label highlighted");
}
