//! Immediate-mode widgets for the bgb-style tool windows. Each is a stateless
//! draw + hit-rect: the window owns the state (checked, pressed, …) and passes
//! it in, and the returned [`Rect`] is what a click is tested against. They
//! compose [`Canvas`] + [`text`](crate::ui::text) under a [`Theme`].

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::font::GLYPH_H;
use crate::ui::text::{draw_text, line_height, measure};

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

/// Horizontal radio group: a small box with a filled centre on the `selected`
/// option, each followed by its label (e.g. the BG-map source `Auto/9800/9C00`).
/// Returns each option's hit-rect, so a click maps to its index.
pub fn radio_group(
    c: &mut Canvas,
    x: i32,
    y: i32,
    options: &[&str],
    selected: usize,
    theme: &Theme,
) -> Vec<Rect> {
    let dot = GLYPH_H as i32 - 4;
    let mut rects = Vec::with_capacity(options.len());
    let mut cx = x;
    for (i, opt) in options.iter().enumerate() {
        c.fill_rect(Rect::new(cx, y, dot, dot), theme.bg);
        c.outline_rect(Rect::new(cx, y, dot, dot), theme.text);
        if i == selected {
            c.fill_rect(Rect::new(cx + 2, y + 2, dot - 4, dot - 4), theme.text);
        }
        let end = draw_text(c, cx + dot + 2, y, opt, theme.text);
        rects.push(Rect::new(cx, y, end - cx, dot));
        cx = end + 8; // gap before the next option
    }
    rects
}

/// A row of tabs (e.g. `BG map / Tiles / OAM / Palettes`); the `active` tab gets
/// a full outline. Returns each tab's hit-rect.
pub fn tab_strip(
    c: &mut Canvas,
    x: i32,
    y: i32,
    labels: &[&str],
    active: usize,
    theme: &Theme,
) -> Vec<Rect> {
    const PAD: i32 = 4;
    let h = GLYPH_H as i32 + 2;
    let mut rects = Vec::with_capacity(labels.len());
    let mut cx = x;
    for (i, lbl) in labels.iter().enumerate() {
        let r = Rect::new(cx, y, measure(lbl) + PAD * 2, h);
        if i == active {
            c.fill_rect(r, theme.bg);
            c.outline_rect(r, theme.text);
        }
        draw_text(c, cx + PAD, y + 1, lbl, theme.text);
        rects.push(r);
        cx += r.w + 2;
    }
    rects
}

/// A vertical slice of text `rows` into `rect`: `rows[offset..]` top-aligned,
/// one per [`line_height`], clipped to `rect`. `highlight` (an index into
/// `rows`) gets a full-width bar in `theme.current` with `theme.bg` text — the
/// disasm pane's current-PC line / the stack pane's SP row. Returns how many
/// rows were drawn. The caller computes `offset` (scroll position).
pub fn scroll_list(
    c: &mut Canvas,
    rect: Rect,
    rows: &[&str],
    offset: usize,
    highlight: Option<usize>,
    theme: &Theme,
) -> usize {
    let lh = line_height();
    let visible = (rect.h / lh).max(0) as usize;
    let saved = c.push_clip(rect);
    let mut drawn = 0;
    for i in 0..visible {
        let Some(text) = rows.get(offset + i) else {
            break;
        };
        let y = rect.y + i as i32 * lh;
        let fg = if Some(offset + i) == highlight {
            c.fill_rect(Rect::new(rect.x, y, rect.w, lh), theme.current);
            theme.bg
        } else {
            theme.text
        };
        draw_text(c, rect.x + 1, y, text, fg);
        drawn += 1;
    }
    c.set_clip(saved);
    drawn
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
