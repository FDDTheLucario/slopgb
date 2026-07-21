//! N-SPC `base16` (instrument base-pitch multiplier) <-> SF2 tuning
//! generators (`coarseTune`/`fineTune`) + sample tuning (`originalPitch`/
//! `pitchCorrection`/`dwSampleRate`).
//!
//! **Tuning convention** (documented, chosen — not the only valid one):
//! `base16` is read as the raw SNES S-DSP `PITCH` register value at the
//! instrument's rest/reference note: `$1000` means "play the BRR data at its
//! native rate", and the S-DSP's native output rate for `PITCH=$1000` is
//! **32000 Hz** (nocash *fullsnes*, "SNES APU DSP - Voice Pitch": output
//! sample rate = 32000 * pitch/`$1000`). So `base16` is a *linear sample-rate
//! scale factor* relative to a fixed 32000 Hz reference, independent of any
//! particular MIDI note.
//!
//! Every exported SF2 sample is written at the canonical `dwSampleRate =
//! 32000`, `originalPitch = 60` (C4), `pitchCorrection = 0` — i.e. the
//! *sample* itself carries no tuning; the entire `base16` value is instead
//! expressed as `coarseTune`/`fineTune` on the *instrument*. This is
//! deliberate: N-SPC's `base16` is a per-instrument property (several
//! instruments can share one BRR sample at different pitches), while SF2's
//! `dwSampleRate`/`originalPitch` are per-*sample* properties — pushing the
//! whole tuning into the always-per-instrument `coarseTune`/`fineTune`
//! generators avoids a sample-sharing conflict. At the SF2 root key (60,
//! where the key-tracking term is zero), the resulting playback rate is
//! exactly `32000 * 2^((coarseTune*100+fineTune)/1200)` Hz — matching
//! `32000 * base16/$1000` by construction, so export/import round-trip
//! exactly (mod rounding to whole cents).
//!
//! Import is the mirror: it folds the sample's own tuning (`dwSampleRate`,
//! `originalPitch` — or the instrument's `overridingRootKey` if set —, and
//! `pitchCorrection`) together with the instrument's `coarseTune`/`fineTune`
//! into one combined cents offset from the 32000 Hz/`$1000` reference, then
//! converts that to `base16`. This is exact for files this crate wrote, and
//! a reasonable best-effort for third-party SF2s (it does not model
//! `scaleTuning`, `keynum` generators, or per-key pitch bend — the N-SPC
//! side has no runtime key-tracking hook at this layer either).

pub const BRR_ENCODE_RATE: u32 = 32000;
pub const SF2_ROOT_KEY: u8 = 60;

/// `base16` -> (`coarseTune` semitones, `fineTune` cents), both generator
/// ranges (`-120..=120` / `-99..=99` per SF2 §8.1.3), for the fixed
/// `dwSampleRate=32000, originalPitch=60, pitchCorrection=0` sample.
pub fn base16_to_coarse_fine(base16: u16) -> (i8, i8) {
    let base16 = base16.max(1);
    let cents = 1200.0 * (f64::from(base16) / 4096.0).log2();
    let coarse = (cents / 100.0).round().clamp(-120.0, 120.0);
    let fine = (cents - coarse * 100.0).round().clamp(-99.0, 99.0);
    (coarse as i8, fine as i8)
}

/// The inverse: sample tuning (`dwSampleRate`, effective root key —
/// `overridingRootKey` if present else the sample's own `originalPitch` —,
/// `pitchCorrection`) plus instrument `coarseTune`/`fineTune` -> `base16`.
#[allow(clippy::too_many_arguments)]
pub fn to_base16(
    sample_rate: u32,
    original_pitch: u8,
    pitch_correction: i8,
    root_key_override: Option<u8>,
    coarse_tune: i8,
    fine_tune: i8,
) -> u16 {
    let root = root_key_override.unwrap_or(original_pitch);
    let cents = 1200.0 * (f64::from(sample_rate.max(1)) / f64::from(BRR_ENCODE_RATE)).log2()
        + (f64::from(SF2_ROOT_KEY) - f64::from(root)) * 100.0
        - f64::from(pitch_correction)
        + f64::from(coarse_tune) * 100.0
        + f64::from(fine_tune);
    (4096.0 * 2f64.powf(cents / 1200.0)).round().clamp(1.0, 65535.0) as u16
}

#[cfg(test)]
#[path = "tuning_tests.rs"]
mod tests;
