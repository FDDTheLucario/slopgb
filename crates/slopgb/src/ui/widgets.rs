//! Immediate-mode widgets for the bgb-style tool windows. Each is a stateless
//! draw + hit-rect: the window owns the state (checked, pressed, …) and passes
//! it in, and the returned [`Rect`] is what a click is tested against. They
//! compose [`Canvas`] + [`text`](crate::ui::text) under a [`Theme`].

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::font::GLYPH_H;
use crate::ui::text::{draw_text, measure};

/// Square check-box drawn at `(x, y)` with `label` to its right; a filled inner
/// square shows `checked`. Returns the clickable rect spanning box + label.
pub fn checkbox(c: &mut Canvas, x: i32, y: i32, checked: bool, label: &str, theme: &Theme) -> Rect {
    let box_sz = GLYPH_H as i32 - 2;
    c.fill_rect(Rect::new(x, y, box_sz, box_sz), theme.bg);
    c.outline_rect(Rect::new(x, y, box_sz, box_sz), theme.text);
    if checked {
        c.fill_rect(Rect::new(x + 2, y + 2, box_sz - 4, box_sz - 4), theme.text);
    }
    let end = draw_text(c, x + box_sz + 3, y, label, theme.text);
    Rect::new(x, y, end - x, box_sz)
}

/// Bordered button with a centred `label`; `pressed` swaps fill/text. Returns
/// `rect` (the hit area).
pub fn button(c: &mut Canvas, rect: Rect, label: &str, pressed: bool, theme: &Theme) -> Rect {
    let (fill, fg) = if pressed {
        (theme.text, theme.bg)
    } else {
        (theme.bg, theme.text)
    };
    c.fill_rect(rect, fill);
    c.outline_rect(rect, theme.text);
    let tx = rect.x + (rect.w - measure(label)) / 2;
    let ty = rect.y + (rect.h - GLYPH_H as i32) / 2;
    draw_text(c, tx, ty, label, fg);
    rect
}

/// Filled colour swatch with a 1px border (the Palettes tab's colour cells).
pub fn swatch(c: &mut Canvas, rect: Rect, color: u32, theme: &Theme) {
    c.fill_rect(rect, color);
    c.outline_rect(rect, theme.text);
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
