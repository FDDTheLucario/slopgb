//! The MSU-1 seam: with `msu1.wasm` attached and a `.pcm` pack, a game reaches
//! the chip the real-hardware way — its resident SNES-side handler drives the
//! MSU-1 registers at SNES `$2000-$2007`, which the host routes to the plugin
//! (`apply_mmio`) and mixes the streamed PCM into the SGB output. Absent =
//! no MSU-1, the audio path unchanged.

use super::*;
use std::sync::OnceLock;

/// The MSU-1 plugin wasm (built once). `None` when unavailable → skip.
fn msu_plugin() -> Option<Vec<u8>> {
    static CACHE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    CACHE
        .get_or_init(|| build("slopgb-msu1-plugin", "slopgb_msu1_plugin"))
        .clone()
}

/// Write a minimal valid `.pcm`: the `MSU1` magic, a zero loop point, then a run
/// of nonzero stereo samples so a playing track produces audible output.
fn write_test_pcm(path: &Path, samples: usize) {
    let mut d = Vec::with_capacity(8 + samples * 4);
    d.extend_from_slice(b"MSU1");
    d.extend_from_slice(&0u32.to_le_bytes()); // loop point (samples)
    for _ in 0..samples {
        d.extend_from_slice(&8000i16.to_le_bytes()); // L
        d.extend_from_slice(&(-8000i16).to_le_bytes()); // R
    }
    fs::write(path, d).unwrap();
}

/// The end-to-end host path: attach the plugin, point it at a one-track pack, and
/// drive the register writes a game's resident handler makes (`MSU_TRACK` +
/// `MSU_CONTROL` play) through the exact routing point (`apply_mmio`). The track
/// streams into the SGB mix — a nonzero peak proves the plugin ran and its PCM
/// reached the output buffer. (The guest-CPU `$2000` capture/shadow half is
/// covered by the w65c816 plugin's `mmio` tests.)
#[test]
fn msu1_track_plays_through_the_2000_bus_routing() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let Some(msu) = msu_plugin() else {
        return;
    };
    cop.attach_msu(&msu).expect("msu1 plugin loads");
    assert!(!cop.msu_present, "no pack yet → chip not advertised");

    let dir = std::env::temp_dir().join("slopgb-msu1-lle-test-pack");
    let _ = fs::create_dir_all(&dir);
    write_test_pcm(&dir.join("track_1.pcm"), 44_100);
    cop.set_msu_pack(&dir);
    assert!(
        cop.msu_present,
        "a pack with a .pcm advertises the S-MSU1 chip"
    );

    // The resident handler selects track 1 (writing $2005 commits the 16-bit
    // index) and sets MSU_CONTROL play (bit 0).
    cop.apply_mmio(0x2004, 0x01);
    cop.apply_mmio(0x2005, 0x00);
    cop.apply_mmio(0x2007, 0x01);
    for _ in 0..4 {
        cop.clock(70_224);
    }
    assert!(
        peak(&cop.out) > 0.0,
        "the MSU-1 track streamed into the SGB output mix",
    );
}

/// Without a pack the chip is never advertised (its `$2000` read shadow stays
/// zero → a game's presence check finds no `S-MSU1`), and no MSU-1 audio mixes.
#[test]
fn no_pack_leaves_the_chip_silent_and_unadvertised() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let Some(msu) = msu_plugin() else {
        return;
    };
    cop.attach_msu(&msu).expect("msu1 plugin loads");
    // No set_msu_pack: even if a game wrote track/play, nothing streams.
    cop.apply_mmio(0x2004, 0x01);
    cop.apply_mmio(0x2005, 0x00);
    cop.apply_mmio(0x2007, 0x01);
    for _ in 0..4 {
        cop.clock(70_224);
    }
    assert!(!cop.msu_present, "no pack → chip stays unadvertised");
    assert_eq!(peak(&cop.out), 0.0, "no track → silence");
}
