//! End-to-end proof that the SPC700 + S-DSP coprocessor plugin
//! (`slopgb-spc700-plugin`) — the same audio subsystem `slopgb-core` runs
//! natively — executes correctly *in wasm*, driven through the host: clocking it
//! runs the real SPC700 IPL ROM (which emits the `$AA`/`$BB` SNES boot
//! handshake) and the S-DSP synthesizes samples. Skips if wasm32 is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_plugin_host::LoadedCoprocessor;

fn build_plugin() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../slopgb-spc700-plugin/Cargo.toml"
    );
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("spc700-plugin-target");
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
    let wasm = target_dir.join("wasm32-unknown-unknown/release/slopgb_spc700_plugin.wasm");
    std::fs::read(wasm).ok()
}

#[test]
fn spc700_ipl_and_dsp_run_in_wasm() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping spc700_ipl_and_dsp_run_in_wasm: wasm32 build unavailable");
        return;
    };

    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    // Clock the SPC700 far enough for its IPL ROM to run the boot loader.
    let reached = cop.run_until(60_000).unwrap();
    assert!(reached >= 60_000);

    // The IPL emits the documented $AA/$BB handshake on comm ports 0/1.
    assert_eq!(cop.port_read(0).unwrap(), 0xAA, "IPL boot handshake byte 0");
    assert_eq!(cop.port_read(1).unwrap(), 0xBB, "IPL boot handshake byte 1");

    // The S-DSP synthesized ~ reached/32 samples while clocked, in wasm.
    let samples =
        u64::from(cop.port_read(4).unwrap()) | (u64::from(cop.port_read(5).unwrap()) << 8);
    assert!(samples > 0, "the S-DSP produced samples in wasm");
    assert!(
        (reached / 32).abs_diff(samples) <= 2,
        "sample count ~= cycles/32 (reached={reached}, samples={samples})"
    );

    // Reset returns to the power-on IPL state.
    cop.reset().unwrap();
    assert_eq!(
        cop.port_read(4).unwrap(),
        0,
        "sample count cleared on reset"
    );
}

/// The tier-3 PCM-drain path: the stereo stream the S-DSP synthesized in wasm
/// crosses back to the host with the right sample count, oldest-first, and the
/// drain consumes the buffer (a second drain with no clocking is empty). This is
/// the plumbing an SGB audio backend needs to mix a plugin like the built-in
/// `mix_into`; whether the samples are silent depends on the driver loaded (the
/// bare IPL keys on no voice), which this does not assert.
#[test]
fn spc700_pcm_drains_to_the_host() {
    let Some(bytes) = build_plugin() else {
        eprintln!("skipping spc700_pcm_drains_to_the_host: wasm32 build unavailable");
        return;
    };

    let mut cop = LoadedCoprocessor::load(&bytes).unwrap();
    cop.reset().unwrap();

    let reached = cop.run_until(60_000).unwrap();
    let pcm = cop.drain_pcm().unwrap();
    // One 32 kHz stereo sample per 32 SPC cycles.
    assert!(
        (reached / 32).abs_diff(pcm.len() as u64) <= 2,
        "drained pair count ~= cycles/32 (reached={reached}, pairs={})",
        pcm.len(),
    );

    // The drain consumed the buffer: nothing new without more clocking.
    assert!(
        cop.drain_pcm().unwrap().is_empty(),
        "a second drain with no clocking is empty",
    );

    // Clocking again yields a fresh batch, and draining twice never
    // double-counts against the running total on ports 4-5.
    let before = sample_count(&mut cop);
    let reached2 = cop.run_until(reached + 32_000).unwrap();
    let pcm2 = cop.drain_pcm().unwrap();
    let after = sample_count(&mut cop);
    assert!(!pcm2.is_empty(), "more clocking drains more PCM");
    assert_eq!(
        after - before,
        pcm2.len() as u64,
        "the drained batch matches the running sample-count delta (reached2={reached2})",
    );
}

fn sample_count(cop: &mut LoadedCoprocessor) -> u64 {
    u64::from(cop.port_read(4).unwrap()) | (u64::from(cop.port_read(5).unwrap()) << 8)
}
