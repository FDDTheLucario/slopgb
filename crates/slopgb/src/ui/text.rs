//! Text drawing: blit the embedded [`font`](crate::ui::font) glyphs onto a
//! [`Canvas`]. Fixed-width — every glyph advances by `GLYPH_W`, so column
//! layout is just `x + col * GLYPH_W`. All drawing clips through the canvas.

use crate::ui::canvas::Canvas;
use crate::ui::font::{self, GLYPH_H, GLYPH_W};

/// Draw one glyph with its top-left at `(x, y)`.
fn draw_glyph(c: &mut Canvas, x: i32, y: i32, ch: char, color: u32) {
    for (row, &bits) in font::glyph(ch).iter().enumerate() {
        for col in 0..GLYPH_W {
            if bits & (1 << (7 - col)) != 0 {
                c.put(x + col as i32, y + row as i32, color);
            }
        }
    }
}

/// Draw `text` left-to-right with its top-left at `(x, y)`. Returns the x just
/// past the last glyph (so callers can chain). Clipped by the canvas.
pub fn draw_text(c: &mut Canvas, x: i32, y: i32, text: &str, color: u32) -> i32 {
    let mut cx = x;
    for ch in text.chars() {
        draw_glyph(c, cx, y, ch, color);
        cx += GLYPH_W as i32;
    }
    cx
}

/// Pixel width [`draw_text`] would occupy: `char count * GLYPH_W`.
#[must_use]
pub fn measure(text: &str) -> i32 {
    text.chars().count() as i32 * GLYPH_W as i32
}

/// The fixed line height (one glyph cell).
#[must_use]
pub const fn line_height() -> i32 {
    GLYPH_H as i32
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
