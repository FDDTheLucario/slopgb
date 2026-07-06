//! On-disk settings persistence. Phase 1: read/write bgb's `bgb.ini` format so
//! the config interops with real bgb, preserving every key we don't model.
//! See `docs/settings-persistence-plan.md` for the two-phase plan + key map.

mod bgb;
mod ini;

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

/// Path to the bgb-format settings file in the config dir.
fn bgb_ini_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("bgb.ini"))
}

/// Load persisted settings + recent ROMs (defaults/empty when no file /
/// unreadable / no config dir).
#[must_use]
pub fn load() -> Loaded {
    bgb_ini_path().map_or_else(
        || Loaded { settings: Settings::default(), recent: Vec::new() },
        |p| load_from(&p),
    )
}

/// Persist `settings` + `recent`, preserving any unknown keys already in the
/// file. No-op if no config dir is discoverable; a write failure is logged.
pub fn save(settings: &Settings, recent: &[PathBuf]) {
    if let Some(path) = bgb_ini_path() {
        save_to(&path, settings, recent);
    }
}

fn load_from(path: &Path) -> Loaded {
    let ini = std::fs::read_to_string(path).map_or_else(|_| ini::Ini::parse(""), |t| ini::Ini::parse(&t));
    let recent = (0..RECENT_MAX)
        .filter_map(|i| ini.get(&format!("Recent{i}")).filter(|v| !v.is_empty()))
        .map(bgb_path_to_posix)
        .collect();
    Loaded { settings: bgb::from_ini(&ini), recent }
}

/// Write to the bgb.ini at `path`, merging over the existing file (unknown keys
/// preserved) and writing atomically (temp file + rename).
fn save_to(path: &Path, settings: &Settings, recent: &[PathBuf]) {
    // Merge over the current file so bgb's unmodelled keys survive; start blank
    // if there's no file yet.
    let mut doc = std::fs::read_to_string(path).map_or_else(|_| ini::Ini::parse(""), |t| ini::Ini::parse(&t));
    bgb::to_ini(settings, &mut doc);
    // Recent0..9: filled slots as wine paths, the rest blank (bgb's shape).
    for i in 0..RECENT_MAX {
        let val = recent.get(i).map(|p| posix_to_bgb_path(p)).unwrap_or_default();
        doc.set(&format!("Recent{i}"), &val);
    }
    let text = doc.serialize();

    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("slopgb: settings dir create failed: {e}");
            return;
        }
    }
    let tmp = path.with_extension("ini.tmp");
    if let Err(e) = std::fs::write(&tmp, text.as_bytes()) {
        eprintln!("slopgb: settings write failed: {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        eprintln!("slopgb: settings rename failed: {e}");
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
