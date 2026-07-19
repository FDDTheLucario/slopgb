use super::*;
use slopgb_core::{GameBoy, Model};

/// A minimal 32 KiB test ROM with `bytes` written at `0x0100`.
fn rom_at_0100(bytes: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100..0x100 + bytes.len()].copy_from_slice(bytes);
    rom
}

fn call_text(call: &Call, gb: &GameBoy, bps: &mut Breakpoints, syms: &SymbolTable) -> String {
    match dispatch(call, gb, bps, syms).unwrap() {
        ToolResult::Text(t) => t,
        ToolResult::Image(_) => panic!("expected text"),
    }
}

#[test]
fn disassemble_labels_and_substitutes_operand() {
    // 0x0100: `jr $0150` (18 4E) — targets 0x0150, which is symbol `Start`.
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x18, 0x4E])).unwrap();
    let syms = SymbolTable::parse("00:0100 Entry\n00:0150 Start\n");
    let mut bps = Breakpoints::default();
    let out = call_text(
        &Call::Disassemble {
            from: "0100".into(),
            to: "0100".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    let line = out.lines().next().unwrap();
    assert!(
        line.starts_with("00:0100\tEntry\tjr "),
        "label + mnemonic: {line:?}"
    );
    assert!(
        line.contains("Start"),
        "operand replaced by symbol: {line:?}"
    );
    assert!(line.ends_with("\t3"), "bare cycle count: {line:?}");
}

#[test]
fn disassemble_empty_label_is_two_tabs() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0xAF])).unwrap(); // xor a
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(
        &Call::Disassemble {
            from: "0100".into(),
            to: "0100".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    assert!(
        out.starts_with("00:0100\t\txor a\t"),
        "no label → two tabs: {out:?}"
    );
}

#[test]
fn peek_rows_of_16() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x18, 0x4E])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(
        &Call::Peek {
            from: "0100".into(),
            to: "0103".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    assert_eq!(out, "00:0100\t18 4E 00 00\n");
    // A 17-byte span wraps to two rows.
    let out = call_text(
        &Call::Peek {
            from: "0100".into(),
            to: "0110".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    let rows: Vec<&str> = out.lines().collect();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].starts_with("00:0100\t") && rows[1].starts_with("00:0110\t"));
}

#[test]
fn cdl_words_reflect_flags() {
    let mut gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0xAF])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    // Off → every cell is '.'.
    let out = call_text(
        &Call::Cdl {
            from: "0100".into(),
            to: "0101".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    assert_eq!(out, "00:0100\t. .\n");
    // Mark ROM offset 0x0100 as executed, 0x0101 as read+write.
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    fx[0x0100] = 4;
    fx[0x0101] = 1 | 2;
    assert!(gb.load_cdl(&fx));
    let out = call_text(
        &Call::Cdl {
            from: "0100".into(),
            to: "0101".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    assert_eq!(out, "00:0100\tx rw\n");
}

#[test]
fn cdl_ranges_lists_continuous_spans_in_address_form() {
    let mut gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0xAF])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    // Off → nothing logged → empty output.
    assert_eq!(call_text(&Call::CdlRanges, &gb, &mut bps, &syms), "");
    // 32 KiB no-RAM DMG layout: rom [0,0x8000), vram [0x8000,0xC000),
    // wram [0xC000,0xE000) (bank0 @ 0xC000, bank1 @ 0xD000).
    gb.set_cdl(true);
    let mut fx = vec![0u8; gb.cdl_flags().unwrap().len()];
    fx[0x0100..=0x0102].fill(4); // ROM0 (bare) 0100-0102
    fx[0xC000 + 0x1000 + 5] = 1; // WRAMX bank1 @ 0xD005 (single byte, banked form)
    assert!(gb.load_cdl(&fx));
    assert_eq!(
        call_text(&Call::CdlRanges, &gb, &mut bps, &syms),
        "0100-0102\n01:d005-01:d005\n"
    );
}

#[test]
fn coprocessor_reports_not_sgb_on_dmg_and_a_backend_on_sgb() {
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    // A DMG machine has no SGB coprocessor at all.
    let dmg = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let out = call_text(&Call::Coprocessor, &dmg, &mut bps, &syms);
    let lower = out.to_lowercase();
    assert!(
        lower.contains("not") && lower.contains("super game boy"),
        "DMG must report no SGB coprocessor: {out:?}"
    );
    // An SGB machine has the built-in HLE APU by default (no wasm coprocessor).
    let sgb = GameBoy::new(Model::Sgb, rom_at_0100(&[0x00])).unwrap();
    let out = call_text(&Call::Coprocessor, &sgb, &mut bps, &syms);
    assert!(
        out.contains("HLE"),
        "SGB default backend is the built-in HLE: {out:?}"
    );
}

#[test]
fn dump_spc_explains_when_nothing_to_dump() {
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    // DMG has no SPC700 — a live dump reports that, and writes no file.
    let dmg = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let out = call_text(
        &Call::DumpSpc {
            mode: "live".into(),
        },
        &dmg,
        &mut bps,
        &syms,
    );
    assert!(out.to_lowercase().contains("no spc"), "DMG live: {out:?}");
    // SGB with no song played yet has no from-start snapshot.
    let sgb = GameBoy::new(Model::Sgb, rom_at_0100(&[0x00])).unwrap();
    let out = call_text(
        &Call::DumpSpc {
            mode: "start".into(),
        },
        &sgb,
        &mut bps,
        &syms,
    );
    assert!(
        out.to_lowercase().contains("no from-start"),
        "SGB start, no song: {out:?}"
    );
    // A bogus mode is a clear error, not a panic or a write.
    let out = call_text(
        &Call::DumpSpc {
            mode: "sideways".into(),
        },
        &sgb,
        &mut bps,
        &syms,
    );
    assert!(out.contains("unknown mode"), "bad mode: {out:?}");
}

#[test]
fn registers_has_every_field() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(&Call::Registers, &gb, &mut bps, &syms);
    for key in [
        "af=", "bc=", "de=", "hl=", "sp=", "pc=", "lcdc=", "stat=", "ly=", "cnt=", "ie=", "if=",
        "ime=", "ima=", "spd=", "rom=", "ram=", "wave=",
    ] {
        assert!(out.contains(key), "missing {key} in {out:?}");
    }
    assert!(out.contains("ram=--"), "no cart RAM → --: {out:?}");
    // wave = 16 bytes → 32 hex nibbles.
    let wave = out.rsplit("wave=").next().unwrap();
    assert_eq!(wave.len(), 32, "wave is 32 hex chars: {wave:?}");
}

#[test]
fn expr_evaluates_and_reports_errors() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(&Call::Expr { expr: "1+2".into() }, &gb, &mut bps, &syms);
    assert_eq!(out, "0x0003 (3)");
    let out = call_text(&Call::Expr { expr: "@#$".into() }, &gb, &mut bps, &syms);
    assert!(
        out.starts_with("error:"),
        "bad expr surfaces the error: {out:?}"
    );
}

#[test]
fn breakpoint_sets_pc() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(
        &Call::Breakpoint {
            addr: "0150".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    assert!(bps.contains(0x0150), "breakpoint inserted");
    assert!(out.contains("0150"));
    // Idempotent: setting again keeps it set.
    let _ = call_text(
        &Call::Breakpoint {
            addr: "0150".into(),
        },
        &gb,
        &mut bps,
        &syms,
    );
    assert!(bps.contains(0x0150));
}

#[test]
fn screencap_returns_a_png_of_the_screen() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let mut bps = Breakpoints::default();
    let syms = SymbolTable::default();
    match dispatch(&Call::Screencap { scale: 1 }, &gb, &mut bps, &syms).unwrap() {
        ToolResult::Image(png) => {
            assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
            // IHDR width/height = 160x144 (SCREEN_W x SCREEN_H), big-endian.
            assert_eq!(&png[16..24], &[0, 0, 0, 160, 0, 0, 0, 144]);
        }
        ToolResult::Text(_) => panic!("expected an image"),
    }
}

#[test]
fn screencap_scale_magnifies_the_png_dimensions() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let mut bps = Breakpoints::default();
    let syms = SymbolTable::default();
    // 3x → 480x432 in the IHDR (160*3, 144*3).
    match dispatch(&Call::Screencap { scale: 3 }, &gb, &mut bps, &syms).unwrap() {
        ToolResult::Image(png) => assert_eq!(&png[16..24], &[0, 0, 1, 224, 0, 0, 1, 176]),
        ToolResult::Text(_) => panic!("expected an image"),
    }
}

#[test]
fn parse_scale_accepts_suffix_and_bare_and_rejects_others() {
    assert_eq!(parse_scale(None), Ok(1)); // absent → native
    assert_eq!(parse_scale(Some("")), Ok(1));
    assert_eq!(parse_scale(Some("4x")), Ok(4));
    assert_eq!(parse_scale(Some("4")), Ok(4)); // bare digit
    assert_eq!(parse_scale(Some(" 6X ")), Ok(6)); // trimmed, case-insensitive
    assert!(parse_scale(Some("7x")).is_err()); // out of 2..6
    assert!(parse_scale(Some("big")).is_err());
}

#[test]
fn bad_address_is_an_error_not_a_panic() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    // VRAM without a bank; and a straddling range.
    assert!(
        dispatch(
            &Call::Peek {
                from: "8000".into(),
                to: "8010".into()
            },
            &gb,
            &mut bps,
            &syms
        )
        .is_err()
    );
    assert!(
        dispatch(
            &Call::Cdl {
                from: "3FF0".into(),
                to: "04:4001".into()
            },
            &gb,
            &mut bps,
            &syms
        )
        .is_err()
    );
}
