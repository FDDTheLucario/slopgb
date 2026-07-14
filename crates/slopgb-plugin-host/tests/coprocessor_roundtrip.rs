//! End-to-end proof of the coprocessor (tier-3) plugin path: a real Rust
//! coprocessor built with slopgb_coprocessor_plugin!, compiled to wasm32, is
//! reset, clocked, and exchanges comm-port values with the host. Skips if
//! wasm32 is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_plugin_host::LoadedCoprocessor;

fn build_fixture() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stub-coprocessor/Cargo.toml"
    );
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("stub-coprocessor-target");
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
    let wasm = target_dir.join("wasm32-unknown-unknown/release/stub_coprocessor.wasm");
    std::fs::read(wasm).ok()
}

#[test]
fn coprocessor_round_trip() {
    let Some(bytes) = build_fixture() else {
        eprintln!("skipping coprocessor_round_trip: wasm32 build unavailable");
        return;
    };

    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();
    cop.port_write(0, 0x42).unwrap();
    assert_eq!(
        cop.run_until(100).unwrap(),
        100,
        "run_until reports the cycle"
    );
    // port 0 latched 0x42; the read folds in cycle & 0xFF = 100 → 0x42 + 100.
    assert_eq!(cop.port_read(0).unwrap(), 0x42u8.wrapping_add(100));
    // A fresh reset clears both cycle and ports.
    cop.reset().unwrap();
    assert_eq!(cop.port_read(0).unwrap(), 0);
}
