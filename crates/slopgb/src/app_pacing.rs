//! `App` emulation pacing loop: the audio-/timer-/turbo-paced frame drivers,
//! the audio-health fallback, FPS accounting, and the breakpoint-arming that
//! lets a free run halt at a breakpoint. The pacing *primitives* (the audio
//! pipe, watchdog, and the pacing decision) live in [`crate::pacing`].

use std::time::{Duration, Instant};

use slopgb_core::GameBoy;

use crate::pacing::{frame_interval, turbo_max_frames};
use crate::{App, FRAME_DURATION, ui};

/// Upper bound on frames emulated per event-loop wake (non-turbo), so a host
/// that can't keep up stays responsive instead of spiraling.
const MAX_FRAMES_PER_WAKE: u32 = 8;

/// Wall-clock emulation budget per wake while turbo is held.
const TURBO_BUDGET: Duration = Duration::from_millis(10);

impl App {
    /// Drain APU output nobody will play, so the core's sample buffer can't
    /// grow without bound while muted.
    fn discard_audio(&mut self) {
        self.discard_buf.clear();
        self.session.gb.drain_audio(&mut self.discard_buf);
    }

    /// Whether the debugger is "armed": its window is open and at least one
    /// halt source is active — a PC breakpoint, a watchpoint, profiler break
    /// mode, or an Options → Exceptions break condition — so the free-run loop
    /// watches for a halt (`run_frame_until_breakpoint` checks the PC list and,
    /// internally, the core watchpoint / profiler / exception hits).
    fn dbg_armed(&self) -> bool {
        self.tools.is_open(ui::ToolWindow::Debugger)
            && (!self.dbg.breakpoints().is_empty()
                || !self.dbg.watchpoints().is_empty()
                || self.session.gb.profile_break()
                || self.session.gb.exceptions() != 0)
    }

    /// The breakpoint PC list to watch this wake, or `None` when not armed (the
    /// pacers then run plain frames). Computed once before the pacing loop so it
    /// doesn't re-borrow `self` while the audio pipe is held.
    fn run_breakpoints(&self) -> Option<Vec<u16>> {
        self.dbg_armed().then(|| self.dbg.breakpoints().pc_list())
    }

    /// Emulate enough frames to keep the audio queue at its fill target. Returns
    /// the frame count and whether a breakpoint halted emulation.
    pub(crate) fn run_audio_paced(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let mut frames = 0;
        let mut hit = false;
        {
            let Some(pipe) = &mut self.audio else {
                return (0, false);
            };
            while frames < MAX_FRAMES_PER_WAKE && pipe.needs_more() && !hit {
                hit = run_one_frame(&mut self.session.gb, &bps, &mut self.link);
                pipe.pump(&mut self.session.gb);
                frames += 1;
            }
        }
        (frames, hit)
    }

    /// Emulate frames owed according to the wall clock at ~59.7275 Hz.
    pub(crate) fn run_timer_paced(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let now = Instant::now();
        // If we fell far behind (stall, drag, debugger), resync instead of
        // fast-forwarding through the backlog.
        if now.duration_since(self.next_frame) > 8 * FRAME_DURATION {
            self.next_frame = now;
        }
        // Options → Misc → framerate limit (0 = native ~59.7275 Hz).
        let interval = frame_interval(self.settings.framerate_limit, FRAME_DURATION);
        let mut frames = 0;
        let mut hit = false;
        while frames < MAX_FRAMES_PER_WAKE && self.next_frame <= now && !hit {
            hit = run_one_frame(&mut self.session.gb, &bps, &mut self.link);
            self.discard_audio();
            self.next_frame += interval;
            frames += 1;
        }
        (frames, hit)
    }

    /// Turbo: emulate as much as fits in a small wall-clock budget.
    pub(crate) fn run_turbo(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let muted = self.muted;
        // Options → Misc → fast-forward speed caps frames per wake.
        let cap = turbo_max_frames(self.settings.ff_speed);
        let start = Instant::now();
        let mut frames = 0;
        let mut hit = false;
        while start.elapsed() < TURBO_BUDGET && frames < cap && !hit {
            hit = run_one_frame(&mut self.session.gb, &bps, &mut self.link);
            match &mut self.audio {
                // The queue keeps ~250 ms and drops the rest.
                Some(pipe) if !muted => pipe.pump(&mut self.session.gb),
                _ => self.discard_audio(),
            }
            frames += 1;
        }
        self.resync_pacing();
        (frames, hit)
    }

    /// Detect a dead or stalled cpal stream and fall back to wall-clock
    /// pacing, so audio-paced emulation can't freeze forever waiting on a
    /// queue nobody drains.
    pub(crate) fn check_audio_health(&mut self, frames: u32) {
        let Some(pipe) = &self.audio else { return };
        let failed = pipe.failed();
        if failed
            || self
                .watchdog
                .is_stalled(frames, pipe.queued(), Instant::now())
        {
            eprintln!(
                "slopgb: audio stream {}; falling back to timer pacing",
                if failed { "failed" } else { "stalled" }
            );
            self.audio = None;
            self.resync_pacing();
        }
    }

    pub(crate) fn update_fps(&mut self, frames: u32) {
        self.fps_frames += frames;
        let elapsed = self.fps_since.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.fps = f64::from(self.fps_frames) / elapsed.as_secs_f64();
            self.fps_frames = 0;
            self.fps_since = Instant::now();
            self.update_title();
        }
    }
}

/// Run one frame, halting early at a breakpoint when armed, then pump the serial
/// link (swap any completed-transfer byte with the peer). A free function (not a
/// method) so the pacers can call it while the audio pipe holds `&mut
/// self.audio` — it borrows only the disjoint machine + link fields, not all of
/// `self`. `link.pump` is a no-op when no peer is connected. Returns whether a
/// breakpoint stopped the frame.
fn run_one_frame(
    gb: &mut GameBoy,
    breakpoints: &Option<Vec<u16>>,
    link: &mut crate::link::Link,
) -> bool {
    let hit = match breakpoints {
        Some(list) => gb.run_frame_until_breakpoint(list).is_some(),
        None => {
            gb.run_frame();
            false
        }
    };
    link.pump(gb);
    hit
}
