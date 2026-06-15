//! `mod_tests` — oam_bug tests (split for file size).

use super::*;

/// blargg oam_bug/4-scanline_timing + 5-timing_bug pin the corruptible
/// window in M-cycle units: the access covering dots 0-3 of a visible
/// line corrupts the first row and the one covering dots 72-75 the
/// last, while 76-79 (and everything later) is clean. Under
/// tick-then-access the accessing CPU observes state(T) with the cycle
/// covering T-4..T, so rows 8..=0x98 map to T in 4..80.
#[test]
fn oam_bug_row_window_tracks_scan() {
    let mut p = dmg();
    assert_eq!(p.oam_bug_row(), None, "LCD off");
    p.write(0xFF40, 0x81);
    // Glitch line: no OAM scan (lcdon_timing-GS), never vulnerable.
    for _ in 0..GLITCH_LINE_DOTS {
        assert_eq!(p.oam_bug_row(), None, "glitch line dot {}", p.dot);
        p.tick();
    }
    // Steady visible line: rows step every 4 dots through 4..80.
    for line in [1u8, 2, 143] {
        run_to(&mut p, line, 0);
        for dot in 0..456u16 {
            let expect = if (4..80).contains(&dot) {
                Some((dot / 4 * 8) as u8)
            } else {
                None
            };
            assert_eq!(p.oam_bug_row(), expect, "line {line} dot {dot}");
            p.tick();
        }
    }
    // VBlank lines never scan.
    run_to(&mut p, 144, 0);
    for _ in 0..456 {
        assert_eq!(p.oam_bug_row(), None, "vblank dot {}", p.dot);
        p.tick();
    }
}

#[test]
fn oam_bug_write_pattern_formula() {
    // Dot 16 -> row 0x20 (row 4).
    let mut p = oam_bug_ppu(1, 16);
    let before = p.oam;
    p.oam_bug(OamBugKind::Write);
    let row = 0x20;
    for i in 0..2 {
        let (a, b, c) = (before[row + i], before[row - 8 + i], before[row - 4 + i]);
        assert_eq!(p.oam[row + i], ((a ^ c) & (b ^ c)) ^ c, "glitched byte {i}");
    }
    for i in 2..8 {
        assert_eq!(p.oam[row + i], before[row - 8 + i], "copied byte {i}");
    }
    for (i, &byte) in p.oam.iter().enumerate() {
        if !(row..row + 8).contains(&i) {
            assert_eq!(byte, before[i], "byte {i} outside the row untouched");
        }
    }
}

#[test]
fn oam_bug_write_pattern_first_row_references_row_zero() {
    // Dot 4 -> row 8: operands come from row 0, which stays intact.
    let mut p = oam_bug_ppu(1, 4);
    let before = p.oam;
    p.oam_bug(OamBugKind::Write);
    let (a, b, c) = (before[8], before[0], before[4]);
    assert_eq!(p.oam[8], ((a ^ c) & (b ^ c)) ^ c);
    assert_eq!(p.oam[..8], before[..8], "row 0 untouched");
}

#[test]
fn oam_bug_read_pattern_formula() {
    let mut p = oam_bug_ppu(1, 16);
    let before = p.oam;
    p.oam_bug(OamBugKind::Read);
    let row = 0x20;
    for i in 0..2 {
        let (a, b, c) = (before[row + i], before[row - 8 + i], before[row - 4 + i]);
        let glitched = b | (a & c);
        assert_eq!(p.oam[row + i], glitched, "current row byte {i}");
        assert_eq!(p.oam[row - 8 + i], glitched, "preceding row byte {i}");
    }
    for i in 2..8 {
        assert_eq!(p.oam[row + i], before[row - 8 + i], "copied byte {i}");
        assert_eq!(p.oam[row - 8 + i], before[row - 8 + i], "prev tail intact");
    }
}

#[test]
fn oam_bug_read_pattern_on_uniform_oam_is_invisible() {
    // blargg 3-non_causes tolerates read corruption only because
    // b | (a & c) is the identity on uniform data.
    let mut p = oam_bug_ppu(1, 16);
    p.oam = [0x5A; 0xA0];
    p.oam_bug(OamBugKind::Read);
    assert_eq!(p.oam, [0x5A; 0xA0]);
}

#[test]
fn oam_bug_read_increase_pattern_at_row_4_and_up() {
    let mut p = oam_bug_ppu(1, 16);
    let before = p.oam;
    p.oam_bug(OamBugKind::ReadIncrease);
    let row = 0x20;
    // Glitched first word lands in the *preceding* row, then that row
    // (glitched word included) is copied to both the current row and
    // two rows back (SameBoy v0.12.1 GB_trigger_oam_bug_read_increase;
    // the trailing plain read corruption is a no-op after the copy).
    let mut expect_prev = [0u8; 8];
    expect_prev.copy_from_slice(&before[row - 8..row]);
    for i in 0..2 {
        let (a, b, c, d) = (
            before[row - 0x10 + i],
            before[row - 8 + i],
            before[row + i],
            before[row - 4 + i],
        );
        expect_prev[i] = (b & (a | c | d)) | (a & c & d);
    }
    for (i, &expect) in expect_prev.iter().enumerate() {
        assert_eq!(p.oam[row - 0x10 + i], expect, "two rows back {i}");
        assert_eq!(p.oam[row - 8 + i], expect, "preceding row {i}");
        assert_eq!(p.oam[row + i], expect, "current row {i}");
    }
    for (i, &byte) in p.oam.iter().enumerate() {
        if !(row - 0x10..row + 8).contains(&i) {
            assert_eq!(byte, before[i], "byte {i} outside the rows untouched");
        }
    }
}

#[test]
fn oam_bug_read_increase_in_first_rows_is_plain_read() {
    // Rows 1..=3 (and the last row) skip the special pattern: SameBoy
    // v0.12.1 guards 0x20 <= row < 0x98. Dot 8 -> row 0x10.
    let mut p = oam_bug_ppu(1, 8);
    let mut reference = oam_bug_ppu(1, 8);
    p.oam_bug(OamBugKind::ReadIncrease);
    reference.oam_bug(OamBugKind::Read);
    assert_eq!(p.oam, reference.oam);

    // Dot 76 -> row 0x98 (the last row): also plain read only.
    let mut p = oam_bug_ppu(1, 76);
    let mut reference = oam_bug_ppu(1, 76);
    p.oam_bug(OamBugKind::ReadIncrease);
    reference.oam_bug(OamBugKind::Read);
    assert_eq!(p.oam, reference.oam);
}

#[test]
fn oam_bug_outside_window_is_a_no_op() {
    for dot in [0u16, 80, 200, 300] {
        let mut p = oam_bug_ppu(1, dot);
        let before = p.oam;
        p.oam_bug(OamBugKind::Write);
        p.oam_bug(OamBugKind::Read);
        p.oam_bug(OamBugKind::ReadIncrease);
        assert_eq!(p.oam, before, "dot {dot}");
    }
}
