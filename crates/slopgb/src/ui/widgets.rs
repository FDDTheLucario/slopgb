//! Immediate-mode widgets for the bgb-style tool windows. Each is a stateless
//! draw + hit-rect: the window owns the state (checked, pressed, …) and passes
//! it in, and the returned [`Rect`] is what a click is tested against. They
//! compose [`Canvas`] + [`text`](crate::ui::text) under a [`Theme`].

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::font::GLYPH_H;
use crate::ui::text::{draw_text, line_height, measure};

/// Hit-rect a [`checkbox`] at `(x, y)` with `label` occupies — the pure
/// geometry, so a window can map a click without a [`Canvas`]. Spans box + gap +
/// label, exactly what [`checkbox`] returns.
#[must_use]
pub fn checkbox_rect(x: i32, y: i32, label: &str) -> Rect {
    let box_sz = GLYPH_H as i32 - 2;
    Rect::new(x, y, box_sz + 3 + measure(label), box_sz)
}

/// Square check-box drawn at `(x, y)` with `label` to its right; a filled inner
/// square shows `checked`. Returns the clickable rect spanning box + label
/// (== [`checkbox_rect`]).
pub fn checkbox(c: &mut Canvas, x: i32, y: i32, checked: bool, label: &str, theme: &Theme) -> Rect {
    let box_sz = GLYPH_H as i32 - 2;
    c.fill_rect(Rect::new(x, y, box_sz, box_sz), theme.bg);
    c.outline_rect(Rect::new(x, y, box_sz, box_sz), theme.text);
    if checked {
        c.fill_rect(
            Rect::new(x + 2, y + 2, box_sz - 4, box_sz - 4),
            theme.accent,
        );
    }
    draw_text(c, x + box_sz + 3, y, label, theme.text);
    checkbox_rect(x, y, label)
}

/// Bordered button with a centred `label`; `pressed` swaps fill/text. Returns
/// `rect` (the hit area). Kept tested and ready for the modal dialog's OK/Cancel
/// (M5b) and the I/O map's Refresh button.
pub fn button(c: &mut Canvas, rect: Rect, label: &str, pressed: bool, theme: &Theme) -> Rect {
    let (fill, fg) = if pressed {
        (theme.text, theme.bg)
    } else {
        (theme.button_face, theme.text)
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

/// Hit-rects for a [`radio_group`] at `(x, y)` with `options` — the pure
/// geometry, so a window can map a click to an option index without a
/// [`Canvas`] (== what [`radio_group`] returns).
#[must_use]
pub fn radio_rects(x: i32, y: i32, options: &[&str]) -> Vec<Rect> {
    let dot = GLYPH_H as i32 - 4;
    let mut rects = Vec::with_capacity(options.len());
    let mut cx = x;
    for opt in options {
        let end = cx + dot + 2 + measure(opt);
        rects.push(Rect::new(cx, y, end - cx, dot));
        cx = end + 8; // gap before the next option
    }
    rects
}

/// Horizontal radio group: a small box with a filled centre on the `selected`
/// option, each followed by its label (e.g. the BG-map source `Auto/9800/9C00`).
/// Returns each option's hit-rect (== [`radio_rects`]), so a click maps to its
/// index.
pub fn radio_group(
    c: &mut Canvas,
    x: i32,
    y: i32,
    options: &[&str],
    selected: usize,
    theme: &Theme,
) -> Vec<Rect> {
    let dot = GLYPH_H as i32 - 4;
    let rects = radio_rects(x, y, options);
    for (i, (opt, r)) in options.iter().zip(&rects).enumerate() {
        c.fill_rect(Rect::new(r.x, y, dot, dot), theme.bg);
        c.outline_rect(Rect::new(r.x, y, dot, dot), theme.text);
        if i == selected {
            c.fill_rect(Rect::new(r.x + 2, y + 2, dot - 4, dot - 4), theme.accent);
        }
        draw_text(c, r.x + dot + 2, y, opt, theme.text);
    }
    rects
}

/// Tab padding (each side of the label) and the strip height — shared by the
/// pure [`tab_rects`] geometry and the [`tab_strip`] that draws over it.
const TAB_PAD: i32 = 4;

/// Hit-rects for a row of tabs at `(x, y)` with the given `labels` — the pure
/// geometry [`tab_strip`] draws over, so a window can map a click to a tab
/// without a [`Canvas`]. Each tab is `measure(label) + 2*PAD` wide; the next
/// starts `w + 2` to the right.
#[must_use]
pub fn tab_rects(x: i32, y: i32, labels: &[&str]) -> Vec<Rect> {
    let h = GLYPH_H as i32 + 2;
    let mut rects = Vec::with_capacity(labels.len());
    let mut cx = x;
    for lbl in labels {
        let r = Rect::new(cx, y, measure(lbl) + TAB_PAD * 2, h);
        cx += r.w + 2;
        rects.push(r);
    }
    rects
}

/// A row of tabs (e.g. `BG map / Tiles / OAM / Palettes`); the `active` tab gets
/// a full outline. Returns each tab's hit-rect (== [`tab_rects`]).
pub fn tab_strip(
    c: &mut Canvas,
    x: i32,
    y: i32,
    labels: &[&str],
    active: usize,
    theme: &Theme,
) -> Vec<Rect> {
    let rects = tab_rects(x, y, labels);
    for (i, (lbl, r)) in labels.iter().zip(&rects).enumerate() {
        if i == active {
            c.fill_rect(*r, theme.bg);
            c.outline_rect(*r, theme.text);
        }
        draw_text(c, r.x + TAB_PAD, r.y + 1, lbl, theme.text);
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

/// Width (px) of a vertical scrollbar's track and thumb.
pub const SCROLLBAR_W: i32 = 8;

/// The right-edge track rect a vertical scrollbar occupies within `pane`. Shared
/// by [`vscrollbar`] (draw) and the drag hit-test so they can't disagree.
#[must_use]
pub fn vscroll_track(pane: Rect) -> Rect {
    Rect::new(pane.right() - SCROLLBAR_W, pane.y, SCROLLBAR_W, pane.h)
}

/// `pane` minus the scrollbar strip — the rect a scrollable pane should render
/// its content into so text doesn't run under the bar.
#[must_use]
pub fn scroll_content(pane: Rect) -> Rect {
    Rect::new(pane.x, pane.y, (pane.w - SCROLLBAR_W).max(0), pane.h)
}

/// Thumb height (px) for a track `h` px tall showing `vis` (0..1) of the content,
/// floored to `2*SCROLLBAR_W` so it stays grabbable over a huge address space.
fn thumb_h(track_h: i32, vis: f32) -> i32 {
    let min = (SCROLLBAR_W * 2).min(track_h.max(1));
    ((vis.clamp(0.0, 1.0) * track_h as f32) as i32).clamp(min, track_h.max(1))
}

/// Draw a vertical scrollbar in `pane`'s right-edge track: a dim full-height
/// track with a brighter thumb sized to `vis` (0..1 of content visible) and
/// positioned at `frac` (0..1 of the scroll travel). Returns the track rect.
pub fn vscrollbar(c: &mut Canvas, pane: Rect, frac: f32, vis: f32, theme: &Theme) -> Rect {
    let track = vscroll_track(pane);
    c.fill_rect(track, theme.border);
    let th = thumb_h(track.h, vis);
    let travel = (track.h - th).max(0);
    let ty = track.y + (frac.clamp(0.0, 1.0) * travel as f32) as i32;
    c.fill_rect(Rect::new(track.x, ty, SCROLLBAR_W, th), theme.scrollbar);
    track
}

/// The scroll fraction (0..1) a cursor at `py` maps to within `track`, sized so
/// the thumb (per `vis`) centers on the cursor and the extremes reach 0.0 / 1.0.
#[must_use]
pub fn vscroll_frac(track: Rect, py: i32, vis: f32) -> f32 {
    let th = thumb_h(track.h, vis);
    let travel = (track.h - th).max(1);
    ((py - track.y - th / 2) as f32 / travel as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
