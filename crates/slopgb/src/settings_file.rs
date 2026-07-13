//! On-disk settings persistence. The native `slopgb.conf` is the default store;
//! bgb's `bgb.ini` format is read/written for interop (import/export + a one-time
//! migration), preserving every key we don't model. See
//! `docs/settings-persistence-plan.md` for the key map.

mod bgb;
mod ini;
mod native;

use std::path::{Path, PathBuf};

use crate::windows::options::Settings;

/// bgb keeps up to 10 recent ROMs as `Recent0..9`.
const RECENT_MAX: usize = 10;

/// Everything loaded from the settings file: the `Settings` plus App-level state
/// slopgb has an equivalent for (the recent-ROM list). bgb's window-geometry /
/// open-on-start keys have no slopgb equivalent, so they're preserved verbatim
/// but not surfaced here.
pub struct Loaded {
    pub settings: Settings,
    pub recent: Vec<PathBuf>,
}

/// The config directory: `$XDG_CONFIG_HOME/slopgb`, else `%APPDATA%\slopgb` on
/// Windows, else `~/.config/slopgb`. `None` if no home is discoverable.
fn config_dir() -> Option<PathBuf> {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("slopgb"));
        }
    }
    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.is_empty() {
                return Some(PathBuf::from(appdata).join("slopgb"));
            }
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".config").join("slopgb"))
}

/// Path to the native settings file (the default store).
fn native_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("slopgb.conf"))
}

/// The native settings-file path as a string, for display (System info box).
#[must_use]
pub fn config_file_display() -> String {
    native_path().map_or_else(
        || "(no config dir)".to_string(),
        |p| p.display().to_string(),
    )
}

/// Path to the bgb-format settings file (import/export interop).
fn bgb_ini_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("bgb.ini"))
}

/// Load persisted settings + recent ROMs. Precedence: the native file wins; if
/// it's absent but a `bgb.ini` exists, import it and write the native file
/// (one-time migration); otherwise defaults.
#[must_use]
pub fn load() -> Loaded {
    load_from_paths(native_path().as_deref(), bgb_ini_path().as_deref())
}

/// Persist `settings` + `recent` to the native file (preserving unknown
/// keys/sections), atomically. No-op without a config dir; errors are logged.
pub fn save(settings: &Settings, recent: &[PathBuf]) {
    if let Some(path) = native_path() {
        save_native(&path, settings, recent);
    }
}

/// Precedence: native file wins; else migrate a bgb.ini into the native store
/// (once); else defaults. Path-injected for tests.
fn load_from_paths(native: Option<&Path>, bgb: Option<&Path>) -> Loaded {
    if let Some(np) = native {
        if np.exists() {
            return load_native(np);
        }
        if let Some(bp) = bgb {
            if bp.exists() {
                let loaded = load_bgb(bp);
                save_native(np, &loaded.settings, &loaded.recent);
                return loaded;
            }
        }
    }
    Loaded {
        settings: Settings::default(),
        recent: Vec::new(),
    }
}

fn save_native(path: &Path, settings: &Settings, recent: &[PathBuf]) {
    let mut doc = std::fs::read_to_string(path)
        .map_or_else(|_| native::Doc::parse(""), |t| native::Doc::parse(&t));
    let recent_str: Vec<String> = recent
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    native::to_doc(settings, &recent_str, &mut doc);
    write_atomic(path, &doc.serialize());
}

/// Export the current `settings` + `recent` to a bgb-format ini at `path`
/// (merging over any existing bgb.ini so its unknown keys survive). For the
/// "Export bgb.ini" menu action.
pub fn export_bgb(path: &Path, settings: &Settings, recent: &[PathBuf]) {
    let mut ini =
        std::fs::read_to_string(path).map_or_else(|_| ini::Ini::parse(""), |t| ini::Ini::parse(&t));
    bgb::to_ini(settings, &mut ini);
    for i in 0..RECENT_MAX {
        let val = recent
            .get(i)
            .map(|p| posix_to_bgb_path(p))
            .unwrap_or_default();
        ini.set(&format!("Recent{i}"), &val);
    }
    write_atomic(path, &ini.serialize());
}

/// Import a bgb-format ini at `path`. For the "Import bgb.ini" menu action.
#[must_use]
pub fn import_bgb(path: &Path) -> Loaded {
    load_bgb(path)
}

fn load_native(path: &Path) -> Loaded {
    let doc = std::fs::read_to_string(path)
        .map_or_else(|_| native::Doc::parse(""), |t| native::Doc::parse(&t));
    let (settings, recent) = native::from_doc(&doc);
    Loaded {
        settings,
        recent: recent.into_iter().map(PathBuf::from).collect(),
    }
}

fn load_bgb(path: &Path) -> Loaded {
    let ini =
        std::fs::read_to_string(path).map_or_else(|_| ini::Ini::parse(""), |t| ini::Ini::parse(&t));
    let recent = (0..RECENT_MAX)
        .filter_map(|i| ini.get(&format!("Recent{i}")).filter(|v| !v.is_empty()))
        .map(bgb_path_to_posix)
        .collect();
    Loaded {
        settings: bgb::from_ini(&ini),
        recent,
    }
}

/// Write `text` to `path` durably (temp file + fsync + rename, creating the
/// parent dir). Errors are logged, not fatal.
fn write_atomic(path: &Path, text: &str) {
    if let Err(e) = crate::session::write_atomic(path, text.as_bytes()) {
        eprintln!("slopgb: settings write failed: {e}");
    }
}

/// A bgb/wine recent-ROM path (`Z:\home\x`) → a POSIX path (wine maps `Z:` to
/// `/`); a drive-less value passes through with `\` normalized to `/`.
fn bgb_path_to_posix(v: &str) -> PathBuf {
    let body = v
        .strip_prefix(|c: char| c.is_ascii_alphabetic())
        .and_then(|r| r.strip_prefix(':'))
        .unwrap_or(v);
    PathBuf::from(body.replace('\\', "/"))
}

/// A POSIX path → a wine `Z:\` path bgb understands (its own recents round-trip;
/// a real wine bgb can still open them). Relative paths are best-effort.
fn posix_to_bgb_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix('/').map_or_else(
        || s.replace('/', "\\"),
        |rest| format!("Z:\\{}", rest.replace('/', "\\")),
    )
}

#[cfg(test)]
#[path = "settings_file_tests.rs"]
mod tests;
