//! Linear-interpolation PCM16 resampler.
//!
//! Ceiling: linear interpolation only (no windowed-sinc / band-limited
//! filtering), so high ratio changes alias more than a production resampler
//! would. Good enough for BRR import/export tuning, where the target is
//! "musically correct pitch", not studio-grade fidelity.

/// Resample `pcm` from `in_rate` Hz to `out_rate` Hz via linear
/// interpolation. A 1:1 ratio is the identity (returns an exact copy, no
/// interpolation error).
pub fn resample(pcm: &[i16], in_rate: u32, out_rate: u32) -> Vec<i16> {
    if pcm.is_empty() || in_rate == 0 || out_rate == 0 {
        return Vec::new();
    }
    if in_rate == out_rate {
        return pcm.to_vec();
    }

    let ratio = f64::from(in_rate) / f64::from(out_rate);
    let out_len = ((pcm.len() as f64) / ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx0 = src_pos.floor() as usize;
        let frac = src_pos - idx0 as f64;
        let s0 = f64::from(pcm[idx0.min(pcm.len() - 1)]);
        let s1 = f64::from(pcm[(idx0 + 1).min(pcm.len() - 1)]);
        let v = s0 + (s1 - s0) * frac;
        out.push(v.round().clamp(-32768.0, 32767.0) as i16);
    }
    out
}

#[cfg(test)]
#[path = "resample_tests.rs"]
mod tests;
