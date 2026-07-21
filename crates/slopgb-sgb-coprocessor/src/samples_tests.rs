//! Tests for the two-axis N-SPC install ([`install_nspc`](super::SgbCoprocessor::install_nspc)):
//! engine source (ROM vs clean-room) x sample source (ROM vs SF2), plus the
//! `install_sgb_bios` regression guard. Builds the two wasm plugins like
//! `lib_tests.rs`; skips (rather than fails) if the wasm target is unavailable.

use super::*;

/// Build a minimal synthetic SGB system ROM: a valid APU upload table at
/// `TABLE_OFF` holding `blocks`, terminated at the (required) `$0400` engine
/// entry — [`parse_apu_blocks`](super::parse_apu_blocks) only accepts a table
/// that loads and jumps to `$0400`. Never touches the real `program.rom`.
fn synth_rom(blocks: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut table = Vec::new();
    for (dest, data) in blocks {
        table.extend_from_slice(&(data.len() as u16).to_le_bytes());
        table.extend_from_slice(&dest.to_le_bytes());
        table.extend_from_slice(data);
    }
    table.extend_from_slice(&0u16.to_le_bytes()); // terminator: len = 0
    table.extend_from_slice(&SPC_PROG_ORG.to_le_bytes()); // entry = $0400
    let mut rom = vec![0u8; TABLE_OFF + table.len()];
    rom[TABLE_OFF..].copy_from_slice(&table);
    rom
}

/// The ROM's sound-data blocks used across these tests: a distinctive `$0400`
/// engine stub (must include the real engine's `RTS`-adjacent opcode so a
/// stray fetch does not crash the plugin) plus distinctive bytes at the three
/// sample-region dests.
fn rom_blocks() -> Vec<(u16, Vec<u8>)> {
    vec![
        (SPC_PROG_ORG, vec![0x2Fu8, 0xFE]), // BRA * (ROM engine stand-in)
        (0x4B00, vec![0xAAu8; 8]),          // dir
        (0x4C30, vec![0xBBu8; 8]),          // instr
        (0x4DB0, vec![0xCCu8; 8]),          // brr
    ]
}

fn sf2_regions() -> SampleRegions {
    SampleRegions {
        dir: vec![0x11u8; 8],
        instr: vec![0x22u8; 8],
        brr: vec![0x33u8; 8],
    }
}

/// `install_sgb_bios` (the `--sgb-bios`-alone path) still uploads the ROM's
/// own sample regions unchanged — a regression guard for the refactor into
/// `install_nspc`.
#[test]
fn install_sgb_bios_uploads_rom_sample_regions_unchanged() {
    let Some(mut cop) = crate::tests::build_cop(48_000) else {
        return;
    };
    let rom = synth_rom(&rom_blocks());
    assert!(cop.install_sgb_bios(&rom), "valid synthetic ROM installs");
    let mut spc = cop.spc.borrow_mut();
    assert_eq!(spc.read_ram(0x4B00, 8).unwrap(), vec![0xAA; 8], "dir");
    assert_eq!(spc.read_ram(0x4C30, 8).unwrap(), vec![0xBB; 8], "instr");
    assert_eq!(spc.read_ram(0x4DB0, 8).unwrap(), vec![0xCC; 8], "brr");
}

/// A garbage ROM (no valid APU upload table) is rejected.
#[test]
fn install_sgb_bios_rejects_garbage_rom() {
    let Some(mut cop) = crate::tests::build_cop(48_000) else {
        return;
    };
    let garbage = vec![0u8; TABLE_OFF + 16];
    assert!(!cop.install_sgb_bios(&garbage));
}

/// `install_nspc(Some(rom), Engine::Rom, Some(&sf2))`: the SF2 regions land
/// at the sample dests (not the ROM's), while the ROM's own engine block
/// still installs at `$0400` (`Engine::Rom`).
#[test]
fn install_nspc_rom_engine_with_sf2_samples() {
    let Some(mut cop) = crate::tests::build_cop(48_000) else {
        return;
    };
    let rom = synth_rom(&rom_blocks());
    let sf2 = sf2_regions();
    assert!(cop.install_nspc(Some(&rom), Engine::Rom, Some(&sf2)));
    let mut spc = cop.spc.borrow_mut();
    assert_eq!(spc.read_ram(0x4B00, 8).unwrap(), vec![0x11; 8], "sf2 dir");
    assert_eq!(spc.read_ram(0x4C30, 8).unwrap(), vec![0x22; 8], "sf2 instr");
    assert_eq!(spc.read_ram(0x4DB0, 8).unwrap(), vec![0x33; 8], "sf2 brr");
    // The ROM's engine stub, not NSPC_ENGINE, sits at $0400.
    let engine_at_0400 = spc.read_ram(u32::from(SPC_PROG_ORG), 2).unwrap();
    assert_eq!(engine_at_0400, vec![0x2F, 0xFE], "ROM engine block, not NSPC_ENGINE");
    assert_ne!(
        NSPC_ENGINE[..2.min(NSPC_ENGINE.len())],
        engine_at_0400[..],
        "sanity: NSPC_ENGINE's own opening bytes differ from the ROM stub"
    );
}

/// `install_nspc(None, Engine::CleanRoom, Some(&sf2))`: no ROM at all — the
/// clean-room engine and the SF2 sample bank supply everything.
#[test]
fn install_nspc_cleanroom_engine_with_sf2_samples_no_rom() {
    let Some(mut cop) = crate::tests::build_cop(48_000) else {
        return;
    };
    let sf2 = sf2_regions();
    assert!(cop.install_nspc(None, Engine::CleanRoom, Some(&sf2)));
    let mut spc = cop.spc.borrow_mut();
    assert_eq!(spc.read_ram(0x4B00, 8).unwrap(), vec![0x11; 8], "sf2 dir");
    assert_eq!(spc.read_ram(0x4C30, 8).unwrap(), vec![0x22; 8], "sf2 instr");
    assert_eq!(spc.read_ram(0x4DB0, 8).unwrap(), vec![0x33; 8], "sf2 brr");
    let installed = spc
        .read_ram(u32::from(SPC_PROG_ORG), NSPC_ENGINE.len())
        .unwrap();
    assert_eq!(installed, NSPC_ENGINE, "NSPC_ENGINE installed with no ROM");
}

/// `Engine::Rom` with no ROM and no SF2 samples fails: the ROM is required
/// both for the engine and (absent an SF2 override) the samples.
#[test]
fn install_nspc_rom_engine_requires_rom() {
    let Some(mut cop) = crate::tests::build_cop(48_000) else {
        return;
    };
    assert!(!cop.install_nspc(None, Engine::Rom, None));
}
