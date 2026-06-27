# C2 write-side leading-edge FF41/FF45 commit — BUILD-MEASURED, REFUTED (#11v)

2026-06-27, post-#11u. The goal's named C2 START was a **write-side leading-edge
FF41/FF45 commit** — the hypothesised fix for the #11u (B) cluster (the ~10
`lyc0_*`/`lyc153_*`/`m1irq_*disable_2` rows the survey flagged as "FF45/FF41 write
commits cc+4 vs SameBoy cc+0"). **Built it, two-bin measured it: net-negative
−3/+0. The write-side is a non-lever — REFUTED.** Zero rows shipped (the change was
reverted to a tracer-only diff); byte-identical OFF.

## What was built

In `interconnect/cycle.rs::write_deferred`, a flag-gated (`tier2_reclock`) branch
for FF41/FF45 that splits the machine advance at the M-cycle **leading edge**
(`lead = before + clock.pending()`), commits the register (`write_no_tick`) there —
ahead of this M-cycle's own pre-commit dots — then advances the remaining
`lead..after` T's, so the per-dot `stat_update_tick` `ly_for_comparison` LYC
compare runs against the NEW register value (SameBoy `cycle_write`, `sm83_cpu.c:113`).

## Result (two-bin, gambatte flag-on probe)

| set | rows | baseline (no LE) | with LE commit | delta |
|---|---|---|---|---|
| (B) cluster | 20 | 6 pass / 14 fail | 6 pass / 14 fail | **0** |
| lyc/m1/ly0/lycm2int/m2enable/lcdirq_precedence | 437 | 364 pass / 73 fail | 361 pass / 76 fail | **−3 / +0** |

The 3 regressions: `ff45_enable_weirdpoint_lcdoffset1_2`, `lyc153_late_enable_m1disable_3`,
`lyc153_late_m1disable_3` (all `want…E0/0`, `got…E2/2` — newly spurious). Zero fixes.

## Root cause of the refutation

**The flag-on deferred clock ALREADY commits writes at the leading edge.** The
eager (flag-off) path is tick-then-access: `tick_machine` advances a full M-cycle
THEN `write_no_tick` (cc+4). But the deferred `write_deferred` does
`advance_machine_t(before, before+pending+δ)` — paying the **previous** M-cycle's
debt (`pending`, typically 4 dots) plus the conflict pre-split (`δ`) — THEN
`write_no_tick`. So the value already commits at ≈`before+pending` = this M-cycle's
leading edge (cc+0). The survey's "writes commit cc+4" premise is the **flag-off**
behaviour; on the flag-on path the writes are already cc+0.

My "fix" therefore only moved the commit by the conflict pre-split `δ` (the WriteCpu
`+1` T for FF41/FF45) — one dot earlier. That single dot fixes nothing (the (B)
edges are not at that dot) and breaks 3 rows that depend on the `+1` WriteCpu phase.
**There is no separable write-side lever; pushing writes earlier than the deferred
leading edge is wrong-direction.**

## What the (B) cluster actually is (dispatch-level trace)

Widened the `reclock.rs::stat_update_tick` dispatch tracer to vblank (`line < 154`,
+`lycln`) and traced two representative rows (CGB, `--cgb --length 4` SameBoy):

- **`lyc153_late_m1disable_2`** (spurious, `want E0 got E2`): slopgb dispatches the
  LYC edge at **ly153 dot6** (`mfi=1 lycln=1`), reads `ff0f ly153 dot52 if=02`.
  SameBoy fires at **ly153 cfl0**. The `_2` test disables the mode-1 (VBlank,
  `0x50→0x40`) source late; SameBoy holds the STAT line HIGH continuously (VBlank
  hands off to the still-matching LYC=153 source → no fresh 0→1 rise → E0), slopgb's
  per-dot engine sees a FRESH LYC rise at dot6 → spurious E2. = STAT line-level
  **continuity across the ly152→153 boundary** + the read frame. NOT write timing.
  **Level trace** (`StatUpdate::level` per dot, ly153): slopgb dots 0-5 `lvl=0`
  (`en=40` VBlank ALREADY disabled, `lycln=0` in the line-153 `ly_for_comparison`
  gap `-1[0,6)`), then dot6 `lvl=1 lycln=1` (ly_for_comparison=153 matches LYC=153)
  → 0→1 edge → fires. The line DIPPED (dots 0-5 low) because the VBlank disable's
  frame position is one M-cycle ahead of the dot-6 LYC match — SameBoy's lands so
  the handoff has no gap. Whether the handoff has a gap is set by the FF41-disable's
  frame position relative to the dot-6 LYC match = the **atomic frame alignment**,
  not a suppressible engine continuity hold (the `_2` rows are bidirectional with
  their SameBoy-passing `_3` siblings — a hold that suppresses the dip would break
  the sibling, an A/B trade only the reclock resolves).
- **`lyc0_ff41_disable_2`** (missing, `want E2 got E0`): slopgb fires only the ly152
  LYC edge (`ly152 dot4 mfi=1 lycln=1`), reads `ff0f ly153 dot32 if=00`. SameBoy
  fires TWO edges — `STAT_IRQ ly152 cfl0` AND `ly153 cfl0` (the FF41=40 re-enable at
  ly153 cfl0) — final read `if=02`. slopgb's engine raises no edge for the ly153
  re-enable. = dispatch/engine + read frame. NOT write timing.

## Tooling note (SBWH cfl is lazy-synced — do not dot-align against it)

SameBoy's `SBWH` write tracer reports `cfl=0` for EVERY FF41/FF45 write in these
rows (`ff45@146 cfl0`, `ff41@150 cfl0`, … `ff45@152 cfl0`, `ff41@153 cfl0`). That is
**not** "every write lands at dot 0" — SameBoy advances the PPU lazily, so `cfl` at
`write_high_memory` entry is the last-synced PPU position (line start), not the
write's true PPU-time commit dot. slopgb's eager per-dot scan position (e.g.
`wff45=00 ly152 dot48`) cannot be aligned 1:1 against it. Trust the **STAT_IRQ
edge** cfl (the dispatch dot) and the aggregate two-bin OCR delta, not SBWH cfl.

## Verdict

The C2 write-side START is refuted: writes are already leading-edge on flag-on, and
the (B) cluster is the **read-frame + dispatch + line-boundary STAT-continuity** —
the SAME atomic reclock as the (A) read-frame floors (#11u). No write-side sub-target
to land alone. Next C2 lever is the dispatch-dot retime + read-frame co-land (the
atomic global reclock), not the write side. Tracers kept (write-frame in
`write_deferred`, vblank-widened dispatch in `stat_update_tick`); byte-identical OFF.
