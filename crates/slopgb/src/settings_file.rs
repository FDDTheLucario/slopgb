//! On-disk settings persistence. Phase 1: read/write bgb's `bgb.ini` format so
//! the config interops with real bgb, preserving every key we don't model.
//! See `docs/settings-persistence-plan.md` for the two-phase plan + key map.

mod bgb;
mod ini;

use std::path::{Path, PathBuf};

use crate::windows::options::Settings;

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

/// Load persisted settings (defaults when no file / unreadable / no config dir).
#[must_use]
pub fn load() -> Settings {
    bgb_ini_path().map_or_else(Settings::default, |p| load_from(&p))
}

/// Persist `settings`, preserving any unknown keys already in the file. No-op if
/// no config dir is discoverable; a write failure is logged, not fatal.
pub fn save(settings: &Settings) {
    if let Some(path) = bgb_ini_path() {
        save_to(&path, settings);
    }
}

/// Read `Settings` from the bgb.ini at `path`; defaults if absent/unreadable.
fn load_from(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => bgb::from_ini(&ini::Ini::parse(&text)),
        Err(_) => Settings::default(),
    }
}

/// Write `settings` to the bgb.ini at `path`, merging over the existing file
/// (unknown keys preserved) and writing atomically (temp file + rename).
fn save_to(path: &Path, settings: &Settings) {
    // Merge over the current file so bgb's unmodelled keys survive; start blank
    // if there's no file yet.
    let mut doc = std::fs::read_to_string(path).map_or_else(|_| ini::Ini::parse(""), |t| ini::Ini::parse(&t));
    bgb::to_ini(settings, &mut doc);
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

#[cfg(test)]
#[path = "settings_file_tests.rs"]
mod tests;
