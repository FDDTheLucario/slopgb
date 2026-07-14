//! Envelope + rate-counter tests, checked against the Blargg per-step rules.

// Tests set one or two `Env` fields on a default to isolate a phase; the struct
// literal would be noisier than the assignment here.
#![allow(clippy::field_reassign_with_default)]

use super::*;

#[test]
fn rate_zero_never_fires_rate_max_always() {
    assert!(!fires(0, 0));
    assert!(!fires(1234, 0));
    // RATE[31] == 1, OFFSET[31] == 0 -> every sample.
    assert!(fires(0, 31));
    assert!(fires(1, 31));
    assert!(fires(9999, 31));
}

#[test]
fn rate_one_fires_on_period_boundary() {
    // RATE[1] == 2048, OFFSET[1] == 0.
    assert!(fires(0, 1));
    assert!(!fires(1, 1));
    assert!(fires(2048, 1));
}

#[test]
fn attack_ramps_up_then_switches_to_decay() {
    let mut e = Env::default();
    e.key_on();
    assert_eq!(e.phase, Phase::Attack);
    // AR = 15 -> rate 31 -> +0x400 every sample.
    let adsr1 = 0x80 | 0x0F;
    e.step(adsr1, 0x00, 0, 0);
    assert_eq!(e.level, 0x400);
    e.step(adsr1, 0x00, 0, 0);
    assert_eq!(e.level, 0x7FF); // clamped
    assert_eq!(e.phase, Phase::Decay);
}

#[test]
fn attack_slow_rate_steps_by_32() {
    let mut e = Env::default();
    e.key_on();
    // AR = 0 -> rate 1 -> +0x20 when (counter)%2048==0.
    let adsr1 = 0x80;
    e.step(adsr1, 0x00, 0, 0); // counter 0 fires
    assert_eq!(e.level, 0x20);
    e.step(adsr1, 0x00, 0, 1); // counter 1 does not fire
    assert_eq!(e.level, 0x20);
}

#[test]
fn release_ramps_down_by_eight_to_zero() {
    let mut e = Env::default();
    e.level = 20;
    e.key_off();
    assert_eq!(e.phase, Phase::Release);
    e.step(0, 0, 0, 12345); // release is not rate-gated
    assert_eq!(e.level, 12);
    e.step(0, 0, 0, 0);
    assert_eq!(e.level, 4);
    e.step(0, 0, 0, 0);
    assert_eq!(e.level, 0); // floored, not negative
}

#[test]
fn gain_direct_sets_level_immediately() {
    let mut e = Env::default();
    e.phase = Phase::Attack; // not release
    // ADSR disabled (bit7 clear), direct gain (gain bit7 clear): env = gain*0x10.
    e.step(0x00, 0x00, 0x40, 12345);
    assert_eq!(e.level, 0x400);
}

#[test]
fn gain_linear_increase_and_decrease() {
    // mode 2 (linear increase): gain = 0xC0 | rate31 = 0xDF, +0x20/sample.
    let mut e = Env::default();
    e.phase = Phase::Attack;
    e.level = 0x100;
    e.step(0x00, 0x00, 0xDF, 0);
    assert_eq!(e.level, 0x120);
    // mode 0 (linear decrease): gain = 0x80 | rate31 = 0x9F, -0x20/sample.
    let mut e = Env::default();
    e.phase = Phase::Attack;
    e.level = 0x100;
    e.step(0x00, 0x00, 0x9F, 0);
    assert_eq!(e.level, 0xE0);
}

#[test]
fn envx_is_level_shifted_right_four() {
    let mut e = Env::default();
    e.level = 0x7F0;
    assert_eq!(e.envx(), 0x7F);
    e.level = 0x123;
    assert_eq!(e.envx(), 0x12);
}
