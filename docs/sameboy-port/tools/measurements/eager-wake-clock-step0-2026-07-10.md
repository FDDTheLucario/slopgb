# Eager wake-clock port — Step 0: the structure scoped + the naive re-host measured net-negative (2026-07-10, #11cx)

Ground-truth foundation for the eager wake-clock port (the 5 CGB halt rows of
the TRUE flip bar). Traced the exact mechanism, attempted the first slice, and
measured it regressing — so the next session starts from a known floor, not a
hypothesis. No code shipped; tree byte-identical at `bdf7624`.

## The structure (corrects the earlier "full mid-cycle machine" fear)

The CGB halt-wake does NOT go through the DMG mid-cycle sampler. Chain:

```
cpu/execute.rs halt loop → Bus::pending_halt_wake_mid → halt_wake_mid_impl
   (its tier2+DMG mid-block is `!is_cgb`-gated → CGB skips it)
   → self.pending_halt_wake() → halt_wake_impl → mask_hidden_m0_stat(w)
```

So CGB `_3b` is `mask_hidden_m0_stat` + a meaningful `stat_vis_from_t`, NOT
`halt_wake_mid_impl`'s `advance_pending(2)` sampling. Narrower than #11cw feared.

## The trace — `late_m0int_halt_m0stat_scx3_3b` [Cgb], ly1 (tier2 PASSES, want out2)

Instrumented `clock.now()` / `stat_vis_from_t` at the m0 rise and at every
`mask_hidden_m0_stat` consult (`SLOPGB_S5DBG`, probes reverted):

| clock | m0 rise (dot 257) | entry consult | outcome |
|---|---|---|---|
| tier2 | `machine_now=5108 clk=5112`; sets `svt=5108` | dot 260 `clk=5112 ≥ svt=5108` → STAT visible → `w=02` → rewind → 336 | `out2` ✓ |
| eager | `machine_now=0 clk=5108`; setter tier2-gated → `svt=0` | mask `svt=0` never fires → STAT always visible | wrong |

**The key measured fact: eager `clock.now()` at the rise (5108) equals tier2's
`machine_now` at the same rise (5108).** So the eager-frame analogue of the
deadline is `stat_vis_from_t = clock.now() + gl` at the rise — no deferred
machine needed. `machine_now` is 0 under eager only because `advance_machine_t`
(tier2's T-by-T machine) is its sole writer and eager runs `tick_machine`
whole-dot.

## Slice 1 — the naive re-host, MEASURED NET-NEGATIVE

Two coupled changes behind `eager_value`:
1. `tick.rs`: add an eager arm setting `stat_vis_from_t = clock.now() + gl` at
   the m0 rise (mirroring the tier2 arm's `machine_now + gl`).
2. `speed.rs::mask_hidden_m0_stat`: un-gate the deadline check to
   `tier2_reclock || eager_value`.

| config | EV CGB | EV DMG |
|---|---:|---:|
| baseline | 358 | 85 |
| slice 1 | **368** (+10) | **95** (+10) |
| slice 1 + `&& cpu_halted` on the eager mask arm | **368** (+10) | **95** (+10) |

**Regresses +10/+10.** The `cpu_halted` scope changes nothing, so the
over-masking is on the halt path itself, not running-CPU dispatch: with
`svt = clock.now()` at the rise, the deadline `clock.now() < svt` masks the STAT
bit at halt consults where it should be VISIBLE, breaking 10 previously-passing
halt rows on each model to (at best) recover a few.

This reproduces #11cn's "net-negative, best −20" verdict from the eager frame,
and pins WHY: the mask is consulted at multiple sites (halt entry, first-idle
wake, plain wake) whose `clock.now()` positions differ between the deferred and
eager frames, so a single deadline calibrated as `clock.now()@rise` over-fires
in the eager consult frame. tier2's deadline works only because its consult
positions are the deferred ones the `machine_now` value was calibrated against.

## Verdict — genuinely multi-session, and here is the actual lever

The port is NOT a gate flip and NOT a one-value re-host. `stat_vis_from_t` is a
single number consumed at ≥3 consult sites; making it correct under eager needs
**per-consult-site frame calibration** — the eager `clock.now()` offset from the
deferred frame differs at the entry check vs the wake sample vs the plain wake.
The next session must:

1. Trace `clock.now()` at EACH consult site (entry / first-idle / plain wake) for
   tier2 vs eager on a PASSING halt row, and tabulate the per-site offset.
2. Set `stat_vis_from_t` in the eager frame with a per-site correction (the way
   the DMG deferred arm at `tick.rs:167` carries `m0_halt_hold` + the per-SCX
   `mask{rise cc==4}` term — that arm is the template for a frame-shifted mask).
3. A/B every one of the ~10 halt rows slice 1 broke, not just the 5 bar targets.

Interaction with #11cv: on CGB the entry peek is NOT shipped (DMG-scoped, because
peek alone breaks `_3b` — #11cw). So the CGB port couples the entry peek AND the
wake mask: `_3a` wants the entry rewind, `_3b` wants the halt-bug at entry then a
correct wake. The wake mask must let `_3b` fall through to the halt-bug while
`_3a` rewinds — the discriminator the whole port turns on.

## Reproduce

```
# slice 1 (reproduces 368/95):
#   tick.rs: after the tier2 `stat_vis_from_t = machine_now + gl` arm, add
#     `else if self.eager_value && (!is_cgb || !double_speed) {
#         let gl = if glitch_line_now {4} else {0}; stat_vis_from_t = clock.now()+gl; }`
#   speed.rs mask_hidden_m0_stat: gate `tier2_reclock || eager_value`.
# trace: SLOPGB_S5DBG probes on the m0rise setter (clk/svt) + mask_hidden_m0_stat consult.
```

## Gate state

No code shipped; tree byte-identical at `bdf7624` (baseline re-confirmed EV CGB
358 after revert). Bar unchanged **49 CGB + 40 DMG = 89**. The 5 CGB halt rows
stay pinned to this port, now with a measured Step-0 floor and the per-consult
calibration named as the lever.
