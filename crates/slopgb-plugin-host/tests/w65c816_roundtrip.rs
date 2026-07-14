//! End-to-end proof that the real 65C816 coprocessor plugin
//! (`slopgb-w65c816-plugin`), compiled to wasm32, executes a program across the
//! host boundary: the host writes a comm port, clocks the CPU with `run_until`,
//! and reads the transformed result back. Skips if wasm32 is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_plugin_host::LoadedCoprocessor;

fn build_plugin() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../slopgb-w65c816-plugin/Cargo.toml"
    );
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("w65c816-plugin-target");
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
    let wasm = target_dir.join("wasm32-unknown-unknown/release/slopgb_w65c816_plugin.wasm");
    std::fs::read(wasm).ok()
}

#[test]
fn w65c816_round_trip() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping w65c816_round_trip: wasm32 build unavailable");
        return;
    };

    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // The hosted program echoes comm-port 1 (host input) + 7 to comm-port 0.
    cop.port_write(1, 0x10).unwrap();
    let reached = cop.run_until(200).unwrap();
    assert!(
        reached >= 200,
        "run_until clocks the CPU to the target cycle"
    );
    assert_eq!(
        cop.port_read(0).unwrap(),
        0x17,
        "0x10 + 7 crossed the wasm boundary in both directions"
    );

    // A fresh input tracks through on the next clock window.
    cop.port_write(1, 0x20).unwrap();
    cop.run_until(reached + 200).unwrap();
    assert_eq!(cop.port_read(0).unwrap(), 0x27);

    // Reset clears the output latch.
    cop.reset().unwrap();
    assert_eq!(cop.port_read(0).unwrap(), 0);
}
