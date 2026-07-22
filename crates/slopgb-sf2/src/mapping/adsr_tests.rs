use super::*;

#[test]
fn gain_mode_falls_back_to_flat_envelope() {
    // ADSR1 bit 7 clear -> GAIN mode, not modeled: flat/instant envelope.
    let env = adsr_to_vol_env(0x00, 0x00);
    assert_eq!(env.attack_tc, VolEnv::default().attack_tc);
    assert_eq!(env.sustain_cb, VolEnv::default().sustain_cb);
}

#[test]
fn adsr_round_trips_through_rate_indices() {
    // adsr1 = enable | decay_rate(dr=3) | attack_rate(ar=7)
    // adsr2 = sustain_level(sl=5) | sustain_rate(sr=12)
    let adsr1 = 0x80 | (3 << 4) | 7;
    let adsr2 = (5 << 5) | 12;
    let env = adsr_to_vol_env(adsr1, adsr2);
    let (back1, back2) = vol_env_to_adsr(&env);
    assert_eq!(
        back1, adsr1,
        "attack/decay rate must round-trip exactly (exact rate table hit)"
    );
    assert_eq!(back2, adsr2, "sustain level/rate must round-trip exactly");
}

#[test]
fn attenuation_round_trips_direct_gain() {
    for gain in [0x00u8, 0x10, 0x40, 0x7F] {
        let cb = gain_to_attenuation_cb(gain);
        let back = attenuation_cb_to_gain(cb);
        let diff = (i32::from(back) - i32::from(gain)).abs();
        assert!(
            diff <= 1,
            "gain {gain:#x} -> {cb}cB -> {back:#x}, diff {diff}"
        );
    }
}

#[test]
fn rate_gain_mode_maps_to_zero_extra_attenuation() {
    assert_eq!(gain_to_attenuation_cb(0x9F), 0);
}

#[test]
fn full_sustain_means_zero_centibels() {
    // sl = 7 -> full level (no attenuation at sustain).
    let adsr2 = (7 << 5) | 10;
    let env = adsr_to_vol_env(0x80, adsr2);
    assert_eq!(env.sustain_cb, 0);
}
