//! Dep-free clipboard write (debugger "Copy data" / "Copy code", RM10).
//!
//! The frontend is restricted to winit/softbuffer/cpal — no clipboard *crate*
//! is allowed. A clipboard write is still possible with **std only** by shelling
//! out to whichever system clipboard utility is installed, the same way many
//! native tools do. We try the common ones in order and pipe the text to the
//! tool's stdin; nothing here touches the emulator core, so it is golden-safe.

use std::io::Write;
use std::process::{Command, Stdio};

/// Candidate clipboard tools, most-modern first: Wayland's `wl-copy`, then the
/// two X11 standbys. Each reads the clipboard text from stdin. Pure (no spawn),
/// so the table is unit-testable.
pub(crate) fn clipboard_candidates() -> [(&'static str, &'static [&'static str]); 3] {
    [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["-ib"]),
    ]
}

/// Candidate tools for an `image/png` clipboard write (Joypad "Screenshot
/// button" → copies). `xsel` has no MIME support, so only wl-copy + xclip
/// qualify. Pure, so the table is unit-testable.
pub(crate) fn image_candidates() -> [(&'static str, &'static [&'static str]); 2] {
    [
        ("wl-copy", &["--type", "image/png"]),
        ("xclip", &["-selection", "clipboard", "-t", "image/png"]),
    ]
}

/// Copy `text` to the system clipboard, returning whether a tool accepted it.
/// Tries each [`clipboard_candidates`] entry until one spawns and exits cleanly;
/// a missing tool (or a spawn error) just falls through to the next. Never
/// panics — the caller logs a miss and carries on.
#[must_use]
pub(crate) fn copy(text: &str) -> bool {
    for (prog, args) in clipboard_candidates() {
        if try_copy(prog, args, text) {
            return true;
        }
    }
    false
}

/// Copy PNG `bytes` to the system clipboard as an image (same fall-through as
/// [`copy`]). Returns whether a tool accepted it.
#[must_use]
pub(crate) fn copy_image_png(bytes: &[u8]) -> bool {
    for (prog, args) in image_candidates() {
        if try_write(prog, args, bytes) {
            return true;
        }
    }
    false
}

/// Spawn one tool and feed it `text`. Returns true only if the child spawned,
/// took the whole write, and exited successfully.
fn try_copy(prog: &str, args: &[&str], text: &str) -> bool {
    try_write(prog, args, text.as_bytes())
}

/// Spawn one tool and feed it `bytes` on stdin. Returns true only if the child
/// spawned, took the whole write, and exited successfully.
fn try_write(prog: &str, args: &[&str], bytes: &[u8]) -> bool {
    let Ok(mut child) = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(bytes).is_ok())
        .unwrap_or(false);
    // Always reap the child; success needs the write AND a clean exit.
    let exited_ok = child.wait().map(|s| s.success()).unwrap_or(false);
    wrote && exited_ok
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod tests;
