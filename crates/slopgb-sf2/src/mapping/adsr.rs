//! SNES S-DSP `ADSR1`/`ADSR2`/`GAIN` <-> SF2 volume-envelope generators.
//!
//! **This mapping is best-effort and lossy** (documented per-field below) —
//! the SNES envelope hardware is a rate-gated multiplicative ramp, not the
//! linear-in-dB attack/decay/sustain/release model SF2 uses, so this is an
//! approximation, not a bit-exact inverse.
//!
//! `RATE`/`OFFSET` are the classic SPC700 DSP envelope rate table (samples
//! between steps, at the DSP's 32000 Hz sample clock), duplicated (not
//! depended on — this crate is independently zero-dep) from
//! `crates/slopgb-snes-apu/src/dsp/envelope.rs`, itself sourced from
//! Blargg's `SPC_DSP.cpp` `counter_rates` / nocash *fullsnes* ("SNES APU DSP
//! - ADSR / GAIN").

use crate::reader::VolEnv;

const RATE: [u16; 32] = [
    0x7800, 2048, 1536, 1280, 1024, 768, 640, 512, 384, 320, 256, 192, 160, 128, 96, 80, 64, 48,
    40, 32, 24, 20, 16, 12, 10, 8, 6, 5, 4, 3, 2, 1,
];

/// Attack ramps `0x20` per step from 0 to `0x7FF` (~`0x800`) — ~64 steps.
const ATTACK_STEPS: f64 = 64.0;
/// Decay/release/sustain-rate steps are a multiplicative ~1/256-per-step
/// ramp (`level -= ((level-1)>>8)+1`); ~256 steps approximates its time
/// constant. Not a physically exact "time to reach sustain" — an
/// approximation stated as a documented ceiling.
const DECAY_TAU_STEPS: f64 = 256.0;

fn rate_seconds(rate_idx: u8) -> f64 {
    f64::from(RATE[(rate_idx & 0x1F) as usize]) / 32000.0
}

fn seconds_to_timecents(s: f64) -> i16 {
    (1200.0 * s.max(1e-6).log2())
        .round()
        .clamp(-12000.0, 8000.0) as i16
}

fn timecents_to_seconds(tc: i16) -> f64 {
    2f64.powf(f64::from(tc) / 1200.0)
}

/// Find the rate index (from `candidates`) whose `rate_seconds * scale`
/// seconds is closest to `target_seconds`.
fn closest_rate(target_seconds: f64, scale: f64, candidates: impl Iterator<Item = u8>) -> u8 {
    candidates
        .min_by(|&a, &b| {
            let da = (rate_seconds(a) * scale - target_seconds).abs();
            let db = (rate_seconds(b) * scale - target_seconds).abs();
            da.partial_cmp(&db).unwrap()
        })
        .unwrap_or(0)
}

/// `ADSR1`/`ADSR2`/`GAIN` -> a best-effort SF2 [`VolEnv`] (no `delayVolEnv`
/// / `holdVolEnv` concept in the SNES envelope: both left at the SF2
/// "instant" default). If `ADSR1` bit 7 is clear (a GAIN-mode envelope, not
/// ADSR), this falls back to a flat instant-attack / full-sustain envelope —
/// the GAIN ramp modes (linear/exponential increase or decrease) are not
/// modeled at all.
pub fn adsr_to_vol_env(adsr1: u8, adsr2: u8) -> VolEnv {
    if adsr1 & 0x80 == 0 {
        return VolEnv::default();
    }
    let ar = adsr1 & 0x0F;
    let dr = (adsr1 >> 4) & 0x07;
    let sr = adsr2 & 0x1F;
    let sl = (adsr2 >> 5) & 0x07;

    let attack_s = rate_seconds(ar * 2 + 1) * ATTACK_STEPS;
    let decay_s = rate_seconds(dr * 2 + 0x10) * DECAY_TAU_STEPS;
    let release_s = rate_seconds(sr) * DECAY_TAU_STEPS;
    let sustain_frac = (f64::from(sl) + 1.0) / 8.0;
    let sustain_cb = (-1000.0 * sustain_frac.max(1e-4).log10())
        .round()
        .clamp(0.0, 1000.0) as i16;

    VolEnv {
        delay_tc: -12000,
        attack_tc: seconds_to_timecents(attack_s),
        hold_tc: -12000,
        decay_tc: seconds_to_timecents(decay_s),
        sustain_cb,
        release_tc: seconds_to_timecents(release_s),
    }
}

/// The inverse: a [`VolEnv`] -> `(ADSR1, ADSR2)`, always in ADSR mode
/// (`ADSR1` bit 7 set) — GAIN-mode envelopes are not synthesized on import.
/// `delayVolEnv`/`holdVolEnv` have no SNES-hardware equivalent and are
/// dropped (lossy).
pub fn vol_env_to_adsr(vol_env: &VolEnv) -> (u8, u8) {
    let attack_s = timecents_to_seconds(vol_env.attack_tc);
    let decay_s = timecents_to_seconds(vol_env.decay_tc);
    let release_s = timecents_to_seconds(vol_env.release_tc);
    let sustain_frac = 10f64.powf(-f64::from(vol_env.sustain_cb) / 1000.0);

    let ar = closest_rate(attack_s, ATTACK_STEPS, (0u8..=15).map(|ar| ar * 2 + 1)) / 2;
    let dr = closest_rate(decay_s, DECAY_TAU_STEPS, (0u8..=7).map(|dr| dr * 2 + 0x10));
    let dr = (dr - 0x10) / 2;
    let sr = closest_rate(release_s, DECAY_TAU_STEPS, 0u8..=31);
    let sl = (sustain_frac * 8.0 - 1.0).round().clamp(0.0, 7.0) as u8;

    let adsr1 = 0x80 | (dr << 4) | ar;
    let adsr2 = (sl << 5) | sr;
    (adsr1, adsr2)
}

/// `GAIN` -> `initialAttenuation` centibels: only meaningful for a direct
/// (bit 7 clear) gain value, linearly mapped `0..=127 -> 200..=0` cB. A
/// rate-based GAIN mode (bit 7 set) has no constant-attenuation
/// interpretation, so it maps to 0 cB (no extra attenuation) — lossy.
pub fn gain_to_attenuation_cb(gain: u8) -> i16 {
    if gain & 0x80 != 0 {
        return 0;
    }
    let level = f64::from(gain & 0x7F) / 127.0;
    ((1.0 - level) * 200.0).round() as i16
}

/// The inverse: `initialAttenuation` centibels -> a direct-mode `GAIN` byte
/// (bit 7 clear).
pub fn attenuation_cb_to_gain(cb: i16) -> u8 {
    let level = (1.0 - f64::from(cb.clamp(0, 200)) / 200.0).clamp(0.0, 1.0);
    (level * 127.0).round() as u8
}

#[cfg(test)]
#[path = "adsr_tests.rs"]
mod tests;
