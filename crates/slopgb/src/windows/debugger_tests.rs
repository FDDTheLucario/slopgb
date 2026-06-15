use super::*;

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
    let rows = disasm_rows(mem, 0x100, 3);
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
    assert_eq!(action, Some(DebugAction::ToggleBreakpoint(0x0102)));
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
    assert_eq!(action, Some(DebugAction::RunToCursor(0x0102)));
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
    // A click far outside the menu also dismisses it.
    let (mut st, _) = open_disasm_menu();
    let action = on_left_click(NOPS, AREA, &mut st, 0x0100, 0xFFFE, 5, 5);
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
