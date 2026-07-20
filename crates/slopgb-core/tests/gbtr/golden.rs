//! Golden safety net for CPU-visible PPU timing.
//!
//! A per-ROM frame + audio FINGERPRINT of the whole collection under one fixed
//! 16-LCD-frame protocol (gambatte's run length). The existing baselines catch
//! pass/fail flips, but a timing change can also drift *rendered pixels* without
//! flipping any suite's verdict. This fingerprint is the net for that silent
//! drift: every (ROM, model) gets a frame hash + audio verdict — an unexpected
//! change blocks immediately, an expected one is reviewed against the targeted
//! cluster.
//!
//! std-only (the core forbids deps): a 64-bit FNV-1a over the XRGB frame bytes.
//!
//! Modes (env `SLOPGB_GOLDEN`):
//! * `capture` — (re)write `tests/gbtr/golden/fingerprint.txt`.
//! * unset, file present — diff against it; FAIL listing every drift.
//! * unset, file absent — skip (keeps normal `cargo test gbtr` fast).

use std::path::{Path, PathBuf};

use slopgb_core::{CYCLES_PER_FRAME, GameBoy, Model};

use crate::common;
use crate::harness;

/// The committed fingerprint snapshot.
fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/gbtr/golden/fingerprint.txt")
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// `(frame_hash, audio_hash)`: a 64-bit FNV-1a over frame 16's XRGB bytes and a
/// second FNV-1a over frame 16's whole raw stereo sample stream (length +
/// every sample's bits). The stream hash catches any waveform drift. The
/// sentinel `(0, 0)` marks a ROM the model rejected or that panicked — a stable
/// marker so drift in *that* is caught too.
fn fingerprint(rom: &[u8], model: Model) -> (u64, u64) {
    let rom = rom.to_vec();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut gb = GameBoy::new(model, rom).ok()?;
        let dots = 15 * u64::from(CYCLES_PER_FRAME);
        while gb.cycles() < dots {
            gb.step();
        }
        // Final-frame raw audio verdict (the gambatte protocol): discard
        // frames 1..=15, then evaluate frame 16.
        let mut s = Vec::new();
        gb.drain_audio_raw(&mut s);
        s.clear();
        while gb.cycles() < dots + u64::from(CYCLES_PER_FRAME) {
            gb.step();
        }
        gb.drain_audio_raw(&mut s);
        let mut h = FNV_OFFSET;
        for &px in gb.frame().iter() {
            h = fnv1a(h, &px.to_le_bytes());
        }
        // Full-waveform audio hash: length first (so "no samples" differs from
        // a silent-but-present frame), then every stereo sample's raw bits.
        let mut ah = fnv1a(FNV_OFFSET, &(s.len() as u64).to_le_bytes());
        for &(l, r) in &s {
            ah = fnv1a(ah, &l.to_bits().to_le_bytes());
            ah = fnv1a(ah, &r.to_bits().to_le_bytes());
        }
        Some((h, ah))
    }));
    match run {
        Ok(Some(fp)) => fp,
        _ => (0, 0),
    }
}

/// Every `(case_key, frame_hash, audio)` line for the whole collection, both
/// models per ROM, computed in parallel and returned sorted.
fn capture(root: &Path) -> Vec<String> {
    let mut roms = Vec::new();
    common::collect_roms(root, true, &mut roms).expect("walk collection");
    roms.sort();
    let n = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .min(16);
    let chunks: Vec<&[PathBuf]> = roms.chunks(roms.len().div_ceil(n).max(1)).collect();
    // Silence the per-ROM panic spew from catch_unwind during the run.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let lines: Vec<String> = std::thread::scope(|sc| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                sc.spawn(move || {
                    let mut out = Vec::new();
                    for path in *chunk {
                        let rel = harness::rel_unix(root, path);
                        let Ok(bytes) = std::fs::read(path) else {
                            continue;
                        };
                        for model in [Model::Dmg, Model::Cgb] {
                            let (h, a) = fingerprint(&bytes, model);
                            out.push(format!(
                                "{} | {h:016x} | {a:016x}",
                                harness::case_key(&rel, model)
                            ));
                        }
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    std::panic::set_hook(prev);
    let mut lines = lines;
    lines.sort();
    lines
}

/// Capture or diff the collection fingerprint. See the module doc.
#[test]
fn golden_fingerprint() {
    let mode = std::env::var("SLOPGB_GOLDEN").ok();
    let path = golden_path();
    if mode.as_deref() != Some("capture") && !path.exists() {
        // Missing snapshot: skip normally, but fail loudly under
        // SLOPGB_REQUIRE_ROMS=1 (as in CI) so this gate cannot silently no-op —
        // the check runs before the gbtr_root gate, which otherwise couldn't
        // force it. The snapshot (fingerprint.txt) is committed, so present
        // checkouts never hit this branch.
        common::skip_or_fail_gbtr(
            "golden_fingerprint",
            "no golden snapshot (set SLOPGB_GOLDEN=capture to create \
             tests/gbtr/golden/fingerprint.txt)",
        );
        return;
    }
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "golden_fingerprint",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let lines = capture(&root);
    if mode.as_deref() == Some("capture") {
        std::fs::create_dir_all(path.parent().expect("golden dir")).expect("mkdir golden");
        std::fs::write(&path, lines.join("\n") + "\n").expect("write golden");
        println!(
            "golden_fingerprint: captured {} cases -> {}",
            lines.len(),
            path.display()
        );
        return;
    }
    let want = std::fs::read_to_string(&path).expect("read golden");
    let want: Vec<&str> = want.lines().filter(|l| !l.trim().is_empty()).collect();
    let have: Vec<&str> = lines.iter().map(String::as_str).collect();
    if want == have {
        println!(
            "golden_fingerprint: {} cases match HEAD snapshot",
            have.len()
        );
        return;
    }
    // Report drift: line-by-line (case sets should match; only the hash/audio
    // moves). Cap the listing so a broad change stays readable.
    use std::collections::BTreeMap;
    let key = |l: &str| l.split('|').next().unwrap_or(l).trim().to_string();
    let wmap: BTreeMap<String, &str> = want.iter().map(|l| (key(l), *l)).collect();
    let hmap: BTreeMap<String, &str> = have.iter().map(|l| (key(l), *l)).collect();
    let mut drift = Vec::new();
    for (k, w) in &wmap {
        match hmap.get(k) {
            Some(h) if h != w => drift.push(format!("CHANGED {w}  ->  {h}")),
            None => drift.push(format!("MISSING {w}")),
            _ => {}
        }
    }
    for (k, h) in &hmap {
        if !wmap.contains_key(k) {
            drift.push(format!("NEW     {h}"));
        }
    }
    let shown: Vec<&String> = drift.iter().take(80).collect();
    panic!(
        "golden_fingerprint: {} case(s) drifted from the HEAD snapshot:\n  {}\n{}",
        drift.len(),
        shown
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
        if drift.len() > shown.len() {
            format!("  ... and {} more", drift.len() - shown.len())
        } else {
            String::new()
        },
    );
}
