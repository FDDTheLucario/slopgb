//! End-to-end proof that the SF2-to-`.smpl` converter plugin
//! (`slopgb-sf2-plugin`) produces byte-identical output in wasm, driven
//! through the host, as calling `slopgb_sf2::import_sf2` natively on the same
//! SF2 bytes. Skips if wasm32 is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_plugin_host::LoadedCoprocessor;

fn build_plugin() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../slopgb-sf2-plugin/Cargo.toml"
    );
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sf2-plugin-target");
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            manifest,
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let wasm = target_dir.join("wasm32-unknown-unknown/release/slopgb_sf2_plugin.wasm");
    std::fs::read(wasm).ok()
}

/// A minimal synthetic APU RAM image (one dir entry, one instrument, one
/// looping BRR sample) — just enough for `export_sf2` to build a valid, tiny
/// SF2 file. Mirrors `slopgb::session_tests::synthetic_apu_ram`.
fn synthetic_apu_ram() -> [u8; 0x1_0000] {
    let mut ram = [0u8; 0x1_0000];
    let brr_addr: usize = 0x2000;
    let square = [0x93u8, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88];
    ram[brr_addr..brr_addr + 9].copy_from_slice(&square);
    // dir[0]: start = loop = brr_addr (a self-looping one-block sample).
    ram[slopgb_sf2::DIR_DEST as usize] = (brr_addr & 0xFF) as u8;
    ram[slopgb_sf2::DIR_DEST as usize + 1] = (brr_addr >> 8) as u8;
    ram[slopgb_sf2::DIR_DEST as usize + 2] = (brr_addr & 0xFF) as u8;
    ram[slopgb_sf2::DIR_DEST as usize + 3] = (brr_addr >> 8) as u8;
    let e = slopgb_sf2::INSTR_DEST as usize;
    ram[e] = 0; // SRCN 0
    ram[e + 1] = 0x9F; // ADSR1
    ram[e + 2] = (3 << 5) | 10; // ADSR2
    ram[e + 3] = 0x7F; // GAIN
    ram[e + 4] = 0x10; // base16 hi: $1000 = unity
    ram[e + 5] = 0x00;
    ram
}

#[test]
fn sf2_converts_in_wasm_identically_to_native() {
    let Some(wasm) = build_plugin() else {
        eprintln!("skipping sf2_converts_in_wasm_identically_to_native: wasm32 build unavailable");
        return;
    };

    let sf2 = slopgb_sf2::export_sf2(&synthetic_apu_ram(), slopgb_sf2::DIR_DEST, slopgb_sf2::INSTR_DEST, 64, 1)
        .unwrap();

    let mut cop = LoadedCoprocessor::load(&wasm).unwrap();
    cop.reset().unwrap();

    cop.set_file(0, sf2.clone()); // SF2_FILE_KEY
    let reached = cop.run_until(1).unwrap();
    assert!(reached >= 1);

    let payload = cop.save_state().unwrap();
    assert!(!payload.is_empty(), "converter emitted the .smpl payload");

    let via_wasm = slopgb_sf2::cache::deserialize(&payload).unwrap();
    let native = slopgb_sf2::import_sf2(&sf2).unwrap();

    assert_eq!(via_wasm.dir, native.dir, "dir region matches native");
    assert_eq!(via_wasm.instr, native.instr, "instr region matches native");
    assert_eq!(via_wasm.brr, native.brr, "brr region matches native");

    // Stronger claim: the whole payload is byte-identical to serializing the
    // native result, not just field-by-field equal.
    assert_eq!(
        payload,
        slopgb_sf2::cache::serialize(&native),
        "wasm payload byte-identical to native serialize"
    );
}
