//! The `--sf2` soundfont import path: resolve an SF2 file to its N-SPC
//! sample regions, via the `.smpl` cache next to it or, on a miss, the
//! `sf2.wasm` tier-3 converter plugin. Split out of `session.rs` to keep that
//! file under the module size cap.

use super::*;

/// Host-file key handed to the SF2 converter plugin's `set_file` (mirrors
/// `slopgb_sf2_plugin::SF2_FILE_KEY`; the plugin reads only this one file, so a
/// single fixed key suffices). Hardcoded to keep the plugin crate out of the
/// frontend's dep list — the same pattern `msu1.rs` uses for `DATA_FILE_KEY`.
const SF2_FILE_KEY: u32 = 0;
/// Filename of the SF2 converter coprocessor plugin inside the plugins dir.
const SF2_PLUGIN_WASM: &str = "sf2.wasm";

/// Resolve an `--sf2` path to its [`SampleRegions`]: check the `.smpl` cache
/// sitting next to the file (named `<hash-of-the-sf2-contents>.smpl` in the
/// SF2's parent directory — content-addressed, so an SF2 edited in place at
/// the same path is never served a stale cache) first (no plugin needed on a
/// cache hit), else drive the `sf2.wasm` tier-3 coprocessor plugin to convert
/// it and write the cache for next time. `None` on any unrecoverable error
/// (read/import/plugin failure), logged — the caller then falls back to the
/// ROM's own samples.
pub(super) fn load_or_import_sf2(
    sf2_path: &Path,
    plugins_dir: Option<&Path>,
) -> Option<SampleRegions> {
    let bytes = match fs::read(sf2_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("slopgb: cannot read SF2 '{}': {e}", sf2_path.display());
            return None;
        }
    };
    use std::hash::Hasher;
    let mut h = std::hash::DefaultHasher::new();
    h.write(&bytes);
    let key = h.finish();
    let cache_path = sf2_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{key:016x}.smpl"));
    if cache_path.exists() {
        match slopgb_sf2::read_cache(&cache_path) {
            Ok(r) => {
                return Some(SampleRegions {
                    dir: r.dir,
                    instr: r.instr,
                    brr: r.brr,
                });
            }
            Err(e) => eprintln!(
                "slopgb: SF2 cache '{}' unreadable ({e}); re-importing",
                cache_path.display()
            ),
        }
    }

    let Some(dir) = plugins_dir else {
        eprintln!(
            "slopgb: --sf2 given but sf2.wasm not found in the plugins dir; no SF2 samples loaded"
        );
        return None;
    };
    let wasm_path = dir.join(SF2_PLUGIN_WASM);
    let wasm_bytes = match fs::read(&wasm_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "slopgb: --sf2 given but sf2.wasm not found in the plugins dir; no SF2 samples loaded ({e})"
            );
            return None;
        }
    };
    let mut cop = match LoadedCoprocessor::load(&wasm_bytes) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "slopgb: --sf2 given but sf2.wasm not found in the plugins dir; no SF2 samples loaded ({e})"
            );
            return None;
        }
    };
    if let Err(e) = cop.reset() {
        eprintln!("slopgb: sf2.wasm reset failed: {e}");
        return None;
    }
    cop.set_file(SF2_FILE_KEY, bytes);
    if let Err(e) = cop.run_until(1) {
        eprintln!("slopgb: sf2.wasm conversion failed: {e}");
        return None;
    }
    let payload = match cop.save_state() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("slopgb: sf2.wasm save_state failed: {e}");
            return None;
        }
    };
    if payload.is_empty() {
        eprintln!(
            "slopgb: SF2 '{}' import (via sf2.wasm) produced no samples",
            sf2_path.display()
        );
        return None;
    }
    if let Err(e) = fs::write(&cache_path, &payload) {
        eprintln!(
            "slopgb: cannot write SF2 cache '{}': {e}",
            cache_path.display()
        );
    }
    match slopgb_sf2::cache::deserialize(&payload) {
        Ok(r) => Some(SampleRegions {
            dir: r.dir,
            instr: r.instr,
            brr: r.brr,
        }),
        Err(e) => {
            eprintln!(
                "slopgb: SF2 '{}' plugin output unreadable: {e}",
                sf2_path.display()
            );
            None
        }
    }
}
