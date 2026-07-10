# The eager halt entry: the rewind was tier2-only, and the entry sample is 4 dots early — TRUE DMG flip bar 46 → 40 (2026-07-10, #11cu + #11cv)

Two shipped slices. The halt class, which #11cp/#11cq/#11cr all treated as one
problem requiring a dispatch move (`stat_late`), turned out to be two problems,
neither of which needs one.

## What was actually wrong

`cpu/execute.rs:481-489` calls `bus.halt_entry_rewind()`. Its impl
(`interconnect/speed.rs::halt_entry_rewind_impl`) early-returned `false` unless
`tier2_reclock`. The call-site comment already said what that costs:

> the whole post-wake stream ran one halt round early (`late_m0int_halt_m0stat_*`)

Those are the halt rows in the TRUE flip bar. **The mechanism was in the tree the
whole time, hosted on one clock.**

### #11cu — host the rewind on eager

SameBoy `halt()` (sm83_cpu.c:1043-1047): when `IE & IF` is already nonzero at the
entry view, HALT is *not* entered — `halted = false; pc--`, so the dispatched ISR
returns *into* the HALT and it re-executes with the IF bit consumed. Production
instead halts and wakes on the first idle check, pushing halt+1.

This is hardware behaviour, not a clock artifact → gated `tier2_reclock ||
eager_value`, no sub-flag. `pending_halt_entry`'s own tier2 gate skips the entry
flush, so eager samples `pending()` unadvanced and never touches the #11cj
double-advance.

**EV CGB 359→358, EV DMG 92→91, zero drops.** Both recovered rows are
`gambatte/halt/ifandie_ei_halt_sra` (`EI; HALT` with `IE & IF` already set). They
are OFF-fail ⇒ **flip GAINS, not bar reductions** — the bar stayed 49/46.

### #11cv — the entry sample is 4 dots early on the eager clock

`halt_entry_impl` reaches SameBoy's post-fetch (t0+4) view by flushing the parked
debt. The eager clock parks none, so the flush is a no-op and the sample sits at
t0. Dual-traced `late_m0int_halt_m0stat_scx3_3a` [Cgb] (`SLOPGB_S5DBG=1`, the
`hentry` probe already in `halt_entry_impl`):

| clock | halt-entry sample | `w` | outcome |
|---|---|---|---|
| OFF | ly1 dot **332**, clk 5180 | `00` | halts → `out0` ✓ |
| **EV** | ly1 dot **256**, clk 5104 | `00` | halts early → `out2` ✗ |
| tier2 | ly1 dot **260**, clk 5112 | `02` | rewind → re-entry dot 336 → `out0` ✓ |

The ly1 mode-0 STAT rise folds at **dot 257**. tier2's flush lands it at 260,
past the fold; eager sits at 256, one dot short. Both EV and tier2 *arrive* early
(both dispatch the setup ISR at dot 0) — tier2 recovers because the flushed entry
view sees the rise and rewinds.

**Fix — a VALUE peek, not an advance.** `Ppu::stat_m0_rise_within(4)` asks whether
the mode-0 rise lands within the next 4 dots, via the existing
`projected_flip_dot()`; the interconnect ORs `IF_STAT` into the entry word.
Nothing moves.

Advancing was rejected on evidence, not taste: `clock.advance_pending` asserts
`t <= pending` and eager parks no debt, so reaching t0+4 would mean
`carry_read(4) + advance_pending(4)` — fabricating 4 T of machine time and
ticking the timers early, which the TIMA-counted `int_hblank_halt` rows pin.
The peek is the same VALUE-at-cc+4 / STATE-at-cc+0 decomposition as
`read_pos_hd`'s `+8hd` debt and `Ppu::boot_read`, and mirrors the DS FF0F
read-view peek already in `stat_irq/ff0f.rs` (`rise <= self.dot + 1`).

**EV DMG 91→85, zero drops. The six recovered rows are EXACTLY the six DMG halt
rows of the TRUE flip bar, all SameBoy-PASS:**

```
late_m0int_halt_m0stat_scx2_3a   late_m0irq_halt_dec_scx2_2
late_m0int_halt_m0stat_scx3_3a   late_m0irq_halt_dec_scx3_2
late_m0int_halt_m0stat_scx3_3b   late_m0irq_halt_m0stat_scx3_3b
```

**TRUE DMG flip bar 46 → 40** (DMG flip-BUGs 55 → 49). The first bar reduction of
this line of work.

## Why the peek is DMG-scoped (an honest hold, not a floor)

On CGB the identical peek measures **+5 / −1**: it also arms the entry view for
the `_3b` skip-path (`late_m0int_halt_m0stat_scx3_3b` [Cgb], want `out2`), where a
rise inside the fetch M-cycle should arm SameBoy's **halt-bug** (no halt; the
following byte runs twice — `halt_entry_impl`'s own comment) rather than the
rewind. The dropped row is OFF-fail and outside the TRUE bar, so CGB is a net
gain of 4.

It was not shipped because **the SameBoy tester was unavailable** (`/tmp/sbbuild`
had been cleaned; rebuilding from a re-downloaded tarball was not authorised) and
**a shipped slice may not drop a SameBoy-PASS row on an unverified guess.** CGB
keeps the t0 sample.

**To finish this: rebuild `sameboy_tester`, take the verdict on
`late_m0int_halt_m0stat_scx3_3b` [Cgb] (want `out2`).** If SameBoy FAILs it, the
row is floor, un-scope the peek to CGB and take **EV CGB 358 → 354, TRUE CGB bar
49 → 44**. If SameBoy PASSes it, the `_3a`/`_3b` split is real: `_3a` (IME=1)
wants the rewind, `_3b` wants the halt-bug, and the peek needs to key on the IME
state at entry rather than fold unconditionally.

**Infrastructure note:** the tester living under `/tmp` makes the entire
classification protocol wipeable between sessions. `PORT-PLAN.md:91`,
`HALFDOT-BUILD-PLAN.md:148,384`, and both classifiers hard-code that path. Build
it somewhere persistent and update those references.

## What this retires

- **"The halt rows need a late dispatch."** Never established. `#11cp`, `#11cq`,
  `#11cr` all built on it — `#11cq` spent the whole coupled landing on `stat_late`
  and dropped 105 SameBoy-pass rows; `#11cr` concluded `intr_2_mode0` is "a real
  independent blocker". Neither slice here moves a dispatch, and every eager
  tripwire — including `intr_2_mode0` on both models — stays `B=03`.
- The halt class splits cleanly: **entry-rewind** (`ifandie_ei_halt_sra`, #11cu),
  **entry read-frame** (`late_m0int_halt_m0stat_*` / `late_m0irq_halt_*`, #11cv,
  DMG shipped / CGB held).

## Method note

Both fixes came from `grep`, not theory. `#11cu` from grepping for the mechanism
tier2 uses to pass the rows eager fails; `#11cv` from a probe (`SLOPGB_S5DBG`'s
`hentry` line) that had been sitting in `halt_entry_impl` unused. Three agents
theorised a dispatch move over the same code. **Grep the tree for a tier2-only
mechanism before theorising a new one.**

## Gate state (both slices SHIPPED, `69abfd4` / `d647a52`)

golden_fingerprint byte-identical (42.1s, real run, `SLOPGB_REQUIRE_ROMS=1`);
tier2 CGB two-bin **291**; OFF CGB **486** / DMG **103**; EV CGB **358** / EV DMG
**85**; mooneye **92** flag-off AND `SLOPGB_MOONEYE_EAGER` AND
`SLOPGB_MOONEYE_RECLOCK`; eager tripwires on BOTH models — wilbertpol
`intr_0_timing`, `intr_2_mode0/mode3/oam_ok/0_timing`, `di_timing-GS`,
`halt_ime0_nointr_timing` — all `B=03 C=05 D=08 E=0D H=15 L=22`; lib 760; clippy
`-D warnings` clean; no `.rs` ≥ 1000. Pins `eager_halt_entry_rewind_passes` and
`eager_halt_entry_m0_peek_passes_dmg`, both verified red-before-green.

**TRUE flip bar: 49 CGB + 40 DMG = 89** (was 49 + 46 = 95).
