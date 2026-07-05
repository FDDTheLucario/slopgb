use super::font::{self, GLYPH_H, GLYPH_W};

#[test]
fn font_table_is_complete_and_sized() {
    assert_eq!(GLYPH_W, 7);
    assert_eq!(GLYPH_H, 13);
    assert_eq!(font::GLYPHS.len(), 95); // 0x20..=0x7E inclusive
}

#[test]
fn space_is_blank_and_printables_have_ink() {
    assert!(
        font::glyph(' ').iter().all(|&r| r == 0),
        "space must be blank"
    );
    for ch in ['A', '#', 'g', '0', '~'] {
        assert!(
            font::glyph(ch).iter().any(|&r| r != 0),
            "{ch:?} should have set pixels"
        );
    }
}

#[test]
fn glyph_indexes_ascii_by_codepoint() {
    assert_eq!(font::glyph(' '), &font::GLYPHS[0]);
    assert_eq!(font::glyph('A'), &font::GLYPHS[0x41 - 0x20]);
    assert_eq!(font::glyph('~'), &font::GLYPHS[94]);
}

#[test]
fn non_printable_falls_back_to_question_mark() {
    let q = font::glyph('?');
    assert_eq!(font::glyph('\n'), q);
    assert_eq!(font::glyph('€'), q); // non-ASCII
    assert_eq!(font::glyph('\u{7F}'), q); // DEL, just past the range
}
