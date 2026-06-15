use super::*;
use crate::ui::font;

const FG: u32 = 0x00_FF_FF_FF;

#[test]
fn measure_and_line_height() {
    assert_eq!(measure(""), 0);
    assert_eq!(measure("Hi"), 2 * GLYPH_W as i32);
    assert_eq!(measure("abcde"), 5 * GLYPH_W as i32);
    assert_eq!(line_height(), GLYPH_H as i32);
}

#[test]
fn draw_text_blits_each_glyph_at_its_column_and_advances() {
    let cw = GLYPH_W * 2;
    let mut buf = vec![0u32; cw * GLYPH_H];
    let end;
    {
        let mut c = Canvas::new(&mut buf, cw, GLYPH_H);
        end = draw_text(&mut c, 0, 0, "Ai", FG);
    }
    assert_eq!(end, 2 * GLYPH_W as i32, "advances by one cell per glyph");
    // Column 0 must match the 'A' bitmap exactly; column 1 the 'i' bitmap.
    for (col_base, ch) in [(0, 'A'), (GLYPH_W, 'i')] {
        for (row, &bits) in font::glyph(ch).iter().enumerate() {
            for col in 0..GLYPH_W {
                let lit = bits & (1 << (7 - col)) != 0;
                let px = buf[row * cw + col_base + col];
                assert_eq!(px == FG, lit, "{ch:?} pixel ({col},{row})");
            }
        }
    }
}

#[test]
fn draw_text_clips_off_screen_without_panicking() {
    let mut buf = vec![0u32; GLYPH_W * GLYPH_H];
    let mut c = Canvas::new(&mut buf, GLYPH_W, GLYPH_H);
    // Negative origin and a string far wider than the surface: must not panic.
    let _ = draw_text(&mut c, -3, -2, "overflow!!", FG);
    // A glyph fully past the right edge writes nothing there (only in-bounds).
    let _ = draw_text(&mut c, 100, 0, "X", FG);
}
