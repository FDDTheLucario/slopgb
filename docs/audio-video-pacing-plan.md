# Audio-paced video judder fix (/tdd-test-plan)

With sound enabled the picture updates roughly every other frame even though the
FPS counter reads ~59.7. Timing is correct; **presentation** is not. Frontend-only,
no core change → trivially golden-safe.

## Diagnosis (2026-07-12)

`run_audio_paced` (`app_pacing.rs`) batches up to `MAX_FRAMES_PER_WAKE` frames per
event-loop wake to refill the audio queue to its ~50 ms target, and `about_to_wait`
(`app_handler.rs`) issues **one** `request_redraw()` per wake, after the batch. The
audio queue drains in device-callback steps (typ. 1024 frames ≈ 21.3 ms @ 48 kHz —
**more than one frame of audio**, ~16.74 ms), so the steady-state cycle is:

1. Callback drains 21.3 ms → queue below target.
2. Next wake: `needs_more()` → emulate frame N, still short → emulate frame N+1,
   target met.
3. One redraw. The framebuffer holds only the latest frame — **frame N is emulated
   but never presented.**
4. Wakes every `FRAME_DURATION/4` find the queue full → 0 frames, no redraw, until
   the next callback.

Presents land at the *audio callback cadence*, alternating 1- and 2-frame bursts,
dropping the first frame of every 2-burst. The FPS counter is honest but counts
**emulated** frames (`update_fps(frames)`), which really do average 59.7/s — it
never counts presents. Sound-off is smooth because `run_timer_paced` wakes exactly
on the `next_frame` grid: one frame, one present, per wake.

## Rejected approaches (do not re-chase)

- **Present inside the batch loop (per emulated frame):** softbuffer/winit presents
  the single latest framebuffer on `RedrawRequested`; there is no queue of frames
  to present. Dead on arrival.
- **Cap audio-paced emulation at 1 frame per wake, keep the `needs_more()` gate:**
  the two catch-up frames then land ~4.2 ms apart (the `FRAME_DURATION/4` wake
  cadence), still inside one 60 Hz refresh interval — the compositor samples only
  the latest present, so alternate frames still vanish. It shifts the judder, it
  doesn't fix it.
- **Bigger audio target / smaller device buffer:** tuning around the beat between
  the audio callback period and the frame period, not removing it. Host-dependent,
  regresses elsewhere.

## Design: slewed wall-clock grid, audio queue as rate servo

Emit frames on a **wall-clock grid** exactly like timer pacing (one frame → one
present per `next_frame` tick), but let the audio queue level *slew* the interval
so the long-term rate stays locked to the audio device clock (the current scheme's
one real virtue — no drift, no periodic underrun):

- `slewed_interval(queued, target, device_rate, nominal) -> Duration` — pure
  P-controller: `nominal × (1 + clamp((queued − target)/target, −1, 1) × MAX_SLEW)`
  with `MAX_SLEW` ≈ 0.5–2%. Queue below target → slightly faster production; above
  → slightly slower. Absorbs device-vs-monotonic clock drift (≪ 0.5% in practice).
  Steady-state P-offset is fine — the queue just settles slightly off target.
- Band classifier for the per-wake frame budget, so transitions stay bounded:
  - **Hold** (`queued > 2×target`, e.g. right after turbo leaves ~250 ms queued):
    emulate 0 frames until the queue drains toward target — identical to today's
    post-turbo behavior, no regression.
  - **Steady** (the normal band): frames owed by the `next_frame` grid, at most 1
    per wake in practice; interval slewed as above.
  - **CatchUp** (`queued < target/2`, e.g. resume from pause/breakpoint): allow up
    to `MAX_FRAMES_PER_WAKE`, dropped presents acceptable transiently — audio
    continuity wins for a few wakes.
  Steady-state oscillation is ~one callback period (≈21 ms) around the 50 ms
  target, comfortably inside the Steady band on typical hosts.
- `about_to_wait` audio flow becomes `ControlFlow::WaitUntil(self.next_frame)` —
  same as timer pacing (and fewer wakeups than today's `FRAME_DURATION/4` poll).
  `resync_pacing()` already resets `next_frame` on every mode transition
  (pause/unpause, mute toggle, turbo exit, reset) — audit each call site, no new
  resync mechanism expected.
- **Watchdog re-spec:** today's "progress" (`frames_emulated > 0` or queue drop)
  breaks — the grid keeps emulating frames even against a dead stream, so the
  frames term would mask a stall forever. New rule: progress ⇔ the queue level
  *dropped*, or sits *below* target (a device eating everything is alive). Stall ⇔
  queue pinned ≥ target with no drop for `AUDIO_STALL_TIMEOUT`. `failed()` path
  unchanged.
- Semantics kept: `framerate_limit` still applies only to timer pacing (audio
  clock stays authoritative when sound is on); mute/turbo/link behavior untouched.

```xml
<plan goal="Smooth per-frame presentation under audio pacing via a slewed wall-clock grid">
  <task id="1" model="sonnet" deps="none">
    <do>pacing.rs: pure servo math — `slewed_interval(queued, target, nominal) -> Duration` (P-term clamped to ±MAX_SLEW) and `PaceBand::classify(queued, target) -> Hold | Steady | CatchUp` with the 2×target / target/2 thresholds. Constants documented (MAX_SLEW, band edges).</do>
    <test>pacing_tests: interval equals nominal at queued==target; monotonic in queued; clamped at both extremes; band edges exact (2×target → Hold, target/2 → CatchUp, between → Steady). Plus a closed-loop simulation: nominal 16.742 ms frames vs a simulated 21.3 ms/1024-frame device drain over ≥10 simulated seconds — assert the queue stays inside the Steady band after warm-up, at most 1 frame is owed per grid tick in steady state, and the long-run frame rate is 59.7275 Hz ± MAX_SLEW.</test>
    <done>The servo + bands are pure, unit-proven, and the simulation shows 1-frame-per-tick steady state with the queue bounded.</done>
  </task>
  <task id="2" model="sonnet" deps="1">
    <do>app_pacing.rs: rework `run_audio_paced` onto the `next_frame` grid — same skeleton as `run_timer_paced` (backlog resync at 8×FRAME_DURATION, breakpoint/link/freeze/cheat handling unchanged) but `next_frame += slewed_interval(...)` and the per-wake frame budget from `PaceBand` (Hold → 0, Steady → frames owed, CatchUp → up to MAX_FRAMES_PER_WAKE). `pump` after every frame (unconditional — the ring drops overflow). app_handler.rs: audio-paced control flow → `WaitUntil(self.next_frame)`; drop the FRAME_DURATION/4 poll arm. Audit every `resync_pacing()` call site (pause, mute toggle, turbo exit, reset, audio fallback) — all must still reset the grid.</do>
    <test>Factor the per-wake decision into a pure `wake_plan(queued, target, now, next_frame, interval) -> (frames_budget, new_next_frame)` and unit-test it: Steady owes exactly the grid backlog capped at 1 in the simulated steady state; Hold returns 0 and does not advance the grid past now+interval; CatchUp caps at MAX_FRAMES_PER_WAKE; the 8-frame backlog resync still snaps next_frame to now.</test>
    <done>Audio-paced emulation emits one frame → one present per grid tick in steady state; post-turbo and resume transitions bounded; no behavior change for timer/turbo/link paths.</done>
  </task>
  <task id="3" model="sonnet" deps="2">
    <do>pacing.rs: re-spec `StallWatchdog::is_stalled` for the grid world — drop the `frames_emulated` progress term (frames no longer imply a draining queue); progress ⇔ `queued < last_queued || queued < target`; stall ⇔ queue pinned ≥ target with no drop for AUDIO_STALL_TIMEOUT. Pass `target` in (from AudioPipe). check_audio_health keeps the `failed()` fast path and the timer-pacing fallback + resync.</do>
    <test>pacing_tests: watchdog trips when the queue sits ≥ target unchanged for > timeout even while frames are being emulated (the new dead-stream shape); does NOT trip while the queue level drops, nor while queued < target (fast-draining device), nor within the grace period after reset.</test>
    <done>A dead stream under grid pacing falls back to timer pacing within ~1 s; a healthy fast-draining device never false-trips.</done>
  </task>
  <task id="4" model="haiku" deps="2,3">
    <do>Docs: new `docs/ui-state/pacing-and-audio.md` (the pacing area has no state file yet) — the three pacers, the servo + bands, the watchdog rule, the FPS-counter semantics (counts emulated frames), and this plan's rejected approaches; add it to docs/ui-state/README.md. One-line pointer refresh in CLAUDE.md only if the ui-state index line needs it (keep CLAUDE.md a lean index).</do>
    <test>/clean-docs conventions hold; links resolve.</test>
    <done>Pacing state is documented in its own ui-state file, discoverable from the README.</done>
    <why>Mechanical doc write from the shipped design.</why>
  </task>
  <task id="5" model="haiku" deps="4">
    <do>Gates: cargo test -p slopgb --bins + -p slopgb-core --lib; clippy --all-targets -D warnings; fmt; no file > 1000 lines; /rust-diff-review on the diff, fix every finding. Manual smoke: `cargo run --release -- <game>` with sound ON — verify visually smooth scrolling (the perceptual bug is the acceptance test) and no audio crackle at steady state, after pause/resume, and after turbo release; toggle Enable sound both ways.</do>
    <test>All gates green; the manual smoke passes on a real host.</test>
    <done>Green, reviewed, and the judder is gone by eye with sound enabled.</done>
    <why>Verification sweep — the defect is perceptual, so a human-eye check is part of done.</why>
  </task>
</plan>
```

Summary: 5 tasks (3 sonnet, 2 haiku). Critical path: 1 → 2 → 3 → 4 → 5. Core
untouched (golden trivially safe); the only user-visible changes are smooth
sound-on presentation and fewer idle wakeups.
