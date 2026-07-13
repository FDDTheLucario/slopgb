//! One of the eight S-DSP voices: BRR sample streaming, the pitch counter,
//! Gaussian interpolation, and the ADSR/GAIN envelope.
//!
//! Each output sample a voice: adds its (possibly pitch-modulated) 14-bit pitch
//! to the interpolation counter; decodes as many new BRR samples as the
//! counter's integer part advanced (following loop/end flags and setting
//! `ENDX`); Gaussian-interpolates the four newest samples; multiplies by the
//! envelope; and returns the enveloped sample for the DSP's per-channel mix.
//!
//! Sources: nocash **fullsnes** ("SNES APU DSP") and **Blargg SPC_DSP** for the
//! pitch-counter layout (12 fractional bits), the loop/end/ENDX handling, and
//! the `(interp * env) >> 11` output scaling.

use super::brr::decode_block;
use super::envelope::Env;
use super::gaussian;

/// One voice's playback + envelope state.
#[derive(Clone)]
pub(super) struct Voice {
    /// Address of the BRR block currently loaded in [`Self::block`].
    brr_addr: u16,
    /// Loop-point block address (from the sample directory).
    loop_addr: u16,
    /// The 16 decoded samples of the current block.
    block: [i16; 16],
    /// Next sample index to consume from [`Self::block`] (`0..=16`).
    block_pos: usize,
    /// End/loop flags of the current block.
    cur_end: bool,
    cur_loop: bool,
    /// Predictor history threaded across blocks (full-scale previous samples).
    p1: i32,
    p2: i32,
    /// The four most-recently-decoded samples (oldest → newest) for the
    /// Gaussian window.
    hist: [i32; 4],
    /// Pitch counter: bits 0-11 fractional, bits 12+ = samples to advance.
    interp_pos: u16,
    /// Envelope (ADSR/GAIN).
    pub env: Env,
    /// Key-on startup delay: output is muted for this many samples while the
    /// BRR pipeline fills (SNES ~5-sample decode startup).
    kon_delay: u8,
    /// Reached an END block with no loop: the voice is muted.
    ended: bool,
    /// `OUTX` readback: the enveloped sample >> 8.
    pub outx: i8,
    /// Last enveloped output (`t_output`), for pitch modulation of the next
    /// voice.
    pub output: i32,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            brr_addr: 0,
            loop_addr: 0,
            block: [0; 16],
            block_pos: 16,
            cur_end: false,
            cur_loop: false,
            p1: 0,
            p2: 0,
            hist: [0; 4],
            interp_pos: 0,
            env: Env::default(),
            kon_delay: 0,
            ended: true,
            outx: 0,
            output: 0,
        }
    }
}

impl Voice {
    /// Key-on: read the sample directory entry for `srcn` (at `dir<<8`), point
    /// at the sample's start, decode the first block, and restart the envelope.
    pub fn key_on(&mut self, ram: &[u8; 0x1_0000], dir: u8, srcn: u8) {
        let entry = ((u32::from(dir) << 8) + u32::from(srcn) * 4) as usize;
        let start = u16::from_le_bytes([ram[entry & 0xFFFF], ram[(entry + 1) & 0xFFFF]]);
        let loop_ = u16::from_le_bytes([ram[(entry + 2) & 0xFFFF], ram[(entry + 3) & 0xFFFF]]);
        self.brr_addr = start;
        self.loop_addr = loop_;
        self.p1 = 0;
        self.p2 = 0;
        self.hist = [0; 4];
        self.interp_pos = 0;
        self.ended = false;
        self.kon_delay = 5;
        self.env.key_on();
        self.outx = 0;
        self.output = 0;
        // Decode the first block now; subsequent blocks load on demand.
        let blk = decode_block(ram, self.brr_addr, &mut self.p1, &mut self.p2);
        self.block = blk.samples;
        self.cur_end = blk.end_flag;
        self.cur_loop = blk.loop_flag;
        self.block_pos = 0;
    }

    /// Decode + latch the next BRR block, honoring loop/end and setting `ENDX`.
    fn load_next_block(&mut self, ram: &[u8; 0x1_0000], endx: &mut u8, idx: usize) {
        if self.cur_end {
            *endx |= 1 << idx;
            if self.cur_loop {
                self.brr_addr = self.loop_addr;
            } else {
                // End with no loop: mute the voice and stop consuming samples.
                self.env.level = 0;
                self.ended = true;
            }
        } else {
            self.brr_addr = self.brr_addr.wrapping_add(9);
        }
        let blk = decode_block(ram, self.brr_addr, &mut self.p1, &mut self.p2);
        self.block = blk.samples;
        self.cur_end = blk.end_flag;
        self.cur_loop = blk.loop_flag;
        self.block_pos = 0;
    }

    /// Consume one BRR sample into the Gaussian window.
    fn advance_sample(&mut self, ram: &[u8; 0x1_0000], endx: &mut u8, idx: usize) {
        if self.block_pos >= 16 {
            self.load_next_block(ram, endx, idx);
        }
        let s = i32::from(self.block[self.block_pos]);
        self.block_pos += 1;
        self.hist = [self.hist[1], self.hist[2], self.hist[3], s];
    }

    pub fn write_state(&self, w: &mut crate::state::Writer) {
        w.u16(self.brr_addr);
        w.u16(self.loop_addr);
        for &s in &self.block {
            w.u16(s as u16);
        }
        w.u8(self.block_pos as u8);
        w.bool(self.cur_end);
        w.bool(self.cur_loop);
        w.u32(self.p1 as u32);
        w.u32(self.p2 as u32);
        for &h in &self.hist {
            w.u32(h as u32);
        }
        w.u16(self.interp_pos);
        self.env.write_state(w);
        w.u8(self.kon_delay);
        w.bool(self.ended);
        w.u8(self.outx as u8);
        w.u32(self.output as u32);
    }

    pub fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::StateError> {
        self.brr_addr = r.u16()?;
        self.loop_addr = r.u16()?;
        for s in &mut self.block {
            *s = r.u16()? as i16;
        }
        self.block_pos = r.u8()? as usize;
        self.cur_end = r.bool()?;
        self.cur_loop = r.bool()?;
        self.p1 = r.u32()? as i32;
        self.p2 = r.u32()? as i32;
        for h in &mut self.hist {
            *h = r.u32()? as i32;
        }
        self.interp_pos = r.u16()?;
        self.env.read_state(r)?;
        self.kon_delay = r.u8()?;
        self.ended = r.bool()?;
        self.outx = r.u8()? as i8;
        self.output = r.u32()? as i32;
        Ok(())
    }

    /// Run one output sample. Returns the enveloped sample (`t_output`, ~16-bit)
    /// the DSP scales by `VOL(L/R)`. `noise` supplies the shared noise sample
    /// when this voice reads the noise source instead of its BRR data.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        ram: &[u8; 0x1_0000],
        pitch: u16,
        adsr1: u8,
        adsr2: u8,
        gain: u8,
        counter: u32,
        noise: i32,
        use_noise: bool,
        endx: &mut u8,
        idx: usize,
    ) -> i32 {
        // A sample that ended with no loop stays silent until re-keyed (GAIN
        // must not revive it).
        if self.ended {
            self.outx = 0;
            self.output = 0;
            return 0;
        }

        // Envelope runs even during the key-on startup delay.
        let level = self.env.step(adsr1, adsr2, gain, counter);

        if self.kon_delay > 0 {
            self.kon_delay -= 1;
            self.outx = 0;
            self.output = 0;
            return 0;
        }

        // Advance the BRR read position by the integer part of the pitch step.
        let advanced = self.interp_pos as u32 + u32::from(pitch);
        let steps = advanced >> 12;
        self.interp_pos = (advanced & 0x0FFF) as u16;
        if !self.ended {
            for _ in 0..steps {
                self.advance_sample(ram, endx, idx);
            }
        }

        // Sample source: shared noise, or Gaussian-interpolated BRR.
        let sample = if use_noise {
            noise
        } else {
            gaussian::interpolate(self.hist, self.interp_pos)
        };

        // Apply the envelope: (sample * env) >> 11, clamped to 16-bit.
        let out = ((sample * level) >> 11).clamp(-32768, 32767);
        self.outx = (out >> 8) as i8;
        self.output = out;
        out
    }
}

#[cfg(test)]
#[path = "voice_tests.rs"]
mod tests;
