//! The Cheat dialog UI (bgb's Cheat window, docs/bgb-reference/cheat/): a centred
//! modal over the LCD listing the cheats with a button grid, plus an Add/Edit
//! text entry. The pure cheat model lives in [`crate::cheat`]; this is the
//! render + hit-test + input-state layer the game window owns.
//!
//! bgb uses two fields (Comment / Code); slopgb's Add/Edit reuses the shared
//! single-line modal with a `comment = code` convention (split on the last `=`).

use crate::cheat::{Effect, parse_code};
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::dialog::{DialogKey, DialogResult, InputDialog};
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::button;

/// A button in the Cheat dialog's grid (bgb: Add/Edit/Delete/Enable/Disable/
/// Enable all/Disable all/Poke; Close replaces bgb's window-close).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheatButton {
    Add,
    Edit,
    Delete,
    Enable,
    Disable,
    EnableAll,
    DisableAll,
    Poke,
    Close,
}

/// Button labels + values, laid out in two rows (5 then 4), matching bgb.
const BUTTONS: [(&str, CheatButton); 9] = [
    ("Add", CheatButton::Add),
    ("Edit", CheatButton::Edit),
    ("Delete", CheatButton::Delete),
    ("Enable", CheatButton::Enable),
    ("Disable", CheatButton::Disable),
    ("Enable all", CheatButton::EnableAll),
    ("Disable all", CheatButton::DisableAll),
    ("Poke", CheatButton::Poke),
    ("Close", CheatButton::Close),
];

/// What a left-click in the dialog resolved to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheatHit {
    Row(usize),
    Button(CheatButton),
}

/// An accepted Add/Edit entry: the parsed comment + code, and the row being
/// edited (`None` = a new cheat).
pub struct CheatEdit {
    pub comment: String,
    pub code: String,
    pub editing: Option<usize>,
}

/// Open Cheat-dialog state (selection + an optional Add/Edit text entry).
#[derive(Default)]
pub struct CheatDialog {
    pub sel: usize,
    /// The Add/Edit entry + the index being edited (`None` = adding).
    input: Option<(InputDialog, Option<usize>)>,
}

impl CheatDialog {
    /// Whether the Add/Edit text entry is open (captures keys).
    #[must_use]
    pub fn input_open(&self) -> bool {
        self.input.is_some()
    }

    /// Open the Add entry.
    pub fn open_add(&mut self) {
        self.input = Some((InputDialog::new("Cheat  (comment = code)", false), None));
    }

    /// Open the Edit entry for row `i`, prefilled with `comment = code`.
    pub fn open_edit(&mut self, i: usize, comment: &str, code: &str) {
        let mut dlg = InputDialog::new("Edit cheat  (comment = code)", false);
        for ch in format!("{comment} = {code}").chars() {
            dlg.on_key(DialogKey::Char(ch));
        }
        self.input = Some((dlg, Some(i)));
    }

    /// Feed a key to the open Add/Edit entry. Returns `Some(edit)` on Accept
    /// (the entry closes), `None` otherwise; Cancel just closes the entry.
    pub fn input_key(&mut self, key: DialogKey) -> Option<CheatEdit> {
        let (dlg, editing) = self.input.as_mut()?;
        match dlg.on_key(key) {
            DialogResult::Continue => None,
            DialogResult::Cancel => {
                self.input = None;
                None
            }
            DialogResult::Accept(text) => {
                let editing = *editing;
                self.input = None;
                Some(parse_entry(&text, editing))
            }
        }
    }

    /// Reference to the open Add/Edit dialog (for rendering).
    #[must_use]
    pub fn input_dialog(&self) -> Option<&InputDialog> {
        self.input.as_ref().map(|(d, _)| d)
    }
}

/// Split a `comment = code` entry on the LAST `=` (so a code containing no `=`
/// works, and a comment may contain one). No `=` → all code, empty comment.
fn parse_entry(text: &str, editing: Option<usize>) -> CheatEdit {
    let (comment, code) = match text.rfind('=') {
        Some(i) => (text[..i].trim().to_string(), text[i + 1..].trim().to_string()),
        None => (String::new(), text.trim().to_string()),
    };
    CheatEdit { comment, code, editing }
}

/// The decoded-effect string bgb shows in Advanced mode (`(C10A)=FF`).
fn decoded(code: &str) -> String {
    match parse_code(code) {
        Some(Effect::Ram { addr, value }) => format!("({addr:04X})={value:02X}"),
        Some(Effect::RomPatch) => "ROM patch".to_string(),
        None => "(bad code)".to_string(),
    }
}

/// The centred dialog panel.
fn panel(area: Rect) -> Rect {
    let (w, h) = (380, 260);
    Rect::new(area.x + (area.w - w).max(0) / 2, area.y + (area.h - h).max(0) / 2, w, h)
}

/// The 9 button rects (two rows of 5 + 4) along the bottom of `p`.
fn button_rects(p: Rect) -> Vec<Rect> {
    let lh = line_height();
    let (pad, gap) = (6, 3);
    let bh = lh + 4;
    let bw = (p.w - 2 * pad - 4 * gap) / 5;
    let row2 = p.bottom() - pad - bh;
    let row1 = row2 - bh - gap;
    (0..9)
        .map(|i| {
            let (col, y) = if i < 5 { (i, row1) } else { (i - 5, row2) };
            Rect::new(p.x + pad + col * (bw + gap), y, bw, bh)
        })
        .collect()
}

/// The y of the first list row + the row height.
fn list_top(p: Rect) -> i32 {
    p.y + 6 + line_height() + 2
}

/// The bottom the list can occupy (above the button grid).
fn list_bottom(p: Rect) -> i32 {
    button_rects(p).first().map_or(p.bottom(), |r| r.y - 2)
}

/// Draw the dialog: title, cheat rows (`[x] CODE  (addr)=val  comment`, selected
/// row highlighted), the button grid, and the Add/Edit entry on top if open.
pub fn render(c: &mut Canvas, d: &CheatDialog, cheats: &crate::cheat::CheatList, theme: &Theme) {
    let area = c.bounds();
    let p = panel(area);
    c.fill_rect(p, theme.bg);
    c.outline_rect(p, theme.border);
    let lh = line_height();
    let pad = 6;
    let title = format!("Cheats ({})", cheats.len());
    draw_text(c, p.x + pad, p.y + pad, &title, theme.text);

    let top = list_top(p);
    let bottom = list_bottom(p);
    if cheats.is_empty() {
        draw_text(c, p.x + pad, top, "(no cheats — Add a GameShark code)", theme.text);
    }
    for (i, ch) in cheats.items().iter().enumerate() {
        let y = top + i as i32 * lh;
        if y + lh > bottom {
            break;
        }
        let mark = if ch.enabled { 'x' } else { ' ' };
        let line = format!("[{mark}] {}  {}  {}", ch.code, decoded(&ch.code), ch.comment);
        let fg = if i == d.sel {
            c.fill_rect(Rect::new(p.x + 2, y, p.w - 4, lh), theme.current);
            theme.bg
        } else {
            theme.text
        };
        draw_text(c, p.x + pad, y, &line, fg);
    }

    for (r, (label, _)) in button_rects(p).into_iter().zip(BUTTONS) {
        button(c, r, label, false, theme);
    }

    if let Some(dlg) = d.input_dialog() {
        crate::ui::dialog::render(c, area, dlg, theme);
    }
}

/// Resolve a left-click to a button or a cheat row (buttons take priority).
#[must_use]
pub fn hit(area: Rect, cheats: &crate::cheat::CheatList, px: i32, py: i32) -> Option<CheatHit> {
    let p = panel(area);
    for (r, (_, btn)) in button_rects(p).into_iter().zip(BUTTONS) {
        if r.contains(px, py) {
            return Some(CheatHit::Button(btn));
        }
    }
    let (top, bottom, lh) = (list_top(p), list_bottom(p), line_height());
    for i in 0..cheats.len() {
        let y = top + i as i32 * lh;
        if y + lh > bottom {
            break;
        }
        if Rect::new(p.x, y, p.w, lh).contains(px, py) {
            return Some(CheatHit::Row(i));
        }
    }
    None
}

#[cfg(test)]
#[path = "cheat_ui_tests.rs"]
mod tests;
