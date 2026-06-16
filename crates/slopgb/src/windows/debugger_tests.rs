use super::*;
use std::collections::BTreeSet;

#[test]
fn layout_panes_tile_the_window_without_overlap() {
    let (w, h) = (1172, 786);
    let l = DebuggerLayout::for_size(w, h);

    // Menu spans the full width at the top.
    assert_eq!(l.menu, Rect::new(0, 0, w, 18));
    // Disasm + memory partition the left column; regs + stack the right column.
    assert_eq!(l.disasm.x, 0);
    assert_eq!(
        l.regs.x,
        l.disasm.right(),
        "right column starts where left ends"
    );
    assert_eq!(l.stack.x, l.regs.x);
    // The body (below the menu) is fully covered, no gaps vertically.
    assert_eq!(l.disasm.y, l.menu.bottom());
    assert_eq!(l.regs.y, l.menu.bottom());
    assert_eq!(l.stack.y, l.regs.bottom());
    assert_eq!(l.memory.y, l.disasm.bottom());
    assert_eq!(l.memory.bottom(), h);
    assert_eq!(
        l.stack.bottom(),
        l.disasm.bottom(),
        "right column meets memory"
    );
    // Memory spans full width at the bottom.
    assert_eq!(l.memory.x, 0);
    assert_eq!(l.memory.w, w);

    // No pane overlaps another.
    let panes = [l.menu, l.disasm, l.regs, l.stack, l.memory];
    for (i, a) in panes.iter().enumerate() {
        for b in &panes[i + 1..] {
            let o = a.intersect(b);
            assert!(o.w == 0 || o.h == 0, "panes {a:?} and {b:?} overlap");
        }
    }
    // Proportions: memory ~38% of the body, right column ~33% of width.
    assert!((l.memory.h - (h - 18) * 38 / 100).abs() <= 1);
    assert!((l.regs.w - w * 33 / 100).abs() <= 1);
}

#[test]
fn disasm_rows_decode_format_and_advance() {
    // 0x100: nop; 0x101: jp 0150 (C3 50 01); 0x104: ld a,FF (3E FF).
    let mem = |a: u16| match a {
        0x101 => 0xC3,
        0x102 => 0x50,
        0x103 => 0x01,
        0x104 => 0x3E,
        0x105 => 0xFF,
        _ => 0x00, // nop fills the rest
    };
    let rows = disasm_rows(mem, 0x100, 3, &BTreeSet::new());
    assert_eq!(rows.len(), 3);

    assert_eq!(rows[0].addr, 0x100);
    assert!(rows[0].text.starts_with("ROM0:0100 "), "{}", rows[0].text);
    assert!(rows[0].text.contains("nop"));
    assert!(rows[0].text.ends_with(";1"));

    assert_eq!(rows[1].addr, 0x101, "advanced past the 1-byte nop");
    assert!(rows[1].text.contains("C3 50 01"));
    assert!(rows[1].text.contains("jp 0150"));
    assert!(rows[1].text.ends_with(";4"));

    assert_eq!(rows[2].addr, 0x104, "advanced past the 3-byte jp");
    assert!(rows[2].text.contains("3E FF"));
    assert!(rows[2].text.contains("ld a,FF"));
}

#[test]
fn render_disasm_highlights_the_pc_row() {
    use crate::ui::Theme;
    use crate::ui::canvas::Canvas;
    use crate::ui::text::line_height;
    let t = Theme::BGB;
    let lh = line_height() as usize;
    let (w, h) = (200usize, lh * 4);
    let mut buf = vec![0x00AA_AAAA_u32; w * h];
    let mem = |_a: u16| 0x00u8; // all nops
    let rows;
    {
        let mut c = Canvas::new(&mut buf, w, h);
        // pc = 0x102: nops are 1 byte, so rows are 0x100,0x101,0x102,... -> pc
        // is the 3rd visible row (viewport index 2).
        rows = render_disasm(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            mem,
            0x100,
            0x102,
            &Breakpoints::default(),
            &BTreeSet::new(),
            &t,
        );
    }
    assert!(rows.iter().any(|r| r.addr == 0x102));
    // The 3rd row (index 2) carries the blue current-PC bar (the bar reaches
    // across the gutter to x=0).
    assert_eq!(buf[(2 * lh) * w], t.current, "PC row highlighted");
    assert_ne!(buf[0], t.current, "first row not highlighted");
}

#[test]
fn layout_degenerate_sizes_do_not_panic_or_go_negative() {
    for (w, h) in [(0, 0), (1, 1), (10, 5), (2000, 1200)] {
        let l = DebuggerLayout::for_size(w, h);
        for r in [l.menu, l.disasm, l.regs, l.stack, l.memory] {
            assert!(r.w >= 0 && r.h >= 0, "negative pane {r:?} at {w}x{h}");
        }
    }
}

#[test]
fn regs_lines_match_bgb_two_column_layout() {
    // Values from the real bgb capture (dbg-regs.png).
    let v = RegsView {
        af: 0x1180,
        bc: 0x0000,
        de: 0xFF56,
        hl: 0x000D,
        sp: 0xFFFE,
        pc: 0x0100,
        ime: false,
        ima: false,
        lcdc: 0x91,
        stat: 0x81,
        ly: 0x90,
        ie: 0x00,
        iflag: 0xF1,
        double_speed: false,
    };
    let l = regs_lines(&v);
    assert_eq!(l[0], "af= 1180   lcdc=91");
    assert_eq!(l[1], "bc= 0000   stat=81");
    assert_eq!(l[2], "de= FF56   ly= 90");
    assert_eq!(l[4], "sp= FFFE   ie= 00");
    assert_eq!(l[5], "pc= 0100   if= F1");
    assert_eq!(l[6], "ime=.   spd= 0");
    assert_eq!(l[7], "ima=.");
}

#[test]
fn stack_lines_label_and_format_words() {
    let stack = [(0xFFFEu16, 0x0022u16), (0xFFFC, 0x00F9), (0xFFFA, 0x05D3)];
    let lines = stack_lines(&stack);
    assert_eq!(lines[0], "HRAM:FFFE 0022");
    assert_eq!(lines[1], "HRAM:FFFC 00F9");
    assert_eq!(lines[2], "HRAM:FFFA 05D3");
}

#[test]
fn memory_rows_dump_sixteen_bytes_per_line() {
    let read = |a: u16| (a & 0xFF) as u8; // byte value = low addr byte
    let rows = memory_rows(read, 0x0000, 2);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].starts_with("ROM0:0000 00 01 02 03 04 05 06 07  08"));
    assert!(rows[1].starts_with("ROM0:0010 10 11 12 13"));
}

// --- interaction (RM4 / RM6 / RM7 / RM12) ---------------------------------

use crate::ui::canvas::Canvas;
use crate::ui::menu::menu_rects;
use crate::ui::text::line_height;

/// The default debugger window size, partitioned the way `render_debugger` does.
const AREA: Rect = Rect::new(0, 0, 760, 560);
const NOPS: fn(u16) -> u8 = |_| 0x00; // every line a 1-byte nop

#[test]
fn render_disasm_draws_a_red_gutter_dot_on_breakpoint_rows() {
    use crate::ui::Theme;
    let t = Theme::BGB;
    let lh = line_height() as usize;
    let (w, h) = (200usize, lh * 4);
    let mut buf = vec![0x00AA_AAAA_u32; w * h];
    let mut bps = Breakpoints::default();
    bps.toggle(0x0101); // rows are 0x100,0x101,... -> dot on visible row 1
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
    );
    assert_eq!(t, ClickTarget::Stack(0xFFFC));
    // Registers pane: just the pane id.
    let t = target_at(NOPS, AREA, &st, 0x0100, 0xFFFE, l.regs.x + 5, l.regs.y + 5);
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
    );
    assert_eq!(t, ClickTarget::Disasm(0x0200));
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
    );
    assert_eq!(st.menu.as_ref().unwrap().items.len(), 11);
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
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
    );
    assert_eq!(
        action,
        Some(MenuOutcome::Act(DebugAction::ToggleBreakpoint(0x0102)))
    );
    assert!(st.menu.is_none(), "menu closes after a selection");
}

#[test]
fn selecting_run_to_cursor_returns_a_run_action() {
    let (mut st, rects) = open_disasm_menu();
    let r = rects[7]; // "Run to cursor"
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
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
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
    );
    assert_eq!(action, None, "pin is a view effect, no machine action");
    assert!(st.pinned, "pin turned on");
    assert_eq!(st.disasm_base, 0x0100, "froze the view at the current PC");
    assert!(st.menu.is_none());
}

#[test]
fn clicking_a_disabled_item_or_away_just_closes_the_menu() {
    // A disabled row ("Copy data", index 2) selects nothing.
    let (mut st, rects) = open_disasm_menu();
    let r = rects[2];
    let action = on_left_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
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
    let action = on_left_click(NOPS, AREA, &mut st, 0x0100, 0xFFFE, 5, l.memory.y + 5);
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
    );
    assert!(st.menu.is_none(), "a second right-click closes the menu");
}

// --- Go to… modal (RM5) ----------------------------------------------------

use crate::ui::dialog::DialogKey;

/// Type a string of chars into an open dialog via feed_goto.
fn type_goto(st: &mut DebuggerState, s: &str) {
    for ch in s.chars() {
        feed_goto(st, DialogKey::Char(ch));
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
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
    );
    assert_eq!(action, None, "opening a dialog is a view effect");
    assert!(st.menu.is_none(), "menu closed");
    let gd = st.dialog.as_ref().expect("Go-to dialog opened");
    assert_eq!(gd.target, GotoTarget::Disasm);
}

#[test]
fn goto_disasm_pins_the_view_to_the_entered_address() {
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Disasm);
    type_goto(&mut st, "0150");
    feed_goto(&mut st, DialogKey::Enter);
    assert!(st.dialog.is_none(), "accept closes the dialog");
    assert!(st.pinned, "disasm Go-to pins the view");
    assert_eq!(st.disasm_base, 0x0150);
}

#[test]
fn goto_memory_repositions_the_memory_base() {
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "C000");
    feed_goto(&mut st, DialogKey::Enter);
    assert!(st.dialog.is_none());
    assert_eq!(st.mem_base, 0xC000);
    assert!(!st.pinned, "memory Go-to does not pin the disasm view");
}

#[test]
fn goto_escape_cancels_without_moving_the_view() {
    let mut st = DebuggerState::default();
    let base = st.disasm_base;
    open_goto(&mut st, GotoTarget::Disasm);
    type_goto(&mut st, "ABCD");
    feed_goto(&mut st, DialogKey::Escape);
    assert!(st.dialog.is_none(), "escape closes");
    assert_eq!(st.disasm_base, base, "view unchanged on cancel");
    assert!(!st.pinned);
}

#[test]
fn feed_goto_with_no_dialog_open_consumes_nothing() {
    let mut st = DebuggerState::default();
    assert!(
        !feed_goto(&mut st, DialogKey::Enter),
        "no dialog -> not consumed"
    );
}

#[test]
fn goto_click_ok_accepts_and_cancel_dismisses() {
    use crate::ui::dialog::{self, DialogLayout};
    let mut st = DebuggerState::default();
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "8000");
    let DialogLayout { ok, cancel, .. } = dialog::layout(AREA);
    // Click OK -> accept.
    assert!(goto_click(&mut st, AREA, ok.x + ok.w / 2, ok.y + ok.h / 2));
    assert_eq!(st.mem_base, 0x8000);
    assert!(st.dialog.is_none());
    // Re-open, click Cancel -> dismiss, no change.
    open_goto(&mut st, GotoTarget::Memory);
    type_goto(&mut st, "1234");
    assert!(goto_click(
        &mut st,
        AREA,
        cancel.x + cancel.w / 2,
        cancel.y + cancel.h / 2
    ));
    assert_eq!(st.mem_base, 0x8000, "cancel left the base unchanged");
    assert!(st.dialog.is_none());
}

// --- code/data hints (RM9) -------------------------------------------------

#[test]
fn data_hint_renders_db_and_advances_one_byte() {
    // 0x0150 = C3 50 01 (jp 0150); as code it is 3 bytes, as data one `db C3`.
    let mem = |a: u16| match a {
        0x0150 => 0xC3,
        0x0151 => 0x50,
        0x0152 => 0x01,
        _ => 0x00,
    };
    let hints: BTreeSet<u16> = [0x0150].into_iter().collect();
    let rows = disasm_rows(mem, 0x0150, 2, &hints);
    assert!(rows[0].text.contains("db C3"), "{}", rows[0].text);
    assert_eq!(
        rows[1].addr, 0x0151,
        "a data byte advances by 1, not the jp's 3"
    );
    // Without the hint the same address decodes as the 3-byte jp.
    let code = disasm_rows(mem, 0x0150, 2, &BTreeSet::new());
    assert!(code[0].text.contains("jp 0150"));
    assert_eq!(code[1].addr, 0x0153);
}

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
    on_right_click(NOPS, AREA, &mut st, 0x0100, 0xFFFE, px, py);
    let rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let r = rects[idx];
    on_left_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
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
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
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
        0x0100,
        0xFFFE,
        r.x + r.w / 2,
        r.y + r.h / 2,
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

// --- menu bar + dropdowns (MB1) --------------------------------------------

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
    let counts = [16, 5, 18, 5, 10, 5]; // File, Search, Run, Debug, Window, Profiler
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
    let action = on_left_click(NOPS, AREA, &mut st, 0x0100, 0xFFFE, r.x + 2, r.y + 2);
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
    on_left_click(NOPS, AREA, &mut st, 0x0100, 0xFFFE, r.x + 2, r.y + 2);
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
        0x0100,
        0xFFFE,
        ir.x + ir.w / 2,
        ir.y + ir.h / 2,
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
        0x0100,
        0xFFFE,
        rects[2].x + 2,
        rects[2].y + 2,
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
        0x0100,
        0xFFFE,
        ir.x + ir.w / 2,
        ir.y + ir.h / 2,
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
    on_left_click(NOPS, AREA, &mut st, 0x0100, 0xFFFE, br.x + 2, br.y + 2);
    let item_rects = menu_rects(
        st.menu.as_ref().unwrap().origin,
        &st.menu.as_ref().unwrap().items,
    );
    let ir = item_rects[item_idx];
    on_left_click(
        NOPS,
        AREA,
        &mut st,
        0x0100,
        0xFFFE,
        ir.x + ir.w / 2,
        ir.y + ir.h / 2,
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
    // The reverse / not-yet-built variants stay greyed (no outcome).
    assert_eq!(click_menubar_item(2, 1), None, "Run no break greyed");
    assert_eq!(click_menubar_item(2, 8), None, "Animate greyed");
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
        click_menubar_item(4, 6),
        Some(MenuOutcome::Command(Action::ToggleTool(ToolWindow::IoMap))),
        "IO map toggles its window"
    );
    // Options (F11) is not built yet — stays greyed.
    assert_eq!(click_menubar_item(4, 3), None, "Options greyed");
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
