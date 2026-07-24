//! Startup resource resolution: the boot-ROM / SGB-BIOS bytes and the opt-in
//! plugin hosts (`--plugins` dir, MSU-1 pack), each resolved from CLI flag /
//! env var / persisted setting. Split out of `main.rs` to keep it under the
//! size cap.

use std::env;
use std::path::PathBuf;

use slopgb_plugin_host::PluginHost;

use crate::cli::Options;
use crate::windows;

/// Resolve the boot ROM bytes from `--boot` or the `SLOPGB_BOOT` env var,
/// reading the file. A read error is logged and treated as no boot ROM
/// (non-fatal) — the machine then boots post-boot as usual.
pub(crate) fn resolve_boot_rom(opts: &Options) -> Option<Vec<u8>> {
    let path = opts
        .boot
        .clone()
        .or_else(|| env::var_os("SLOPGB_BOOT").map(PathBuf::from))?;
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            eprintln!("slopgb: cannot read boot ROM '{}': {e}", path.display());
            None
        }
    }
}

/// Load wasm plugins from `--plugins`, `SLOPGB_PLUGINS_DIR`, or the persisted
/// `settings.plugins.dir` (in that precedence). Absent → an empty host (no
/// plugins, golden path untouched); a directory that can't be read is logged and
/// treated as empty (non-fatal).
pub(crate) fn load_plugins(opts: &Options, settings: &windows::options::Settings) -> PluginHost {
    let persisted =
        (!settings.plugins.dir.is_empty()).then(|| PathBuf::from(&settings.plugins.dir));
    let Some(dir) = opts
        .plugins_dir
        .clone()
        .or_else(|| env::var_os("SLOPGB_PLUGINS_DIR").map(PathBuf::from))
        .or(persisted)
    else {
        return PluginHost::new();
    };
    match PluginHost::load_dir(&dir) {
        Ok(host) => {
            let total = host.infos().len();
            if total == 0 {
                eprintln!("slopgb: no plugins found in '{}'", dir.display());
            } else if host.is_empty() {
                // Discovered plugins, but none the per-frame pump drives — all
                // higher-tier (subsystem/tool), driven via their own seams.
                eprintln!(
                    "slopgb: {total} subsystem/tool plugin(s) in '{}' — the SGB \
                     coprocessor (spc700 + w65c816) auto-loads from here; MSU-1 via \
                     --msu1. Not the per-frame --plugins pump.",
                    dir.display()
                );
            }
            host
        }
        Err(e) => {
            eprintln!("slopgb: cannot read plugins dir '{}': {e}", dir.display());
            PluginHost::new()
        }
    }
}

/// Resolve the optional MSU-1 pack directory from `--msu1` or `SLOPGB_MSU1` (in
/// that precedence). Only the path is resolved here — the SGB coprocessor drives
/// MSU-1 as part of the SNES-side bridge (`$2000-$2007`), so the pack is fed to
/// the session, not loaded as a standalone coprocessor.
pub(crate) fn msu1_override(opts: &Options) -> Option<PathBuf> {
    opts.msu1
        .clone()
        .or_else(|| env::var_os("SLOPGB_MSU1").map(PathBuf::from))
}

/// Resolve the optional SGB BIOS bytes from `--sgb-bios` or `SLOPGB_SGB_BIOS`,
/// reading the file. A read error is logged and treated as no BIOS (non-fatal).
/// The border/title-palette are *not* extracted from it — slopgb is high-level
/// and never runs the SNES CPU — so only the SGB audio path is fed; the honest
/// status is logged and the default border stands (`docs/hardware-state/sgb.md`).
pub(crate) fn resolve_sgb_bios(opts: &Options) -> Option<Vec<u8>> {
    let path = opts
        .sgb_bios
        .clone()
        .or_else(|| env::var_os("SLOPGB_SGB_BIOS").map(PathBuf::from))?;
    match std::fs::read(&path) {
        Ok(bytes) => {
            eprintln!(
                "slopgb: loaded SGB BIOS '{}' ({} bytes) — audio-driver image only; \
                 the Nintendo border/palette are not extracted (HLE), default border kept",
                path.display(),
                bytes.len()
            );
            Some(bytes)
        }
        Err(e) => {
            eprintln!("slopgb: cannot read SGB BIOS '{}': {e}", path.display());
            None
        }
    }
}

/// Resolve the optional `--sf2` soundfont path from `--sf2` or `SLOPGB_SF2`
/// (in that precedence). Only the path is resolved here — the bytes are read
/// in [`crate::session`], which needs the path itself to place the `.smpl`
/// cache file alongside it.
pub(crate) fn resolve_sf2(opts: &Options) -> Option<PathBuf> {
    let path = opts
        .sf2
        .clone()
        .or_else(|| env::var_os("SLOPGB_SF2").map(PathBuf::from))?;
    eprintln!(
        "slopgb: using SF2 soundfont '{}' for the SGB sample bank",
        path.display()
    );
    Some(path)
}
