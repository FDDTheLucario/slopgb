//! Modal single-field prompt for the bgb-style debugger (right-click-menu plan
//! RM3): a titled box with an editable hex/text field and OK / Cancel. bgb pops
//! these for `Go to…`, `Set break/condition…`, `edit register`, and
//! `Evaluate expression`. The state machine ([`InputDialog::on_key`]) is pure
//! and unit-tested; the winit layer maps real keys onto [`DialogKey`] and routes
//! OK/Cancel clicks against the pure [`layout`].

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::font::{GLYPH_H, GLYPH_W};
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::button;

/// An abstract editing key. The winit layer translates physical keys / typed
/// text into these so the dialog logic stays testable without a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKey {
    /// A typed character (filtered by the field's [hex](InputDialog::hex_only) rule).
    Char(char),
    /// Delete the last character.
    Backspace,
    /// Accept the current buffer.
    Enter,
    /// Dismiss without accepting.
    Escape,
}

/// The outcome of feeding the dialog a key (or clicking OK/Cancel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogResult {
    /// Still open — keep editing.
    Continue,
    /// OK / Enter, carrying the trimmed buffer.
    Accept(String),
    /// Cancel / Escape.
    Cancel,
}

/// A modal text/hex input prompt. Pure state owned by the window; mutated by
/// [`on_key`](InputDialog::on_key) and read by [`render`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDialog {
    pub title: String,
    pub buffer: String,
    /// Accept only hex digits (stored uppercased) — for address/value fields.
    pub hex_only: bool,
    /// Cap on the number of characters.
    pub max_len: usize,
}

impl InputDialog {
    /// A fresh prompt. Hex fields cap at a 4-digit `u16` address; text fields
    /// (paths, expressions, conditions) get a generous cap — long enough for any
    /// real filesystem path — since [`render`] scrolls the field horizontally so
    /// a long value never spills past the box (it used to cap at 40, truncating
    /// most paths and overflowing the box visually).
    #[must_use]
    pub fn new(title: impl Into<String>, hex_only: bool) -> Self {
        Self {
            title: title.into(),
            buffer: String::new(),
            hex_only,
            max_len: if hex_only { HEX_MAX_LEN } else { TEXT_MAX_LEN },
        }
    }

    /// Pre-fill the buffer (the current register value the edit-register
    /// prompt opens with, RM11).
    #[must_use]
    pub fn with_initial(mut self, text: impl Into<String>) -> Self {
        self.buffer = text.into();
        self
    }

    /// Feed one key; returns whether the dialog stays open, was accepted, or
    /// cancelled.
    pub fn on_key(&mut self, key: DialogKey) -> DialogResult {
        match key {
            DialogKey::Char(ch) => {
                self.push(ch);
                DialogResult::Continue
            }
            DialogKey::Backspace => {
                self.buffer.pop();
                DialogResult::Continue
            }
            DialogKey::Enter => DialogResult::Accept(self.buffer.trim().to_owned()),
            DialogKey::Escape => DialogResult::Cancel,
        }
    }

    /// Append `ch` if it passes the field's filter and the length cap.
    fn push(&mut self, ch: char) {
        if self.buffer.chars().count() >= self.max_len {
            return;
        }
        if self.hex_only {
            if ch.is_ascii_hexdigit() {
                self.buffer.push(ch.to_ascii_uppercase());
            }
        } else if !ch.is_control() {
            self.buffer.push(ch);
        }
    }
}

/// Fixed modal box size (centred in the window).
const DIALOG_W: i32 = 232;
const DIALOG_H: i32 = 78;
const PAD: i32 = 6;
const BTN_W: i32 = 56;
/// A `u16` address is at most four hex digits.
const HEX_MAX_LEN: usize = 4;
/// Generous cap for text/path fields — beyond any real path, and the field
/// scrolls so length is not a visual constraint.
const TEXT_MAX_LEN: usize = 1024;

/// How many characters to scroll off the left of a field so the caret (at the
/// buffer's end) stays visible: show the trailing chars that fit, reserving one
/// cell for the caret. Pure, so the scroll logic is unit-tested without a canvas.
#[must_use]
pub fn field_scroll(buffer_chars: usize, visible_chars: usize) -> usize {
    // Text cells available with one reserved for the caret bar.
    let text_cells = visible_chars.saturating_sub(1);
    buffer_chars.saturating_sub(text_cells)
}

/// Geometry of the modal: the box, the input field, and the OK/Cancel buttons —
/// a pure function of the window `area`, shared by [`render`] and the click
/// routing so they can't disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogLayout {
    pub boxr: Rect,
    pub field: Rect,
    pub ok: Rect,
    pub cancel: Rect,
}

/// Centre the modal in `area` and partition it.
#[must_use]
pub fn layout(area: Rect) -> DialogLayout {
    let bx = area.x + (area.w - DIALOG_W) / 2;
    let by = area.y + (area.h - DIALOG_H) / 2;
    let boxr = Rect::new(bx, by, DIALOG_W, DIALOG_H);
    let lh = line_height();
    let field = Rect::new(bx + PAD, by + PAD + lh + 2, DIALOG_W - 2 * PAD, lh + 4);
    let btn_y = boxr.bottom() - PAD - (lh + 6);
    let ok = Rect::new(boxr.right() - PAD - 2 * BTN_W - PAD, btn_y, BTN_W, lh + 6);
    let cancel = Rect::new(boxr.right() - PAD - BTN_W, btn_y, BTN_W, lh + 6);
    DialogLayout {
        boxr,
        field,
        ok,
        cancel,
    }
}

/// Resolve a left-click at `(px, py)` over the modal: OK accepts the buffer,
/// Cancel dismisses; a click elsewhere keeps it open.
#[must_use]
pub fn click(dlg: &InputDialog, area: Rect, px: i32, py: i32) -> DialogResult {
    let l = layout(area);
    if l.ok.contains(px, py) {
        DialogResult::Accept(dlg.buffer.trim().to_owned())
    } else if l.cancel.contains(px, py) {
        DialogResult::Cancel
    } else {
        DialogResult::Continue
    }
}

/// Draw the modal over `area`: titled box, the buffer in a bordered field with
/// a caret, and OK / Cancel buttons.
pub fn render(c: &mut Canvas, area: Rect, dlg: &InputDialog, theme: &Theme) {
    let l = layout(area);
    c.fill_rect(l.boxr, theme.bg);
    c.outline_rect(l.boxr, theme.border);
    // Title.
    draw_text(c, l.boxr.x + PAD, l.boxr.y + PAD, &dlg.title, theme.text);
    // Field: bordered, the buffer text, then a caret bar. The text is clipped to
    // the field interior and scrolled so a long value keeps its tail (and caret)
    // visible instead of spilling past the box.
    c.fill_rect(l.field, theme.panel);
    c.outline_rect(l.field, theme.text);
    let inner_w = (l.field.w - 4).max(0); // 2px padding each side
    let visible = (inner_w / GLYPH_W as i32).max(0) as usize;
    let skip = field_scroll(dlg.buffer.chars().count(), visible);
    let shown: String = dlg.buffer.chars().skip(skip).collect();
    let saved = c.push_clip(l.field);
    let tx = draw_text(c, l.field.x + 2, l.field.y + 2, &shown, theme.text);
    c.vline(tx + 1, l.field.y + 2, GLYPH_H as i32, theme.text);
    c.set_clip(saved);
    button(c, l.ok, "OK", false, theme);
    button(c, l.cancel, "Cancel", false, theme);
}

#[cfg(test)]
#[path = "dialog_tests.rs"]
mod tests;
