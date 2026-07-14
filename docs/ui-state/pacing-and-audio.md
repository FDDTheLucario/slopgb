# Emulation pacing + audio

How the frontend decides *when* to emulate a frame and *when* to present it. The
primitives live in [`crate::pacing`](../../crates/slopgb/src/pacing.rs) (the audio
pipe, the servo/band math, the stall watchdog, the pacing decision); the pacing
*loops* on `App` live in [`crate::app_pacing`](../../crates/slopgb/src/app_pacing.rs);
the wake in `about_to_wait` lives in
[`crate::app_handler`](../../crates/slopgb/src/app_handler.rs). Frontend-only — no
core cycle is advanced or state mutated by any of it, so it's trivially golden-safe.

## The three pacers

`about_to_wait` picks one per wake (`audio_pacing(has_audio, muted)` decides
audio-vs-timer):

| Pacer | When | Cadence |
|---|---|---|
| **turbo** (`run_turbo`) | fast-forward held | free-run inside a 10 ms wall-clock budget, `ControlFlow::Poll` |
| **audio** (`run_audio_paced`) | live pipe + sound un-muted | slewed `next_frame` grid, `WaitUntil(next_frame)` |
| **timer** (`run_timer_paced`) | muted or no pipe | fixed `next_frame` grid at the framerate limit, `WaitUntil(next_frame)` |

Both non-turbo pacers now march the **same `next_frame` wall-clock grid**: one wake
schedules one frame → one present. That is the fix for the audio-paced judder (see
below); before, audio pacing woke on a `FRAME_DURATION/4` poll and refilled the queue
with a `needs_more()` gate, so it presented at the *audio-callback cadence* and
dropped every other emulated frame.

## The judder bug (why the grid)

With sound on the old `run_audio_paced` batched frames per wake to refill the audio
queue to its ~50 ms target, and `about_to_wait` issued **one** `request_redraw()`
after the batch. The device drains in callback-sized steps (typ. 1024 frames ≈
21.3 ms @ 48 kHz — more than one 16.74 ms video frame), so the queue fell short every
other callback and the wake emulated *two* frames but presented only the latest —
frame N was emulated and never shown. The FPS counter read an honest ~59.7 because it
counts **emulated** frames, not presents.

## The servo + bands (audio pacing)

The audio queue is used as a *rate servo*, not a frame batcher — it keeps the
long-run production rate locked to the device clock (no drift, no periodic underrun)
while the grid presents one frame per tick.

- **`slewed_interval(queued, target, nominal)`** — a pure P-controller on the queue
  fill level. `nominal × (1 + clamp((queued − target)/target, −1, 1) × MAX_SLEW)`,
  `MAX_SLEW = 1%`. Queue below target → shorter interval (produce faster to refill);
  above → longer. A steady-state P-offset is fine — the queue settles slightly off
  target. Nominal is always `FRAME_DURATION`; the framerate limit applies only to
  timer pacing (the audio device clock is authoritative when sound is on).
- **`PaceBand::classify(queued, target)`** — the per-wake frame budget band, so
  transitions stay bounded (thresholds strict; the exact edges are `Steady`):
  - **Hold** (`queued > 2×target`, e.g. right after turbo leaves ~250 ms queued):
    0 frames, park the grid at `now + interval` until the queue drains toward target.
  - **Steady** (the normal band): the frames the `next_frame` grid owes, ≤1 per wake
    in practice.
  - **CatchUp** (`queued < target/2`, e.g. resume from pause/breakpoint): a bounded
    burst of `MAX_FRAMES_PER_WAKE` to refill; a few dropped presents are acceptable
    transiently while audio continuity is restored, then Hold drains the overshoot.
- **`wake_plan(queued, target, now, next_frame, interval)`** ties them together and
  returns `(frames_budget, new_next_frame)`. Includes the backlog resync (fell more
  than `MAX_FRAMES_PER_WAKE` intervals behind → snap the grid to `now` instead of
  fast-forwarding the whole backlog). `run_audio_paced` emulates up to `frames_budget`
  frames, `pump`ing the device ring after each (overflow is dropped by the ring).

`resync_pacing()` (on pause/unpause, mute toggle, turbo exit, reset, load, focus
resume) sets `next_frame = now` and resets the watchdog — it already covered every
mode transition, so the grid needed no new resync mechanism.

## The stall watchdog

`StallWatchdog` falls audio pacing back to timer pacing when the cpal stream dies
without reporting an error (`failed()` is the fast path). Under grid pacing frames
are emulated on the wall clock regardless of the queue, so "frames emulated" no
longer implies a live device — **only the queue draining does**:

- **Progress** ⇔ the queue level dropped since the last wake, **or** sits below
  `target` (a fast-draining device is alive and never trips).
- **Stall** ⇔ the queue pinned at/above `target` with no drop for
  `AUDIO_STALL_TIMEOUT` (1 s). A dead device leaves the grid topping the queue up to
  ~2×target, then Hold pins it there — stuck high with no drop.

`reset()` restarts the grace period so an unpause never trips on a stale timestamp.

## FPS-counter semantics

`update_fps(frames)` counts **emulated** frames, not presents — it is honest about
emulation rate but was blind to the presentation judder (that's why the counter read
~59.7 while the picture updated every other frame). It stays emulated-frame-based; on
the grid, emulated ≈ presented in steady state, so the two now agree.

## Known trade-offs (grid vs device backpressure)

Grid pacing swaps the old `needs_more()` device backpressure for a wall-clock
servo. That is what removes the judder, but it moves three edge cases onto
pathological hardware. Each has a designed backstop; don't "fix" one in a way that
reintroduces the judder:

- **Watchdog vs a healthy-but-slow device.** A device whose true clock is slightly
  slower parks the queue a hair above `target`. The watchdog stays honest because a
  live device *drains* the queue every callback (~21 ms), so `queued < last_queued`
  fires many times a second — far inside the 1 s timeout. Only a device that stops
  draining leaves the queue pinned with no dip, which is the stall we want to catch.
  A monotonic climb with no observed dip for a full second requires the device to
  have stopped consuming — i.e. an actual stall.
- **Wake headroom.** Audio pacing wakes once per grid tick (~16.7 ms) instead of the
  old `FRAME_DURATION/4` (~4 ms) poll. The ~50 ms queue is still ~3 grid ticks of
  buffer, and a wake delayed enough to drop the queue below `target/2` triggers a
  `CatchUp` burst that refills it — so scheduling jitter is absorbed by the band, not
  by oversampling.
- **Clock divergence beyond ±MAX_SLEW.** The ±1% servo tracks the ≪0.5% host-vs-device
  drift seen in practice smoothly. Larger divergence saturates the slew, and the
  `Hold`/`CatchUp` bands take over as a second-order correction: bounded (queue can't
  run away), degrading to occasional dropped presents rather than drift or underrun.
  Widening `MAX_SLEW` trades pitch stability for drift range (plan range 0.5–2%).

## Rejected approaches (do not re-chase)

- **Present inside the batch loop (per emulated frame):** softbuffer/winit presents
  only the single latest framebuffer on `RedrawRequested`; there is no frame queue to
  present. Dead on arrival.
- **Cap audio-paced emulation at 1 frame per wake, keep the `needs_more()` gate:** the
  two catch-up frames then land ~4.2 ms apart (the poll cadence), still inside one
  60 Hz refresh — the compositor samples only the latest, alternate frames still
  vanish. Shifts the judder, doesn't fix it.
- **Bigger audio target / smaller device buffer:** tunes around the beat between the
  callback period and the frame period instead of removing it. Host-dependent,
  regresses elsewhere.
