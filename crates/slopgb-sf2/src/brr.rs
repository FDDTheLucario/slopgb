//! BRR (Bit Rate Reduction) codec — the SNES ADPCM sample format.
//!
//! A BRR sample is a chain of 9-byte blocks. Byte 0 is the header:
//! `SSSS FF LE` — shift `S` (bits 4-7), filter `F` (bits 2-3), loop `L` (bit
//! 1), end `E` (bit 0). Bytes 1-8 hold 16 signed 4-bit nibbles (high nibble
//! first), each decoded via the shift and one of four linear predictors, then
//! clamped to 16 bits and re-wrapped (`* 2`) into the final sample.
//!
//! The per-sample decode math (including the `shift >= 13` quirk) is a port of
//! `crates/slopgb-snes-apu/src/dsp/brr.rs`'s `decode_block`, itself a verbatim
//! port of Blargg's `SPC_DSP.cpp` decode loop (the reference used by
//! higan/bsnes/snes9x); see that file's header for full sourcing. It is
//! reproduced here (rather than depended on) because this crate is
//! independently zero-dep.

/// Sign-extend a 4-bit nibble (0-15) to `-8..=7`.
#[inline]
fn nib(n: u8) -> i32 {
    ((n << 4) as i8 >> 4) as i32
}

/// Decode one nibble at the given shift/filter against predictor history
/// (`p1` = previous full-scale sample, `p2` = the one before that), returning
/// the new full-scale sample. Shared by [`decode`] (real nibbles from a byte
/// stream) and [`encode`] (brute-forcing the best nibble per position), so
/// both paths compute samples identically to the reference decoder.
fn decode_sample(nibble: i32, shift: u8, filter: u8, p1: i32, p2: i32) -> i16 {
    // Header shift (half-scale domain), with the shift >= 13 quirk: only the
    // sign survives (`s = s < 0 ? -0x800 : 0`).
    let mut s = (nibble << shift) >> 1;
    if shift >= 13 {
        s = (s >> 25) << 11;
    }

    // Linear predictor. `p1` is the previous sample at full scale; `p2h` is
    // the one before that at half scale (Blargg's `pos[-2] >> 1`).
    let p2h = p2 >> 1;
    match filter {
        0 => {}
        1 => {
            s += p1 >> 1;
            s += (-p1) >> 5;
        }
        2 => {
            s += p1;
            s -= p2h;
            s += p2h >> 4;
            s += (p1 * -3) >> 6;
        }
        _ => {
            s += p1;
            s -= p2h;
            s += (p1 * -13) >> 7;
            s += (p2h * 3) >> 4;
        }
    }

    let s = s.clamp(-32768, 32767);
    (s * 2) as i16
}

/// A decoded BRR sample: the full PCM16 chain plus whether the block that set
/// the end flag also set the loop flag (the hardware only consults the loop
/// bit on the end block; the actual loop *address* lives in the sample
/// directory, not in the BRR data itself, so the caller supplies/derives it).
pub struct Decoded {
    pub pcm: Vec<i16>,
    pub loops: bool,
}

/// Decode a whole BRR sample (a chain of 9-byte blocks) starting at byte
/// offset `start` in `buf`, threading predictor history across blocks, and
/// stopping at (and including) the block that sets the end flag.
///
/// Errors if the chain runs off the end of `buf` before an end-flag block is
/// found (a malformed/truncated sample), rather than wrapping addresses the
/// way real 16-bit APU memory would — callers here always pass a bounded
/// region they control, so wraparound is not a real scenario.
pub fn decode(buf: &[u8], start: usize) -> Result<Decoded, String> {
    let mut pcm = Vec::new();
    let mut p1 = 0i32;
    let mut p2 = 0i32;
    let mut addr = start;
    let loops = loop {
        if addr + 9 > buf.len() {
            return Err(format!(
                "BRR chain starting at {start:#x} ran off the end of the buffer \
                 ({} bytes) before an end-flag block",
                buf.len()
            ));
        }
        let header = buf[addr];
        let shift = header >> 4;
        let filter = (header >> 2) & 3;
        let end = header & 0x01 != 0;
        let loop_flag = header & 0x02 != 0;
        for i in 0..16usize {
            let byte = buf[addr + 1 + i / 2];
            let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
            let s = decode_sample(nib(nibble), shift, filter, p1, p2);
            p2 = p1;
            p1 = i32::from(s);
            pcm.push(s);
        }
        addr += 9;
        if end {
            break loop_flag;
        }
    };
    Ok(Decoded { pcm, loops })
}

/// Result of [`encode`]: the BRR byte chain plus, if the sample loops, the
/// block index (0-based) the loop point was rounded to. The caller (mapping)
/// turns that into an absolute APU address (`start_addr + loop_block * 9`).
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub loop_block: Option<usize>,
}

/// Encode PCM16 to BRR. `loop_sample`, if given, is the sample index (into
/// `pcm`) playback should resume at on loop; it is rounded to the nearest
/// 16-sample block boundary (BRR can only loop at block starts).
///
/// Each 9-byte block brute-forces all 4 filters x shifts `0..=12` (shift >= 13
/// is the decode-only degenerate quirk, skipped for encoding per the SNES BRR
/// encoder convention), decoding each candidate nibble with the exact same
/// [`decode_sample`] math used by [`decode`] so the encoder's threaded
/// `p1`/`p2` history never drifts from what a real decode would reconstruct.
/// The (filter, shift) minimizing summed squared error against the source 16
/// samples wins; its reconstructed `p1`/`p2` are threaded into the next block.
/// The first block is forced to filter 0 (no valid history yet).
pub fn encode(pcm: &[i16], loop_sample: Option<usize>) -> Encoded {
    let n_blocks = pcm.len().div_ceil(16).max(1);
    // Pad the last block by repeating the final sample (avoids a false
    // transient a zero-pad would introduce).
    let last = *pcm.last().unwrap_or(&0);
    let mut padded = pcm.to_vec();
    padded.resize(n_blocks * 16, last);

    let loop_block = loop_sample.map(|s| ((s + 8) / 16).min(n_blocks.saturating_sub(1)));

    let mut bytes = Vec::with_capacity(n_blocks * 9);
    let mut p1 = 0i32;
    let mut p2 = 0i32;
    for block_idx in 0..n_blocks {
        let target = &padded[block_idx * 16..block_idx * 16 + 16];
        let is_first = block_idx == 0;
        let is_last = block_idx == n_blocks - 1;

        // Best candidate: (filter, shift, error, decoded samples, predictor
        // carries p1/p2, nibbles).
        type BrrCandidate = (u8, u8, i64, [i16; 16], i32, i32, [i32; 16]);
        let mut best: Option<BrrCandidate> = None;
        let filters: &[u8] = if is_first { &[0] } else { &[0, 1, 2, 3] };
        for &filter in filters {
            for shift in 0u8..=12 {
                let mut trial_p1 = p1;
                let mut trial_p2 = p2;
                let mut samples = [0i16; 16];
                let mut nibbles = [0i32; 16];
                let mut err = 0i64;
                for i in 0..16 {
                    let want = i32::from(target[i]);
                    let mut best_nib = -8i32;
                    let mut best_sample = 0i16;
                    let mut best_diff = i64::MAX;
                    for n in -8i32..=7 {
                        let s = decode_sample(n, shift, filter, trial_p1, trial_p2);
                        let diff = (i64::from(s) - i64::from(want)).abs();
                        if diff < best_diff {
                            best_diff = diff;
                            best_nib = n;
                            best_sample = s;
                        }
                    }
                    err += best_diff * best_diff;
                    nibbles[i] = best_nib;
                    samples[i] = best_sample;
                    trial_p2 = trial_p1;
                    trial_p1 = i32::from(best_sample);
                }
                if best.as_ref().is_none_or(|b| err < b.2) {
                    best = Some((filter, shift, err, samples, trial_p1, trial_p2, nibbles));
                }
            }
        }
        let (filter, shift, _, _samples, new_p1, new_p2, nibbles) =
            best.expect("at least one (filter, shift) candidate always exists");
        p1 = new_p1;
        p2 = new_p2;

        let loops_here = loop_block.is_some();
        let header = (shift << 4) | (filter << 2) | (u8::from(loops_here) << 1) | u8::from(is_last);
        bytes.push(header);
        for pair in 0..8 {
            let hi = (nibbles[pair * 2] & 0x0F) as u8;
            let lo = (nibbles[pair * 2 + 1] & 0x0F) as u8;
            bytes.push((hi << 4) | lo);
        }
    }
    Encoded { bytes, loop_block }
}

#[cfg(test)]
#[path = "brr_tests.rs"]
mod tests;
