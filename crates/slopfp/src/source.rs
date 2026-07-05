//! The **only** module that touches `std::fs` — the picker's filesystem source.
//! Everything is std-only. Tested against a real temp directory (see
//! `source_tests.rs`), platform-specific bits gated behind `#[cfg(unix)]` etc.

use super::Entry;
use std::io;
use std::path::{Path, PathBuf};

/// Read `path` into a listing. Each child becomes an [`Entry`]: `is_dir`
/// **follows** symlinks (a symlink to a directory reads as a directory); `size`
/// and `mtime` (epoch seconds) come from metadata and are `None` when it cannot
/// be read (a broken symlink, a permission-denied child). An individual child
/// whose metadata fails is still listed (best-effort) — only a failure to open
/// `path` itself is an `Err`. `.`/`..` are not included.
pub fn read_dir(path: &Path) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path)? {
        // A bad DirEntry (rare, e.g. a raced-away file) is skipped, not fatal.
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = entry.path();

        // Prefer symlink-following metadata (is_dir must follow); fall back to
        // the no-follow file type (a broken symlink -> not a dir) with no
        // size/mtime when metadata can't be read at all.
        let (is_dir, size, mtime) = match std::fs::metadata(&child) {
            Ok(md) => {
                let is_dir = md.is_dir();
                let size = (!is_dir).then_some(md.len());
                let mtime = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                (is_dir, size, mtime)
            }
            Err(_) => {
                let is_dir = entry.file_type().is_ok_and(|ft| ft.is_dir());
                (is_dir, None, None)
            }
        };

        out.push(Entry::new(name, is_dir, size, mtime));
    }
    Ok(out)
}

/// The user's home directory: `$HOME` on unix, `%USERPROFILE%` on Windows.
/// `None` if unset. Std only (no `dirs` crate).
#[must_use]
pub fn home() -> Option<PathBuf> {
    #[cfg(windows)]
    let var = "USERPROFILE";
    #[cfg(not(windows))]
    let var = "HOME";
    std::env::var_os(var).map(PathBuf::from)
}

/// Filesystem roots to offer as quick jumps: `/` on unix; existing drive roots
/// `A:\\`..=`Z:\\` on Windows (probe existence, std only — no winapi).
#[must_use]
pub fn roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|b| PathBuf::from(format!("{}:\\", b as char)))
            .filter(|p| p.exists())
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/")]
    }
}

/// Create directory `name` inside `parent`, returning its path. Errors
/// (already-exists, permission denied) propagate for the caller to surface as
/// a status message.
pub fn make_dir(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let p = parent.join(name);
    std::fs::create_dir(&p)?;
    Ok(p)
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
