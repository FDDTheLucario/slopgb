# C2 #11ay — the "sub-T co-temporal" barrier REFUTED by the absolute monotonic clock; the flip-blocker pairs separate 2–386 half-dots (RENDER-LENGTH / WAKE / READ-FRAME, not an operation-order tie)

**2026-07-01, `phase-b-s7` (base `6f375fe`), ESCAPE. NO slopgb-tree code (SameBoy
tracer + measurement only); core byte-identical OFF; mooneye 91/91 flag-on; defaults
NOT flipped; 115 SameBoy-pass blockers UNCHANGED.**

## Result

The #11ax terminal-barrier diagnosis — *"the co-temporal render-length pairs
(`late_scx4` both half-dot 520) AND the WAKE pairs (`halt *_m0stat` both `ly2 cfl0
dc0`) read at the IDENTICAL half-dot, split by SameBoy into opposite modes … the
discriminator is the sub-T operation ORDER"* — **is a `cfl*2+dc` METRIC ARTIFACT.**

Built a genuine absolute monotonic 8 MHz clock in SameBoy (`fp = absolute_debugger_ticks
− display_cycles`) and re-traced the three flagship "co-temporal / atomic" pairs. **On
the absolute clock every pair separates cleanly (2–386 half-dots).** The barrier is NOT
an unbreakable operation-order tie at one tick; it is the *already-named* read-FRAME +
render-LENGTH + sub-M-cycle-WAKE levers, at 1–4-dot resolution slopgb's whole-dot /
whole-M-cycle model collapses. This **re-validates the goal's half-dot/T-resolution
lever as necessary AND plausibly sufficient**, correcting #11ax's "necessary but
insufficient."

## Why `cfl*2+dc` lied

`cycles_for_line` (cfl, whole dots) and `display_cycles` (dc, signed 8 MHz budget)
are SameBoy PPU-internal counters that **reset at each line** (`display.c:1751`
`cfl=0`) and are **conserved across the mode-0 transition** (the mode-3-end sequence
bumps `cfl += k` while a `GB_SLEEP` drives `dc` negative by `2k`, so `cfl*2+dc` is
invariant across the flip — `display.c:2113-2146`). So `cfl*2+dc` is NOT a monotonic
time axis: it maps two genuinely different absolute instants (one before, one after a
transition; or one on each side of a line wrap) to the same number.

`absolute_debugger_ticks` (`gb.h:778`) is the true monotonic 8 MHz counter, bumped by
the full post-`<<=1` `cycles` at the top of every `GB_advance_cycles` (`timing.c:492`).
The display coroutine has consumed up to `absolute_debugger_ticks − display_cycles`
half-dots (adt = total advanced; dc = budget added-but-unconsumed). That difference is
the unambiguous fine position (`fp`). Tracers patched into SBMODE (`display.c:528`),
SBREAD ff41 (`memory.c:634`), SBPALR (`memory.c:709`); folded into
`build_sameboy_tracers.sh` so they survive a `/tmp` wipe.

## The three flagship pairs (fresh dual-emulator trace, CGB `--length 4`)

### 1. `m2int_m3stat/scx/late_scx4` — RENDER-LENGTH (not operation-order)

| leg | want | SameBoy read | SameBoy m3→0 flip | slopgb read |
|---|---|---|---|---|
| `_1` | 3 | `ly1 cfl260 dc0` **fp 26179816 → 3** | `cfl261 dc6` **fp 26179818** | `ly1 dot256 clk5108 lrd=false vm=3 → 3` ✓ |
| `_2` | 0 | `ly1 cfl261 dc-2` **fp 26179818 → 0** | `cfl257 dc6` **fp 26179810** | `ly1 dot256 clk5108 lrd=false vm=3 → 3` ✗ |

- The two **reads** are 2 half-dots apart (26179816 vs 26179818) — essentially the
  SAME program point (as expected: `late_scx4` legs differ in the SCX write, not the
  read). `cfl*2+dc` = 520 for both — the coincidence #11ax read as "co-temporal."
- The **discriminator is the mode-3 FLIP position**: `_1` flips LATE (fp 26179818,
  cfl261), `_2` flips EARLY (fp 26179810, cfl257) — **8 half-dots = 4 dots apart**.
  This is the late-SCX effect on the mode-3 length (`167 + SCX&7`, `display.c:1512`):
  in `_1` the SCX write is caught by the current line's penalty, in `_2` it lands too
  late. `_1`'s longer mode 3 still covers the read (mode 3); `_2`'s shorter mode 3 has
  already exited (mode 0).
- **slopgb sees the legs IDENTICALLY** — same dot, same `clk=5108`, same
  `lrd=false vm=3` — because it applies the SCX penalty uniformly (both mode-3 long).
  No FF41 read-law can separate them (they are byte-identical at the read); the only
  fix is the **per-config mode-3 render length** (move `line_render_done` for `_2` but
  not `_1`), which moves the counter-pinned mode-0 IRQ dispatch → the atomic render
  reclock. **This is RENDER-LENGTH, cleanly — NOT a sub-T operation-order tie.**
- **The discriminating WRITE is observable but the fix stays counter-pinned (new lead,
  build-measured feasibility):** traced the SCX writes (SBWSCX `fp`) — `_1` writes
  SCX=4 at `ly1 cfl89` (fp 26179474), `_2` at `ly1 cfl92` (fp 26179480), **3 dots
  apart, straddling the fine-scroll drop (~cfl90-91)**. `_1`'s early write is caught by
  the drop (`position_in_line&7 == SCX&7`, `display.c:700`) → +SCX&7 penalty → longer
  mode 3; `_2`'s late write misses it → bare exit. So unlike the shipped window shadows
  (`wy_trig_sb`/`win_predraw_abort`, keyed on a clean on/off write like LCDC.5), a
  late_scx4 shadow would key on the SCX-write-vs-drop **sub-dot timing** — and slopgb's
  SCX fine-scroll comparator (`render.rs:388` `hunt_idx == eff.scx&7`, reading the
  +2-dot-staged `eff.scx` over mode-3 dots 5-12) is **counter-pinned by mooneye
  `hblank_ly_scx_timing`** (a convergence-gate test). A read shadow would be a
  per-SCX-value curve-fit threshold on that pinned render-timing (high A/B risk); the
  clean fix is to recalibrate the whole SCX-write→drop timing to SameBoy (the cc+0
  write commit vs the +2 stage delay) in the atomic render reclock, where the pinned
  `hblank_ly_scx` co-moves. NOT a standalone slice.

### 2. `halt/late_m0irq_halt_m0stat_scx*` — sub-M-cycle WAKE (spread across the mode-0→2 boundary)

| leg | want | SameBoy read | slopgb read |
|---|---|---|---|
| `scx2_1a`/`scx2_2a` | 0 | `ly2 cfl0 dc0` **fp 26180216 → 0** (at the m2 rise, read-before-flip) | `ly2 dot4 clk5312 → mode 2` ✗ (too LATE) |
| `scx3_3a` | 0 | `ly2 cfl0 dc0` **fp 26180216 → 0** | `ly1 dot452 clk5304 → mode 0` ✓ |
| `scx3_3b` | 2 | `ly2 cfl0 dc0` **fp 26180224 → 2** (8 past the m2 rise) | `ly2 dot0 clk5308 → mode 0` ✗ (too EARLY) |

- SameBoy's line-2 mode-2 rise is at **fp 26180216** (measurement frame). The want-0
  legs read AT it (mode 0, read ordered before the flip); the want-2 leg reads 8
  half-dots (4 dots) later (mode 2). **Reads separate by 8 half-dots — NOT "identical
  ly2 cfl0 dc0."** (`cfl*2+dc` = 0 for all — the line-wrap reset collapse.)
- slopgb DOES place the legs at different clk (5304/5308/5312) but reads the WRONG mode
  on **opposite sides** of the boundary: `1a/2a` land `ly2 dot4` (mode 2, want 0 —
  4 dots too late), `3b` lands `ly2 dot0` (mode 0, want 2 — 4 dots too early). No
  single whole-dot read-law flips both directions → **atomic**, confirming #11av's
  `halt_mode_phase` refutation (+5/−13). The fix is the **sub-M-cycle halt-WAKE resume
  position** (the `halt_ly_phase` analogue for FF41-mode) landing the post-wake read at
  SameBoy's exact half-dot relative to the mode-2 rise.

### 3. `cgbpal_m3/cgbpal_m3end_scx2` — READ-FRAME (193 dots apart, not "inverted at dot84/86")

| leg | want | SameBoy palette read |
|---|---|---|
| `_1` | 7 (blocked) | `ly1 cfl261 dc-2` **fp 26179818 → blocked=1** |
| `_2` | 0 (accessible) | `ly1 cfl0 dc-380` **fp 26180204 → blocked=0** |

- The two reads are **386 half-dots = 193 dots apart** — different line positions
  entirely (`_1` at the mode-3 end, `_2` ~0.4 line later). #11ax's "offset0
  blocked@176 / offset1 accessible@174, slopgb INVERTED at dot84/dot86" measured the
  palette-LOCK window edges (a different aspect); on the absolute read clock the two
  measurement reads are nowhere near each other. This is a read-POSITION / frame
  problem, not a 2-dot inversion.

## Implication for the flip

The barrier is real but its NATURE is corrected: **slopgb collapses genuinely-different
absolute instants because its PPU advances in whole dots and its CPU clock is
M-cycle-quantized** — it renders the two `late_scx4` legs with the same mode-3 length,
wakes the two halt legs onto the wrong dots, and reads the two cgbpal legs at frame-
shifted positions. Every one of these is the goal's lever (§1 half-dot PPU advance
driven by `cycle_clock`'s exact T, §2 reads sample at the exact T, §4 re-derive every
boundary constant to the half-dot frame). The evidence says that lever is **sufficient**
(the differences exist and are 1–4+ dots, representable at half-dot grain), not merely
necessary — the #11ax "half-dot necessary but INSUFFICIENT" verdict rested on the
`cfl*2+dc` collapse and does not survive the absolute clock.

**What is NOT unlocked this session:** a clean +N/−0 slice. `late_scx4` is
read-identical in slopgb (no read-law reaches it); the halt family is opposite-signed
across the boundary (whole-dot laws refuted, #11av); cgbpal is frame-shifted. All three
need the atomic reclock (render-length move → dispatch, or the half-dot advance), which
lands the ~7000-row rebaseline RED-intermediate and cannot converge 115 with zero drop
in one session. → ESCAPE, with the barrier diagnosis corrected and the absolute-clock
tool banked.

## For the next session

1. **Use `fp` (absolute_debugger_ticks − display_cycles), never `cfl*2+dc`, as the
   SameBoy time axis** — the latter is non-monotonic (resets per line, conserved across
   transitions) and has repeatedly produced false "co-temporal" verdicts (#11i, #11ap,
   #11ax). The tracer is in `build_sameboy_tracers.sh` (SBMODE/SBREAD/SBPALR `fp=`).
2. The reclock is per-class: **RENDER-LENGTH** (`late_scx4`, the per-config mode-3
   length / SCX-during-m3 timing — move `line_render_done` decoupled from the dispatch,
   the `mode_for_interrupt` split SameBoy already has); **WAKE** (halt — sub-M-cycle
   resume, the `halt_ly_phase` analogue for FF41-mode); **READ-FRAME** (cgbpal/serial/
   tima — the per-ISR deferred-read T-position). They co-land in the half-dot advance
   (goal §1) + the boundary re-derivation (goal §4). They are NOT co-temporal — the
   half-dot grain resolves them.
