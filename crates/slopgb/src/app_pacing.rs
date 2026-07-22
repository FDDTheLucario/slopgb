//! `App` emulation pacing loop: the audio-/timer-/turbo-paced frame drivers,
//! the audio-health fallback, FPS accounting, and the breakpoint-arming that
//! lets a free run halt at a breakpoint. The pacing *primitives* (the audio
//! pipe, watchdog, and the pacing decision) live in [`crate::pacing`].

use std::time::{Duration, Instant};

use slopgb_core::{CYCLES_PER_FRAME, GameBoy};

use crate::pacing::{
    MAX_FRAMES_PER_WAKE, advance_grid, frame_interval, slewed_interval, turbo_max_frames, wake_plan,
};
use crate::{App, FRAME_DURATION, ui};

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
    /// doesn't re-borrow `self` while the audio pipe is held. When armed (the
    /// debugger is open with a halt source), it first drains any stale hit the
    /// core recorded while the debugger was closed — see
    /// [`Self::drain_stale_debug_hits`].
    fn run_breakpoints(&mut self) -> Option<Vec<(u16, Option<u16>)>> {
        if !self.dbg_armed() {
            return None;
        }
        self.drain_stale_debug_hits();
        Some(self.dbg.breakpoints().bp_list())
    }

    /// Discard any watchpoint / exception-break / profiler-break hit the core
    /// recorded while the debugger was closed. A closed debugger still leaves
    /// watchpoints and the exception mask armed (bgb keeps them too), so
    /// `check_access` keeps setting hits on every CPU access — but the plain
    /// `run_frame` pacing path never consumes them. Opening the debugger would
    /// then replay that stale, wrongly-timed hit as a spurious halt on the first
    /// armed frame. An armed wake consumes a hit the instant it happens
    /// (`run_frame_until_breakpoint` takes it after every step), so a hit still
    /// pending at the *start* of an armed wake is always stale: dropping it here
    /// is safe and never discards a live halt. Golden-safe — `clear_debug_hits`
    /// only clears the core's debug fields, advancing no cycle.
    fn drain_stale_debug_hits(&mut self) {
        self.session.gb.clear_debug_hits();
    }

    /// Emulate frames on the wall-clock `next_frame` grid — one frame → one
    /// present per tick in steady state — with the interval slewed and the
    /// per-wake budget banded by the audio queue level, so the long-run rate
    /// stays locked to the device clock without the audio-callback-cadence
    /// judder of a `needs_more()` refill. Returns the frame count and whether a
    /// breakpoint halted emulation.
    pub(crate) fn run_audio_paced(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let freeze = self.dbg.freezes().list();
        let cheats = self.cheats.pokes();
        // Game Genie ROM patches are a persistent core read-intercept: push the
        // current set once per wake (re-syncs after a ROM reload clears them).
        self.session.gb.set_gg_patches(self.cheats.gg_patches());
        let now = Instant::now();
        let mut frames = 0;
        let mut hit = false;
        {
            let Some(pipe) = &mut self.audio else {
                return (0, false);
            };
            let queued = pipe.queued();
            let target = pipe.target();
            // Audio clock is authoritative when sound is on — the framerate
            // limit applies only to timer pacing, so the nominal is always
            // FRAME_DURATION, slewed by the queue level.
            let interval = slewed_interval(queued, target, FRAME_DURATION);
            let (budget, next) = wake_plan(queued, target, now, self.next_frame, interval);
            self.next_frame = next;
            while frames < budget && !hit {
                hit = run_one_frame(&mut self.session.gb, &bps, &mut self.link, &freeze, &cheats);
                // Unconditional — the device ring drops any overflow. Any MSU-1
                // audio is mixed in by the SGB coprocessor inside `drain_audio`.
                pipe.pump(&mut self.session.gb);
                frames += 1;
                // A silent link peer left the master stalled (run_one_frame
                // timed out): stop the wake instead of blocking again per frame
                // (audio underrun) — the next wake retries.
                if self.session.gb.link_stalled() {
                    break;
                }
            }
        }
        (frames, hit)
    }

    /// Emulate frames owed according to the wall clock at ~59.7275 Hz.
    pub(crate) fn run_timer_paced(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let freeze = self.dbg.freezes().list();
        let cheats = self.cheats.pokes();
        // Game Genie ROM patches are a persistent core read-intercept: push the
        // current set once per wake (re-syncs after a ROM reload clears them).
        self.session.gb.set_gg_patches(self.cheats.gg_patches());
        let now = Instant::now();
        // Options → Misc → framerate limit (0 = native ~59.7275 Hz). The grid
        // resync + owed-frame count is the same march audio pacing uses (shared
        // advance_grid): rebase if we fell far behind, else emit the backlog.
        let interval = frame_interval(self.settings.framerate_limit, FRAME_DURATION);
        let (budget, next) = advance_grid(now, self.next_frame, interval, MAX_FRAMES_PER_WAKE);
        self.next_frame = next;
        let mut frames = 0;
        let mut hit = false;
        while frames < budget && !hit {
            hit = run_one_frame(&mut self.session.gb, &bps, &mut self.link, &freeze, &cheats);
            self.discard_audio();
            frames += 1;
            if self.session.gb.link_stalled() {
                break; // silent peer: stop the wake (see run_audio_paced)
            }
        }
        (frames, hit)
    }

    /// Turbo: emulate as much as fits in a small wall-clock budget.
    pub(crate) fn run_turbo(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let freeze = self.dbg.freezes().list();
        let cheats = self.cheats.pokes();
        // Game Genie ROM patches are a persistent core read-intercept: push the
        // current set once per wake (re-syncs after a ROM reload clears them).
        self.session.gb.set_gg_patches(self.cheats.gg_patches());
        let muted = self.muted;
        // Options → Misc → fast-forward speed caps frames per wake.
        let cap = turbo_max_frames(self.settings.ff_speed);
        let start = Instant::now();
        let mut frames = 0;
        let mut hit = false;
        while start.elapsed() < TURBO_BUDGET && frames < cap && !hit {
            hit = run_one_frame(&mut self.session.gb, &bps, &mut self.link, &freeze, &cheats);
            match &mut self.audio {
                // The queue keeps ~250 ms and drops the rest.
                Some(pipe) if !muted => pipe.pump(&mut self.session.gb),
                _ => self.discard_audio(),
            }
            frames += 1;
            if self.session.gb.link_stalled() {
                break; // silent peer: stop the wake (see run_audio_paced)
            }
        }
        self.resync_pacing();
        (frames, hit)
    }

    /// Detect a dead or stalled cpal stream and fall back to wall-clock
    /// pacing, so audio-paced emulation can't freeze forever topping up a
    /// queue nobody drains.
    pub(crate) fn check_audio_health(&mut self) {
        let Some(pipe) = &self.audio else { return };
        let failed = pipe.failed();
        if failed
            || self
                .watchdog
                .is_stalled(pipe.queued(), pipe.target(), Instant::now())
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
            // GB-CPU-usage meter: the non-halted share of the cycles elapsed
            // this window (same 1 s cadence as the FPS counter). A reset or a
            // state-load rewinds the counters (halt_cycles isn't serialized), so
            // a backward delta just resyncs the baseline and reports 0.
            let (cyc, halt) = (self.session.gb.cycles(), self.session.gb.halt_cycles());
            self.cpu_usage = match (
                cyc.checked_sub(self.cpu_cycles_prev),
                halt.checked_sub(self.cpu_halt_prev),
            ) {
                (Some(dc), Some(dh)) => crate::cpu_usage_pct(dc, dh),
                _ => 0.0,
            };
            self.cpu_cycles_prev = cyc;
            self.cpu_halt_prev = halt;
            self.update_title();
        }
    }
}

/// Emulated cycles per chunk when a link peer is connected: the frontend runs a
/// frame in slices this big, pumping the serial link between each. A slave
/// exchanges one byte per chunk while still advancing a full chunk of cycles per
/// byte, so its serial routine has ample time to prepare each reply — too few
/// cycles per byte and a game's serial handler reads a stale SB and replies with
/// garbage. ~17 chunks/frame ⇒ ~17× the old once-per-frame slave rate, plenty to
/// make a Pokémon trade snappy without overrunning the slave (one slow-clock
/// transfer is 4096 cycles).
const LINK_CHUNK_CYCLES: u32 = 4096;

/// Run one frame, halting early at a breakpoint when armed, then pump the serial
/// link (swap any completed-transfer byte with the peer). A free function (not a
/// method) so the pacers can call it while the audio pipe holds `&mut
/// self.audio` — it borrows only the disjoint machine + link fields, not all of
/// `self`. `link.pump` is a no-op when no peer is connected. Returns whether a
/// breakpoint stopped the frame.
///
/// **Lockstep:** a connected master that runs out of peer bytes *stalls* and
/// `run_slice`/`run_frame` yields; we pump, then block briefly for the peer's
/// reply. When a peer is connected we run the frame in [`LINK_CHUNK_CYCLES`]
/// slices, pumping between each, so a slave (which never stalls) exchanges many
/// bytes per frame while still running a full slice of cycles per byte. With no
/// peer the path is byte-for-byte the old `run_frame` (golden-safe). The
/// debugger path stays a single breakpoint-aware frame.
/// Emulate one frame, then re-apply the freeze list (bgb's frozen values). An
/// empty `freeze` writes nothing, so the golden path stays byte-identical; a
/// non-empty list re-forces each locked byte at the frame boundary via the
/// golden-safe `debug_write`.
fn run_one_frame(
    gb: &mut GameBoy,
    breakpoints: &Option<Vec<(u16, Option<u16>)>>,
    link: &mut crate::link::Link,
    freeze: &[(u16, u8)],
    cheats: &[(u16, u8)],
) -> bool {
    let hit = advance_frame(gb, breakpoints, link);
    // Re-apply freezes + enabled GameShark cheats each frame (same golden-safe
    // debug_write path).
    for &(addr, value) in freeze.iter().chain(cheats) {
        gb.debug_write(addr, value);
    }
    hit
}

fn advance_frame(
    gb: &mut GameBoy,
    breakpoints: &Option<Vec<(u16, Option<u16>)>>,
    link: &mut crate::link::Link,
) -> bool {
    // Debugger armed: a single breakpoint-aware frame (breakpoints take priority
    // over link cadence). A stalled master still pumps for its reply.
    if let Some(list) = breakpoints {
        let hit = gb.run_frame_until_breakpoint(list).is_some();
        link.pump(gb);
        if gb.link_stalled() {
            link.pump_blocking(gb);
        }
        return hit;
    }
    // No peer: unchanged full frame (golden-safe, no chunking overhead).
    if !link.is_connected() {
        gb.run_frame();
        link.pump(gb);
        return false;
    }
    // Linked: run the frame in chunks, pumping between each. The master stall
    // breaks a chunk early (per-byte); the slave runs full chunks (one byte per
    // pump, with cycles to spare). A silent peer times out and yields the
    // partial frame (resumed next tick).
    run_chunked_linked_frame(gb, link);
    false
}

/// Run one frame in [`LINK_CHUNK_CYCLES`] slices, pumping the link between each,
/// and return the number of slices run. A connected slave exchanges a byte per
/// slice; a stalled master whose peer stays silent (`pump_blocking` yields no
/// reply) breaks the frame early, resuming next tick. Extracted from
/// [`advance_frame`] so the chunk cadence is unit-testable without a live socket:
/// a disconnected frontend [`Link`](crate::link::Link) makes `pump`/`pump_blocking`
/// inert, so the loop is driven purely by the core's frame boundary and stall.
fn run_chunked_linked_frame(gb: &mut GameBoy, link: &mut crate::link::Link) -> usize {
    let target = gb.frame_count().wrapping_add(1);
    let deadline = gb.cycles().wrapping_add(u64::from(CYCLES_PER_FRAME));
    let mut chunks = 0;
    while gb.frame_count() != target && gb.cycles() < deadline {
        gb.run_slice(LINK_CHUNK_CYCLES);
        chunks += 1;
        link.pump(gb);
        if gb.link_stalled() && !link.pump_blocking(gb) {
            break;
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use slopgb_core::{Model, Watchpoint};

    /// A blank, muted, no-ROM `App` (as `main` builds it when launched without a
    /// ROM) whose live machine runs `rom`. Only `session.gb` is swapped — enough
    /// to drive the pacing seams headlessly.
    fn app_running(rom: Vec<u8>) -> App {
        let opts = crate::cli::Options {
            rom: None,
            model: None,
            scale: 3,
            mute: true,
            boot: None,
            sgb_bios: None,
            mcp_port: None,
            plugins_dir: None,
            msu1: None,
            ram_init: None,
        };
        let mut app = App::new(
            opts,
            crate::session::Session::blank(Model::Dmg),
            false,
            None,
            None,
        );
        app.session.gb = GameBoy::new(Model::Dmg, rom).expect("valid test ROM");
        app
    }

    /// ROM: write 0x42 to WRAM `0xC000` once, then self-loop (never touching
    /// `0xC000` again) — a write-watchpoint on `0xC000` fires exactly once, early.
    fn write_once_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0100..0x0107].copy_from_slice(&[
            0x3E, 0x42, // ld a, 0x42
            0xEA, 0x00, 0xC0, // ld (0xC000), a
            0x18, 0xFE, // jr -2   (self-loop at 0x0105)
        ]);
        rom
    }

    /// ROM: arm a master (internal-clock) serial transfer, then self-loop — with
    /// a peer attached to the core it stalls (lockstep) awaiting the reply byte.
    fn master_xfer_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0100..0x010A].copy_from_slice(&[
            0x3E, 0x00, // ld a, 0
            0xE0, 0x01, // ldh (FF01), a   ; SB
            0x3E, 0x81, // ld a, 0x81
            0xE0, 0x02, // ldh (FF02), a   ; SC = transfer + internal clock
            0x18, 0xFE, // jr -2
        ]);
        rom
    }

    const WP_C000_WRITE: Watchpoint = Watchpoint {
        addr: 0xC000,
        read: false,
        write: true,
    };

    // ---- Task A: stale debugger-hit discard ----

    #[test]
    fn opening_debugger_discards_a_hit_recorded_while_closed() {
        let mut app = app_running(write_once_rom());
        // Debugger CLOSED but the watchpoint is armed in the live core (bgb keeps
        // watchpoints armed regardless of the window). With the window shut,
        // `dbg_armed()` is false, so the pacers drive plain `run_frame` — which
        // records the hit but never consumes it.
        app.session.gb.set_watchpoints(&[WP_C000_WRITE]);
        app.session.gb.run_frame(); // the one 0xC000 write records a pending watch_hit
        // Opening the debugger begins an armed wake, which drains stale hits before
        // the first breakpoint-aware frame. Headless can't open a real window (so
        // `dbg_armed()`/`run_breakpoints` can't fire), so we call the drain that an
        // armed wake performs — the exact seam under test.
        app.drain_stale_debug_hits();
        // The CPU is parked in a self-loop that never touches 0xC000, so a genuine
        // hit is impossible this frame: any halt here is the drained-away stale one.
        assert_eq!(
            app.session.gb.run_frame_until_breakpoint(&[]),
            None,
            "no spurious halt from a hit recorded while the debugger was closed",
        );
    }

    #[test]
    fn a_watchpoint_hit_after_opening_still_halts() {
        let mut app = app_running(write_once_rom());
        app.session.gb.set_watchpoints(&[WP_C000_WRITE]);
        // Debugger just opened (armed): drain first, exactly as an armed wake does.
        app.drain_stale_debug_hits();
        // The 0xC000 write happens *this* (armed) frame, after the drain — it must
        // still surface as a real, correctly-timed halt.
        assert_eq!(
            app.session.gb.run_frame_until_breakpoint(&[]),
            Some(0xC000),
            "a hit occurring after the debugger opened is surfaced",
        );
    }

    // ---- Task B: linked-frame chunk cadence ----

    #[test]
    fn linked_frame_runs_the_expected_chunk_cadence() {
        // All-NOP ROM: no serial, so the frame runs to its boundary uninterrupted.
        let mut gb = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
        let mut link = crate::link::Link::new(); // no peer: pump/pump_blocking inert
        gb.run_frame(); // align to a frame boundary so the next span is a full frame
        let chunks = run_chunked_linked_frame(&mut gb, &mut link);
        // CYCLES_PER_FRAME (70224) / LINK_CHUNK_CYCLES (4096) = 17.14 ⇒ 18 slices
        // (17 full + one partial that reaches the frame boundary).
        assert_eq!(
            chunks, 18,
            "one linked frame runs in 18 chunks (17 full 4096-cycle slices + a partial)",
        );
    }

    #[test]
    fn linked_frame_breaks_early_on_a_silent_peer() {
        let mut gb = GameBoy::new(Model::Dmg, master_xfer_rom()).unwrap();
        gb.link_connect(true); // the core has a peer attached → the master goes lockstep
        let mut link = crate::link::Link::new(); // ...but the frontend link is silent
        let chunks = run_chunked_linked_frame(&mut gb, &mut link);
        assert!(
            gb.link_stalled(),
            "the master stalled awaiting the silent peer's reply byte",
        );
        assert!(
            chunks <= 2,
            "a silent peer breaks the frame early ({chunks} chunks), not the full 18-chunk frame",
        );
    }
}
