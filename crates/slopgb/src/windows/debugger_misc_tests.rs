//! Debugger tests added in the bgb-viewers-debugger round: memory-pane scroll,
//! double-click breakpoints, go-to-by-symbol, and breakpoint-manager symbol
//! labels. Split from `debugger_tests.rs` to keep that file under the size cap.

use super::*;
use crate::symbols::SymbolTable;
use crate::ui::canvas::Rect;
use crate::ui::text::line_height;
use std::rc::Rc;

const AREA: Rect = Rect::new(0, 0, 760, 560);
const NOPS: fn(u16) -> u8 = |_| 0x00; // every line a 1-byte nop

#[test]
fn scroll_memory_moves_the_base_by_rows_and_wraps() {
    let mut st = DebuggerState {
        mem_base: 0xFF00,
        ..DebuggerState::default()
    };
    st.scroll_memory(-1);
    assert_eq!(st.mem_base, 0xFEF0, "one row up = -16 bytes");
    st.scroll_memory(2);
    assert_eq!(st.mem_base, 0xFF10, "two rows down = +32 bytes");
    // Page-sized and wrapping moves.
    st.scroll_memory(8);
    assert_eq!(st.mem_base, 0xFF90);
    st.mem_base = 0xFFF0;
    st.scroll_memory(1);
    assert_eq!(st.mem_base, 0x0000, "wraps past the top of memory");
    st.scroll_memory(-1);
    assert_eq!(st.mem_base, 0xFFF0, "wraps past the bottom");
}

#[test]
fn disasm_follow_keeps_view_until_pc_leaves_then_repages() {
    let mut st = DebuggerState {
        disasm_base: 0x0100,
        ..DebuggerState::default()
    };
    // 8 visible NOP rows cover 0x0100..=0x0107.
    st.disasm_follow(0x0105, NOPS, 8);
    assert_eq!(st.disasm_base, 0x0100, "pc in window: base unchanged");
    st.disasm_follow(0x0400, NOPS, 8);
    assert_eq!(st.disasm_base, 0x0400, "pc off window: re-page to pc");
    // Pinned never follows.
    let mut p = DebuggerState {
        disasm_base: 0x0100,
        pinned: true,
        ..DebuggerState::default()
    };
    p.disasm_follow(0x0400, NOPS, 8);
    assert_eq!(p.disasm_base, 0x0100, "pinned stays put");
}

#[test]
fn scroll_disasm_moves_by_instruction_and_pins() {
    // 0x0100 = LD BC,d16 (opcode 0x01, 3 bytes); everything else a NOP.
    let read = |a: u16| if a == 0x0100 { 0x01 } else { 0x00 };
    let mut st = DebuggerState {
        disasm_base: 0x0100,
        ..DebuggerState::default()
    };
    st.scroll_disasm(1, read);
    assert_eq!(st.disasm_base, 0x0103, "down one = past the 3-byte insn");
    assert!(st.pinned, "manual scroll detaches follow");
    st.scroll_disasm(-1, read);
    assert_eq!(
        st.disasm_base, 0x0100,
        "up one = back-scan to predecessor start"
    );
}

#[test]
fn scroll_stack_offsets_and_clamps() {
    let mut st = DebuggerState::default();
    assert_eq!(st.stack_off, 0);
    st.scroll_stack(-1);
    assert_eq!(st.stack_off, 0, "clamps at the top of the stack");
    st.scroll_stack(3);
    assert_eq!(st.stack_off, 3);
    st.scroll_stack(-1);
    assert_eq!(st.stack_off, 2);
}

#[test]
fn stack_hit_test_honors_offset() {
    let st = DebuggerState {
        stack_off: 2,
        ..DebuggerState::default()
    };
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    // Row 0 of a stack scrolled down 2 words = SP - (2+0)*2 = SP - 4.
    let t = target_at(
        NOPS,
        AREA,
        &st,
        0x0100,
        0xFFFE,
        l.stack.x + 2,
        l.stack.y + 1,
        |_| 0,
    );
    assert_eq!(t, ClickTarget::Stack(0xFFFA));
}

#[test]
fn goto_resolves_a_symbol_name_then_hex() {
    let mut st = DebuggerState {
        symbols: Rc::new(SymbolTable::parse("00:4000 Reset")),
        ..DebuggerState::default()
    };
    // A symbol name pins the disasm to its address.
    accept_dialog(&mut st, DialogKind::Goto(GotoTarget::Disasm), "Reset");
    assert!(st.pinned && st.disasm_base == 0x4000, "name resolved");
    // Resolution is case-insensitive.
    let mut st = DebuggerState {
        symbols: Rc::new(SymbolTable::parse("00:4000 Reset")),
        ..DebuggerState::default()
    };
    accept_dialog(&mut st, DialogKind::Goto(GotoTarget::Memory), "reset");
    assert_eq!(st.mem_base, 0x4000);
    // A bare hex address still works (no matching symbol).
    let mut st = DebuggerState::default();
    accept_dialog(&mut st, DialogKind::Goto(GotoTarget::Memory), "C000");
    assert_eq!(st.mem_base, 0xC000);
    // The rendered RGBDS `$`-hex form is accepted too.
    let mut st = DebuggerState::default();
    accept_dialog(&mut st, DialogKind::Goto(GotoTarget::Memory), "$D000");
    assert_eq!(st.mem_base, 0xD000);
    // An unknown name that isn't valid hex changes nothing.
    let mut st = DebuggerState::default();
    accept_dialog(&mut st, DialogKind::Goto(GotoTarget::Memory), "nope");
    assert_eq!(st.mem_base, 0xFF00, "unresolved -> unchanged");
}

#[test]
fn double_click_disasm_toggles_a_breakpoint() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let lh = line_height();
    let st = DebuggerState::default();
    // Row 2 of the disasm pane = pc + 2 = 0x0102 (NOPS = 1-byte lines).
    let (px, py) = (l.disasm.x + 9, l.disasm.y + 2 * lh + 1);
    assert_eq!(
        on_double_click(NOPS, AREA, &st, 0x0100, 0xFFFE, px, py, |_| 0),
        Some(MenuOutcome::Act(DebugAction::ToggleBreakpoint(
            0x0102, None
        ))),
        "double-click on a disasm line toggles its breakpoint"
    );
    // Off the disasm pane (the menu bar) it does nothing.
    assert_eq!(
        on_double_click(
            NOPS,
            AREA,
            &st,
            0x0100,
            0xFFFE,
            l.menu.x + 2,
            l.menu.y + 1,
            |_| 0
        ),
        None
    );
    // With a context menu open, a double-click is swallowed.
    let mut st2 = DebuggerState::default();
    on_right_click(NOPS, AREA, &mut st2, 0x0100, 0xFFFE, px, py, |_| 0);
    assert!(on_double_click(NOPS, AREA, &st2, 0x0100, 0xFFFE, px, py, |_| 0).is_none());
}

#[test]
fn address_list_menu_appends_symbol_names() {
    let syms = SymbolTable::parse("00:0150 Reset");
    let m = address_list_menu(
        &[0x0150, 0xC000],
        DebugAction::ClearBreakpoint,
        &syms,
        |_| 0,
        (40, 30),
    );
    // The known address gets its symbol name appended; the unknown one doesn't.
    assert!(m.items[0].label.contains("0150") && m.items[0].label.contains("Reset"));
    assert!(m.items[1].label.contains("C000") && !m.items[1].label.contains("Reset"));
}
