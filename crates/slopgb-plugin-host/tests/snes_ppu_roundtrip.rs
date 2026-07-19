//! End-to-end proof that the SNES PPU coprocessor plugin
//! (`slopgb-snes-ppu-plugin`), compiled to wasm32, renders across the host
//! boundary: B-bus port writes build a scene, a host-window write renders a
//! line, and the framebuffer bytes cross back out. Skips if wasm32 is
//! unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_plugin_host::LoadedCoprocessor;

fn build_plugin() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../slopgb-snes-ppu-plugin/Cargo.toml"
    );
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("snes-ppu-plugin-target");
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
    let wasm = target_dir.join("wasm32-unknown-unknown/release/slopgb_snes_ppu_plugin.wasm");
    std::fs::read(wasm).ok()
}

/// The plugin's host window (`slopgb-snes-ppu-plugin` consts — wasm-loaded,
/// never linked).
const HW_LINE: u32 = 0x0100_0000;
const HW_FB: u32 = 0x0100_1000;

#[test]
fn snes_ppu_round_trip() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping snes_ppu_round_trip: wasm32 build unavailable");
        return;
    };

    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // Backdrop-only scene: full brightness, CGRAM color 0 = $2A55.
    cop.port_write(0x00, 0x0F).unwrap();
    cop.port_write(0x21, 0x00).unwrap();
    cop.port_write(0x22, 0x55).unwrap();
    cop.port_write(0x22, 0x2A).unwrap();

    cop.write_ram(HW_LINE, &[5, 0]).unwrap();
    let px = cop.read_ram(HW_FB + 5 * 512, 4).unwrap();
    assert_eq!(px, vec![0x55, 0x2A, 0x55, 0x2A], "backdrop crossed out");

    let unrendered = cop.read_ram(HW_FB, 2).unwrap();
    assert_eq!(unrendered, vec![0, 0], "other rows untouched");

    // The passive chip absorbs the clock.
    assert!(cop.run_until(500).unwrap() >= 500);

    // Reset blanks the framebuffer.
    cop.reset().unwrap();
    assert_eq!(cop.read_ram(HW_FB + 5 * 512, 2).unwrap(), vec![0, 0]);
}
