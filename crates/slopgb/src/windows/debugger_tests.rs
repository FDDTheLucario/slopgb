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
            &t,
        );
    }
    assert!(rows.iter().any(|r| r.addr == 0x102));
    // The 3rd row (index 2) carries the blue current-PC bar.
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
