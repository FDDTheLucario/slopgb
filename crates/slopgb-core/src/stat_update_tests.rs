//! Unit tests for the SameBoy `GB_STAT_update` rising-edge core
//! ([`super::StatUpdate`]). Pin the level OR, the 0→1 edge detection, the
//! STAT-blocking "already high → no re-fire", and the `mode_for_interrupt`
//! source selection against `display.c:523-560`.

use super::*;

// STAT enable bits, for readable test fixtures.
const EN_HBLANK: u8 = 0x08;
const EN_VBLANK: u8 = 0x10;
const EN_OAM: u8 = 0x20;
const EN_LYC: u8 = 0x40;

#[test]
fn level_selects_the_one_mode_source_by_interrupt_mode() {
    // Each mode picks exactly its own enable bit (display.c:545-550).
    assert!(StatUpdate::level(0, EN_HBLANK, false));
    assert!(!StatUpdate::level(0, EN_OAM, false), "mode 0 ignores the OAM enable");
    assert!(StatUpdate::level(1, EN_VBLANK, false));
    assert!(StatUpdate::level(2, EN_OAM, false));
    assert!(!StatUpdate::level(2, EN_HBLANK, false), "mode 2 ignores the HBlank enable");
}

#[test]
fn level_mode_three_and_none_select_no_mode_source() {
    // Mode 3 and the -1/NONE sentinel are the display.c `default:` arm: no
    // mode source, only LYC can hold the line.
    assert!(!StatUpdate::level(3, !EN_LYC, false), "mode 3: no mode source");
    assert!(
        !StatUpdate::level(MODE_FOR_INTERRUPT_NONE, EN_HBLANK | EN_OAM | EN_VBLANK, false),
        "NONE: no mode source even with every mode enable set"
    );
    // ...but LYC still works through the NONE state.
    assert!(StatUpdate::level(MODE_FOR_INTERRUPT_NONE, EN_LYC, true));
}

#[test]
fn level_or_s_the_lyc_source() {
    // LYC contributes only when both its enable and the match are set.
    assert!(!StatUpdate::level(3, EN_LYC, false), "enable set, no match");
    assert!(!StatUpdate::level(3, 0, true), "match, no enable");
    assert!(StatUpdate::level(3, EN_LYC, true), "enable + match");
}

#[test]
fn rising_edge_fires_once_then_stays_silent() {
    let mut s = StatUpdate::new();
    assert!(!s.line());
    // Mode-0 source goes high: 0→1 edge fires.
    assert!(s.update(0, EN_HBLANK, false), "first rise raises IF");
    assert!(s.line());
    // Still high next dot: no new interrupt (the level is unchanged).
    assert!(!s.update(0, EN_HBLANK, false), "already high → no re-fire");
    assert!(s.line());
}

#[test]
fn stat_blocking_a_second_source_joining_does_not_refire() {
    // The classic STAT blocking case: the line is already high from the mode-0
    // source; LYC then also goes high. No new rising edge (display.c:557).
    let mut s = StatUpdate::new();
    assert!(s.update(0, EN_HBLANK | EN_LYC, false), "mode-0 source raises the line");
    assert!(
        !s.update(0, EN_HBLANK | EN_LYC, true),
        "LYC joining an already-high line does not re-fire"
    );
    assert!(s.line());
}

#[test]
fn line_refires_after_a_fall() {
    let mut s = StatUpdate::new();
    assert!(s.update(2, EN_OAM, false), "rise");
    // Source disabled: the line falls (no edge on a fall).
    assert!(!s.update(2, 0, false), "fall raises nothing");
    assert!(!s.line());
    // It comes back: a fresh 0→1 edge fires again.
    assert!(s.update(2, EN_OAM, false), "re-rise fires again");
}

#[test]
fn lyc_source_can_hold_the_line_across_a_mode_change() {
    // With LYC high throughout, switching the interrupt mode does not produce
    // a new edge — the line never falls (no double interrupt across modes).
    let mut s = StatUpdate::new();
    assert!(s.update(2, EN_OAM | EN_LYC, true), "OAM+LYC rise");
    assert!(
        !s.update(MODE_FOR_INTERRUPT_NONE, EN_OAM | EN_LYC, true),
        "LYC holds the line high through the no-mode gap"
    );
    assert!(s.line(), "line stayed high on LYC alone");
}

#[test]
fn a_disabled_source_never_raises_the_line() {
    let mut s = StatUpdate::new();
    // Mode 0 active but its enable bit is clear: no line, no interrupt.
    assert!(!s.update(0, EN_OAM | EN_VBLANK | EN_LYC, false));
    assert!(!s.line());
}
