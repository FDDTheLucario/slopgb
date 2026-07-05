//! Dep-free native file picker (Load ROM / Save state / Load state / Load
//! symbols / the Options bootrom `...` browse buttons).
//!
//! The frontend is restricted to winit/softbuffer/cpal — no file-dialog *crate*
//! is allowed. A native picker is still possible with **std only** by shelling
//! out to whichever system dialog utility is installed, the same trick as
//! [`crate::clipboard`]. We try the common ones in order, read the chosen path
//! from the tool's stdout, and report a tri-state so the caller can tell a
//! user-cancelled dialog (do nothing) from "no picker installed" (fall back to
//! the typed-path modal). Nothing here touches the emulator core (golden-safe).
//!
//! `pick_open`/`pick_save` block the UI thread while the native dialog is open
//! (the dialogs are modal), which matches bgb's own modal file dialogs.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The result of trying to show a native file dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PickResult {
    /// The user chose `path`.
    Picked(PathBuf),
    /// A dialog ran but the user cancelled (or gave no path).
    Cancelled,
    /// No picker tool is installed — the caller should fall back to typing.
    Unavailable,
}

/// One candidate's outcome — kept separate from [`PickResult`] so `run_first`
/// can tell "this tool isn't installed, try the next" (`NoSpawn`) from "a tool
/// ran and the user cancelled" (`Cancelled`, which stops the search).
#[derive(Clone, Debug, PartialEq, Eq)]
enum TryOutcome {
    Picked(PathBuf),
    Cancelled,
    NoSpawn,
}

/// Open-file dialog candidates, most-likely-installed first. Each prints the
/// chosen path to stdout and exits 0; a cancel exits non-zero. Pure, so the
/// table is unit-testable (like [`crate::clipboard::clipboard_candidates`]).
pub(crate) fn open_candidates() -> [(&'static str, &'static [&'static str]); 4] {
    [
        ("zenity", &["--file-selection"]),
        ("kdialog", &["--getopenfilename", "."]),
        ("yad", &["--file"]),
        ("qarma", &["--file-selection"]),
    ]
}

/// Save-file dialog candidates (a writable target path; the tool confirms an
/// overwrite where it supports it).
pub(crate) fn save_candidates() -> [(&'static str, &'static [&'static str]); 4] {
    [
        (
            "zenity",
            &["--file-selection", "--save", "--confirm-overwrite"],
        ),
        ("kdialog", &["--getsavefilename", "."]),
        ("yad", &["--file", "--save"]),
        (
            "qarma",
            &["--file-selection", "--save", "--confirm-overwrite"],
        ),
    ]
}

/// Trim a picker's stdout to a path: drop the trailing newline/whitespace;
/// `None` on a failed/cancelled run or empty output. Pure → unit-tested.
pub(crate) fn parse_pick_output(stdout: &str, success: bool) -> Option<String> {
    if !success {
        return None;
    }
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Show an open-file dialog. Never panics; blocks until the dialog closes.
#[must_use]
pub(crate) fn pick_open() -> PickResult {
    run_first(open_candidates())
}

/// Show a save-file dialog.
#[must_use]
pub(crate) fn pick_save() -> PickResult {
    run_first(save_candidates())
}

/// Try each candidate in order: the first tool that **spawns** decides the
/// result (a path or a cancel); only spawn failures fall through. If no tool
/// spawns at all, the picker is unavailable.
fn run_first(candidates: [(&'static str, &'static [&'static str]); 4]) -> PickResult {
    for (prog, args) in candidates {
        match try_pick(prog, args) {
            TryOutcome::Picked(p) => return PickResult::Picked(p),
            TryOutcome::Cancelled => return PickResult::Cancelled,
            TryOutcome::NoSpawn => {}
        }
    }
    PickResult::Unavailable
}

/// Spawn one dialog tool and read the chosen path from its stdout. `NoSpawn` on a
/// spawn error (tool missing); `Cancelled` on a non-zero exit / empty output;
/// `Picked` otherwise.
fn try_pick(prog: &str, args: &[&str]) -> TryOutcome {
    let Ok(out) = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return TryOutcome::NoSpawn;
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    match parse_pick_output(&stdout, out.status.success()) {
        Some(path) => TryOutcome::Picked(PathBuf::from(path)),
        None => TryOutcome::Cancelled,
    }
}

#[cfg(test)]
#[path = "filepicker_tests.rs"]
mod tests;
