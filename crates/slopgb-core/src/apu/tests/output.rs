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
