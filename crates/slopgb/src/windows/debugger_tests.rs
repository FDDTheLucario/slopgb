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
fn layout_degenerate_sizes_do_not_panic_or_go_negative() {
    for (w, h) in [(0, 0), (1, 1), (10, 5), (2000, 1200)] {
        let l = DebuggerLayout::for_size(w, h);
        for r in [l.menu, l.disasm, l.regs, l.stack, l.memory] {
            assert!(r.w >= 0 && r.h >= 0, "negative pane {r:?} at {w}x{h}");
        }
    }
}
