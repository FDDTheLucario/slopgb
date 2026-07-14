//! MSU-1 frontend wiring: a fixture pack (author it here — MSU-1 is an open spec,
//! packs are user content) driven through the register mapping produces mixed
//! audio; the register reads route back from the plugin's ports.
//!
//! The plugin `.wasm` is built on the fly (like `slopgb-plugin-host`'s
//! `msu1_roundtrip`); the tests that need it skip when a wasm32 toolchain is
//! unavailable.

use std::path::{Path, PathBuf};
use std::process::Command;

use slopgb_core::{GameBoy, Model};

use super::*;

/// The `.pcm` magic (mirrors `slopgb_msu1_plugin::PCM_MAGIC`).
const PCM_MAGIC: [u8; 4] = *b"MSU1";

// --- track_number (pure, no wasm) -------------------------------------------

#[test]
fn track_number_reads_the_trailing_digits() {
    assert_eq!(track_number("track_1.pcm"), Some(1));
    assert_eq!(track_number("track_42.pcm"), Some(42));
    assert_eq!(track_number("game-7.pcm"), Some(7));
    assert_eq!(track_number("5.pcm"), Some(5));
    assert_eq!(track_number("intro.pcm"), None, "no trailing digits");
    assert_eq!(track_number("track_1.msu"), None, "not a .pcm");
    assert_eq!(track_number("notes.txt"), None);
}

// --- wasm-backed integration ------------------------------------------------

/// Build `slopgb-msu1-plugin` to wasm; `None` (skip) when wasm32 is unavailable.
fn build_plugin() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../slopgb-msu1-plugin/Cargo.toml"
    );
    let target_dir = std::env::temp_dir().join("slopgb-msu1-plugin-target");
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
    let wasm = target_dir.join("wasm32-unknown-unknown/release/slopgb_msu1_plugin.wasm");
    std::fs::read(wasm).ok()
}

/// A fixture `.pcm`: the magic, a loop point, then interleaved LE i16 samples.
fn pcm(loop_point: u32, samples: &[(i16, i16)]) -> Vec<u8> {
    let mut v = PCM_MAGIC.to_vec();
    v.extend_from_slice(&loop_point.to_le_bytes());
    for &(l, r) in samples {
        v.extend_from_slice(&l.to_le_bytes());
        v.extend_from_slice(&r.to_le_bytes());
    }
    v
}

/// A fresh temp pack dir holding the built plugin `.wasm` and a loud `track_1.pcm`.
fn pack_dir(wasm: &[u8], tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slopgb-msu1-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("msu1.wasm"), wasm).unwrap();
    // A short, full-scale square wave so mixed output is unmistakably non-silent.
    let track: Vec<(i16, i16)> = (0..2048)
        .map(|i| {
            if i % 2 == 0 {
                (30000, 30000)
            } else {
                (-30000, -30000)
            }
        })
        .collect();
    std::fs::write(dir.join("track_1.pcm"), pcm(0, &track)).unwrap();
    dir
}

/// A 32 KiB MBC1+RAM cart (type `$03`, 8 KiB SRAM) so `$A000-$BFFF` is real RAM
/// the MSU-1 registers live in.
fn ram_cart() -> GameBoy {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x03; // MBC1 + RAM + BATTERY
    rom[0x148] = 0x00; // 32 KiB ROM
    rom[0x149] = 0x02; // 8 KiB RAM
    GameBoy::new(Model::Dmg, rom).expect("valid MBC1+RAM cart")
}

fn peak(samples: &[(f32, f32)]) -> f32 {
    samples
        .iter()
        .map(|&(l, r)| l.abs().max(r.abs()))
        .fold(0.0, f32::max)
}

#[test]
fn register_reads_route_from_the_plugin_ports() {
    let Some(wasm) = build_plugin() else {
        eprintln!("skipping register_reads_route_from_the_plugin_ports: wasm32 unavailable");
        return;
    };
    let dir = pack_dir(&wasm, "id");
    let mut m = Msu1::load(&dir).expect("pack loads");
    // Registers $A002-$A007 read back the chip id "S-MSU1" (how a game detects
    // MSU-1) — proving reads route to the plugin's comm ports.
    let id: Vec<u8> = (2..=7).map(|r| m.read_reg(r)).collect();
    assert_eq!(&id, b"S-MSU1");
    // A register write routes to the port too: select + play, then the status
    // register ($A000) reports playing.
    m.write_reg(REG_TRACK_LO, 1);
    m.write_reg(REG_TRACK_HI, 0);
    m.write_reg(REG_CONTROL, 0x01);
    assert_ne!(m.read_reg(0) & 0x10, 0, "status shows audio playing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn polled_registers_play_a_track_into_mixed_audio() {
    let Some(wasm) = build_plugin() else {
        eprintln!("skipping polled_registers_play_a_track_into_mixed_audio: wasm32 unavailable");
        return;
    };
    let dir = pack_dir(&wasm, "play");
    let mut m = Msu1::load(&dir).expect("pack loads");
    let mut gb = ram_cart();

    // No track selected yet: pumping a frame yields silence (nothing plays).
    assert_eq!(
        peak(m.pump_frame(&gb)),
        0.0,
        "silent before any register write"
    );

    // The game writes the MSU-1 registers at $A004/$A005 (select track 1) and
    // $A007 (play) into cart RAM — enable it first (MBC1 $0000 = $0A).
    gb.debug_write(0x0000, 0x0A);
    gb.debug_write(REG_BASE + u16::from(REG_TRACK_LO), 1);
    gb.debug_write(REG_BASE + u16::from(REG_TRACK_HI), 0);
    gb.debug_write(REG_BASE + u16::from(REG_CONTROL), 0x01);

    // A pumped frame polls those registers, forwards them to the chip, and mixes
    // the streamed PCM — the output is now non-silent.
    let out = peak(m.pump_frame(&gb));
    assert!(
        out > 0.1,
        "the selected track streamed into mixed audio (peak {out})"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_plugin_is_a_non_fatal_load_error() {
    // A pack directory with no msu1.wasm returns an error the caller logs (and
    // then runs without MSU-1) — never a panic.
    let dir = std::env::temp_dir().join(format!("slopgb-msu1-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(Msu1::load(&dir).is_err());
    // A directory that does not exist at all is likewise an error, not a panic.
    assert!(Msu1::load(Path::new("/nonexistent/slopgb/msu1")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
