use super::*;

#[test]
fn next_mark_walks_and_wraps() {
    let marks = [0x0100u16, 0x0200];
    assert_eq!(next_mark(&marks, 0x0150, true), Some(0x0200));
    assert_eq!(next_mark(&marks, 0x0150, false), Some(0x0100));
    // Wrap at the ends.
    assert_eq!(next_mark(&marks, 0x0200, true), Some(0x0100));
    assert_eq!(next_mark(&marks, 0x0100, false), Some(0x0200));
    // On a mark, "next" advances strictly past it.
    assert_eq!(next_mark(&marks, 0x0100, true), Some(0x0200));
    // An empty set is a no-op.
    assert_eq!(next_mark(&[], 0x0150, true), None);
}

#[test]
fn find_match_hex_byte_sequence() {
    // 3E 01 at 0x0105.
    let read = |a: u16| match a {
        0x0105 => 0x3E,
        0x0106 => 0x01,
        _ => 0x00,
    };
    assert_eq!(find_match(read, 0x0100, "3E 01"), Some(0x0105));
    assert_eq!(find_match(read, 0x0100, "3E01"), Some(0x0105));
    // No such bytes anywhere -> None.
    assert_eq!(find_match(|_| 0u8, 0x0100, "AB CD"), None);
}

#[test]
fn find_match_mnemonic_substring_case_insensitive_and_wraps() {
    // 0x0100: 3E 01 decodes to "ld a,01".
    let read = |a: u16| match a {
        0x0100 => 0x3E,
        0x0101 => 0x01,
        _ => 0x00,
    };
    assert_eq!(find_match(read, 0x0100, "ld a,"), Some(0x0100));
    assert_eq!(find_match(read, 0x0100, "LD A,"), Some(0x0100));
    // Starting past it, the scan wraps back around to it.
    assert_eq!(find_match(read, 0x0200, "ld a,"), Some(0x0100));
}

#[test]
fn find_match_empty_query_is_none() {
    assert_eq!(find_match(|_| 0u8, 0x0100, "   "), None);
}
