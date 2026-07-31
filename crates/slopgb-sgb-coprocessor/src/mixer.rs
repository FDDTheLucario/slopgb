//! The audio output stage: resampling the 32 kHz S-DSP and 44.1 kHz MSU-1
//! sources onto the host output rate, then mixing that stream into (or
//! draining it out of) the Game Boy audio.

use super::*;

impl SgbCoprocessor {
    /// Emit the output-rate samples owed for a `span` of GB T-cycles, resampling
    /// the 32 kHz S-DSP source by holding the current sample (32 kHz < output).
    pub(crate) fn emit_output(&mut self, span: u64) {
        self.samp_acc += span as f64;
        while self.samp_acc >= self.cycles_per_sample {
            self.samp_acc -= self.cycles_per_sample;
            self.src_acc += DSP_RATE;
            while self.src_acc >= f64::from(self.out_rate) {
                self.src_acc -= f64::from(self.out_rate);
                if let Some(s) = self.src.pop_front() {
                    self.cur = s;
                }
            }
            // The MSU-1 source (44.1 kHz) rides the same output timeline. Unlike
            // the S-DSP hold, an underrun/stop drops to silence (pop-or-zero) so a
            // finished track leaves no held-DC click.
            self.msu_src_acc += MSU_RATE;
            while self.msu_src_acc >= f64::from(self.out_rate) {
                self.msu_src_acc -= f64::from(self.out_rate);
                self.msu_cur = self.msu_src.pop_front().unwrap_or((0, 0));
            }
            let amp = f32::from(self.cur.0.unsigned_abs().max(self.cur.1.unsigned_abs()));
            if amp > self.dbg_pcm_peak {
                self.dbg_pcm_peak = amp;
            }
            if self.out.len() < self.max_out {
                self.out.push((
                    f32::from(self.cur.0) * MIX_SCALE + f32::from(self.msu_cur.0) * MSU_MIX_SCALE,
                    f32::from(self.cur.1) * MIX_SCALE + f32::from(self.msu_cur.1) * MSU_MIX_SCALE,
                ));
            }
        }
    }

    pub(crate) fn mix_into(&mut self, gb: &mut [(f32, f32)]) {
        // The SGB hardware mix routes the GB audio into the SNES mixer below the
        // SNES-side level: duck the GB channels when the SNES side is the primary
        // voice — the resident N-SPC driver (--sgb-bios) or a playing MSU-1 track
        // (whose game has already muted its own GB music, leaving only SFX). With
        // neither, leave the GB channels at full so nothing is quieted for no
        // benefit (clean-room square SFX / no enhancement).
        if self.nspc_resident || self.msu_playing {
            for s in gb.iter_mut() {
                s.0 *= GB_GAIN;
                s.1 *= GB_GAIN;
            }
        }
        let n = gb.len().min(self.out.len());
        for (dst, src) in gb.iter_mut().zip(self.out.iter()).take(n) {
            dst.0 += src.0;
            dst.1 += src.1;
        }
        self.out.drain(..n);
    }

    pub(crate) fn set_output_rate(&mut self, hz: u32) {
        let hz = hz.max(1);
        self.out_rate = hz;
        self.cycles_per_sample = f64::from(GB_CLOCK_HZ) / f64::from(hz);
        self.max_out = hz as usize;
        self.samp_acc = 0.0;
        self.src_acc = 0.0;
        self.src.clear();
        self.out.clear();
        self.msu_src_acc = 0.0;
        self.msu_src.clear();
        self.msu_cur = (0, 0);
    }

    /// Drain the stereo output-rate PCM synthesized since the last drain, oldest
    /// first — the equivalent of the tier-3 plugin ABI's `drain_pcm`, for a host
    /// that would rather pull the samples than have them mixed in.
    pub fn drain_pcm(&mut self) -> Vec<(f32, f32)> {
        std::mem::take(&mut self.out)
    }
}
