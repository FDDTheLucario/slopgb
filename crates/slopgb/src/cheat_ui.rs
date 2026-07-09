//! The Cheat dialog UI (bgb's Cheat window, docs/bgb-reference/cheat/): a centred
//! modal over the LCD listing the cheats with a button grid + an Advanced toggle,
//! plus a two-field Add/Edit editor (Comment / Code, exactly like bgb). The pure
//! cheat model lives in [`crate::cheat`]; this is the render + hit-test + editor
//! layer the game window owns.

use crate::cheat::{Effect, parse_code};
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::button;

/// A button in the Cheat dialog's grid — bgb's full set plus Close.
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
    Load,
    Save,
    Advanced,
    Close,
}

/// Button labels + values, laid out in a grid (matches bgb's Add/Edit/Delete/
/// Enable/Disable/Enable all/Disable all/Poke/Load/Save/Advanced set).
const BUTTONS: [(&str, CheatButton); 12] = [
    ("Add", CheatButton::Add),
    ("Edit", CheatButton::Edit),
    ("Delete", CheatButton::Delete),
    ("Enable all", CheatButton::EnableAll),
    ("Enable", CheatButton::Enable),
    ("Disable", CheatButton::Disable),
    ("Poke", CheatButton::Poke),
    ("Disable all", CheatButton::DisableAll),
    ("Load", CheatButton::Load),
    ("Save", CheatButton::Save),
    ("Advanced", CheatButton::Advanced),
    ("Close", CheatButton::Close),
];

/// What a left-click in the dialog resolved to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheatHit {
    Row(usize),
    Button(CheatButton),
}

/// An accepted Add/Edit entry: the comment + code, and the row being edited
/// (`None` = a new cheat).
pub struct CheatEdit {
    pub comment: String,
    pub code: String,
    pub editing: Option<usize>,
}

/// Which field the two-field editor is typing into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Comment,
    Code,
}

/// The Add/Edit editor: bgb's two stacked fields (Comment / Code) + OK/Cancel.
struct Editor {
    comment: String,
    code: String,
    field: Field,
    editing: Option<usize>,
}

/// Open Cheat-dialog state.
#[derive(Default)]
pub struct CheatDialog {
    pub sel: usize,
    /// Advanced mode: show the decoded `(addr)=val` column (bgb's checkbox).
    pub advanced: bool,
    editor: Option<Editor>,
}

impl CheatDialog {
    /// Whether the Add/Edit editor is open (it captures keys).
    #[must_use]
    pub fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    pub fn open_add(&mut self) {
        self.editor = Some(Editor {
            comment: String::new(),
            code: String::new(),
            field: Field::Comment,
            editing: None,
        });
    }

    pub fn open_edit(&mut self, i: usize, comment: &str, code: &str) {
        self.editor = Some(Editor {
            comment: comment.to_string(),
            code: code.to_string(),
            field: Field::Comment,
            editing: Some(i),
        });
    }

    /// Tab between the Comment and Code fields.
    pub fn switch_field(&mut self) {
        if let Some(e) = &mut self.editor {
            e.field = match e.field {
                Field::Comment => Field::Code,
                Field::Code => Field::Comment,
            };
        }
    }

    /// Type a char into the focused field.
    pub fn type_char(&mut self, ch: char) {
        if let Some(e) = &mut self.editor {
            let f = match e.field {
                Field::Comment => &mut e.comment,
                Field::Code => &mut e.code,
            };
            f.push(ch);
        }
    }

    /// Backspace the focused field.
    pub fn backspace(&mut self) {
        if let Some(e) = &mut self.editor {
            match e.field {
                Field::Comment => e.comment.pop(),
                Field::Code => e.code.pop(),
            };
        }
    }

    /// Accept the editor (returns the entry + closes it).
    pub fn accept(&mut self) -> Option<CheatEdit> {
        let e = self.editor.take()?;
        Some(CheatEdit {
            comment: e.comment.trim().to_string(),
            code: e.code.trim().to_string(),
            editing: e.editing,
        })
    }

    /// Cancel the editor (closes it, no entry).
    pub fn cancel_editor(&mut self) {
        self.editor = None;
    }
}

/// The decoded-effect string (Advanced column): GameShark `(C10A)=FF`, Game Genie
/// `ROM (addr)=val [?cmp]`.
fn decoded(code: &str) -> String {
    match parse_code(code) {
        Some(Effect::Ram { addr, value }) => format!("({addr:04X})={value:02X}"),
        Some(Effect::Rom {
            addr,
            value,
            compare: Some(c),
        }) => {
            format!("ROM ({addr:04X})={value:02X} ?{c:02X}")
        }
        Some(Effect::Rom {
            addr,
            value,
            compare: None,
        }) => format!("ROM ({addr:04X})={value:02X}"),
        None => "(bad code)".to_string(),
    }
}

/// The centred dialog panel.
fn panel(area: Rect) -> Rect {
    let (w, h) = (420, 300);
    Rect::new(
        area.x + (area.w - w).max(0) / 2,
        area.y + (area.h - h).max(0) / 2,
        w,
        h,
    )
}

/// The 12 button rects (3 rows of 4) along the bottom of `p`.
fn button_rects(p: Rect) -> Vec<Rect> {
    let lh = line_height();
    let (pad, gap) = (6, 3);
    let (cols, rows) = (4, 3);
    let bh = lh + 4;
    let bw = (p.w - 2 * pad - (cols - 1) * gap) / cols;
    let grid_top = p.bottom() - pad - rows * bh - (rows - 1) * gap;
    (0..12)
        .map(|i| {
            let (col, row) = (i % cols, i / cols);
            Rect::new(
                p.x + pad + col * (bw + gap),
                grid_top + row * (bh + gap),
                bw,
                bh,
            )
        })
        .collect()
}

fn list_top(p: Rect) -> i32 {
    p.y + 6 + line_height() + 2
}

fn list_bottom(p: Rect) -> i32 {
    button_rects(p).first().map_or(p.bottom(), |r| r.y - 4)
}

/// Draw the dialog: title, cheat rows (`[x] CODE  [decoded]  comment`, decoded
/// only in Advanced; selected row highlighted), the button grid, and the two-
/// field Add/Edit editor on top if open.
pub fn render(c: &mut Canvas, d: &CheatDialog, cheats: &crate::cheat::CheatList, theme: &Theme) {
    let area = c.bounds();
    let p = panel(area);
    c.fill_rect(p, theme.bg);
    c.outline_rect(p, theme.border);
    let lh = line_height();
    let pad = 6;
    let adv = if d.advanced { "  [Advanced]" } else { "" };
    draw_text(
        c,
        p.x + pad,
        p.y + pad,
        &format!("Cheats ({}){adv}", cheats.len()),
        theme.text,
    );

    let (top, bottom) = (list_top(p), list_bottom(p));
    if cheats.is_empty() {
        draw_text(
            c,
            p.x + pad,
            top,
            "(no cheats — Add a GameShark or Game Genie code)",
            theme.text,
        );
    }
    for (i, ch) in cheats.items().iter().enumerate() {
        let y = top + i as i32 * lh;
        if y + lh > bottom {
            break;
        }
        let mark = if ch.enabled { 'x' } else { ' ' };
        let line = if d.advanced {
            format!(
                "[{mark}] {}  {}  {}",
                ch.code,
                decoded(&ch.code),
                ch.comment
            )
        } else {
            format!("[{mark}] {}  {}", ch.code, ch.comment)
        };
        let fg = if i == d.sel {
            c.fill_rect(Rect::new(p.x + 2, y, p.w - 4, lh), theme.current);
            theme.bg
        } else {
            theme.text
        };
        draw_text(c, p.x + pad, y, &line, fg);
    }

    for (r, (label, btn)) in button_rects(p).into_iter().zip(BUTTONS) {
        let pressed = btn == CheatButton::Advanced && d.advanced;
        button(c, r, label, pressed, theme);
    }

    if let Some(e) = &d.editor {
        render_editor(c, area, e, theme);
    }
}

/// bgb's two-field Add/Edit dialog: a Comment box then a Code box (the focused
/// one framed in the accent colour) + OK/Cancel hints.
fn render_editor(c: &mut Canvas, area: Rect, e: &Editor, theme: &Theme) {
    let lh = line_height();
    let (w, h) = (300, 6 * lh);
    let x = area.x + (area.w - w).max(0) / 2;
    let y = area.y + (area.h - h).max(0) / 2;
    let box_ = Rect::new(x, y, w, h);
    c.fill_rect(box_, theme.bg);
    c.outline_rect(box_, theme.hilight);
    let pad = 6;
    let field_w = w - 2 * pad;
    let mut draw_field = |cy: i32, label: &str, text: &str, focused: bool| {
        draw_text(c, x + pad, cy, label, theme.text);
        let fr = Rect::new(x + pad, cy + lh, field_w, lh);
        c.outline_rect(fr, if focused { theme.hilight } else { theme.border });
        let shown = if focused {
            format!("{text}_")
        } else {
            text.to_string()
        };
        draw_text(c, fr.x + 2, fr.y, &shown, theme.text);
    };
    draw_field(y + pad, "Comment", &e.comment, e.field == Field::Comment);
    draw_field(
        y + pad + 2 * lh + 2,
        "Code",
        &e.code,
        e.field == Field::Code,
    );
    draw_text(
        c,
        x + pad,
        box_.bottom() - pad - lh,
        "Enter=OK  Tab=switch  Esc=cancel",
        theme.text,
    );
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
