//! Timer tests: the 8 kHz (T0/T1) and 64 kHz (T2) dividers, the 4-bit
//! read-and-clear output, target-0-means-256, enable-edge reset, and the
//! disabled-timer freeze. Driven through the real I/O path (`$F1`, `$FA-$FF`)
//! on a non-flat CPU.

use super::*;

/// A production-mode CPU (real I/O decode, IPL enabled).
fn apu() -> Spc700 {
    Spc700::new()
}

#[test]
fn timer2_runs_at_64khz() {
    let mut s = apu();
    s.write8(0x00FC, 2); // T2 target = 2
    s.write8(0x00F1, 0x04); // enable T2 (control bit 2)
    s.tick_timers(16); // one 64 kHz tick; stage 0→1 (< target)
    assert_eq!(s.read8(0x00FF), 0);
    s.tick_timers(16); // stage 1→2 == target → out = 1
    assert_eq!(s.read8(0x00FF), 1);
}

#[test]
fn timer0_runs_at_8khz() {
    let mut s = apu();
    s.write8(0x00FA, 1); // T0 target = 1
    s.write8(0x00F1, 0x01); // enable T0
    s.tick_timers(127); // just under one 8 kHz period
    assert_eq!(s.read8(0x00FD), 0);
    s.tick_timers(1); // 128 cycles total → one tick → out = 1
    assert_eq!(s.read8(0x00FD), 1);
}

#[test]
fn reading_output_clears_it() {
    let mut s = apu();
    s.write8(0x00FC, 1);
    s.write8(0x00F1, 0x04);
    s.tick_timers(16 * 3); // out = 3
    assert_eq!(s.read8(0x00FF), 3);
    assert_eq!(s.read8(0x00FF), 0, "read-and-clear");
}

#[test]
fn target_zero_means_256() {
    let mut s = apu();
    s.write8(0x00FC, 0); // target 0 → period 256
    s.write8(0x00F1, 0x04);
    s.tick_timers(16 * 255); // 255 ticks, stage 255 < 256
    assert_eq!(s.read8(0x00FF), 0);
    s.tick_timers(16); // 256th tick → out = 1
    assert_eq!(s.read8(0x00FF), 1);
}

#[test]
fn output_is_four_bits() {
    let mut s = apu();
    s.write8(0x00FC, 1);
    s.write8(0x00F1, 0x04);
    s.tick_timers(16 * 17); // 17 increments; 17 mod 16 = 1
    assert_eq!(s.read8(0x00FF), 1);
}

#[test]
fn disabled_timer_is_frozen() {
    let mut s = apu();
    s.write8(0x00FC, 1); // target set, but never enabled
    s.tick_timers(16 * 5);
    assert_eq!(s.read8(0x00FF), 0);
}

#[test]
fn enable_edge_resets_stage_and_output() {
    let mut s = apu();
    s.write8(0x00FC, 1);
    s.write8(0x00F1, 0x04);
    s.tick_timers(16 * 3); // out = 3
    s.write8(0x00F1, 0x00); // disable
    s.write8(0x00F1, 0x04); // re-enable → 0→1 edge resets counters
    assert_eq!(s.read8(0x00FF), 0);
}

#[test]
fn all_three_timers_independent() {
    let mut s = apu();
    s.write8(0x00FA, 1); // T0
    s.write8(0x00FB, 1); // T1
    s.write8(0x00FC, 1); // T2
    s.write8(0x00F1, 0x07); // enable all three
    s.tick_timers(128); // T0/T1: 1 tick; T2: 8 ticks
    assert_eq!(s.read8(0x00FD), 1);
    assert_eq!(s.read8(0x00FE), 1);
    assert_eq!(s.read8(0x00FF), 8);
}
