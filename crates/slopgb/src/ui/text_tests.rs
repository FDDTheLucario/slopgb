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
    // Each pixel is the glyph's coverage blended over the black bg: white FG
    // over black gives a gray whose channels equal the coverage value.
    for (col_base, ch) in [(0, 'A'), (GLYPH_W, 'i')] {
        let g = font::glyph(ch);
        for row in 0..GLYPH_H {
            for col in 0..GLYPH_W {
                let cov = g[row * GLYPH_W + col] as u32;
                let expect = (cov << 16) | (cov << 8) | cov;
                assert_eq!(
                    buf[row * cw + col_base + col],
                    expect,
                    "{ch:?} ({col},{row})"
                );
            }
        }
    }
}

#[test]
fn draw_text_is_antialiased_not_one_bit() {
    // A rendered glyph must contain a partial-coverage pixel (neither bg nor full
    // fg) — the whole point of the AA font.
    let mut buf = vec![0u32; GLYPH_W * GLYPH_H];
    {
        let mut c = Canvas::new(&mut buf, GLYPH_W, GLYPH_H);
        draw_text(&mut c, 0, 0, "e", FG);
    }
    let partial = buf.iter().any(|&p| {
        let ch = p & 0xFF;
        ch != 0x00 && ch != 0xFF
    });
    assert!(partial, "AA text must have partial-coverage pixels");
}

#[test]
fn hex_row_matches_bgb_memory_format() {
    let bytes: Vec<u8> = (0..16).collect();
    assert_eq!(
        hex_row("MEM:0000", &bytes),
        "MEM:0000 00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  |................|"
    );
    // Printable bytes appear in the ASCII gutter; others are '.'; missing
    // tail bytes pad with spaces. The gutter is always 16 wide, framed by |…|.
    let mixed = [0x41u8, 0x42, 0x00, 0x7E];
    let row = hex_row("X", &mixed);
    assert!(row.starts_with("X 41 42 00 7E"), "got {row:?}");
    let gutter = row.rsplit('|').nth(1).expect("gutter between pipes");
    assert_eq!(gutter.len(), 16);
    assert!(gutter.starts_with("AB.~"));
    assert!(gutter[4..].chars().all(|ch| ch == ' '), "tail padded");
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
