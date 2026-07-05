//! The in-app fallback file browser: `slopfp`'s pure state
//! machine wired to the game window, used only when [`crate::filepicker`]'s
//! native-dialog shell-out finds no installed picker tool
//! ([`crate::filepicker::PickResult::Unavailable`]). Slots into the exact same
//! sites as the typed [`crate::ui::dialog::InputDialog`] path modal (see
//! `app_path.rs`, `main.rs::handle_key`, `app_menu.rs::on_game_click`, and the
//! game redraw) — a sibling modal, not a replacement plumbing path.

use std::path::PathBuf;

use slopfp::{Key, Mode, Outcome, Picker};
use winit::keyboard::{KeyCode, ModifiersState};

use crate::PathPurpose;
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::scroll_list;

/// Panel padding + max size, centred over the game window.
const PAD: i32 = 4;
const MAX_W: i32 = 560;
const MAX_H: i32 = 420;
const MARGIN: i32 = 40;

pub(crate) struct FallbackPicker {
    picker: Picker,
    purpose: PathPurpose,
    /// Scroll offset of the last `view()` (rows are pre-sliced to the window,
    /// so a click's row index is `last_offset + row_in_view`).
    last_offset: usize,
    /// The list's hit-rect from the last `render`, for [`Self::on_click`].
    list_rect: Rect,
    /// Number of rows actually drawn in the last `render` (`v.rows.len()`),
    /// which can be less than `list_rect.h / line_height()` when the list is
    /// shorter than the viewport, or the rect's height isn't an exact multiple
    /// of the line height (leaving a sub-row sliver at the bottom). A click
    /// past this many rows-in-view hits blank space, not an undrawn row.
    last_rowcount: usize,
}

impl FallbackPicker {
    /// Open a picker rooted at `start_dir`. `filters` are extensions without
    /// the dot; empty = show everything.
    // ponytail: per-purpose ext filters (e.g. gb/gbc/gbs/sav), add when a
    // purpose needs one — every current purpose just wants "any file".
    #[must_use]
    pub(crate) fn open(
        purpose: PathPurpose,
        start_dir: PathBuf,
        filters: &[&str],
        title: &str,
        save: bool,
    ) -> Self {
        let mode = if save { Mode::Save } else { Mode::Open };
        let picker = Picker::new(mode, start_dir, filters).with_title(title);
        Self {
            picker,
            purpose,
            last_offset: 0,
            list_rect: Rect::new(0, 0, 0, 0),
            last_rowcount: 0,
        }
    }

    /// What an [`Outcome::Picked`] from this picker should be run as.
    #[must_use]
    pub(crate) fn purpose(&self) -> PathPurpose {
        self.purpose
    }

    pub(crate) fn feed_key(&mut self, key: Key) -> Outcome {
        self.picker.on_key(key)
    }

    /// Draw the panel centred over a `window_w` x `window_h` surface: title,
    /// path bar, the scrollable listing, an (always-reserved, only-drawn-in-
    /// Save-mode) filename line, and a status line at the bottom.
    pub(crate) fn render(&mut self, c: &mut Canvas, window_w: i32, window_h: i32, theme: &Theme) {
        let w = (window_w - MARGIN).clamp(0, MAX_W);
        let h = (window_h - MARGIN).clamp(0, MAX_H);
        let panel = Rect::new((window_w - w) / 2, (window_h - h) / 2, w, h);
        c.fill_rect(panel, theme.bg);
        c.outline_rect(panel, theme.border);

        let lh = line_height();
        let content_x = panel.x + PAD;
        let title_y = panel.y + PAD;
        let pathbar_y = title_y + lh + 2;
        // The shortcut hint is the bottom-most line, then status, then the
        // (always-reserved) save-name line above that.
        let hint_y = panel.bottom() - PAD - lh;
        let status_y = hint_y - lh - 2;
        // Reserved unconditionally (Save mode only draws into it) — a one-line
        // blank in Open mode is cheaper than a second `view()` call to learn
        // the mode first.
        let filename_y = status_y - lh - 2;
        let list_y = pathbar_y + lh + 4;
        let list_rect = Rect::new(content_x, list_y, w - 2 * PAD, (filename_y - 2 - list_y).max(0));

        let rows_fit = (list_rect.h / lh).max(0) as usize;
        let v = self.picker.view(rows_fit);

        draw_text(c, content_x, title_y, &v.title, theme.text);
        let path_line = if v.path_focused { format!("{}_", v.path_bar) } else { v.path_bar.clone() };
        draw_text(c, content_x, pathbar_y, &path_line, theme.text);

        let display_rows: Vec<String> = v
            .rows
            .iter()
            .map(|r| {
                let marker = if r.is_dir { "[]/ " } else { "    " };
                format!("{marker}{:<40}{:>10}  {}", r.name, r.size, r.mtime)
            })
            .collect();
        let refs: Vec<&str> = display_rows.iter().map(String::as_str).collect();
        scroll_list(c, list_rect, &refs, 0, v.highlight, theme);

        if let Some(name) = &v.save_name {
            let hint = if v.overwrite_pending { " (overwrite? press Enter again)" } else { "" };
            draw_text(c, content_x, filename_y, &format!("Save as: {name}_{hint}"), theme.text);
        }
        draw_text(c, content_x, status_y, &v.status, theme.text);
        // The Ctrl+<letter> hotkeys above have no other affordance (no native
        // dialog, no menu bar here), so spell them out — dim (hilight) color to
        // read as a footnote, not a fourth status line.
        draw_text(
            c,
            content_x,
            hint_y,
            "^L path  ^K sort  ^R rev  ^H hidden  ^A all  Tab complete",
            theme.hilight,
        );

        self.list_rect = list_rect;
        self.last_offset = v.offset;
        self.last_rowcount = v.rows.len();
    }

    /// Route a click at `(px, py)`: outside the list is a no-op; inside,
    /// single-click selects, double-click activates (open dir / pick file).
    /// A click below the last *drawn* row (the sub-row sliver left when
    /// `list_rect.h` isn't an exact multiple of `line_height()`, or the list is
    /// shorter than the viewport) is also a no-op — there is no row there.
    pub(crate) fn on_click(&mut self, px: i32, py: i32, double: bool) -> Outcome {
        if !self.list_rect.contains(px, py) {
            return Outcome::None;
        }
        let rel = ((py - self.list_rect.y) / line_height()).max(0) as usize;
        if rel >= self.last_rowcount {
            return Outcome::None;
        }
        let abs = self.last_offset + rel;
        if double {
            self.picker.on_activate(abs)
        } else {
            self.picker.on_click(abs);
            Outcome::None
        }
    }
}

/// Translate a winit key into the picker's semantic [`Key`], mirroring
/// `main::dialog_key_from` (the typed-modal translator this picker sits
/// alongside). `text` is the winit `KeyEvent::text` (a printable char, if
/// any); `mods` gates the Ctrl+<letter> hotkeys (path bar / sort / hidden /
/// all-files — see the bottom-of-panel hint drawn in [`FallbackPicker::render`]).
#[must_use]
pub(crate) fn winit_key_to_picker(code: KeyCode, text: Option<&str>, mods: ModifiersState) -> Option<Key> {
    match code {
        KeyCode::ArrowUp => return Some(Key::Up),
        KeyCode::ArrowDown => return Some(Key::Down),
        KeyCode::PageUp => return Some(Key::PageUp),
        KeyCode::PageDown => return Some(Key::PageDown),
        KeyCode::Home => return Some(Key::Home),
        KeyCode::End => return Some(Key::End),
        KeyCode::Enter | KeyCode::NumpadEnter => return Some(Key::Enter),
        // Browse focus treats Backspace as "up a level"; PathBar/SaveName focus
        // treat it as a normal delete — both are the picker's own `Key::Backspace`.
        KeyCode::Backspace => return Some(Key::Backspace),
        KeyCode::Escape => return Some(Key::Cancel),
        KeyCode::Tab => return Some(Key::Tab),
        KeyCode::KeyH if mods.control_key() => return Some(Key::ToggleHidden),
        KeyCode::KeyL if mods.control_key() => return Some(Key::FocusPath),
        KeyCode::KeyK if mods.control_key() => return Some(Key::CycleSort),
        KeyCode::KeyR if mods.control_key() => return Some(Key::ToggleSortDir),
        KeyCode::KeyA if mods.control_key() => return Some(Key::ToggleAllFiles),
        _ => {}
    }
    let ch = text?.chars().next()?;
    (!ch.is_control()).then_some(Key::Char(ch))
}

#[cfg(test)]
#[path = "fallback_picker_tests.rs"]
mod tests;
