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

/// Upper bound on frames emulated per event-loop wake (non-turbo), so a host
/// that can't keep up — or a resume/underrun burst — stays bounded instead of
/// spiraling. Shared by both the audio grid ([`wake_plan`]) and the timer pacer.
pub(crate) const MAX_FRAMES_PER_WAKE: u32 = 8;

/// Maximum fractional adjustment the audio-queue servo applies to the nominal
/// frame interval (a 1% pitch nudge is inaudible). At the clamp extremes the
/// grid produces frames 1% faster (queue empty) or slower (queue full) than
/// nominal — enough to absorb device-vs-monotonic clock drift (≪ 0.5% in
/// practice) while keeping the long-run rate locked to the device clock.
const MAX_SLEW: f64 = 0.01;

/// Audio-paced emulation falls back to wall-clock pacing if the device queue
/// stops draining for this long (the cpal stream stalled or died without
/// reporting an error).
pub(crate) const AUDIO_STALL_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) struct AudioPipe {
    out: AudioOutput,
    resampler: Resampler,
    /// Queue fill target in device-rate frames (~[`AUDIO_TARGET_MS`]).
    target_fill: usize,
    /// Master volume gain (Options → Sound → Volume), 0.0..=1.0.
    gain: f32,
    /// Mono downmix (Options → Sound → mono output).
    mono: bool,
    /// Scratch: samples drained from the core (core rate).
    drain_buf: Vec<(f32, f32)>,
    /// Scratch: resampled samples (device rate).
    device_buf: Vec<(f32, f32)>,
    /// Audio recorder (Joypad → "Audio"): `Some` accumulates every core-rate
    /// frame drained here, taken by the frontend when recording stops.
    record: Option<Vec<(f32, f32)>>,
}

impl AudioPipe {
    /// Build the pipe, choosing resampler quality (Sound → "High quality sound
    /// rendering"): `hq` = linear interpolation, false = zero-order hold.
    pub(crate) fn new_with_quality(out: AudioOutput, hq: bool) -> Self {
        let rate = out.sample_rate();
        Self {
            resampler: Resampler::new_quality(CORE_SAMPLE_RATE, rate, hq),
            target_fill: usize::try_from(u64::from(rate) * AUDIO_TARGET_MS / 1000)
                .unwrap_or(usize::MAX),
            out,
            gain: 1.0,
            mono: false,
            drain_buf: Vec::new(),
            device_buf: Vec::new(),
            record: None,
        }
    }

    /// The core APU sample rate the recorder's samples are at (WAV rate).
    pub(crate) fn record_rate() -> u32 {
        CORE_SAMPLE_RATE
    }

    /// Start accumulating recorded audio (Joypad → "Audio").
    pub(crate) fn start_record(&mut self) {
        self.record = Some(Vec::new());
    }

    /// Whether audio is currently being recorded.
    pub(crate) fn is_recording(&self) -> bool {
        self.record.is_some()
    }

    /// Stop recording and take the accumulated core-rate frames (empty if none).
    pub(crate) fn take_record(&mut self) -> Vec<(f32, f32)> {
        self.record.take().unwrap_or_default()
    }

    /// Update the master volume gain + mono downmix (from Options → Sound).
    pub(crate) fn set_volume(&mut self, gain: f32, mono: bool) {
        self.gain = gain.clamp(0.0, 1.0);
        self.mono = mono;
    }

    /// Move all pending core samples to the device queue (resampling on the
    /// way). Excess beyond the queue capacity is dropped by `push`.
    pub(crate) fn pump(&mut self, gb: &mut GameBoy) {
        self.pump_mixing(gb, &[]);
    }

    /// Like [`Self::pump`], but adds `extra` (core-rate samples, e.g. an MSU-1
    /// track's resampled PCM) into the Game Boy stream sample-for-sample before
    /// the device resample + gain. Both are frame-locked, so their counts match
    /// within a sample or two; the mix runs to the shorter of the two and drops
    /// any residual tail (as the built-in SGB `mix_into` does). An empty `extra`
    /// is exactly [`Self::pump`] (byte-identical output).
    pub(crate) fn pump_mixing(&mut self, gb: &mut GameBoy, extra: &[(f32, f32)]) {
        self.drain_buf.clear();
        gb.drain_audio(&mut self.drain_buf);
        for (dst, src) in self.drain_buf.iter_mut().zip(extra) {
            dst.0 += src.0;
            dst.1 += src.1;
        }
        // Recorder (Joypad → "Audio"): capture the mixed core-rate stream before
        // the device resample + gain, so the WAV is the game's own output.
        if let Some(rec) = &mut self.record {
            rec.extend_from_slice(&self.drain_buf);
        }
        self.device_buf.clear();
        self.resampler.run(&self.drain_buf, &mut self.device_buf);
        apply_gain(&mut self.device_buf, self.gain, self.mono);
        self.out.push(&self.device_buf);
    }

    /// The device queue's current fill level (device-rate frames).
    pub(crate) fn queued(&self) -> usize {
        self.out.queued()
    }

    /// The queue fill target in device-rate frames (~[`AUDIO_TARGET_MS`]); the
    /// setpoint the servo ([`slewed_interval`]) and band classifier
    /// ([`PaceBand::classify`]) steer toward.
    pub(crate) fn target(&self) -> usize {
        self.target_fill
    }

    /// Whether the cpal stream reported a fatal error.
    pub(crate) fn failed(&self) -> bool {
        self.out.failed()
    }
}

/// Watchdog for a dead audio stream. Under grid pacing frames are emulated on
/// the wall clock regardless of the queue, so "frames emulated" no longer
/// implies a live device — only the *queue draining* does. Progress therefore
/// means the queue level dropped, or sits below `target` (a device eating
/// everything is alive). A stall is the queue pinned at/above `target` with no
/// drop for [`AUDIO_STALL_TIMEOUT`] — the grid keeps topping it up to ~2×target
/// (then `Hold`s), so a dead device leaves it stuck high — even if cpal never
/// reported an error; without intervention the emulator would run silent.
pub(crate) struct StallWatchdog {
    /// Queue level at the last observation.
    last_queued: usize,
    /// Last time the queue drained or sat below target.
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

    /// Record one wake's outcome; true if the stream looks stalled. Progress ⇔
    /// the queue level dropped since last wake, or sits below `target` (a
    /// fast-draining device never trips). Otherwise the queue is pinned ≥ target
    /// — a stall once that has held for [`AUDIO_STALL_TIMEOUT`].
    pub(crate) fn is_stalled(&mut self, queued: usize, target: usize, now: Instant) -> bool {
        let progressed = queued < self.last_queued || queued < target;
        self.last_queued = queued;
        if progressed {
            self.progress_at = now;
            return false;
        }
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

/// Target wall-clock interval per emulated frame given the Options framerate
/// limit. `limit == 0` means "real speed" (the native ~59.7275 Hz `default`);
/// a positive limit paces to `1/limit` seconds per frame.
#[must_use]
pub(crate) fn frame_interval(limit: u32, default: Duration) -> Duration {
    if limit == 0 {
        default
    } else {
        Duration::from_secs_f64(1.0 / f64::from(limit))
    }
}

/// Nominal frame interval slewed by the audio-queue fill level: a pure
/// P-controller that keeps the long-run production rate locked to the device
/// clock without drift. When the queue sits below `target` the interval shrinks
/// (produce slightly faster to refill); above `target` it grows (slow down).
/// The error term `(queued − target)/target` is clamped to ±1 and scaled by
/// [`MAX_SLEW`], so the interval stays within ±1% of `nominal`. A steady-state
/// P-offset is fine — the queue just settles slightly off `target`.
///
/// Monotonic non-decreasing in `queued`: more queued → longer interval (slower
/// production). `target == 0` (no device) degenerates to `nominal`.
#[must_use]
pub(crate) fn slewed_interval(queued: usize, target: usize, nominal: Duration) -> Duration {
    if target == 0 {
        return nominal;
    }
    let err = (queued as f64 - target as f64) / target as f64;
    let slew = err.clamp(-1.0, 1.0) * MAX_SLEW;
    nominal.mul_f64(1.0 + slew)
}

/// The per-wake frame budget band, from the audio-queue fill level. Bounds the
/// transient behavior so post-turbo (over-full) and resume/underrun (empty)
/// states don't judder or spiral; the normal `Steady` band emits exactly the
/// frames the wall-clock grid owes (≤1 per wake in practice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaceBand {
    /// Queue over-full (`queued > 2×target`, e.g. right after turbo leaves
    /// ~250 ms queued): emulate 0 frames until it drains back toward target.
    Hold,
    /// Normal band: emit the frames owed by the `next_frame` grid.
    Steady,
    /// Queue nearly empty (`queued < target/2`, e.g. resume from
    /// pause/breakpoint): burst up to [`MAX_FRAMES_PER_WAKE`] to refill — a few
    /// dropped presents are acceptable while audio continuity is restored.
    CatchUp,
}

impl PaceBand {
    /// Classify the fill level into a band. Thresholds are strict, so the exact
    /// edges (`2×target`, `target/2`) fall in `Steady`.
    #[must_use]
    pub(crate) fn classify(queued: usize, target: usize) -> Self {
        if queued > 2 * target {
            PaceBand::Hold
        } else if queued < target / 2 {
            PaceBand::CatchUp
        } else {
            PaceBand::Steady
        }
    }
}

/// The audio-paced per-wake decision, factored pure for testing: given the
/// queue level, the wall-clock grid position (`next_frame`) and the slewed
/// `interval`, return how many frames to emulate this wake and the updated
/// `next_frame`. The caller emulates up to `budget` frames (breaking early on a
/// breakpoint / link stall) and stores the returned grid position.
///
/// - **Backlog resync:** fell more than [`MAX_FRAMES_PER_WAKE`] intervals behind
///   (stall, drag, debugger) → rebase the grid to `now` instead of
///   fast-forwarding through the whole backlog.
/// - **Hold:** 0 frames; park the grid at `now + interval` so no backlog accrues
///   while the over-full queue drains.
/// - **Steady:** the frames the grid owes (`next_frame ≤ now`), capped at
///   [`MAX_FRAMES_PER_WAKE`] — ≤1 per wake in steady state.
/// - **CatchUp:** a bounded burst of [`MAX_FRAMES_PER_WAKE`] to refill the queue,
///   rebasing the grid to `now + interval` (the burst decouples from the grid).
#[must_use]
pub(crate) fn wake_plan(
    queued: usize,
    target: usize,
    now: Instant,
    next_frame: Instant,
    interval: Duration,
) -> (u32, Instant) {
    match PaceBand::classify(queued, target) {
        PaceBand::Hold => (0, now + interval),
        PaceBand::CatchUp => (MAX_FRAMES_PER_WAKE, now + interval),
        PaceBand::Steady => advance_grid(now, next_frame, interval, MAX_FRAMES_PER_WAKE),
    }
}

/// Resync-then-march the wall-clock frame grid: rebase to `now` if we fell more
/// than `cap` intervals behind (rather than fast-forwarding the whole backlog),
/// then count the frames the grid owes (`next_frame <= now`, inclusive), capped
/// at `cap`, advancing the grid past each. Shared by the timer pacer
/// ([`crate::App::run_timer_paced`]) and [`wake_plan`]'s Steady band so the two
/// can't drift apart.
#[must_use]
pub(crate) fn advance_grid(
    now: Instant,
    next_frame: Instant,
    interval: Duration,
    cap: u32,
) -> (u32, Instant) {
    let mut next = if now.duration_since(next_frame) > interval * cap {
        now
    } else {
        next_frame
    };
    let mut budget = 0;
    while next <= now && budget < cap {
        next += interval;
        budget += 1;
    }
    (budget, next)
}

/// Max frames to emulate per turbo wake, scaling with the Options fast-forward
/// speed (monotonic; clamped to at least 1 so turbo always advances).
#[must_use]
pub(crate) fn turbo_max_frames(ff_speed: u32) -> u32 {
    ff_speed.max(1)
}

/// Apply the master volume `gain` (and optional `mono` downmix) to a batch of
/// stereo frames in place. `gain` 1.0 + `mono` false is the identity.
pub(crate) fn apply_gain(frames: &mut [(f32, f32)], gain: f32, mono: bool) {
    let g = gain.clamp(0.0, 1.0);
    for f in frames.iter_mut() {
        let (mut l, mut r) = *f;
        if mono {
            let m = 0.5 * (l + r);
            l = m;
            r = m;
        }
        *f = (l * g, r * g);
    }
}

#[cfg(test)]
#[path = "pacing_tests.rs"]
mod tests;
