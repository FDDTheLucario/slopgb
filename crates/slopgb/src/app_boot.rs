//! Startup resource resolution: the boot-ROM / SGB-BIOS bytes, the plugin
//! registry (manifest-declared CLI flags — see `docs/ui-state/plugin-api.md`),
//! and the opt-in tier-1 plugin host (`--plugins` dir), each resolved from CLI
//! flag / env var / persisted setting. Split out of `main.rs` to keep it under
//! the size cap.

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use slopgb_plugin_host::{PluginHost, PluginRegistry, RegistryError};

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
                     coprocessor (spc700 + w65c816) auto-loads from here; MSU-1/SF2 \
                     via their own manifest-declared flags. Not the per-frame \
                     --plugins pump.",
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

/// Pre-scan `args` for `--plugins <dir>` (bypassing the full [`Options::parse`],
/// which needs the declared-flag table this very directory produces — see
/// [`build_registry`]), falling back to `SLOPGB_PLUGINS_DIR` then the persisted
/// `settings.plugins.dir`, matching [`load_plugins`]'s precedence so a
/// directory set only in Options doesn't lose its declared flags.
pub(crate) fn prescan_plugins_dir(
    args: impl Iterator<Item = String>,
    settings: &windows::options::Settings,
) -> Option<PathBuf> {
    let mut cli_dir = None;
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        if a == "--plugins" {
            cli_dir = args.next().map(PathBuf::from);
        }
    }
    cli_dir
        .or_else(|| env::var_os("SLOPGB_PLUGINS_DIR").map(PathBuf::from))
        .or_else(|| {
            (!settings.plugins.dir.is_empty()).then(|| PathBuf::from(&settings.plugins.dir))
        })
}

/// Build the [`PluginRegistry`] for `dir` (from [`prescan_plugins_dir`]): an
/// empty registry with no dir; every manifest [`PluginRegistry::scan`] finds in
/// one. Two plugins declaring the same role is a fatal startup error (prints
/// naming both files and exits `2`) — unlike a bad/missing directory
/// ([`RegistryError::Io`]), which is logged and treated as an empty registry so
/// a typo can't wedge startup.
pub(crate) fn build_registry(dir: Option<&Path>) -> PluginRegistry {
    let Some(dir) = dir else {
        return PluginRegistry::new();
    };
    match PluginRegistry::scan(dir) {
        Ok(reg) => reg,
        Err(e @ RegistryError::DuplicateRole { .. }) => {
            eprintln!("slopgb: fatal: {e}");
            process::exit(2);
        }
        Err(e @ RegistryError::Io(_)) => {
            eprintln!("slopgb: cannot read plugins dir '{}': {e}", dir.display());
            PluginRegistry::new()
        }
    }
}

/// Apply each declared plugin flag's explicit value into `registry`: the CLI
/// value (`cli_flags`, from `Options::plugin_flags`) if given, else the
/// generic env fallback `SLOPGB_<NAME>` (the flag's declared name, uppercased,
/// `-` → `_` — e.g. `sf2` → `SLOPGB_SF2`, `msu1` → `SLOPGB_MSU1`, matching
/// today's fixed names). Neither present leaves the manifest's own default
/// (already resolved lazily by `PluginRegistry::flag` against its `Context`).
pub(crate) fn apply_plugin_flags(registry: &mut PluginRegistry, cli_flags: &[(String, String)]) {
    let declared: Vec<String> = registry
        .flags()
        .into_iter()
        .map(|(_, f)| f.name.clone())
        .collect();
    for name in declared {
        if let Some((_, v)) = cli_flags.iter().find(|(n, _)| n == &name) {
            registry.set_flag(&name, v);
        } else {
            let env_name = format!("SLOPGB_{}", name.to_ascii_uppercase().replace('-', "_"));
            if let Ok(v) = env::var(&env_name) {
                registry.set_flag(&name, &v);
            }
        }
    }
}

/// The registry's already-resolved effective value for every flag it declares
/// (`[(name, value)]`, only the flags that resolved to `Some` — an explicit
/// CLI/env value, else the manifest's own default expanded against the
/// registry's current [`Context`], else omitted) — what
/// `Session::set_plugin_flags` consumes.
pub(crate) fn effective_plugin_flags(registry: &PluginRegistry) -> Vec<(String, String)> {
    registry
        .flags()
        .into_iter()
        .map(|(_, f)| f.name.clone())
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|name| registry.flag(&name).map(|v| (name, v)))
        .collect()
}
