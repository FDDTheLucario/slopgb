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

/// Frame geometry (`slopgb-snes-ppu-plugin` consts — wasm-loaded, never linked).
const FB_BYTES: usize = 256 * 224 * 2;

/// The whole-frame pull the SGB coprocessor makes every vblank is served by the
/// plugin's zero-copy path (it hands the host its framebuffer region instead of
/// building the bytes). Pin it against the general path: a request the fast path
/// declines (odd length, odd start) is answered by the byte-by-byte reader, so
/// the two must agree everywhere on a frame with distinct content per row.
#[test]
fn whole_frame_pull_matches_the_byte_by_byte_path() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping whole_frame_pull_matches_the_byte_by_byte_path: no wasm32");
        return;
    };
    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // Full brightness, then one row per backdrop colour: every row differs, and
    // the two bytes of a pixel differ from each other, so a wrong offset,
    // length, or byte order cannot compare equal by accident.
    cop.port_write(0x00, 0x0F).unwrap();
    for y in 0..224u16 {
        cop.port_write(0x21, 0x00).unwrap();
        cop.port_write(0x22, y as u8).unwrap();
        cop.port_write(0x22, (0x2A ^ y >> 3) as u8).unwrap();
        cop.write_ram(0x0100_0000, &y.to_le_bytes()).unwrap();
    }

    let frame = cop.read_ram(HW_FB, FB_BYTES).unwrap();
    assert_eq!(frame.len(), FB_BYTES, "the whole frame crossed");
    assert_eq!(&frame[..4], &[0x00, 0x2A, 0x00, 0x2A], "row 0 backdrop");
    assert_eq!(
        &frame[223 * 512..223 * 512 + 2],
        &[223, 0x2A ^ (223 >> 3)],
        "row 223 backdrop"
    );

    let odd_len = cop.read_ram(HW_FB, FB_BYTES - 1).unwrap();
    assert_eq!(odd_len, frame[..FB_BYTES - 1], "same bytes, general path");
    let odd_start = cop.read_ram(HW_FB + 1, 3).unwrap();
    assert_eq!(odd_start, frame[1..4], "unaligned start still lines up");
}
