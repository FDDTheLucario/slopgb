//! Click resolution, context menus, modal dialogs, register edit, and
//! code/data-hint tests.

use super::super::*;
use super::{AREA, NOPS, regs0};
use crate::dbg::Breakpoints;
use crate::ui::canvas::Canvas;
use crate::ui::menu::menu_rects;
use crate::ui::text::line_height;

#[test]
fn render_disasm_draws_a_red_gutter_dot_on_breakpoint_rows() {
    use crate::ui::Theme;
    let t = Theme::BGB;
    let lh = line_height() as usize;
    let (w, h) = (200usize, lh * 4);
    let mut buf = vec![0x00AA_AAAA_u32; w * h];
    let mut bps = Breakpoints::default();
    bps.toggle(0x0101, None); // rows are 0x100,0x101,... -> dot on visible row 1
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_disasm(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            NOPS,
            0x0100,
            0x0100,
            &bps,
            &BTreeSet::new(),
            DisasmFmt::default(),
            &SymbolTable::default(),
            |_| 0,
            &t,
        );
    }
    // A red pixel sits in the gutter (x in 1..GUTTER) of row 1; row 0 has none.
    let dot_y = lh + lh / 2;
    assert_eq!(buf[dot_y * w + 1], t.breakpoint, "breakpoint dot on row 1");
    let no_dot_y = lh / 2;
    assert_ne!(buf[no_dot_y * w + 1], t.breakpoint, "no dot on row 0");
}

#[test]
fn target_at_resolves_each_pane_to_its_address() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let st = DebuggerState::default();
    let lh = line_height();
    // Disasm row 2 (all nops): 0x100 -> 0x101 -> 0x102.
    let t = target_at(
        NOPS,
        AREA,
        &st,
        0x0100,
        0xFFFE,
        l.disasm.x + 9,
        l.disasm.y + 2 * lh + 1,
        |_| 0,
    );
    assert_eq!(t, ClickTarget::Disasm(0x0102));
    // Memory row 1 from the 0xFF00 base: 0xFF00 + 16.
    let t = target_at(
        NOPS,
        AREA,
        &st,
        0x0100,
        0xFFFE,
        l.memory.x + 9,
        l.memory.y + lh,
        |_| 0,
    );
    assert_eq!(t, ClickTarget::Memory(0xFF10));
    // Stack row 1 descends by 2 from SP.
    let t = target_at(
        NOPS,
        AREA,
        &st,
        0x0100,
        0xFFFE,
        l.stack.x + 5,
        l.stack.y + lh,
        |_| 0,
    );
    assert_eq!(t, ClickTarget::Stack(0xFFFC));
    // Registers pane: row 0 (af) is an editable pair; a row past pc (ime/ima)
    // is the non-editable `Registers`.
    let t = target_at(
        NOPS,
        AREA,
        &st,
        0x0100,
        0xFFFE,
        l.regs.x + 5,
        l.regs.y + 5,
        |_| 0,
    );
    assert_eq!(t, ClickTarget::Reg(RegField::Af));
    let t = target_at(
        NOPS,
        AREA,
        &st,
        0x0100,
        0xFFFE,
        l.regs.x + 5,
        l.regs.y + 6 * lh + 1,
        |_| 0,
    );
    assert_eq!(t, ClickTarget::Registers);
}

#[test]
fn pinned_disasm_follows_the_base_not_pc() {
    let st = DebuggerState {
        pinned: true,
        disasm_base: 0x0200,
        ..DebuggerState::default()
    };
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    // Row 0 resolves to the pinned base, not PC (0x0100).
    let t = target_at(
        NOPS,
        AREA,
        &st,
        0x0100,
        0xFFFE,
        l.disasm.x + 9,
        l.disasm.y + 1,
        |_| 0,
    );
    assert_eq!(t, ClickTarget::Disasm(0x0200));
}

#[test]
fn center_disasm_on_pc_puts_pc_mid_pane_and_unpins() {
    // A scrolled/frozen view (pinned, base elsewhere)…
    let mut st = DebuggerState {
        pinned: true,
        disasm_base: 0x0500,
        ..DebuggerState::default()
    };
    // …recenters on PC when tracing: a 10-row pane over a 1-byte NOP stream
    // walks back visible/2 = 5 instructions, so PC lands on the middle row.
    st.center_disasm_on_pc(0x0100, NOPS, 10);
    assert!(
        !st.pinned,
        "tracing unpins a scrolled view so it follows PC"
    );
    assert_eq!(
        st.disasm_base, 0x00FB,
        "PC sits 5 rows below the base (centered)"
    );
}

#[test]
fn scrollbar_models_round_trip_and_disasm_drag_pins() {
    let mut st = DebuggerState::default();
    // Memory: frac 0.5 -> ~0x8000, row-aligned; and the reported frac tracks it.
    st.set_mem_scroll(0.5);
    assert!((i32::from(st.mem_base) - 0x8000).abs() <= 0x10);
    assert_eq!(st.mem_base & 0x0F, 0, "row-aligned");
    let (mf, mv) = st.mem_scroll(30);
    assert!((mf - 0.5).abs() < 0.01, "mem frac tracks the base");
    assert!(
        mv > 0.0 && mv < 1.0,
        "mem thumb smaller than the whole space"
    );
    // Disasm: a drag pins (stops PC-follow) and jumps the base.
    assert!(!st.pinned);
    st.set_disasm_scroll(0.25);
    assert!(st.pinned, "dragging the disasm bar pins the view");
    assert!((st.disasm_base as f32 / f32::from(u16::MAX) - 0.25).abs() < 0.01);
    // Stack: frac 1.0 -> the max offset; 0.0 -> top.
    st.set_stack_scroll(1.0);
    assert_eq!(st.stack_off, 0x800);
    st.set_stack_scroll(0.0);
    assert_eq!(st.stack_off, 0);
}

#[test]
fn right_click_opens_the_matching_pane_menu_and_sets_the_cursor() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let lh = line_height();
    // Disasm pane -> the 12-item rc-disasm menu, cursor at the clicked row.
    let mut st = DebuggerState::default();
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.disasm.x + 9,
        l.disasm.y + 2 * lh + 1,
        |_| 0,
    );
    let om = st.menu.as_ref().expect("disasm menu opened");
    assert_eq!(om.items.len(), 12, "rc-disasm has 12 items");
    assert_eq!(st.cursor, Some(0x0102));
    // Memory pane -> 11 items (rc-disasm minus "force code view").
    let mut st = DebuggerState::default();
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.memory.x + 9,
        l.memory.y + lh,
        |_| 0,
    );
    assert_eq!(st.menu.as_ref().unwrap().items.len(), 12);
    // Stack -> 4, registers -> 1.
    let mut st = DebuggerState::default();
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.stack.x + 5,
        l.stack.y + lh,
        |_| 0,
    );
    assert_eq!(st.menu.as_ref().unwrap().items.len(), 4);
    let mut st = DebuggerState::default();
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.regs.x + 5,
        l.regs.y + 5,
        |_| 0,
    );
    assert_eq!(st.menu.as_ref().unwrap().items.len(), 1);
}

/// Open the disasm menu at the cursor and return (state, item rects) for clicking.
fn open_disasm_menu() -> (DebuggerState, Vec<Rect>) {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let lh = line_height();
    let mut st = DebuggerState::default();
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.disasm.x + 9,
        l.disasm.y + 2 * lh + 1,
        |_| 0,
    );
    let om = st.menu.as_ref().unwrap();
    let rects = menu_rects(om.origin, &om.items);
    (st, rects)
}

#[test]
fn selecting_set_break_returns_a_toggle_breakpoint_action() {
    let (mut st, rects) = open_disasm_menu();
    // "Set break/condition…" is the last (index 11) item; cursor is 0x0102.
    let r = rects[11];
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(
        action,
        Some(MenuOutcome::Act(DebugAction::ToggleBreakpoint(
            0x0102, None
        )))
    );
    assert!(st.menu.is_none(), "menu closes after a selection");
}

#[test]
fn copy_data_and_code_route_to_clipboard_actions() {
    use crate::input::Action;
    // RM10: the Copy rows (indices 2/3) are live + carry the clicked address
    // (cursor 0x0102) to the clipboard actions.
    let (mut st, rects) = open_disasm_menu();
    let r = rects[2]; // "Copy data"
    let a = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(a, Some(MenuOutcome::Command(Action::DbgCopyData(0x0102))));
    let (mut st, rects) = open_disasm_menu();
    let r = rects[3]; // "Copy code"
    let a = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(a, Some(MenuOutcome::Command(Action::DbgCopyCode(0x0102))));
}

#[test]
fn selecting_run_to_cursor_returns_a_run_action() {
    let (mut st, rects) = open_disasm_menu();
    let r = rects[7]; // "Run to cursor"
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(
        action,
        Some(MenuOutcome::Act(DebugAction::RunToCursor(0x0102)))
    );
}

#[test]
fn stay_on_bank_toggles_pin_and_freezes_the_view() {
    let (mut st, rects) = open_disasm_menu();
    assert!(!st.pinned);
    let r = rects[6]; // "Stay on bank and address"
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(action, None, "pin is a view effect, no machine action");
    assert!(st.pinned, "pin turned on");
    assert_eq!(st.disasm_base, 0x0100, "froze the view at the current PC");
    assert!(st.menu.is_none());
}

#[test]
fn clicking_a_disabled_item_or_away_just_closes_the_menu() {
    // A disabled row ("Insert size", index 4) selects nothing.
    let (mut st, rects) = open_disasm_menu();
    let r = rects[4];
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(action, None);
    assert!(st.menu.is_none(), "disabled item dismisses the menu");
    assert_eq!(
        st.cursor,
        Some(0x0102),
        "a disabled item is swallowed — the cursor set by the right-click is not \
         overwritten by falling through to the line under the item"
    );
    // A click off the menu (and off the menu bar) also dismisses it.
    let (mut st, _) = open_disasm_menu();
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let action = on_left_click(NOPS, AREA, &mut st, regs0(), 5, l.memory.y + 5, |_| 0);
    assert_eq!(action, None);
    assert!(st.menu.is_none(), "click-away dismisses the menu");
}

#[test]
fn right_click_with_a_menu_open_dismisses_it() {
    let (mut st, _) = open_disasm_menu();
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.disasm.x + 9,
        l.disasm.y + 1,
        |_| 0,
    );
    assert!(st.menu.is_none(), "a second right-click closes the menu");
}

// --- Go to… modal (RM5) ----------------------------------------------------

use crate::ui::dialog::DialogKey;

/// Type a string of chars into an open dialog via feed_dialog.
fn type_goto(st: &mut DebuggerState, s: &str) {
    for ch in s.chars() {
        feed_dialog(st, DialogKey::Char(ch));
    }
}

#[test]
fn disasm_menu_go_to_is_enabled_and_opens_the_dialog() {
    let (mut st, rects) = open_disasm_menu();
    // "Go to…" is the first item, now enabled (was greyed in M5a).
    let r = rects[0];
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(action, None, "opening a dialog is a view effect");
    assert!(st.menu.is_none(), "menu closed");
    let gd = st.dialog.as_ref().expect("Go-to dialog opened");
    assert_eq!(gd.kind, DialogKind::Goto(GotoTarget::Disasm));
}

#[test]
fn goto_disasm_pins_the_view_to_the_entered_address() {
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Disasm);
    type_goto(&mut st, "0150");
    feed_dialog(&mut st, DialogKey::Enter);
    assert!(st.dialog.is_none(), "accept closes the dialog");
    assert!(st.pinned, "disasm Go-to pins the view");
    assert_eq!(st.disasm_base, 0x0150);
}

#[test]
fn goto_disasm_bank_prefixed_pins_the_disasm_bank() {
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Disasm);
    // `01:6401` while some other bank is live must show bank 1, not the live bank.
    type_goto(&mut st, "01:6401");
    feed_dialog(&mut st, DialogKey::Enter);
    assert_eq!(st.disasm_base, 0x6401);
    assert_eq!(
        st.disasm_bank,
        Some(1),
        "BB:AAAA pins the disasm pane's bank"
    );
    assert!(st.pinned, "a banked Go-to pins the view");
    // A breakpoint toggled here qualifies to bank 1 (switchable-ROM address).
    assert_eq!(st.disasm_bp_bank(0x6401), Some(1));
    // Re-attaching to PC drops the pinned bank (follows the live bank again).
    st.center_disasm_on_pc(0x0100, |_| 0x00, 10);
    assert_eq!(st.disasm_bank, None, "go-to-PC clears the pinned bank");
}

#[test]
fn goto_disasm_symbol_pins_its_own_bank() {
    let mut st = DebuggerState {
        symbols: std::rc::Rc::new(crate::symbols::SymbolTable::parse("01:6401 SomeWhere")),
        ..DebuggerState::default()
    };
    open_goto(&mut st, GotoTarget::Disasm);
    type_goto(&mut st, "SomeWhere");
    feed_dialog(&mut st, DialogKey::Enter);
    assert_eq!(
        (st.disasm_base, st.disasm_bank),
        (0x6401, Some(1)),
        "a banked symbol jumps into its own bank"
    );
}

#[test]
fn goto_memory_repositions_the_memory_base() {
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "C000");
    feed_dialog(&mut st, DialogKey::Enter);
    assert!(st.dialog.is_none());
    assert_eq!(st.mem_base, 0xC000);
    assert!(!st.pinned, "memory Go-to does not pin the disasm view");
}

#[test]
fn goto_memory_bank_prefixed_pins_the_bank_and_base() {
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "05:4000");
    feed_dialog(&mut st, DialogKey::Enter);
    assert_eq!(st.mem_base, 0x4000);
    assert_eq!(st.mem_bank, Some(5), "BB:AAAA pins the memory pane's bank");
    // A colon-less Go-to still just repositions, leaving the pinned bank.
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "6000");
    feed_dialog(&mut st, DialogKey::Enter);
    assert_eq!(
        (st.mem_base, st.mem_bank),
        (0x6000, Some(5)),
        "plain goto keeps bank"
    );
}

#[test]
fn goto_escape_cancels_without_moving_the_view() {
    let mut st = DebuggerState::default();
    let base = st.disasm_base;
    open_goto(&mut st, GotoTarget::Disasm);
    type_goto(&mut st, "ABCD");
    feed_dialog(&mut st, DialogKey::Escape);
    assert!(st.dialog.is_none(), "escape closes");
    assert_eq!(st.disasm_base, base, "view unchanged on cancel");
    assert!(!st.pinned);
}

#[test]
fn feed_dialog_with_no_dialog_open_consumes_nothing() {
    let mut st = DebuggerState::default();
    let (consumed, outcome) = feed_dialog(&mut st, DialogKey::Enter);
    assert!(!consumed, "no dialog -> not consumed");
    assert_eq!(outcome, None);
}

#[test]
fn dialog_click_ok_accepts_and_cancel_dismisses() {
    use crate::ui::dialog::{self, DialogLayout};
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "8000");
    let DialogLayout { ok, cancel, .. } = dialog::layout(AREA);
    // Click OK -> accept.
    assert!(dialog_click(&mut st, AREA, ok.x + ok.w / 2, ok.y + ok.h / 2).0);
    assert_eq!(st.mem_base, 0x8000);
    assert!(st.dialog.is_none());
    // Re-open, click Cancel -> dismiss, no change.
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "1234");
    assert!(
        dialog_click(
            &mut st,
            AREA,
            cancel.x + cancel.w / 2,
            cancel.y + cancel.h / 2
        )
        .0
    );
    assert_eq!(st.mem_base, 0x8000, "cancel left the base unchanged");
    assert!(st.dialog.is_none());
}

// --- edit register (RM11) + jump/call cursor (RM7) -------------------------

#[test]
fn jump_and_call_cursor_return_their_actions() {
    // Disasm menu (cursor 0x0102): Jump to cursor = index 8, Call cursor = 9.
    let (mut st, rects) = open_disasm_menu();
    let r = rects[8];
    let out = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(out, Some(MenuOutcome::Act(DebugAction::SetPc(0x0102))));

    let (mut st, rects) = open_disasm_menu();
    let r = rects[9];
    let out = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(out, Some(MenuOutcome::Act(DebugAction::Call(0x0102))));
}

#[test]
fn address_list_menu_lists_entries_with_clear_choices() {
    // Breakpoint manager: each row clears (toggles) its breakpoint.
    let m = address_list_menu(
        &[0x0150, 0xC000],
        DebugAction::ClearBreakpoint,
        &SymbolTable::default(),
        |_| 0,
        (40, 30),
    );
    assert_eq!(m.items.len(), 2);
    assert!(m.items[0].label.contains("0150"));
    assert_eq!(
        m.choices[0],
        MenuChoice::Act(DebugAction::ClearBreakpoint(0x0150))
    );
    assert_eq!(
        m.choices[1],
        MenuChoice::Act(DebugAction::ClearBreakpoint(0xC000))
    );
    // Watchpoint manager uses the watchpoint clear action.
    let w = address_list_menu(
        &[0xFF44],
        DebugAction::ClearWatchpoint,
        &SymbolTable::default(),
        |_| 0,
        (40, 30),
    );
    assert_eq!(
        w.choices[0],
        MenuChoice::Act(DebugAction::ClearWatchpoint(0xFF44))
    );
    // Empty → a single greyed "(none)".
    let e = address_list_menu(
        &[],
        DebugAction::ClearBreakpoint,
        &SymbolTable::default(),
        |_| 0,
        (40, 30),
    );
    assert_eq!(e.items.len(), 1);
    assert!(!e.items[0].enabled, "(none) is greyed");
    assert_eq!(e.choices[0], MenuChoice::None);
}

#[test]
fn set_watchpoint_menu_item_returns_a_toggle_action() {
    // "Set watchpoint..." is index 10 (RM8); cursor 0x0102.
    let (mut st, rects) = open_disasm_menu();
    let r = rects[10];
    let out = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(
        out,
        Some(MenuOutcome::Act(DebugAction::ToggleWatchpoint(0x0102)))
    );
}

#[test]
fn editable_register_row_opens_a_seeded_prompt_and_writes_on_accept() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let mut st = DebuggerState::default();
    // Right-click the af row (row 0): the menu's lone item is enabled.
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.regs.x + 5,
        l.regs.y + 5,
        |_| 0,
    );
    let om = st.menu.as_ref().expect("registers menu");
    assert_eq!(om.items.len(), 1);
    assert!(om.items[0].enabled, "edit register enabled on an af row");
    let rects = menu_rects(om.origin, &om.items);
    let r = rects[0];
    // Click it with AF=0x12F0 live (F low nibble already 0) → a prompt seeded
    // with the current value.
    let mut regs = Registers::default();
    regs.set_af(0x12F0);
    let out = on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs,
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert_eq!(out, None, "opening the prompt is a view effect");
    let md = st.dialog.as_ref().expect("edit-register prompt opened");
    assert_eq!(md.kind, DialogKind::EditReg(RegField::Af));
    assert_eq!(md.input.buffer, "12F0", "seeded with the live AF");
    // Accepting the seeded value yields the register write for `main`.
    let (consumed, out) = feed_dialog(&mut st, DialogKey::Enter);
    assert!(consumed);
    assert_eq!(
        out,
        Some(MenuOutcome::Act(DebugAction::SetReg(RegField::Af, 0x12F0)))
    );
    assert!(st.dialog.is_none(), "accept closes the prompt");
}

#[test]
fn registers_not_editable_option_greys_edit_register() {
    // Debug → "Registers can be edited" off: even the af row's edit item greys.
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let mut st = DebuggerState {
        registers_editable: false,
        ..DebuggerState::default()
    };
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.regs.x + 5,
        l.regs.y + 5,
        |_| 0,
    );
    let om = st.menu.as_ref().expect("registers menu");
    assert_eq!(om.items.len(), 1);
    assert!(
        !om.items[0].enabled,
        "edit register greyed when the option is off"
    );
}

#[test]
fn non_editable_register_row_greys_edit_register() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let lh = line_height();
    let mut st = DebuggerState::default();
    // A row past pc (ime/spd, ima) is not an editable pair.
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.regs.x + 5,
        l.regs.y + 6 * lh + 1,
        |_| 0,
    );
    let om = st.menu.as_ref().expect("registers menu");
    assert_eq!(om.items.len(), 1);
    assert!(!om.items[0].enabled, "ime/ima row: edit register greyed");
}

// --- code/data hints (RM9) -------------------------------------------------

// `disasm_rows` data-hint + format tests live in `debugger/disasm_tests.rs`
// alongside the `disasm` submodule they cover.

/// Open a pane's menu and click the item at `idx`; returns the state after.
fn click_menu_item(target_kind: char, idx: usize) -> DebuggerState {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let lh = line_height();
    let (px, py) = match target_kind {
        'd' => (l.disasm.x + 9, l.disasm.y + 2 * lh + 1),
        's' => (l.stack.x + 5, l.stack.y + lh),
        _ => unreachable!(),
    };
    let mut st = DebuggerState::default();
    on_right_click(NOPS, AREA, &mut st, 0x0100, 0xFFFE, px, py, |_| 0);
    let rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let r = rects[idx];
    on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    st
}

#[test]
fn modify_code_data_toggles_the_hint_at_the_cursor() {
    // Disasm "Modify code/data" is index 1; cursor resolves to 0x0102.
    let st = click_menu_item('d', 1);
    assert!(st.data_hints.contains(&0x0102), "marked data");
    assert!(st.menu.is_none());
    // Toggling again (re-open + click) clears it.
    let mut st = st;
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let lh = line_height();
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.disasm.x + 9,
        l.disasm.y + 2 * lh + 1,
        |_| 0,
    );
    let rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let r = rects[1];
    on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert!(!st.data_hints.contains(&0x0102), "toggled back to code");
}

#[test]
fn force_code_view_clears_a_data_hint() {
    // Pre-mark 0x0102 as data, then "force code view" (disasm index 5) clears it.
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let lh = line_height();
    let mut st = DebuggerState::default();
    st.data_hints.insert(0x0102);
    on_right_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        l.disasm.x + 9,
        l.disasm.y + 2 * lh + 1,
        |_| 0,
    );
    let rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let r = rects[5]; // "force code view"
    on_left_click(
        NOPS,
        AREA,
        &mut st,
        regs0(),
        r.x + r.w / 2,
        r.y + r.h / 2,
        |_| 0,
    );
    assert!(!st.data_hints.contains(&0x0102), "forced back to code");
}

#[test]
fn stack_data_go_here_marks_data_and_code_go_here_clears() {
    // Stack menu: index 3 = "Data go here", index 2 = "Code go here".
    // Stack row 1 from SP 0xFFFE resolves to 0xFFFC.
    let st = click_menu_item('s', 3);
    assert!(st.data_hints.contains(&0xFFFC), "Data go here marked it");
    let st = click_menu_item('s', 2);
    assert!(
        !st.data_hints.contains(&0xFFFC),
        "Code go here leaves it code"
    );
}
