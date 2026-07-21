use super::*;

#[test]
fn default_base_is_unity() {
    let (coarse, fine) = base16_to_coarse_fine(0x1000);
    assert_eq!((coarse, fine), (0, 0));
    let back = to_base16(BRR_ENCODE_RATE, SF2_ROOT_KEY, 0, None, coarse, fine);
    assert_eq!(back, 0x1000);
}

#[test]
fn round_trips_within_a_cent_of_rounding() {
    for base16 in [0x0800u16, 0x0C00, 0x1000, 0x1800, 0x2000, 0x0100, 0x4000] {
        let (coarse, fine) = base16_to_coarse_fine(base16);
        let back = to_base16(BRR_ENCODE_RATE, SF2_ROOT_KEY, 0, None, coarse, fine);
        let diff = (i32::from(back) - i32::from(base16)).abs();
        assert!(diff <= 1, "base16 {base16:#x} -> ({coarse},{fine}) -> {back:#x}, diff {diff}");
    }
}

#[test]
fn overriding_root_key_shifts_tuning_by_semitones() {
    // A sample recorded a root key higher plays back slower at rest (60) —
    // one semitone (12) up in overridingRootKey halves nothing precise, but
    // should shift the resulting base16 down (lower pitch multiplier).
    let base_default = to_base16(BRR_ENCODE_RATE, SF2_ROOT_KEY, 0, None, 0, 0);
    let base_shifted = to_base16(BRR_ENCODE_RATE, SF2_ROOT_KEY, 0, Some(SF2_ROOT_KEY + 12), 0, 0);
    assert!(base_shifted < base_default);
}
