//! Popup (context / dropdown) menu widget for the bgb-style debugger windows
//! (right-click-menu plan RM1/RM2). bgb draws every menu the same way — a
//! bordered white box, one row per item: an optional left check-mark, the
//! label, and a right-aligned shortcut (`Ctrl+G`, `F2`, `*`) or a submenu
//! arrow. Like the other [`widgets`](crate::ui::widgets), this is a stateless
//! draw plus pure hit-rects: the window owns the item list + which row is
//! hovered and passes them in, and [`item_at`] maps a click to an enabled row.

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::font::{GLYPH_H, GLYPH_W};
use crate::ui::text::{draw_text, line_height, measure};

/// One row of a [`menu`](menu_rects): a label with optional decorations, or a
/// separator divider. Built fluently — `MenuItem::new("Go to…").shortcut("Ctrl+G")`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    /// Row text (empty for a [separator](MenuItem::separator)).
    pub label: String,
    /// Right-aligned accelerator / marker (`Ctrl+G`, `F2`, `*`), if any.
    pub shortcut: Option<String>,
    /// A disabled row draws greyed and is skipped by [`item_at`].
    pub enabled: bool,
    /// A thin divider line, no label; never selectable.
    pub separator: bool,
    /// A left check-mark (bgb's toggled items, e.g. `Enable sound`).
    pub checked: bool,
    /// A right-aligned submenu arrow (bgb's `State▶`, `Other▶`, …).
    pub submenu: bool,
}

impl MenuItem {
    /// A plain enabled item with the given label.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
            enabled: true,
            separator: false,
            checked: false,
            submenu: false,
        }
    }

    /// Attach a right-aligned shortcut / marker string.
    #[must_use]
    pub fn shortcut(mut self, s: impl Into<String>) -> Self {
        self.shortcut = Some(s.into());
        self
    }

    /// Mark this item disabled (drawn greyed, not selectable).
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Show a left check-mark when `on`.
    #[must_use]
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }

    /// Show a right-aligned submenu arrow (the main-window menu's State/Other/…
    /// rows).
    #[must_use]
    pub fn submenu(mut self) -> Self {
        self.submenu = true;
        self
    }

    /// A non-selectable divider line.
    // TODO(MB1): consumed by the menu-bar dropdowns (e.g. the Debug menu).
    #[allow(dead_code)]
    #[must_use]
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            shortcut: None,
            enabled: false,
            separator: true,
            checked: false,
            submenu: false,
        }
    }
}

/// 1px menu border.
const BORDER: i32 = 1;
/// Left column reserved for a check-mark (keeps every label indented alike).
const MARK_W: i32 = GLYPH_W as i32 + 3;
/// Minimum gap between a label and its right-aligned shortcut/arrow.
const GAP: i32 = 2 * GLYPH_W as i32;
/// Right padding inside the border.
const PAD_R: i32 = GLYPH_W as i32;
/// A separator row's height (gap, line, gap).
const SEP_H: i32 = 5;
/// Submenu-arrow width (right column).
const ARROW_W: i32 = 5;

/// Height of one item row (text rows get a 2px vertical pad; separators are thin).
fn item_h(item: &MenuItem) -> i32 {
    if item.separator {
        SEP_H
    } else {
        line_height() + 2
    }
}

/// Right-column width an item needs: its shortcut text, an arrow, or nothing.
fn tail_w(item: &MenuItem) -> i32 {
    if let Some(s) = &item.shortcut {
        measure(s)
    } else if item.submenu {
        ARROW_W
    } else {
        0
    }
}

/// Total menu box width: the mark column + the widest label + (when any row has
/// a shortcut/arrow) the gap + widest tail, padded inside the border.
#[must_use]
pub fn menu_width(items: &[MenuItem]) -> i32 {
    let max_label = items
        .iter()
        .filter(|i| !i.separator)
        .map(|i| measure(&i.label))
        .max()
        .unwrap_or(0);
    let max_tail = items.iter().map(tail_w).max().unwrap_or(0);
    let tail = if max_tail > 0 { GAP + max_tail } else { 0 };
    2 * BORDER + MARK_W + max_label + tail + PAD_R
}

/// Total menu box height: the border plus every row's height.
#[must_use]
pub fn menu_height(items: &[MenuItem]) -> i32 {
    2 * BORDER + items.iter().map(item_h).sum::<i32>()
}

/// The whole menu box rect with its top-left at `origin` — for click-away
/// dismissal (a click outside it closes the menu) and the border.
#[must_use]
pub fn menu_bounds(origin: (i32, i32), items: &[MenuItem]) -> Rect {
    Rect::new(origin.0, origin.1, menu_width(items), menu_height(items))
}

/// Per-item hit-rects, each spanning the full menu width, stacked from `origin`
/// — the pure geometry [`render`] draws over, so a window can map a click
/// without a [`Canvas`].
#[must_use]
pub fn menu_rects(origin: (i32, i32), items: &[MenuItem]) -> Vec<Rect> {
    let w = menu_width(items);
    let mut y = origin.1 + BORDER;
    let mut rects = Vec::with_capacity(items.len());
    for it in items {
        let h = item_h(it);
        rects.push(Rect::new(origin.0, y, w, h));
        y += h;
    }
    rects
}

/// The index of the enabled, non-separator item containing `(px, py)`, if any —
/// what a click or hover resolves to (separators and disabled rows return
/// `None`).
#[must_use]
pub fn item_at(origin: (i32, i32), items: &[MenuItem], px: i32, py: i32) -> Option<usize> {
    menu_rects(origin, items)
        .iter()
        .enumerate()
        .find(|(i, r)| r.contains(px, py) && !items[*i].separator && items[*i].enabled)
        .map(|(i, _)| i)
}

/// Draw a small check-mark (a tick) in the mark column at row top `(x, y)`.
fn draw_check(c: &mut Canvas, x: i32, y: i32, color: u32) {
    let cy = y + GLYPH_H as i32 / 2;
    for k in 0..3 {
        c.put(x + 1 + k, cy + k, color); // short down-right stroke
    }
    for k in 0..5 {
        c.put(x + 3 + k, cy + 2 - k, color); // longer up-right stroke
    }
}

/// Draw a small right-pointing triangle (submenu arrow) centred on `ymid` with
/// its left edge at `x`.
fn draw_arrow(c: &mut Canvas, x: i32, ymid: i32, color: u32) {
    for k in 0..4 {
        let span = 3 - k; // widest at the left, narrowing to a point on the right
        c.vline(x + k, ymid - span, 2 * span + 1, color);
    }
}

/// Render a popup menu with its top-left at `origin`; `hovered` (an index into
/// `items`) gets the highlight bar. bgb-style: white box, grey border, label
/// left after a mark column, shortcut/arrow right-aligned, separators as thin
/// lines, disabled rows greyed.
pub fn render(
    c: &mut Canvas,
    origin: (i32, i32),
    items: &[MenuItem],
    hovered: Option<usize>,
    theme: &Theme,
) {
    let bounds = menu_bounds(origin, items);
    c.fill_rect(bounds, theme.bg);
    theme.frame(c, bounds, theme.border);
    let rects = menu_rects(origin, items);
    for (i, (it, r)) in items.iter().zip(&rects).enumerate() {
        if it.separator {
            let sy = r.y + r.h / 2;
            c.hline(r.x + 2, sy, r.w - 4, theme.border);
            continue;
        }
        let hot = it.enabled && hovered == Some(i);
        let fg = if !it.enabled {
            theme.disabled_text
        } else if hot {
            c.fill_rect(*r, theme.selection_bg);
            theme.selection_fg
        } else {
            theme.text
        };
        let ty = r.y + (r.h - GLYPH_H as i32) / 2;
        if it.checked {
            draw_check(c, r.x + BORDER, r.y, fg);
        }
        draw_text(c, r.x + BORDER + MARK_W, ty, &it.label, fg);
        if let Some(s) = &it.shortcut {
            draw_text(c, r.right() - PAD_R - measure(s), ty, s, fg);
        } else if it.submenu {
            draw_arrow(c, r.right() - PAD_R - ARROW_W, r.y + r.h / 2, fg);
        }
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
