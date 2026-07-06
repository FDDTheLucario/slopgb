//! Layout partition + regs/stack/memory rendering tests.

use super::super::*;

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

// `disasm_rows` decode/format + `render_disasm` highlight tests live in
// `debugger/disasm_tests.rs` (next to the `disasm` submodule).

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
        cnt: 144,
        rom_bank: 0x2A,
        ram_bank: Some(1),
    };
    let l = regs_lines(&v);
    assert_eq!(l[0], "af= 1180   lcdc=91");
    assert_eq!(l[1], "bc= 0000   stat=81");
    assert_eq!(l[2], "de= FF56   ly= 90");
    // The hl line carries the user-clock counter (RM14) in its right column.
    assert_eq!(l[3], "hl= 000D   cnt= 144");
    assert_eq!(l[4], "sp= FFFE   ie= 00");
    assert_eq!(l[5], "pc= 0100   if= F1");
    assert_eq!(l[6], "ime=.   spd= 0");
    // The ima line's right column carries the cartridge ROM/RAM bank indicator
    // (distinct from the VRAM/WRAM banks at FF4F/FF70).
    assert_eq!(l[7], "ima=.   rom 02A ram 01");
}

#[test]
fn regs_lines_show_double_dash_when_ram_bank_disabled() {
    let v = RegsView {
        af: 0,
        bc: 0,
        de: 0,
        hl: 0,
        sp: 0,
        pc: 0,
        ime: false,
        ima: false,
        lcdc: 0,
        stat: 0,
        ly: 0,
        ie: 0,
        iflag: 0,
        double_speed: false,
        cnt: 0,
        rom_bank: 1,
        ram_bank: None,
    };
    assert_eq!(regs_lines(&v)[7], "ima=.   rom 001 ram --");
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
    let rows = memory_rows(read, 0x0000, 2, &SymbolTable::default());
    assert_eq!(rows.len(), 2);
    assert!(rows[0].starts_with("ROM0:0000 00 01 02 03 04 05 06 07  08"));
    assert!(rows[1].starts_with("ROM0:0010 10 11 12 13"));
}

#[test]
fn memory_rows_append_symbol_name_at_row_base() {
    let read = |a: u16| (a & 0xFF) as u8;
    let syms = SymbolTable::parse("00:0010 WorkVar");
    let rows = memory_rows(read, 0x0000, 2, &syms);
    // Row base 0x0000 has no symbol -> unchanged.
    assert!(!rows[0].contains("WorkVar"));
    // Row base 0x0010 == symbol WorkVar -> name appended to that row.
    assert!(rows[1].contains("WorkVar"), "{}", rows[1]);
}
