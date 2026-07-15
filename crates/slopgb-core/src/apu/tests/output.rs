//! `tests` — output tests (split for file size).

use super::*;

#[test]
fn default_sample_rate_produces_48000_per_second() {
    let mut h = H::dmg();
    h.ticks(1_048_576); // one second of M-cycles
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    assert!((47999..=48001).contains(&out.len()), "got {}", out.len());
}

#[test]
fn set_sample_rate_changes_output_rate() {
    let mut h = H::dmg();
    h.apu.set_sample_rate(22050);
    h.ticks(1_048_576);
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    assert!((22049..=22051).contains(&out.len()), "got {}", out.len());
}

#[test]
fn set_sample_rate_resets_capacitors_and_drops_stale_samples() {
    let mut h = H::dmg();
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0xFF);
    h.w(0xFF12, 0xF0); // ch1 DAC on: a DC offset charges the capacitors
    h.ticks(10_000);
    assert!(!h.apu.samples.is_empty());
    assert_ne!(h.apu.hp_cap_l, 0.0);
    assert_ne!(h.apu.hp_cap_r, 0.0);
    // A mid-run rate change must not mix stale state into the new
    // stream: pending samples at the old rate are dropped and the
    // high-pass capacitors restart discharged.
    h.apu.set_sample_rate(22_050);
    assert!(h.apu.samples.is_empty(), "stale samples must be dropped");
    assert_eq!(h.apu.hp_cap_l, 0.0);
    assert_eq!(h.apu.hp_cap_r, 0.0);
}

#[test]
fn drain_moves_the_buffer() {
    let mut h = H::dmg();
    h.ticks(10_000);
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    assert!(!out.is_empty());
    let n = out.len();
    h.apu.drain_samples(&mut out);
    assert_eq!(out.len(), n, "second drain adds nothing");
}

#[test]
fn silence_when_all_dacs_off() {
    let mut h = H::dmg();
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0xFF);
    h.ticks(50_000);
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    assert!(out.iter().all(|&(l, r)| l == 0.0 && r == 0.0));
}

#[test]
fn record_channels_captures_four_isolated_streams() {
    // The Joypad → "Audio channels" tap: armed, it records each channel's
    // isolated mono output at the same rate/length as the mixed stream.
    let mut h = H::dmg();
    h.apu.set_record_channels(true);
    h.w(0xFF24, 0x77); // NR50 master vol
    h.w(0xFF25, 0xFF); // NR51: route everything
    h.w(0xFF11, 0x80); // ch1 50% duty
    h.w(0xFF13, 0x00);
    h.w(0xFF14, 0x84); // ch1 trigger, audible period
    h.w(0xFF12, 0xF0); // ch1 DAC on + max envelope
    h.ticks(50_000);
    let mut mixed = Vec::new();
    h.apu.drain_samples(&mut mixed);
    let mut chans: [Vec<f32>; 4] = Default::default();
    h.apu.drain_audio_channels(&mut chans);
    // Every track is the same length as the mix (they share the resampling
    // window), so the four WAVs line up with the mixed recording.
    for (i, c) in chans.iter().enumerate() {
        assert_eq!(c.len(), mixed.len(), "channel {i} length matches the mix");
    }
    // Ch1 is playing → its track carries the square wave; ch2-4 DACs are off,
    // so those tracks are pure silence (the tap is per-channel, not the mix).
    let energy: f32 = chans[0].iter().map(|&s| s * s).sum();
    assert!(energy > 1.0, "ch1 track carries audio, got {energy}");
    for (ch, track) in chans.iter().enumerate().skip(1) {
        assert!(
            track.iter().all(|&s| s == 0.0),
            "ch{} DAC off → silent track",
            ch + 1
        );
    }
}

#[test]
fn record_channels_disarmed_records_nothing_and_disarming_clears() {
    let mut h = H::dmg();
    h.start_ch1();
    h.ticks(10_000);
    let mut chans: [Vec<f32>; 4] = Default::default();
    // Never armed → nothing captured.
    h.apu.drain_audio_channels(&mut chans);
    assert!(
        chans.iter().all(Vec::is_empty),
        "disarmed tap records nothing"
    );
    // Arm, capture, then disarm: disarming drops the buffered samples.
    h.apu.set_record_channels(true);
    h.ticks(10_000);
    h.apu.set_record_channels(false);
    h.apu.drain_audio_channels(&mut chans);
    assert!(chans.iter().all(Vec::is_empty), "disarm drops the buffer");
}

#[test]
fn playing_pulse_is_audible_and_routed_by_nr51() {
    let mut h = H::dmg();
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0x10); // ch1 left only
    h.w(0xFF11, 0x80); // 50% duty
    h.w(0xFF12, 0xF0);
    h.w(0xFF13, 0x00);
    h.w(0xFF14, 0x84); // trigger, freq 0x400: audible period
    h.ticks(100_000);
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    let energy_l: f32 = out.iter().map(|&(l, _)| l * l).sum();
    let energy_r: f32 = out.iter().map(|&(_, r)| r * r).sum();
    assert!(energy_l > 1.0, "left should carry the square wave");
    assert!(
        energy_r < energy_l / 100.0,
        "right is unrouted: {energy_r} vs {energy_l}"
    );
}

#[test]
fn nr50_zero_does_not_mute() {
    let mut h = H::dmg();
    h.w(0xFF24, 0x00); // volume 0 = gain 1/8
    h.w(0xFF25, 0xFF);
    h.w(0xFF11, 0x80);
    h.w(0xFF12, 0xF0);
    h.w(0xFF14, 0x84);
    h.ticks(100_000);
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    let energy: f32 = out.iter().map(|&(l, _)| l * l).sum();
    assert!(energy > 0.01, "NR50 never mutes, got {energy}");
}

#[test]
fn sample_buffer_is_capped_without_a_consumer() {
    // Headless runs (the mooneye harness never drains audio) must not
    // grow the buffer without bound: capped at one second of audio.
    let mut h = H::dmg();
    h.apu.set_sample_rate(1000);
    h.ticks(2 * 1_048_576); // two emulated seconds, never drained
    assert_eq!(h.apu.samples.len(), 1000);
    // Draining frees the cap and output resumes.
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    assert_eq!(out.len(), 1000);
    h.ticks(10_000);
    assert!(!h.apu.samples.is_empty());
}

#[test]
fn dac_maps_digital_zero_to_positive_analog() {
    // Pan Docs "Audio Details" (DACs): the DAC slope is negative —
    // digital 0 is analog +1, digital 15 is analog -1. A live DAC on a
    // silent channel is therefore a *positive* DC offset.
    let mut h = H::dmg();
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0xFF);
    h.w(0xFF12, 0xF0); // ch1 DAC on, channel not triggered: digital 0
    h.ticks(100);
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    let first = out[0].0;
    assert!(first > 0.05, "digital 0 must map to analog +1, got {first}");
}

#[test]
fn pcm_readouts_expose_channel_digital_outputs() {
    // Pan Docs "PCM amplitude readouts": PCM12 low nibble = ch1 digital
    // output, high nibble = ch2; PCM34 likewise for ch3/ch4. DAC-off
    // channels read 0.
    let mut h = H::dmg();
    assert_eq!(h.apu.pcm12(), 0x00, "all DACs off at power-on");
    assert_eq!(h.apu.pcm34(), 0x00);
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0xFF);
    // ch2: max volume, no envelope; duty 2 (50%); trigger.
    h.w(0xFF17, 0xF0);
    h.w(0xFF18, 0x00);
    h.w(0xFF19, 0x87);
    // A full duty cycle is 8 steps of (2048-1024)*4 T-cycles; sample the
    // high nibble across one cycle and expect both 0 and 15 phases.
    let mut seen = [false; 16];
    for _ in 0..8 * 1024 {
        h.apu.tick(0, false);
        seen[usize::from(h.apu.pcm12() >> 4)] = true;
    }
    assert!(seen[0] && seen[15], "50% duty must swing 0<->15: {seen:?}");
    assert_eq!(h.apu.pcm12() & 0x0F, 0, "ch1 DAC off reads 0");
}

#[test]
fn high_pass_removes_dc_offset() {
    // A DAC turned on with the channel silent is a pure DC offset; the
    // output capacitor must drain it to (near) zero.
    let mut h = H::dmg();
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0xFF);
    h.w(0xFF12, 0xF0); // ch1 DAC on, channel not triggered
    h.ticks(1_048_576); // one second
    let mut out = Vec::new();
    h.apu.drain_samples(&mut out);
    let tail = &out[out.len() - 100..];
    assert!(
        tail.iter().all(|&(l, r)| l.abs() < 0.01 && r.abs() < 0.01),
        "DC offset must decay"
    );
    // ...but the first samples did see the offset (DAC actually mixes).
    assert!(out[0].0.abs() > 0.05);
}

#[test]
fn raw_tap_is_pre_average_pre_high_pass() {
    // Constant DC input (DAC on, channel silent): the raw pre-filter
    // tap must report bit-identical samples for the whole run —
    // gambatte's testrunner judges silence by raw-sample equality —
    // while the filtered drain_samples output decays through the
    // output capacitor (i.e. varies).
    let mut h = H::dmg();
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0xFF);
    h.w(0xFF12, 0xF0); // ch1 DAC on, channel not triggered -> pure DC
    h.ticks(8192);
    let mut raw = Vec::new();
    h.apu.drain_raw_samples(&mut raw);
    assert_eq!(raw.len(), 8192 * 4, "one raw sample per dot");
    let (l0, r0) = raw[0];
    assert!(l0 != 0.0, "the DC offset must reach the tap");
    assert!(
        raw.iter()
            .all(|&(l, r)| l.to_bits() == l0.to_bits() && r.to_bits() == r0.to_bits()),
        "raw samples must be bit-identical under constant DC"
    );
    let mut filtered = Vec::new();
    h.apu.drain_samples(&mut filtered);
    let f0 = filtered[0].0;
    assert!(
        filtered.iter().any(|&(l, _)| l.to_bits() != f0.to_bits()),
        "high-passed output must decay (vary) under constant DC"
    );
}

#[test]
fn high_pass_dc_input_decays_geometrically_by_the_charge_factor() {
    // The output capacitor is a single-pole high-pass:
    //   out = input - cap;  cap = input - out * charge
    // Driven with a constant input x from a discharged start, its output is
    // a pure geometric decay, out_n = x * charge^(n-1) (proof: cap_n =
    // x*(1 - charge^n), so out_{n+1} = x - cap_n = x*charge^n). Verify with
    // an exactly-representable charge so the whole ladder is bit-exact and an
    // off-by-one in the recurrence (or dropping the `- out*charge` leak)
    // fails immediately.
    let mut cap = 0.0_f32;
    let charge = 0.5_f32;
    let outs = [
        high_pass(&mut cap, 1.0, charge),
        high_pass(&mut cap, 1.0, charge),
        high_pass(&mut cap, 1.0, charge),
        high_pass(&mut cap, 1.0, charge),
    ];
    assert_eq!(outs, [1.0, 0.5, 0.25, 0.125], "x * 0.5^(n-1)");

    // charge == 1 is the no-leak limit: DC passes forever (no high-pass at
    // all). This pins that `charge` is exactly what removes the DC — a
    // mis-scaled charge that drifts toward 1 stops removing DC.
    let mut cap = 0.0_f32;
    let held = [
        high_pass(&mut cap, 1.0, 1.0),
        high_pass(&mut cap, 1.0, 1.0),
        high_pass(&mut cap, 1.0, 1.0),
    ];
    assert_eq!(held, [1.0, 1.0, 1.0], "charge==1 leaks nothing: DC held");
}

#[test]
fn hp_charge_is_scaled_from_per_t_cycle_to_per_output_sample() {
    // Physical model (Blargg's DMG measurement): the output capacitor keeps
    // a fraction PER_T_CYCLE of its charge every *T-cycle*. One 48 kHz output
    // sample spans CLOCK_HZ/48000 T-cycles, so the per-*sample* charge factor
    // the resampled stream must use is PER_T_CYCLE raised to that many cycles
    // (~0.9963, not the raw 0.999958). Deriving the expectation from the raw
    // physical constant + the clock (not from hp_charge) makes this a real
    // pin, not a mirror: leaving hp_charge at the per-T-cycle value, or
    // mis-scaling it, decays audibly wrong yet is invisible to the golden.
    const PER_T_CYCLE: f64 = 0.999_958;
    let cps = f64::from(crate::CLOCK_HZ) / 48_000.0;
    let expected = PER_T_CYCLE.powf(cps); // ~0.9963366 at 48 kHz

    let h = H::dmg(); // built at DEFAULT_SAMPLE_RATE = 48 kHz
    assert!(
        (f64::from(h.apu.hp_charge) - expected).abs() < 1e-6,
        "hp_charge {} must be the per-sample factor {expected}",
        h.apu.hp_charge
    );
    // The classic bug is leaving hp_charge at the raw per-T-cycle value; the
    // two differ by >0.003, orders of magnitude outside the 1e-6 pin above.
    assert!(
        (expected - PER_T_CYCLE).abs() > 0.003,
        "per-sample and per-T-cycle charge must be distinguishable"
    );

    // Drive pure DC through the *production* capacitor from discharged and
    // read the output 100 samples in. Independently, out_100 = x*charge^99.
    // A 2x-mis-scaled hp_charge (~1.99) makes the filter unstable (grows
    // >1); a halved one (~0.50) collapses it to ~1e-30 by sample 100. Either
    // lands nowhere near the ~0.695 asserted here to 1e-4.
    let mut cap = 0.0_f32;
    let mut out = 0.0_f32;
    for _ in 0..100 {
        out = high_pass(&mut cap, 1.0, h.apu.hp_charge);
    }
    let analytic = expected.powi(99); // ~0.6953505
    assert!(
        (f64::from(out) - analytic).abs() < 1e-4,
        "DC decay at sample 100: got {out}, expected {analytic}"
    );
}

#[test]
fn box_average_emits_the_exact_window_mean() {
    // The resampler is a box average: it sums every dot in a window of
    // `cycles_per_sample` T-cycles and divides by the dot count. Pick a rate
    // whose window is an exact integer (4194304/524288 = 8 dots) and feed a
    // ramp so the mean depends on *which* dots the window spans — an
    // off-by-one bound (`>` instead of `>=`, or the wrong frac reset) then
    // changes both when a sample emits and its value.
    let mut h = H::dmg();
    h.apu.set_sample_rate(524_288); // cycles_per_sample == 8.0 exactly

    // A window closes only on the 8th dot, never earlier: dots 1..=7 emit
    // nothing (the `>=` boundary).
    for n in 1..=7 {
        assert_eq!(
            h.apu.accumulate_output(n as f32, (2 * n) as f32),
            None,
            "dot {n} must not close the window"
        );
    }
    // 8th dot closes window 1: mean of 1..=8 = 36/8 = 4.5 (right channel
    // 2x = 9.0). The capacitor is discharged, so the high-pass passes the
    // mean through unchanged (out_1 = input - 0).
    let s1 = h.apu.accumulate_output(8.0, 16.0).expect("8th dot emits");
    assert_eq!(s1, (4.5, 9.0), "window 1 = arithmetic mean of dots 1..=8");

    // Window 2 must span exactly dots 9..=16 (the sum/count reset after
    // emit, the window advanced by 8 not 7 or 9): mean 100/8 = 12.5, right
    // 25.0. Rezero the capacitor so this mean is again exact (out_1 rule).
    h.apu.hp_cap_l = 0.0;
    h.apu.hp_cap_r = 0.0;
    for n in 9..=15 {
        assert_eq!(
            h.apu.accumulate_output(n as f32, (2 * n) as f32),
            None,
            "dot {n} must not close window 2"
        );
    }
    let s2 = h.apu.accumulate_output(16.0, 32.0).expect("16th dot emits");
    assert_eq!(
        s2,
        (12.5, 25.0),
        "window 2 = arithmetic mean of dots 9..=16"
    );
}

#[test]
fn raw_tap_is_capped_and_draining_restarts_collection() {
    let mut h = H::dmg();
    // Run far past the cap: the buffer must stop growing, not OOM.
    h.ticks(RAW_SAMPLE_CAP as u32 / 4 + 10_000);
    assert_eq!(h.apu.raw_samples.len(), RAW_SAMPLE_CAP);
    let mut out = Vec::new();
    h.apu.drain_raw_samples(&mut out);
    assert_eq!(out.len(), RAW_SAMPLE_CAP);
    assert!(h.apu.raw_samples.is_empty());
    // Collection resumes after a drain (the gambatte harness drains the
    // 15 warm-up frames, then captures exactly the final frame).
    h.ticks(100);
    assert_eq!(h.apu.raw_samples.len(), 400);
}
