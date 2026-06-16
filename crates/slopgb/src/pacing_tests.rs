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
fn watchdog_trips_after_sustained_stall() {
    let mut w = StallWatchdog::new();
    let t0 = Instant::now();
    // First observation records the baseline.
    assert!(!w.is_stalled(0, 100, t0));
    // Queue stuck at the same level, no frames: not stalled until the
    // timeout has fully elapsed.
    assert!(!w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT / 2));
    assert!(!w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT));
    assert!(w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT * 2));
}

#[test]
fn watchdog_treats_drain_or_frames_as_progress() {
    let long = AUDIO_STALL_TIMEOUT * 2;
    // Queue level dropping counts as progress.
    let mut w = StallWatchdog::new();
    let t0 = Instant::now();
    assert!(!w.is_stalled(0, 100, t0));
    assert!(!w.is_stalled(0, 99, t0 + long));
    assert!(!w.is_stalled(0, 99, t0 + long + AUDIO_STALL_TIMEOUT / 2));
    // Emulated frames count as progress even if the queue grew.
    let mut w = StallWatchdog::new();
    assert!(!w.is_stalled(0, 100, t0));
    assert!(!w.is_stalled(3, 200, t0 + long));
    assert!(!w.is_stalled(0, 200, t0 + long + AUDIO_STALL_TIMEOUT / 2));
}

#[test]
fn watchdog_reset_restarts_grace_period() {
    let mut w = StallWatchdog::new();
    let t0 = Instant::now();
    assert!(!w.is_stalled(0, 100, t0));
    w.reset();
    // Stale `progress_at` must not trip right after a reset (unpause).
    assert!(!w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT * 2));
}
