//! End-to-end proof of the MSU-1 streaming-audio coprocessor plugin
//! (`slopgb-msu1-plugin`) running *in wasm*, driven through the host over the v4
//! bulk channels. Covers both usage modes:
//!
//! 1. **Register interface** — track select + play (`$2004/$2005/$2007`) streams
//!    a fixture `.pcm` (served via `host_file`) into drained PCM, and the data
//!    port (`$2000-$2003` seek, `$2001` read) walks a fixture `.msu` data ROM.
//! 2. **Polled mailbox** — a game-written play-request (`set_mailbox`, served via
//!    `host_recv`) starts playback with no register writes.
//!
//! Skips if a wasm32 toolchain is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_msu1_plugin::{DATA_FILE_KEY, PCM_MAGIC};
use slopgb_plugin_host::LoadedCoprocessor;

fn build_plugin() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../slopgb-msu1-plugin/Cargo.toml"
    );
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("msu1-plugin-target");
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

/// Build a fixture `.pcm`: the `"MSU1"` magic, a loop point, then the samples.
fn pcm(loop_point: u32, samples: &[(i16, i16)]) -> Vec<u8> {
    let mut v = PCM_MAGIC.to_vec();
    v.extend_from_slice(&loop_point.to_le_bytes());
    for &(l, r) in samples {
        v.extend_from_slice(&l.to_le_bytes());
        v.extend_from_slice(&r.to_le_bytes());
    }
    v
}

/// A sample as the plugin emits it at full volume (0xFF ≈ 255/256 unity gain).
fn scaled(s: i16) -> i16 {
    ((i32::from(s) * 0xFF) >> 8) as i16
}

// Status-register bits (MSU_STATUS `$2000` read).
const ST_TRACK_MISSING: u8 = 1 << 3;
const ST_AUDIO_PLAYING: u8 = 1 << 4;

#[test]
fn register_interface_selects_seeks_and_plays() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping register_interface_selects_seeks_and_plays: wasm32 unavailable");
        return;
    };
    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // The six ID ports spell out the chip (how a game detects MSU-1).
    let id: Vec<u8> = (2..=7).map(|p| cop.port_read(p).unwrap()).collect();
    assert_eq!(&id, b"S-MSU1");

    // Register a track `.pcm` (host-file key == track number) and select track 1
    // via $2004 (lo) then $2005 (hi, triggers the select).
    let samples = [
        (1000i16, -1000i16),
        (2000, -2000),
        (3000, -3000),
        (4000, -4000),
    ];
    cop.set_file(1, pcm(0, &samples));
    cop.port_write(4, 1).unwrap();
    cop.port_write(5, 0).unwrap();

    // Track present → the missing bit is clear; not playing yet.
    let status = cop.port_read(0).unwrap();
    assert_eq!(status & ST_TRACK_MISSING, 0, "the selected track exists");
    assert_eq!(status & ST_AUDIO_PLAYING, 0, "playback has not started");

    // Play (MSU_CONTROL bit 0), then advance four output samples and drain them.
    cop.port_write(7, 0x01).unwrap();
    assert_eq!(
        cop.port_read(0).unwrap() & ST_AUDIO_PLAYING,
        ST_AUDIO_PLAYING
    );
    assert_eq!(cop.run_until(4).unwrap(), 4);
    let out = cop.drain_pcm().unwrap();
    let expect: Vec<(i16, i16)> = samples
        .iter()
        .map(|&(l, r)| (scaled(l), scaled(r)))
        .collect();
    assert_eq!(out, expect, "the fixture track streamed out as scaled PCM");

    // Past the (non-looping) end, playback stops and drains nothing more.
    assert_eq!(cop.run_until(64).unwrap(), 64);
    assert!(
        cop.drain_pcm().unwrap().is_empty(),
        "stops at end of a track"
    );
    assert_eq!(cop.port_read(0).unwrap() & ST_AUDIO_PLAYING, 0);
}

#[test]
fn data_port_walks_the_msu_rom_by_seek() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping data_port_walks_the_msu_rom_by_seek: wasm32 unavailable");
        return;
    };
    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    let rom = [0xDEu8, 0xAD, 0xBE, 0xEF, 0x12, 0x34];
    cop.set_file(DATA_FILE_KEY, rom.to_vec());

    // Seek to offset 2: write the 32-bit LE address across $2000-$2003 ($2003
    // commits it).
    cop.port_write(0, 2).unwrap();
    cop.port_write(1, 0).unwrap();
    cop.port_write(2, 0).unwrap();
    cop.port_write(3, 0).unwrap();

    // $2001 reads auto-increment the pointer, so successive reads walk the ROM.
    assert_eq!(cop.port_read(1).unwrap(), 0xBE);
    assert_eq!(cop.port_read(1).unwrap(), 0xEF);
    assert_eq!(cop.port_read(1).unwrap(), 0x12);
    // Past the end reads back 0 (no data), not a trap.
    cop.port_write(0, 99).unwrap();
    cop.port_write(3, 0).unwrap();
    assert_eq!(cop.port_read(1).unwrap(), 0x00);
}

#[test]
fn missing_track_sets_the_status_bit_and_will_not_play() {
    let Some(bytes) = build_plugin() else {
        eprintln!(
            "skipping missing_track_sets_the_status_bit_and_will_not_play: wasm32 unavailable"
        );
        return;
    };
    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // Select a track with no registered file → the missing bit sets.
    cop.port_write(4, 7).unwrap();
    cop.port_write(5, 0).unwrap();
    assert_eq!(
        cop.port_read(0).unwrap() & ST_TRACK_MISSING,
        ST_TRACK_MISSING
    );
    // Play is refused and nothing streams.
    cop.port_write(7, 0x01).unwrap();
    assert_eq!(cop.run_until(16).unwrap(), 16);
    assert!(cop.drain_pcm().unwrap().is_empty());
}

#[test]
fn polled_mailbox_starts_playback_from_a_game_write() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping polled_mailbox_starts_playback_from_a_game_write: wasm32 unavailable");
        return;
    };
    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // Track 2 is available, but the game touches no MSU-1 register — it writes a
    // `[cmd=1(play), track_lo, track_hi, flags]` request into the shared mailbox.
    let samples = [(500i16, 400i16), (600, 700), (800, 900)];
    cop.set_file(2, pcm(0, &samples));
    cop.set_mailbox(&[1, 2, 0, 0]);

    // The resident handler polls the mailbox each `run_until` and starts playing.
    assert_eq!(cop.run_until(3).unwrap(), 3);
    let out = cop.drain_pcm().unwrap();
    let expect: Vec<(i16, i16)> = samples
        .iter()
        .map(|&(l, r)| (scaled(l), scaled(r)))
        .collect();
    assert_eq!(out, expect, "the mailbox play-request streamed track 2");

    // Re-polling the same mailbox does not re-trigger (edge-detected).
    assert_eq!(cop.run_until(16).unwrap(), 16);
    assert!(cop.drain_pcm().unwrap().is_empty());
}

#[test]
fn looping_track_repeats_from_the_loop_point() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping looping_track_repeats_from_the_loop_point: wasm32 unavailable");
        return;
    };
    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // Two samples, loop point = sample 1 (the second sample).
    let samples = [(100i16, 100i16), (200, 200)];
    cop.set_file(3, pcm(1, &samples));
    cop.port_write(4, 3).unwrap();
    cop.port_write(5, 0).unwrap();
    // Play + repeat (MSU_CONTROL bits 0 and 1).
    cop.port_write(7, 0x03).unwrap();

    // Six samples from a two-sample track that loops back to sample 1: the tail
    // is the second sample repeated, proving the loop point is honored.
    assert_eq!(cop.run_until(6).unwrap(), 6);
    let out = cop.drain_pcm().unwrap();
    assert_eq!(out.len(), 6);
    let s0 = (scaled(100), scaled(100));
    let s1 = (scaled(200), scaled(200));
    assert_eq!(
        out,
        vec![s0, s1, s1, s1, s1, s1],
        "repeats from the loop point"
    );
}
