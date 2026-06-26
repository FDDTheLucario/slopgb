# Mech-2 wake-clock ground truth — halt `*_m0stat_*` want-0/want-2 split

2026-06-25 (#11m). This session built the NEW SameBoy HALT-exit / IRQ-dispatch
tracer the goal called for, measured the halt-wake → FF41-read sub-dot chain for
the canonical want-0/want-2 collapse pair, and reached the **decision gate
verdict: FALL BACK**. The wake-clock discriminator is real, constant, and
distinct — but it lives in the CPU's *sub-dot* (8 MHz `display_cycles`) wake
phase, finer than slopgb's whole-dot `cc`, and slopgb has no field to record it
on without the S7 sub-M-cycle deferred clock (out-of-session infra).

## The new tracer (SB_TRACE-gated, `Core/sm83_cpu.c`)

The existing SameBoy tracers are all read/dispatch/level observers (`SBREAD ff41`
in `read_high_memory`, `SBLEVEL`/`STAT_IRQ`/`SBMODE` in `GB_STAT_update`). #11i
proved the FF41-read `dc` is identical for both want directions, so a read-side
observer is useless. Two NEW probes, both `SB_TRACE`-gated:

- **`SBWAKE`** at the two HALT-exit branches of `GB_cpu_run` (the
  `gb->halted = false` sites: `noisr` = wake-without-call `:1643`, `isr` =
  call-interrupt `:1654`): logs `current_line / cycles_for_line(cfl) /
  display_cycles(dc) / pending_cycles(pc) / GB_IO_STAT&3 / mode_for_interrupt /
  interrupt_queue` at the instant the SM83 core exits halt.
- **`SBCYR ff41 pre/post`** in `cycle_read` (the generic CPU memory read), gated
  on `addr == 0xFF41`: logs the position **before** the `pending_cycles` flush
  (`pre`, with `pend`) and **after** the flush+read (`post`). The `pre`→`post`
  pair brackets the deferred read's M-cycle window; the `SBMODE` line that lands
  *between* them shows whether the flush crossed the mode-2 commit.

`make tester`; run `SB_TRACE=1 sameboy_tester --dmg --length 2 ROM`; isolate the
measurement read by its measurement-frame `SBCYR ff41 post ly=2 cfl=0` line (the
setup frames read other lines). Full patch text in `../stat-irq-trace.md`.

## The measurement (DMG, collapse pair + family)

Target rows (`tests/gbtr/baselines/gambatte.txt`): want-0 `m0int_m0stat_scx3_2`,
want-2 (SameBoy-PASSING) `late_m0irq_halt_m0stat_scx3_2b` (the slopgb cc-collapse
partner) and `..._scx3_3b` (the goal's want-2 pin). The mode-0 HBlank IRQ rises
at `ly1 cfl≈257+SCX&7` for all; the FF41 read flush lands at `ly2 cfl0` for all.
The discriminator is the **sub-dot (`dc`) phase at which that read flush lands**:

| ROM | want | mode-0 IRQ rise | FF41 read flush lands (ly2 cfl0) | SameBoy reads |
|---|---|---|---|---|
| `m0int_m0stat_scx2_1` | 0 | cfl259 dc2 | **dc2** (line-start mode-0 hold) | mode 0 ✓ |
| `m0int_m0stat_scx3_2` | 0 | cfl260 dc4 | **dc2** | mode 0 ✓ |
| `m0int_m0stat_scx4_2` | 0 | cfl261 dc2 | **dc2** | mode 0 ✓ |
| `m0int_m0stat_scx5_2` | 2 | cfl262 dc4 | **dc8** (mode-2 OAM commit) | mode 2 ✓ |
| `late_m0irq_halt_..scx3_2b` | 2 | cfl260 dc8 | **dc8** | mode 2 ✓ |
| `late_m0irq_halt_..scx3_3b` | 2 | cfl260 dc8 | **dc8** | mode 2 ✓ |

`dc` is the 8 MHz sub-dot counter; the line-start mode-0 hold spans `ly cfl0
dc0..dc8`, the mode-2 OAM commit is at `dc8` (≈ slopgb dot 4 = the documented
"first 4 dots mode-0 hold"). **Every want-0 read flush ends inside the hold
(dc2 ≈ dot 1); every want-2 read flush ends at the mode-2 commit (dc8 ≈ dot 4).**
Constant and distinct, per direction. The full pre→post bracket makes the
mechanism explicit (want-2 `scx3_2b`):

```
SBCYR ff41 pre  ly=2 cfl=0 dc=0 pend=4 stat=0   # pre-flush: visible mode 0
SBMODE          ly=2 cfl=0 dc=8 vis=2           # the 4-cycle flush CROSSES the mode-2 commit
SBCYR ff41 post ly=2 cfl=0 dc=0 stat=2          # read samples mode 2
```

vs want-0 `scx3_2`, whose flush ends at `dc2` (`SBMODE ly2 cfl0 dc2 vis=0`)
before the commit → reads mode 0. The 4-cycle read flush is fixed; what shifts
its landing across the `dc8` commit is the **CPU sub-dot wake phase**, visible
upstream as the IRQ-rise `dc` (dc2/dc4 for want-0 vs dc8 for want-2) — a sub-cfl
(sub-dot) phase, *not* a whole-dot or `cfl` difference (the SCX-driven `cfl`
already tracks SCX&7 identically for both directions).

## slopgb side (flag-on tier2, binary `0db1b83`) — the collapse

`SLOPGB_S5DBG` probe, same rows, DMG:

| ROM | want | slopgb dispatch | slopgb FF41 read | result |
|---|---|---|---|---|
| `m0int_m0stat_scx3_2` | 0 | ly1 **dot257** mfi0 | **ly2 dot4 mode2** | FAIL (got 2) |
| `late_m0irq_halt_..scx3_2b` | 2 | ly1 **dot257** mfi0 | **ly2 dot4 mode2** | PASS |
| `late_m0irq_halt_..scx3_3b` | 2 | ly1 dot257 mfi0 | ly2 dot0 mode0 | FAIL (got 0) |

**slopgb produces byte-identical engine output for the collapse pair** `scx3_2`
(want0) and `scx3_2b` (want2): same dispatch dot (`ly1 dot257`), same deferred
read (`ly2 dot4 mode2`), and (per #11e) the same wake `cc=1`. The two ROMs differ
only in their CPU-side halt/instruction setup, which on SameBoy maps to the
sub-dot wake phase above; slopgb's whole-dot M-cycle model quantizes that away at
every M-cycle, so by the time both reach the halt-wake they are in the identical
PPU/`cc` state. (The other want-2 row `scx3_3b` lands at slopgb `dot0` — a third,
*different* whole-dot quantum — confirming the deferred read landing is scattered
relative to SameBoy's sub-dot truth, not uniformly offset.)

## Decision gate

> if the two directions land at a CONSTANT distinct sub-cc T-phase → implementable
> (record it on a finer-than-`cc` field); if PPU-phase-dependent / needs the full
> S7 sub-M-cycle clock → document + FALL BACK.

The phase IS constant and distinct (read flush dc2/want0 vs dc8/want2). But the
"record it on a finer-than-`cc` field" precondition **fails**: the phase is the
CPU's sub-dot (8 MHz) position at the wake, and slopgb's `cc` is its *finest*
phase (within-M-cycle **dot**, 1..=4). scx3_2 and scx3_2b collapse to identical
`cc=1`; there is no slopgb-computed quantity that differs between them at the
wake, so nothing to record. Populating a finer field would require the **S7
sub-M-cycle deferred clock** (the CPU/PPU coupling at 8 MHz instead of whole
dots) — out-of-session infra, the documented class-A lift condition. **VERDICT:
not implementable at slopgb's current whole-dot resolution → FALL BACK** (this
session pivots to mech-3 late-disable).

This is the first **direct** confirmation (vs #11e's inferred `cc`-collapse) that
the halt family is a genuine sub-dot wake residual: the SameBoy ground truth
shows *why* (read-flush landing across the mode-2 commit) and *what slopgb is
missing* (the CPU sub-dot wake phase), with exact `dc` numbers per direction.

## Refuted / not re-chased (carried from #11e, now hard-confirmed)
- cc-gated wake back-date (#11e +11/−3): `cc` collapses the pair → cannot gate.
- read-observer / eighth-grid (#11i): read coordinate identical even at SameBoy's
  finest `dc`; here we see the read landing IS the discriminator but it is driven
  by the wake phase, not derivable from the read frame.
- a uniform −N back-date fixes want-0 but forces the SameBoy-passing want-2 rows
  (`scx3_2b`, `scx3_3b`, `scx5_2`) to mode 0 — must not drop them.

## Fallback (mech-3 late-disable) — ground truth REFUTES a clean fix

The goal's anti-thrash fallback was "ship SOMETHING" via the mech-3 late-disable
family (engine-driver continuation of #11j/k/l), premised on
`m2enable/late_enable_m0disable_2` being a fixable spurious-/missing-edge bug.
Built its SameBoy ground truth FIRST (the discipline) — the premise is **false**.
Flag-on, the whole late-disable family is `pass=10 fail=2` (m2enable) + 1
(lycEnable); the three live fails classify as:

| ROM | want | slopgb flag-on | SameBoy | classification |
|---|---|---|---|---|
| `m2enable/late_enable_m0disable_2` [Dmg] | 0 | got 2 | **reads 2** (`SBREAD ff41 ly0 cfl0 mode=2`) | SameBoy≠gambatte → **C2 AGREE-floor**, not a bug |
| `m2enable/late_m1disable_ly0_2` [Cgb] | 0 | got 2 | (CGB) | CGB lcd-offset territory |
| `lycEnable/lyc153_late_m1disable_2` [Cgb] | E0 | got E2 (`ff0f ly153 dot52 if=02`) | suppresses (`if=00` all ly152-153) | CGB "lycwirq E2" residual |

- **`late_enable_m0disable_2` [Dmg]** is NOT baselined ([Cgb] is); production
  (flag-off) PASSES it (reads mode 0). But SameBoy reads mode 2, and slopgb
  flag-on already reads mode 2 — i.e. flag-on **already matches SameBoy**, which
  disagrees with the gambatte DMG `out0` reference. This is a C2 AGREE row
  (SameBoy==tier2≠gambatte) destined for baselining, **not** an engine bug;
  "fixing" slopgb to read 0 would drop a SameBoy-matching state. The handoff's
  "suppress an already-armed source" framing is refuted for this row.
- **The two CGB fails** are the goal's separately-scoped "CGB lcd-offset
  carryover" / "CGB lycwirq E2" residuals — CGB-only, the lcd_offset-port
  territory that gated #11j/k/l DMG-only (a CGB-only engine suppress risks the
  lcd-offset-shifted real edge landing on the suppressed dot, dropping a
  SameBoy-passing CGB row — exactly the #11l hazard). No DMG component to gate.

**No clean, in-scope, shippable engine fix exists in the late-disable family this
session.** Per the anti-thrash rule (ship ONLY if clean) + "never drop a
SameBoy-passing row" + "ground truth FIRST", the session ships the measurement
+ this refutation, not code. The mech-2 verdict (FALL BACK, sub-dot infra) is the
primary deliverable; the fallback is now bounded ground truth for the next pass.

## Status
Measurement only — no slopgb code changed (docs-only diff; gate byte-identical to
`0db1b83` by construction). SameBoy tracer additions are `SB_TRACE`-gated
(byte-identical unset). Defaults NOT flipped.
