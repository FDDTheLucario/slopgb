//! End-to-end proof that a real Rust plugin — built with the `slopgb_plugin!`
//! macro and the `GameBoyView` wrappers, compiled to wasm32 — round-trips
//! through the host: its register/memory reads and its log all resolve against
//! the live `GameBoy`. Builds the fixture on the fly; skips if wasm32 is
//! unavailable (CI installs it — see the plugin-api plan).

use std::path::PathBuf;
use std::process::Command;

use slopgb_core::{GameBoy, Model};
use slopgb_plugin_host::PluginHost;

fn build_fixture() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/frame-probe/Cargo.toml"
    );
    // Its own target dir (never share a CARGO_TARGET_DIR with the outer build).
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("frame-probe-target");
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
    let wasm = target_dir.join("wasm32-unknown-unknown/release/frame_probe.wasm");
    std::fs::read(wasm).ok()
}

#[test]
fn fixture_round_trip() {
    let Some(bytes) = build_fixture() else {
        eprintln!("skipping fixture_round_trip: wasm32 build unavailable");
        return;
    };

    let gb = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    let pc = gb.cpu_regs().pc;
    let op = gb.debug_read(pc);

    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("frame_probe", &bytes).unwrap());
    host.pump(&gb);

    assert_eq!(
        host.take_log(),
        vec![format!("[frame_probe] pc={pc:04X} op={op:02X}")]
    );
}
