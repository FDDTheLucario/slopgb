use super::*;

#[test]
fn audio_pacing_requires_a_live_pipe_and_unmuted_sound() {
    assert!(audio_pacing(true, false), "pipe + sound on → audio-paced");
    assert!(
        !audio_pacing(true, true),
        "muted → timer-paced even with a pipe"
    );
    assert!(!audio_pacing(false, false), "no pipe → timer-paced");
    assert!(!audio_pacing(false, true), "no pipe + muted → timer-paced");
}

#[test]
fn frame_interval_caps_to_limit() {
    let real = Duration::from_micros(16742);
    assert_eq!(frame_interval(0, real), real, "0 = real speed");
    assert_eq!(
        frame_interval(30, real),
        Duration::from_secs_f64(1.0 / 30.0)
    );
    // a higher cap is a shorter interval (faster)
    assert!(frame_interval(120, real) < frame_interval(60, real));
}

#[test]
fn turbo_max_frames_scales_and_floors() {
    assert_eq!(turbo_max_frames(10), 10);
    assert_eq!(turbo_max_frames(0), 1, "never zero");
    assert!(turbo_max_frames(20) > turbo_max_frames(5), "monotonic");
}

#[test]
fn apply_gain_scales_mutes_and_downmixes() {
    // identity
    let mut f = vec![(0.4, -0.6)];
    apply_gain(&mut f, 1.0, false);
    assert_eq!(f, vec![(0.4, -0.6)]);
    // half gain halves amplitude
    let mut f = vec![(0.4, -0.6)];
    apply_gain(&mut f, 0.5, false);
    assert_eq!(f, vec![(0.2, -0.3)]);
    // zero gain silences
    let mut f = vec![(0.4, -0.6)];
    apply_gain(&mut f, 0.0, false);
    assert_eq!(f, vec![(0.0, 0.0)]);
    // mono averages L/R (before gain)
    let mut f = vec![(0.4, 0.8)];
    apply_gain(&mut f, 1.0, true);
    assert_eq!(f, vec![(0.6, 0.6)]);
}

// Queue pinned at/above target with no drop is the dead-stream shape under grid
// pacing: the grid keeps emulating (topping the queue up), so "frames emulated"
// no longer implies life — only a draining queue does.
#[test]
fn watchdog_trips_when_queue_pinned_at_target() {
    let mut w = StallWatchdog::new();
    let t0 = Instant::now();
    let target = 2400;
    // First observation records the baseline (queued == target: not below,
    // starts the clock).
    assert!(!w.is_stalled(target, target, t0));
    // Queue stuck at target, never dropping: not stalled until the timeout has
    // fully elapsed.
    assert!(!w.is_stalled(target, target, t0 + AUDIO_STALL_TIMEOUT / 2));
    assert!(!w.is_stalled(target, target, t0 + AUDIO_STALL_TIMEOUT));
    assert!(w.is_stalled(target, target, t0 + AUDIO_STALL_TIMEOUT * 2));
    // Above target (grid topped it up to ~2×target) is equally pinned.
    let mut w = StallWatchdog::new();
    assert!(!w.is_stalled(2 * target, target, t0));
    assert!(w.is_stalled(2 * target, target, t0 + AUDIO_STALL_TIMEOUT * 2));
}

#[test]
fn watchdog_treats_a_dropping_queue_as_progress() {
    let long = AUDIO_STALL_TIMEOUT * 2;
    let target = 2400;
    let mut w = StallWatchdog::new();
    let t0 = Instant::now();
    assert!(!w.is_stalled(target, target, t0));
    // A level drop resets the clock even after a long gap.
    assert!(!w.is_stalled(target - 1, target, t0 + long));
    assert!(!w.is_stalled(target - 1, target, t0 + long + AUDIO_STALL_TIMEOUT / 2));
}

#[test]
fn watchdog_never_trips_while_below_target() {
    let target = 2400;
    let mut w = StallWatchdog::new();
    let t0 = Instant::now();
    // A fast-draining device keeps the queue below target: alive, never stalls,
    // even when the level holds steady far beyond the timeout.
    assert!(!w.is_stalled(target / 2, target, t0));
    assert!(!w.is_stalled(target / 2, target, t0 + AUDIO_STALL_TIMEOUT * 3));
    assert!(!w.is_stalled(target / 2, target, t0 + AUDIO_STALL_TIMEOUT * 6));
}

#[test]
fn watchdog_reset_restarts_grace_period() {
    let target = 2400;
    let mut w = StallWatchdog::new();
    let t0 = Instant::now();
    assert!(!w.is_stalled(target, target, t0));
    w.reset();
    // Stale `progress_at` must not trip right after a reset (unpause).
    assert!(!w.is_stalled(target, target, t0 + AUDIO_STALL_TIMEOUT * 2));
}

#[test]
fn slewed_interval_is_nominal_at_target() {
    let nominal = Duration::from_nanos(16_742_706);
    assert_eq!(slewed_interval(2400, 2400, nominal), nominal);
    // Degenerate target (no device) → nominal, no divide-by-zero.
    assert_eq!(slewed_interval(0, 0, nominal), nominal);
}

#[test]
fn slewed_interval_is_monotonic_and_clamped() {
    let nominal = Duration::from_nanos(16_742_706);
    let target = 2400;
    // Below target → shorter than nominal (produce faster); above → longer.
    assert!(slewed_interval(0, target, nominal) < nominal);
    assert!(slewed_interval(4 * target, target, nominal) > nominal);
    // Monotonic non-decreasing across the range.
    let mut prev = Duration::ZERO;
    for q in (0..=6 * target).step_by(200) {
        let i = slewed_interval(q, target, nominal);
        assert!(i >= prev, "interval must not decrease as queued grows");
        prev = i;
    }
    // Clamped at ±MAX_SLEW: empty and 2×target-below both hit the -1 clamp,
    // huge fills hit +1. Extremes are exactly nominal × (1 ∓ MAX_SLEW).
    let lo = slewed_interval(0, target, nominal);
    let hi = slewed_interval(100 * target, target, nominal);
    assert_eq!(lo, nominal.mul_f64(1.0 - 0.01));
    assert_eq!(hi, nominal.mul_f64(1.0 + 0.01));
    // Beyond the clamp point the value stops moving.
    assert_eq!(slewed_interval(0, target, nominal), lo);
    assert_eq!(slewed_interval(100 * target, target, nominal), hi);
}

#[test]
fn paceband_edges_are_exact() {
    let target = 2400;
    // Strict thresholds → the exact edges are Steady.
    assert_eq!(PaceBand::classify(2 * target, target), PaceBand::Steady);
    assert_eq!(PaceBand::classify(2 * target + 1, target), PaceBand::Hold);
    assert_eq!(PaceBand::classify(target / 2, target), PaceBand::Steady);
    assert_eq!(
        PaceBand::classify(target / 2 - 1, target),
        PaceBand::CatchUp
    );
    // Interior + extremes.
    assert_eq!(PaceBand::classify(target, target), PaceBand::Steady);
    assert_eq!(PaceBand::classify(0, target), PaceBand::CatchUp);
    assert_eq!(PaceBand::classify(10 * target, target), PaceBand::Hold);
}

#[test]
fn wake_plan_hold_emits_nothing_and_parks_the_grid() {
    let target = 2400;
    let now = Instant::now();
    let interval = Duration::from_nanos(16_742_706);
    // Over-full queue: 0 frames, grid parked at now+interval (never past it).
    let (budget, next) = wake_plan(3 * target, target, now, now, interval);
    assert_eq!(budget, 0);
    assert_eq!(next, now + interval);
    // Even with a stale (far-behind) grid, Hold does not fast-forward it.
    let (budget, next) = wake_plan(3 * target, target, now, now - interval * 50, interval);
    assert_eq!(budget, 0);
    assert_eq!(next, now + interval);
}

#[test]
fn wake_plan_steady_owes_the_grid_backlog() {
    let target = 2400;
    let now = Instant::now();
    let interval = Duration::from_nanos(16_742_706);
    // Woke on schedule (grid due at/just before now) → exactly one frame, grid
    // marches forward. The `<= now` boundary is inclusive (matching the timer
    // pacer), so a wake within the current interval owes one, not two.
    let (budget, next) = wake_plan(target, target, now, now - interval / 2, interval);
    assert_eq!(budget, 1);
    assert!(next > now && next <= now + interval);
    // Exactly on the grid tick likewise owes one.
    let (budget, _) = wake_plan(target, target, now, now, interval);
    assert_eq!(budget, 1);
    // Woke early (grid in the future) → nothing owed, grid untouched.
    let future = now + interval / 2;
    let (budget, next) = wake_plan(target, target, now, future, interval);
    assert_eq!(budget, 0);
    assert_eq!(next, future);
}

#[test]
fn wake_plan_catchup_bursts_to_the_cap() {
    let target = 2400;
    let now = Instant::now();
    let interval = Duration::from_nanos(16_742_706);
    // Nearly empty queue → burst MAX_FRAMES_PER_WAKE, rebasing the grid.
    let (budget, next) = wake_plan(0, target, now, now, interval);
    assert_eq!(budget, MAX_FRAMES_PER_WAKE);
    assert_eq!(next, now + interval);
}

#[test]
fn wake_plan_backlog_resync_snaps_to_now() {
    let target = 2400;
    let now = Instant::now();
    let interval = Duration::from_nanos(16_742_706);
    // Steady band but fell way behind (> MAX_FRAMES_PER_WAKE intervals): snap
    // to now rather than fast-forward — exactly one frame, grid at now+interval.
    let stale = now - interval * (MAX_FRAMES_PER_WAKE + 20);
    let (budget, next) = wake_plan(target, target, now, stale, interval);
    assert_eq!(budget, 1);
    assert_eq!(next, now + interval);
    assert!(next >= now, "grid must not stay in the past after resync");
}

// Closed-loop simulation: the slewed grid feeding a chunky device drain. Proves
// the servo settles to one-frame-per-tick with the queue bounded in the Steady
// band, and the long-run rate stays locked to the device clock.
#[test]
fn closed_loop_grid_settles_to_one_frame_per_tick() {
    let nominal = Duration::from_nanos(16_742_706); // ~59.7275 Hz
    let device_rate = 48_000.0_f64;
    let samples_per_frame = device_rate * nominal.as_secs_f64(); // ~803.6
    let target = (device_rate * 0.050) as usize; // 50 ms → 2400
    // Device drains 1024 frames every ~21.3 ms (a typical cpal callback).
    let drain_chunk = 1024.0_f64;
    let drain_period = Duration::from_secs_f64(drain_chunk / device_rate);

    let base = Instant::now();
    let mut now = base;
    let mut next_frame = base;
    let mut next_drain = base + drain_period;
    let mut queued = 0.0_f64;

    let warmup = base + Duration::from_secs(2);
    let sim_end = base + Duration::from_secs(12);
    let mut post_warmup_frames = 0u64;
    let mut max_steady_budget = 0u32;
    let mut measure_from: Option<Instant> = None;

    let mut guard = 0u32;
    while now < sim_end {
        guard += 1;
        assert!(guard < 2_000_000, "simulation failed to advance");
        // Apply every device drain due by `now`.
        while next_drain <= now {
            queued = (queued - drain_chunk).max(0.0);
            next_drain += drain_period;
        }
        let q = queued as usize;
        let interval = slewed_interval(q, target, nominal);
        let (budget, next) = wake_plan(q, target, now, next_frame, interval);
        next_frame = next;
        queued += f64::from(budget) * samples_per_frame;

        if now >= warmup {
            measure_from.get_or_insert(now);
            post_warmup_frames += u64::from(budget);
            max_steady_budget = max_steady_budget.max(budget);
            // The queue never leaves the Steady band once warmed up: no Hold
            // (judder) and no CatchUp (underrun) in steady state.
            assert_eq!(
                PaceBand::classify(queued as usize, target),
                PaceBand::Steady,
                "queue left the Steady band in steady state: queued={}",
                queued as usize,
            );
        }
        // Park until the scheduled next frame (ControlFlow::WaitUntil). The grid
        // always advances strictly past `now`, so the sim can't stall.
        assert!(next_frame > now, "grid must advance past now");
        now = next_frame;
    }

    // At most one frame owed per grid tick in steady state.
    assert_eq!(
        max_steady_budget, 1,
        "steady state must be one frame per tick"
    );

    // Long-run production rate locked to the device clock within ±MAX_SLEW.
    let span = now
        .duration_since(measure_from.expect("warm-up reached"))
        .as_secs_f64();
    let fps = post_warmup_frames as f64 / span;
    let nominal_hz = 1.0 / nominal.as_secs_f64();
    assert!(
        (fps - nominal_hz).abs() / nominal_hz <= 0.01,
        "long-run fps {fps:.4} strayed >1% from nominal {nominal_hz:.4}",
    );
}
