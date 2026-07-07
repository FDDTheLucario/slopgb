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
    let out = call_text(&Call::Disassemble { from: "0100".into(), to: "0100".into() }, &gb, &mut bps, &syms);
    let line = out.lines().next().unwrap();
    assert!(line.starts_with("00:0100\tEntry\tjr "), "label + mnemonic: {line:?}");
    assert!(line.contains("Start"), "operand replaced by symbol: {line:?}");
    assert!(line.ends_with("\t3"), "bare cycle count: {line:?}");
}

#[test]
fn disassemble_empty_label_is_two_tabs() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0xAF])).unwrap(); // xor a
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(&Call::Disassemble { from: "0100".into(), to: "0100".into() }, &gb, &mut bps, &syms);
    assert!(out.starts_with("00:0100\t\txor a\t"), "no label → two tabs: {out:?}");
}

#[test]
fn peek_rows_of_16() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x18, 0x4E])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(&Call::Peek { from: "0100".into(), to: "0103".into() }, &gb, &mut bps, &syms);
    assert_eq!(out, "00:0100\t18 4E 00 00\n");
    // A 17-byte span wraps to two rows.
    let out = call_text(&Call::Peek { from: "0100".into(), to: "0110".into() }, &gb, &mut bps, &syms);
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
    let out = call_text(&Call::Cdl { from: "0100".into(), to: "0101".into() }, &gb, &mut bps, &syms);
    assert_eq!(out, "00:0100\t. .\n");
    // Mark ROM offset 0x0100 as executed, 0x0101 as read+write.
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    fx[0x0100] = 4;
    fx[0x0101] = 1 | 2;
    assert!(gb.load_cdl(&fx));
    let out = call_text(&Call::Cdl { from: "0100".into(), to: "0101".into() }, &gb, &mut bps, &syms);
    assert_eq!(out, "00:0100\tx rw\n");
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
    assert!(out.starts_with("error:"), "bad expr surfaces the error: {out:?}");
}

#[test]
fn breakpoint_sets_pc() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    let out = call_text(&Call::Breakpoint { addr: "0150".into() }, &gb, &mut bps, &syms);
    assert!(bps.contains(0x0150), "breakpoint inserted");
    assert!(out.contains("0150"));
    // Idempotent: setting again keeps it set.
    let _ = call_text(&Call::Breakpoint { addr: "0150".into() }, &gb, &mut bps, &syms);
    assert!(bps.contains(0x0150));
}

#[test]
fn bad_address_is_an_error_not_a_panic() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let syms = SymbolTable::default();
    let mut bps = Breakpoints::default();
    // VRAM without a bank; and a straddling range.
    assert!(dispatch(&Call::Peek { from: "8000".into(), to: "8010".into() }, &gb, &mut bps, &syms).is_err());
    assert!(dispatch(&Call::Cdl { from: "3FF0".into(), to: "04:4001".into() }, &gb, &mut bps, &syms).is_err());
}
