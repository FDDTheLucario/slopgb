use super::*;
use slopgb_core::{GameBoy, Model};

/// A minimal 32 KiB test ROM with `bytes` written at `0x0100` (the DMG entry).
fn rom_at_0100(bytes: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100..0x100 + bytes.len()].copy_from_slice(bytes);
    rom
}

fn text(r: ToolResult) -> String {
    match r {
        ToolResult::Text(t) => t,
        ToolResult::Image(_) => panic!("expected text result"),
    }
}

/// A process-unique temp path, so parallel test binaries don't share a file.
fn tmp(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("slopgb_sim_{tag}_{}.bin", std::process::id()))
}

/// Drive the fork to completion (cooperative slices), then return its poll text.
fn run_to_done(mcp: &mut Mcp, id: u64) -> String {
    for _ in 0..1000 {
        mcp.advance_sim();
        if mcp.sim.as_ref().unwrap().done.is_some() {
            break;
        }
    }
    text(mcp.sim_result(id).unwrap())
}

#[test]
fn simulate_overlays_runs_and_dumps_leaving_live_untouched() {
    // 0100: LD A,(C000); INC A; LD (C001),A; NOP  — reads the overlay, +1, stores.
    let rom = rom_at_0100(&[0xFA, 0x00, 0xC0, 0x3C, 0xEA, 0x01, 0xC0, 0x00]);
    let gb = GameBoy::new(Model::Dmg, rom).unwrap();

    let dump = tmp("overlay");
    std::fs::write(&dump, [0x41u8]).unwrap();

    let args = SimArgs {
        memdump: dump.to_string_lossy().into_owned(),
        in_from: "C000".into(),
        in_to: "C000".into(),
        out_from: "C001".into(),
        out_to: "C001".into(),
        start: "0100".into(),
        end: Some("0107".into()),
        budget: "1000".into(),
        savestate: None,
    };

    let mut mcp = Mcp::default();
    let started = text(mcp.start_sim(&gb, &args).unwrap());
    assert!(started.contains("job 0"), "start reply: {started}");

    let out = run_to_done(&mut mcp, 0);
    assert!(out.contains("stop: reached_end"), "stop code: {out}");
    // 0x41 came from the overlay; INC -> 0x42, stored to C001 and dumped.
    assert!(
        out.lines().last().unwrap().contains("42"),
        "output dump should show 42: {out}"
    );
    // Golden-safe: the live machine never stepped (PC still at the reset entry).
    assert_eq!(gb.cpu_regs().pc, 0x0100, "live machine advanced");

    std::fs::remove_file(&dump).ok();
}

#[test]
fn simulate_reports_runaway_on_illegal_opcode() {
    // 0xD3 is an undefined opcode: the CPU hard-locks (gbctr "undefined opcodes").
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0xD3])).unwrap();

    let dump = tmp("runaway");
    std::fs::write(&dump, [0x00u8]).unwrap();

    let args = SimArgs {
        memdump: dump.to_string_lossy().into_owned(),
        in_from: "C000".into(),
        in_to: "C000".into(),
        out_from: "C000".into(),
        out_to: "C000".into(),
        start: "0100".into(),
        end: None,
        budget: "1000".into(),
        savestate: None,
    };

    let mut mcp = Mcp::default();
    mcp.start_sim(&gb, &args).unwrap();
    let out = run_to_done(&mut mcp, 0);
    assert!(out.contains("stop: runaway"), "stop code: {out}");

    std::fs::remove_file(&dump).ok();
}

#[test]
fn simulate_rejects_memdump_size_mismatch() {
    let gb = GameBoy::new(Model::Dmg, rom_at_0100(&[0x00])).unwrap();
    let dump = tmp("mismatch");
    std::fs::write(&dump, [0x00u8, 0x00]).unwrap(); // 2 bytes...

    let args = SimArgs {
        memdump: dump.to_string_lossy().into_owned(),
        in_from: "C000".into(),
        in_to: "C000".into(), // ...but a 1-byte range
        out_from: "C000".into(),
        out_to: "C000".into(),
        start: "0100".into(),
        end: None,
        budget: "1000".into(),
        savestate: None,
    };

    let mut mcp = Mcp::default();
    let err = match mcp.start_sim(&gb, &args) {
        Err(e) => e,
        Ok(_) => panic!("expected a size-mismatch error"),
    };
    assert!(err.contains("bytes"), "size-mismatch error: {err}");

    std::fs::remove_file(&dump).ok();
}
