//! Gaussian table + interpolation tests. The strongest correctness check
//! without hardware capture is the *unity-gain* property: at every fractional
//! index the four selected coefficients sum to ≈ 2048, so `>> 11` leaves a
//! constant input unchanged.

use super::*;

#[test]
fn table_endpoints_match_the_hardware_rom() {
    assert_eq!(GAUSS[0], 0);
    assert_eq!(GAUSS[511], 1305);
    assert_eq!(GAUSS.len(), 512);
}

#[test]
fn coefficients_sum_to_unity_at_every_index() {
    for i in 0..256usize {
        let sum = GAUSS[255 - i] + GAUSS[511 - i] + GAUSS[256 + i] + GAUSS[i];
        assert!(
            (sum - 2048).abs() <= 2,
            "index {i}: coeff sum {sum} not ~2048"
        );
    }
}

#[test]
fn constant_input_passes_through_unchanged() {
    // A constant sample sequence must interpolate to ~itself at any fraction.
    for frac in [0u16, 0x100, 0x400, 0x800, 0xC00, 0xFF0] {
        let out = interpolate([1000, 1000, 1000, 1000], frac);
        assert!((out - 1000).abs() <= 2, "frac {frac:#x}: {out}");
    }
}

#[test]
fn output_low_bit_is_always_cleared() {
    // The DSP Gaussian output is always even.
    for frac in [0u16, 0x123, 0x555, 0xABC, 0xFFF] {
        assert_eq!(interpolate([12345, -9999, 7777, -3333], frac) & 1, 0);
    }
}

#[test]
fn zero_input_is_silence() {
    assert_eq!(interpolate([0, 0, 0, 0], 0x800), 0);
}
