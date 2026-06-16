//! Emulation pacing helpers: the audio pipeline (core → resampler → device
//! queue), a watchdog for a dead cpal stream, and the audio-vs-timer pacing
//! decision. The pacing *loop* lives on `App` in [`crate::app_pacing`].

use std::time::{Duration, Instant};

use slopgb_core::GameBoy;

use crate::audio::{AudioOutput, Resampler};

/// The APU's default output rate, exported by the core so the resampler ratio
/// can't silently drift from it.
use slopgb_core::DEFAULT_SAMPLE_RATE as CORE_SAMPLE_RATE;

/// Audio-driven pacing keeps about this much queued for the device.
const AUDIO_TARGET_MS: u64 = 50;

/// Audio-paced emulation falls back to wall-clock pacing if the device queue
/// stops draining for this long (the cpal stream stalled or died without
/// reporting an error).
pub(crate) const AUDIO_STALL_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) struct AudioPipe {
    out: AudioOutput,
    resampler: Resampler,
    /// Queue fill target in device-rate frames (~[`AUDIO_TARGET_MS`]).
    target_fill: usize,
    /// Scratch: samples drained from the core (core rate).
    drain_buf: Vec<(f32, f32)>,
    /// Scratch: resampled samples (device rate).
    device_buf: Vec<(f32, f32)>,
}

impl AudioPipe {
    pub(crate) fn new(out: AudioOutput) -> Self {
        let rate = out.sample_rate();
        Self {
            resampler: Resampler::new(CORE_SAMPLE_RATE, rate),
            target_fill: usize::try_from(u64::from(rate) * AUDIO_TARGET_MS / 1000)
                .unwrap_or(usize::MAX),
            out,
            drain_buf: Vec::new(),
            device_buf: Vec::new(),
        }
    }

    /// Move all pending core samples to the device queue (resampling on the
    /// way). Excess beyond the queue capacity is dropped by `push`.
    pub(crate) fn pump(&mut self, gb: &mut GameBoy) {
        self.drain_buf.clear();
        gb.drain_audio(&mut self.drain_buf);
        self.device_buf.clear();
        self.resampler.run(&self.drain_buf, &mut self.device_buf);
        self.out.push(&self.device_buf);
    }

    pub(crate) fn needs_more(&self) -> bool {
        self.out.queued() < self.target_fill
    }

    /// The device queue's current fill level (device-rate frames).
    pub(crate) fn queued(&self) -> usize {
        self.out.queued()
    }

    /// Whether the cpal stream reported a fatal error.
    pub(crate) fn failed(&self) -> bool {
        self.out.failed()
    }
}

/// Watchdog for a dead audio stream. Audio-paced emulation only makes
/// progress when the device drains the queue, so "zero frames emulated and
/// the queue level never dropping" sustained for [`AUDIO_STALL_TIMEOUT`]
/// means the stream is stalled even if cpal never reported an error — and
/// without intervention the emulator would silently freeze.
pub(crate) struct StallWatchdog {
    /// Queue level at the last observation.
    last_queued: usize,
    /// Last time the queue drained or emulation produced frames.
    progress_at: Instant,
}

impl StallWatchdog {
    pub(crate) fn new() -> Self {
        Self {
            last_queued: usize::MAX,
            progress_at: Instant::now(),
        }
    }

    /// Restart the grace period (after pause, resume, audio re-open).
    pub(crate) fn reset(&mut self) {
        self.last_queued = usize::MAX;
        self.progress_at = Instant::now();
    }

    /// Record one wake's outcome; true if the stream looks stalled.
    pub(crate) fn is_stalled(&mut self, frames_emulated: u32, queued: usize, now: Instant) -> bool {
        if frames_emulated > 0 || queued < self.last_queued {
            self.last_queued = queued;
            self.progress_at = now;
            return false;
        }
        self.last_queued = queued;
        now.duration_since(self.progress_at) > AUDIO_STALL_TIMEOUT
    }
}

/// Whether to pace against the audio queue this wake: only with a live pipe and
/// sound un-muted. When muted the pipe stays open (drains to silence) but the
/// timer paces instead, so toggling "Enable sound" never tears down the stream.
/// A free function for the truth-table test (an `App` with a real pipe can't be
/// built headless).
#[must_use]
pub(crate) fn audio_pacing(has_audio: bool, muted: bool) -> bool {
    has_audio && !muted
}

#[cfg(test)]
#[path = "pacing_tests.rs"]
mod tests;
