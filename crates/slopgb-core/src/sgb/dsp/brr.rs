//! BRR (Bit Rate Reduction) sample decoding — the SNES ADPCM format.
//!
//! A BRR sample is a chain of 9-byte blocks. Byte 0 is the header:
//! `SSSS FF LE` — shift `S` (bits 4-7), filter `F` (bits 2-3), loop `L`
//! (bit 1), end `E` (bit 0). Bytes 1-8 hold 16 signed 4-bit nibbles (high
//! nibble first), each decoded to a 16-bit sample via the shift and one of the
//! four linear predictors, then clamped to 16 bits and wrapped to 15 (the
//! characteristic BRR overflow behaviour).
//!
//! This is a **verbatim port of Blargg's `SPC_DSP.cpp` decode loop** (the
//! reference used by higan/bsnes/snes9x): samples are computed at half scale
//! (hence `p2 >> 1` and the `>> 1` after the header shift), then stored back at
//! full 16-bit scale (`s * 2`). Sources: Blargg `snes_spc/SPC_DSP.cpp`; nocash
//! **fullsnes** ("SNES APU DSP - BRR Samples") for the block layout and the
//! shift ≥ 13 quirk.

/// One decoded 9-byte BRR block: the 16 samples plus the header flag bits.
pub(super) struct BrrBlock {
    pub samples: [i16; 16],
    pub loop_flag: bool,
    pub end_flag: bool,
}

/// Sign-extend a 4-bit nibble (0-15) to `-8..=7`.
#[inline]
fn nib(n: u8) -> i32 {
    ((n << 4) as i8 >> 4) as i32
}

/// Decode one 9-byte BRR block at `addr` in `ram`, threading the predictor
/// history (`p1`/`p2` — the last two full-scale decoded samples) across blocks
/// so the filter continues seamlessly. Reads wrap the 16-bit APU address space.
pub(super) fn decode_block(ram: &[u8; 0x1_0000], addr: u16, p1: &mut i32, p2: &mut i32) -> BrrBlock {
    let header = ram[addr as usize];
    let shift = header >> 4;
    let filter = (header >> 2) & 3;
    let mut out = [0i16; 16];
    for i in 0..16 {
        let byte = ram[addr.wrapping_add(1 + (i as u16) / 2) as usize];
        let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };

        // Header shift (half-scale domain), with the shift ≥ 13 quirk: only the
        // sign survives (`s = s < 0 ? -0x800 : 0`).
        let mut s = (nib(nibble) << shift) >> 1;
        if shift >= 13 {
            s = (s >> 25) << 11;
        }

        // Linear predictor. `p1` is the previous sample at full scale; `p2h` is
        // the one before that at half scale (Blargg's `pos[-2] >> 1`).
        let p1v = *p1;
        let p2h = *p2 >> 1;
        match filter {
            0 => {}
            1 => {
                s += p1v >> 1;
                s += (-p1v) >> 5;
            }
            2 => {
                s += p1v;
                s -= p2h;
                s += p2h >> 4;
                s += (p1v * -3) >> 6;
            }
            _ => {
                s += p1v;
                s -= p2h;
                s += (p1v * -13) >> 7;
                s += (p2h * 3) >> 4;
            }
        }

        // Clamp to signed 16-bit, then wrap to 15 (`* 2` re-wraps as int16).
        let s = s.clamp(-32768, 32767);
        let s = (s * 2) as i16;
        *p2 = *p1;
        *p1 = i32::from(s);
        out[i] = s;
    }
    BrrBlock {
        samples: out,
        loop_flag: header & 0x02 != 0,
        end_flag: header & 0x01 != 0,
    }
}

#[cfg(test)]
#[path = "brr_tests.rs"]
mod tests;
