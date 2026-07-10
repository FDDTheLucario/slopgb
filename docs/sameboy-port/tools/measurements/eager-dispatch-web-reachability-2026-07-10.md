# The CGB single-speed dispatch/IRQ web is NOT a second wall — it is 2 read-frame + 14 write-commit-frame re-host rows (2026-07-10, #11db)

The load-bearing #11cs question, answered by end-to-end dual-tracing: **is the
~16-row CGB single-speed dispatch/IRQ web reachable on the eager clock, or a
second wall like the 5 halt rows?** Answer: **reachable.** No row is a
#11cz-style sub-M-cycle wall. Two rows are PROVEN read-frame reachable this
session (clean slice, zero SameBoy-pass drops); the other 14 are the
write-conflict commit position the eager `Bus::write` computes and DISCARDS —
a whole-dot (1-T at single speed) re-host against machinery that already exists.

## Baselines reproduced (trust nothing)

`flagon_probe` on `cgb_rowlist.txt` at `e307e7a`: OFF **486**, EV **358**, tier2
**291** — all exact. flip-BUGs (OFF-pass ∩ EV-fail) = 91. Of the single-speed
web families, 32 rows are flip-BUGs; `classify_cgb_regr.py` (SameBoy tester
`~/.cache/sbbuild/…`) splits them **BUG=16 / FLOOR=16 / UNK=0**. The 16 BUG rows
(OFF-pass ∩ EV-fail ∩ SameBoy-pass) are the TRUE-bar SS web — exactly the #11cs
estimate:

```
lycEnable 5   ff41_disable_2 · late_ff41_enable_2 · lyc0_ff41_disable_2 ·
              lyc153_late_ff41_enable_2 · lyc153_late_m1disable_3
m0enable 2    lycdisable_ff41_2 · lycdisable_ff45_3
m2int_m0irq 2 m2int_m0irq_scx3_ifw_2 · m2int_m0irq_scx3_ifw_4
irq_precedence 2  late_m0irq_retrigger_2 · late_m0irq_retrigger_scx1_2
ly0 2         lycint152_lyc153irq_2 · lycint152_lyc153irq_ifw_2
lyc153int_m2irq 1 · m2enable 1 · miscmstatirq 1
```

(The 16 FLOOR rows — `m0enable/disable_*`, `lycdisable_ff45_scx*`, `m1/*`,
`m2enable/late_m1disable_ly0_*`, `lyc0_m1disable` etc. — are SameBoy-FAIL,
rebaseline-OK, and are correctly left alone.)

## Method

Dual-trace OFF / EV / tier2 with `SLOPGB_S5DBG` + `--features port_probe` on
`run_gambatte` (with `SLOPGB_EAGER`), tracing the FF0F/FF41/FF45 CPU writes
(pre-tick dot), the FF0F/FF41/FF44 reads (post-tick cc+4 dot), the two-phase
FF41 engine dispatch (`SLOPGB dispatch … (ff41 t0)`) and the mode-0 rise
(`SLOPGB m0rise`). Find the exact dot where EV's digit-producing STAT bit
diverges from tier2's. All probe/experiment code was reverted; tree
byte-identical at `e307e7a`.

## Bucket 1 — READ-FRAME reachable, PROVEN + shippable (2 rows)

`ly0/lycint152_lyc153irq_2` (want E2, EV E0) and `lyc153int_m2irq_1` (want 0, EV
2 — measured recovery, same arm).

Trace of `lycint152_lyc153irq_2`: the digit comes from ONE `ldh a,(FF0F)` at
ly153 **dot 4**; the LYC=153 latch dispatches at ly153 **dot 6**. EV's cc+4 read
at dot 4 sits 2 dots *before* the latch → reads `intf=00` → E0. This is exactly
the `Ppu::ff0f_stat_peek` LYC arm (b) case the docstring names — the CGB LYC
delivery latch lands *beyond* cc+4, so even the eager trailing read misses it and
needs the verdict-only peek. tier2 applies `ff0f_stat_peek` in `read_deferred`;
the eager `Bus::read` does not.

**Experiment (reverted):** OR `ff0f_stat_peek() & !ff0f_ly0_pulse_mask()` into
the eager FF0F read under `eager_value`. **EV CGB 358 → 349** (+9): recovers
BOTH bar rows + 7 OFF-fail flip-gains, and breaks **ZERO** rows (no SameBoy-pass
drop). This is the same VALUE-peek shape as the #11cv halt-entry fix — directly
shippable next session as a CGB-scoped slice.

## Bucket 2 — WRITE-COMMIT-FRAME, reachable-in-principle, NOT a wall (14 rows)

The rest — lycEnable 5, m0enable 2, m2enable 1, m2int_m0irq 2, irq_precedence 2,
miscmstatirq 1, ly0/`lycint152_lyc153irq_ifw_2` 1. Twelve show a SPURIOUS STAT
bit (want E0/0, EV E2/2), two a MISSING one (`ff41_disable_2` want 2 EV 0;
`lyc0_ff41_disable_2` want E2 EV E0). Both directions have the **same root**,
traced independently in two rows:

### `ff41_disable_2` (want 2, EV 0) — the LYC-latch straddle

| dot (ly6=456) | EV | tier2 |
|---|---|---|
| LYC=6 coincidence latch | ly6 dot 0 | ly6 dot 0 |
| FF41=0x00 (disable STAT) commit | **ly6 dot 0** | **ly6 dot 1** |
| LYC STAT dispatch | — (source already off) | **ly6 dot 4** (`lycln=1`) |
| FF0F read ly6 dot 8 | 00 → **0** | 02 → **2** ✓ |

EV commits the disabling FF41 write at the M-cycle boundary (dot 0), coincident
with the LYC latch, killing the source before it dispatches. tier2 commits one
dot later (dot 1), after the latch, so the LYC IRQ still fires.

### `m2int_m0irq_scx3_ifw_2` (want 0, EV 2) — the IF-clear straddle

| dot | EV | tier2 |
|---|---|---|
| mode-0 STAT rise (`m0rise`) | ly1 **dot 257** | ly1 **dot 257** |
| FF0F=0x00 clear commit | ly1 **dot 256** | ly1 **dot 257** |
| FF0F read ly1 dot 268 | e2 → **2** | 00 → **0** ✓ |

**The mode-0 rise fires at the IDENTICAL dot 257 on both clocks** (so this is
NOT a render-frame / `flip_dot` divergence — the render is byte-identical). The
sole difference is the FF0F clear commit: EV lands it at dot 256 (M-cycle
boundary, one dot *before* the rise → doesn't clear it), tier2 at dot 257
(coincident, event-first ordering → clears it).

### Why it is write-frame, and why it is not a wall

The eager `Bus::write` computes the SameBoy write-conflict class
(`write_conflict`: IF/STAT/LYC = `GB_CONFLICT_WRITE_CPU`) and then **discards
the commit position** — the write lands at cc+4 (the whole-M-cycle boundary,
because `tick_machine` advances all 4 dots at once). SameBoy commits WriteCpu one
T into the M-cycle. At single speed 1 T = 1 dot, so the two land one dot apart —
and every web row here turns on which side of a STAT rise / LYC latch that single
dot falls.

This is **not** the #11cz halt wall: there the discriminator (the sub-M-cycle
WAKE instant) is *quantized away* by the eager whole-M-cycle wake, so no
downstream lever can recover it. Here the discriminator is a **known whole-dot
write-commit T** the eager clock *can* represent — the shipped `ppu.stage_write`
mechanism already places render-register writes (FF40/42/43/47-4B) at chosen
sub-M-cycle dots under `eager_value`. Extending that staging to the STAT/IF/LYC
engine view (`stat_en`/`eng_stat`/`intf`) at the WriteCpu dot is the lever — an
eager write-conflict-commit port using existing machinery, moderate build, no
new physics. No #11cz-style mutual-exclusion pair exists in this cluster: the
two colliding rows are separated by the write-commit dot, which is dot-resolvable.

### Refuted levers (do NOT re-chase)

- **Blanket FF0F cc+0 read (`SLOPGB_FF0F_LE`):** EV CGB 358 → **433** (recovers 2,
  breaks 80). The reads are mostly frame-correct at cc+4; a wholesale cc+0 view
  is wrong for every read that legitimately wants the folded rise.
- **Commit IF (FF0F) write at cc+0 (`SLOPGB_IFWR_EARLY`):** EV CGB 358 → **370**
  (recovers 0 of the bar, breaks 18). The fix needs the write *later* (at the
  WriteCpu T), not earlier — cc+0 is the wrong direction.
- **A dispatch move.** #11cl already proved the eager dispatch is at cc+4 =
  production = SameBoy; every rise dot traced here is identical EV↔tier2. The
  "counter-pinned, lands with the flip" label from older maps is **wrong** — the
  web is a write-commit-frame miss, not a dispatch-position miss.

## Bucket 3 — RENDER-FRAME / WALL: 0 rows

No web row is render-frame (the mode-0/LYC rise dots are byte-identical EV↔tier2
in every trace) and none is a sub-M-cycle wall (no #11cz mutual exclusion; the
discriminator is always a whole-dot write-commit or a value-peekable latch).

## Bucket counts

| bucket | rows | status |
|---|---:|---|
| READ-FRAME reachable | 2 | **PROVEN** (EV 358→349, 0 drops) — shippable |
| WRITE-COMMIT-FRAME | 14 | reachable-in-principle, machinery exists, not shipped |
| RENDER-FRAME | 0 | — |
| WALL (sub-M-cycle) | 0 | — |

## THE VERDICT

**The eager flip is achievable.** The largest single CGB cluster — the ~16-row
single-speed dispatch/IRQ web — is NOT a second wall. It is entirely read-frame
+ write-commit-frame re-host work against mechanisms that already exist in the
tree (`ff0f_stat_peek`; `write_conflict` + `stage_write`), at whole-dot
single-speed resolution. The endgame stands as:

1. **L1** — CGB DS re-host of the shipped SS eager slices (~19 rows, mechanical).
2. **L2** — DMG window / `late_wy` re-host (~23 rows, proven `|| eager_value`).
3. **The read-frame FF0F peek slice** — 2 web rows PROVEN here, shippable now.
4. **The eager write-conflict-commit port** — the 14 write-frame web rows; a
   moderate build extending `stage_write` to the STAT/IF/LYC engine view at the
   WriteCpu dot. Reachable, not a wall.
5. **The 5 CGB halt rows** — the ONE genuine sub-M-cycle wall (#11cz): either the
   eager half-dot wake-clock build or a documented class-F-style rebaseline
   exemption of 5 SameBoy-PASS rows.

Only the 5 halt rows are a true wall. Everything in the dispatch/IRQ web is
tractable — the flip does not need a hybrid clock for the web.

## Reproduce

```
# baselines: SLOPGB_PROBE_{OFF,EV,RECLOCK}=1 flagon_probe on cgb_rowlist.txt → 486/358/291
# classify SS web flip-BUGs: python3 docs/sameboy-port/tools/classify_cgb_regr.py <rels> → BUG=16
# read-frame slice (EV 358→349, 0 drops): in Bus::read, under eager_value+addr==FF0F,
#   trailing = (trailing | ppu.ff0f_stat_peek()) & !ppu.ff0f_ly0_pulse_mask();
# traces: run_gambatte + SLOPGB_EAGER=1 + SLOPGB_S5DBG=1 --features port_probe;
#   add EAGERWR (pre-tick) / EAGERRD (post-tick) / T2WR (write_deferred) dot prints.
# All experiment/trace code REVERTED; tree byte-identical at e307e7a.
```

## Gate state

Map-only. No `.rs` modified (all probes reverted) → golden byte-identical, tier2
291, EV 358 unchanged, defaults never flipped. Signed commit.
