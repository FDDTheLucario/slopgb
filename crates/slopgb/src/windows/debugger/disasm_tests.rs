use super::*;
use crate::dbg::Breakpoints;
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use std::collections::BTreeSet;

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
    let rows = disasm_rows(mem, 0x100, 3, &BTreeSet::new(), DisasmFmt::default());
    assert_eq!(rows.len(), 3);

    assert_eq!(rows[0].addr, 0x100);
    assert!(rows[0].text.starts_with("ROM0:0100 "), "{}", rows[0].text);
    assert!(rows[0].text.contains("nop"));
    assert!(rows[0].text.ends_with(";1"));

    assert_eq!(rows[1].addr, 0x101, "advanced past the 1-byte nop");
    assert!(rows[1].text.contains("C3 50 01"));
    assert!(rows[1].text.contains("jp $0150"), "rgbds default: {}", rows[1].text);
    assert!(rows[1].text.ends_with(";4"));

    assert_eq!(rows[2].addr, 0x104, "advanced past the 3-byte jp");
    assert!(rows[2].text.contains("3E FF"));
    assert!(rows[2].text.contains("ld a,$FF"));
}

#[test]
fn render_disasm_highlights_the_pc_row() {
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
            DisasmFmt::default(),
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
fn data_hint_renders_db_and_advances_one_byte() {
    // 0x0150 = C3 50 01 (jp 0150); as code it is 3 bytes, as data one `db C3`.
    let mem = |a: u16| match a {
        0x0150 => 0xC3,
        0x0151 => 0x50,
        0x0152 => 0x01,
        _ => 0x00,
    };
    let hints: BTreeSet<u16> = [0x0150].into_iter().collect();
    let rows = disasm_rows(mem, 0x0150, 2, &hints, DisasmFmt::default());
    assert!(rows[0].text.contains("db $C3"), "{}", rows[0].text);
    assert_eq!(
        rows[1].addr, 0x0151,
        "a data byte advances by 1, not the jp's 3"
    );
    // Without the hint the same address decodes as the 3-byte jp.
    let code = disasm_rows(mem, 0x0150, 2, &BTreeSet::new(), DisasmFmt::default());
    assert!(code[0].text.contains("jp $0150"));
    assert_eq!(code[1].addr, 0x0153);
}

#[test]
fn disasm_fmt_lowercase_hex_and_hide_clocks() {
    // 0x0100: ld a,FF (3E FF) — an operand with A-F hex digits.
    let mem = |a: u16| match a {
        0x0100 => 0x3E,
        0x0101 => 0xFF,
        _ => 0x00,
    };
    let lower = DisasmFmt {
        lowercase_hex: true,
        show_clocks: true,
        rgbds: false,
    };
    let rows = disasm_rows(mem, 0x0100, 1, &BTreeSet::new(), lower);
    assert!(
        rows[0].text.contains("3e ff"),
        "lowercase byte hex: {}",
        rows[0].text
    );
    assert!(
        rows[0].text.contains("ld a,ff"),
        "operand hex lowercased: {}",
        rows[0].text
    );
    assert!(
        rows[0].text.starts_with("ROM0:0100"),
        "region label stays upper: {}",
        rows[0].text
    );

    let no_clk = DisasmFmt {
        lowercase_hex: false,
        show_clocks: false,
        rgbds: false,
    };
    let rows = disasm_rows(mem, 0x0100, 1, &BTreeSet::new(), no_clk);
    assert!(
        !rows[0].text.contains(';'),
        "clocks column hidden: {}",
        rows[0].text
    );
    assert!(
        rows[0].text.contains("3E FF"),
        "upper hex retained: {}",
        rows[0].text
    );
    assert!(
        rows[0].text.contains("ld a,FF"),
        "operand hex upper: {}",
        rows[0].text
    );
}

#[test]
fn disasm_fmt_rgbds_toggle_switches_syntax() {
    // 0x0100: ld a,[$1234] (FA 34 12) — a memory load that differs by dialect.
    let mem = |a: u16| match a {
        0x0100 => 0xFA,
        0x0101 => 0x34,
        0x0102 => 0x12,
        _ => 0x00,
    };
    let rgbds = disasm_rows(mem, 0x0100, 1, &BTreeSet::new(), DisasmFmt::default());
    assert!(
        rgbds[0].text.contains("ld a,[$1234]"),
        "default is rgbds: {}",
        rgbds[0].text
    );
    let bgb = DisasmFmt {
        rgbds: false,
        ..DisasmFmt::default()
    };
    let bgb = disasm_rows(mem, 0x0100, 1, &BTreeSet::new(), bgb);
    assert!(
        bgb[0].text.contains("ld a,(1234)"),
        "toggled to bgb: {}",
        bgb[0].text
    );
}
