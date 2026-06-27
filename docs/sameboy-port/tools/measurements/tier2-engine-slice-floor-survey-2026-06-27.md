# Tier2 engine-driver / read-observer slice — build-measure floor survey (#11u)

2026-06-27, post-#11t. The goal's START slice was the **engine-driver lyc/m1
lineage** (lycEnable 19 + m1 14 + ly0 5 + lyc153int_m2irq 5 ≈ 43, the #11j/k/l/r
roots). Build-measured **26 representative rows spanning every in-scope family**
(every non-window / non-S6 / non-S7 family in the 220-regression survey) against
SameBoy ground truth. **Result: ALL 26 are C2-class floors. No clean tier2
read-frame ADD lever remains in the surveyed families.** Zero rows shipped, zero
SameBoy-passing rows touched, byte-identical OFF (no code change).

This is the convergence signal: the S5 incremental clean-lever phase (#11j–#11t,
which extracted the DMG engine roots, the CGB lcd-offset accessibility windows,
and the DS sprite read-grid snap) has run dry. The residual 220 are uniformly the
atomic **C2 global reclock** (deferred-read frame position / dispatch dot / render
mode-3 length), not more incremental slices.

## TOOLING LESSON (load-bearing — cost ~half the session before caught)

**CGB gambatte ROMs need `sameboy_tester --cgb --length 4`; DMG needs `--length
2`.** Shorter and the gambatte setup has not finished — SameBoy is still in its
pre-setup spin (reads IF=01 in a loop, never writes FF41/FF45, STAT line constant)
so SBLEVEL/STAT_IRQ/SBWRITE all trace **zero**, which reads exactly like "SameBoy
does nothing for this ROM" and produces a FALSE register-divergence diagnosis.
Confirmed: `lycint143_m1irq_2` SBLEVEL = 0 (len1) → 143 (len2) → 383 (len3);
`lyc153int_m2irq_1` CGB needs len4 (len2/3 = 0, len4 = 15366 SBLEVEL with the real
en=0x60 / LYC=153 that MATCHES slopgb). Always confirm SBWH/SBLEVEL is non-zero
before trusting a SameBoy trace; bump `--length` until the register writes appear.

New SameBoy tracers added this session (kept in `/tmp/sbbuild`, document in
`stat-irq-trace.md` next): **`SBWH addr=.. val=.. ly=.. cfl=..`** at
`memory.c::write_high_memory` entry (FF41/FF45 register-write timing — the
existing `SBWRITE ff45` at the `case GB_IO_LYC` works too once length is right);
**`SBU ly=.. mfi=.. stat=.. lycln=.. line=..`** (env `SB_DBGU`) per
`GB_STAT_update` for `current_line<=2` (the per-step mfi/stat dump).

## Method (per row)

`ON==OFF` ⇒ render floor; `OFF pass ∧ ON fail` ⇒ the survey's "regression" (all 26
are this). The classifier (`scratchpad/classify.sh` + `classify2.sh`): slopgb-ON
dispatch edge-set + measurement read (the single non-`if=00` FF0F or the FF41
mode read) via `SLOPGB_TIER2=1 SLOPGB_S5DBG=1 run_gambatte … cgb`; SameBoy
`--cgb --length 4 SB_TRACE=1` STAT_IRQ edge-set + measurement read + SBWH writes.
A clean lever ⇒ slopgb's edge-set differs from SameBoy's in a fixable way (a
missing/spurious EDGE with registers matching). A floor ⇒ the edge-set + registers
MATCH SameBoy and only the deferred **read frame position** diverges (mech-1), or
the **dispatch dot** / **render mode-3 length** diverges.

## Floor taxonomy (26 rows)

### 1. Read-frame frame-position (mech-1) — the bulk (lyc/m1/ly0/lyc153/m0enable/m2int_*/miscmstatirq/irq_precedence/m2enable)
slopgb-ON register writes are **identical to slopgb-OFF** (no control-flow
divergence) AND **match SameBoy** (verified via SBWH at length 4). slopgb fires
the **same** STAT edges SameBoy does (same lines, ±2-dot dispatch). The failure is
purely that the deferred cc+0 measurement read lands at a **different frame
position** than SameBoy's read, sampling IF/FF41 at the wrong moment:

| row | want/ON | slopgb meas read | SameBoy meas read | note |
|---|---|---|---|---|
| `lyc153int_m2irq_1` | 0/2 | `ff0f ly0 dot4 if=02` | `ff0f ly144 if=01` | same per-line OAM edges both |
| `lycint152_lyc153irq_ifw_2` | E0/E2 | `ff0f ly153 dot16 if=02` | (reads ly144 if=01) | ly153 LYC edge fires both; spurious ly152 is NOT the cause |
| `lycint152_lyc153irq_2` | E2/E0 | `if=00` | `ff0f ly153 cfl0 if=02` | E0/E2 flip pair — 4-dot shift can't fix both |
| `lyc0_ff41_disable_2` | E2/E0 | fires ly152 only | ly152+ly153 (LYC=0 line-153 window) | FF41=00 disable races the ly153 edge at same dot |
| `irq_precedence/late_m0irq_retrigger_scx1_1` | E2/E0 | misses m0-retrigger STAT | `ff0f ly1 cfl0 if=02` | per-line m0 both |
| `miscmstatirq/lycstatwirq_trigger_ly00_10_50_1` | E0/E2 | `ff0f ly0 dot20 if=02` | `ff0f ly0 cfl0 if=02` (passes E0) | #11k named-target family; read-frame |
| `miscmstatirq/lycwirq_trigger_m0_late_ly44_4` | E0/E2 | `ff0f ly69 dot16 if=02` | `ff0f ly144 if=01` | per-line m0 both |
| `m2int_m2irq_late_retrigger_1` | 2/0 | misses m2-retrigger | `ff0f ly2 cfl0 if=02` | per-line OAM both |
| `lyc153int_m2irq_ifw_1` | 2/0 | `if=00` (misses) | per-line OAM | read-frame |
| `m2enable/lyc1_m2irq_late_lyc255_2` | 0(cgb)/2 | `ff0f ly2 dot8 if=02` | per-line OAM, reads ly144 | read-frame |
| `m0enable/disable_2`, `late_enable_2` | — | FF41 mode + FF0F mixed | matching edges | read-frame |

slopgb's line-153 `ly_for_comparison` model was VERIFIED correct vs SameBoy
(`display.c:2235-2253` SS: -1[0,6) 153[6,8) -1[8,12) 0[12,…) == slopgb
`ly_for_comparison_line_153`) — the lyc153 failures are NOT the ly_for_comparison
model, they are the read frame.

### 2. Render mode-3-length (m2int_m3stat scx, vram_m3 scx2, oam_access scx2, cgbpal_m3end)
slopgb reads the FF41 mode (or accessibility) at ~the right position but its
**mode-3 extends differently** from SameBoy, so the read returns the wrong mode.
`m2int_m3stat/scx/late_scx4_2`: slopgb `ff41 ly1 dot256 mode3` vs SameBoy `ly1
cfl261 mode0`. `cgbpal_m3end_{scx2,scx5,ds}` all ON=7 (mode-3 bits) want 0. This
is the render-grid mode-3 length = C2 render rebaseline (the survey's "scx2/scx5
read-collapse floor").

### 3. Glitch-line render (enable_display/ly0_m0irq_scx1)
slopgb fires ly0 mode-0 at dot250 (frame-0 glitch line) / dot254; SameBoy at
**cfl260** (glitch-line mode-3 is LONGER on SameBoy). The dot-252 read catches
slopgb's too-early edge → E2 vs want E0. Glitch-line render length = A13 / C2.

### 4. Startup frame (display_startstate/stat_2, stat_scx2_2)
The first FF41 read after display-on: slopgb `ff41 ly0 dot252 mode3` (→ 0x87)
vs SameBoy `ff41 ly2 cfl0 mode0` (→ 0x84). Different LINE → startup read-frame.

### 5. Pinned A/B window (cgbpal_m3 m3start read/write) — CONFIRMED irreducible
`cgbpal_read_m3start_2` outFF (slopgb reads accessible=0), `cgbpal_write_m3start_2`
out00 (slopgb writes through=01), `cgbpal_m3start_2` out0 (ON=1). **Experiment**
(temporarily set the tier2 palette lock `84+PAL_M3START_OPEN`→`84`): the 3 base
`_2` rows all PASS, but the **pinned** `cgbpal_{read,write}_m3start_lcdoffset1_1`
(pin `tier2_cgbpal_m3start_lcdoffset1_passes`) both BREAK. The base wants the lock
≤84; the lcdoffset variant (read shifted to dot86 by the offset) wants the lock
≥87. No single lock dot serves both — only modeling the lcd-offset (C2) shifts the
lcdoffset read so a dot-84 lock works for both. Irreducible A/B trade → floor.

## Verdict

The engine-driver lyc/m1 START slice — and every other in-scope family (read-
observer, glitch, startup, m2/m0/lyc/misc engine) — is build-measured **floor**.
The dispatch edge-sets and register state MATCH SameBoy (the engine is correct);
the residual is the deferred-read frame position (mech-1, counter-pinned dispatch
dot), the render mode-3 length, or a pinned A/B window — all **C2 global reclock**.
The S5 incremental clean-lever phase is exhausted. Next: C2 (the ~7000-row render
rebaseline + window-length model + CGB-OCR frame-alignment + genuine-floor
baseline) → C3 (flip defaults) → C4 (golden + all-oracle-zero-drop). Defaults NOT
flipped. Gate unchanged (no code touched): gbtr+mooneye OFF byte-identical, 21
tier2 pins held, mooneye flag-on 91/91.

Rows + traces: `scratchpad/classify_out.txt`, `classify2_out.txt` (this session).

## ADDENDUM — full edge-set sweep of all 26 non-ds lyc/m1 rows (per-row, not representatives)

After the representative pass, swept ALL 26 non-ds rows of the START slice
(lycEnable 13 + m1 8 + ly0 2 + lyc153int_m2irq 3) comparing slopgb-ON's STAT
edge-set (every dispatch fire-line) against SameBoy's STAT_IRQ edge-set at
`--length 4` (`scratchpad/sweep_edges.sh` → `sweep_edges_out.txt`). The slice
splits cleanly into TWO floor classes, both = the C2 global reclock:

### (A) read-frame floors — slopgb edge-set MATCHES SameBoy exactly
`m1irq_m2enable_lyc_1`, `m2m1irq_ifw_2`, `lyc153int_m2irq_{1,ifw_1,late_retrigger_1}`,
`ly143_late_m0enable_2`, `lyc143_late_m0enable_lycdisable_2`: slopgb fires the
SAME per-line OAM (mfi2 cfl0/dot0) / mode-0 (mfi0 cfl257/dot254) / ly144 vblank
(mfi1) edges SameBoy does — every line, same set. Only the deferred cc+0
measurement read lands at a different frame position. = mech-1 read-frame = C2.

### (B) write-frame floors — a SPURIOUS `ly152 dot4 mfi=1` (LYC=152 match) cluster
**~10 rows** fire a spurious line-152 LYC edge SameBoy never fires:
`lyc0_ff41_disable_2`, `lyc0_ff45_disable_2`, `lyc0_late_ff45_enable_2`,
`lyc0_m1disable_2`, `lyc153_late_enable_m1disable_2`, `lyc153_late_ff41_enable_2`,
`lyc153_late_m1disable_2`, `lycint152_lyc153irq_2`, `lycint152_lyc153irq_ifw_2`,
`m1irq_late_enable_2` (+ the `m1irq_m{0,2}disable` / `ff4{1,5}_disable` siblings
fire spurious ly5/6/7 / ly142/143 by the same mechanism). At the edge: **en=0x40
(LYC), lyc=152** — a legit LYC=152 match at line 152 dot 4 in slopgb's frame.
**Root (SBWH-confirmed):** the test writes FF45 to change/clear LYC at line 152
`cfl0` (e.g. `lyc0_ff41_disable_2` SameBoy `SBWH ff45 val=00 ly152 cfl0` → LYC=0
BEFORE its dot-4 `ly_for_comparison`=152 compare → no match → no edge). slopgb is
**tick-then-access**: the PPU dot-4 `stat_update_tick` (compare, sees stale
LYC=152 → match → edge) runs BEFORE the CPU's FF45 write commits (cc+4, end of
M-cycle). SameBoy is leading-edge: write at cc+0 THEN compare. So the write that
should preempt the line-start LYC match commits one M-cycle too late in slopgb.
= the **write-side** of the C2 reclock (FF45/FF41 commit cc+4 vs SameBoy cc+0).
Not fixable in tier2 without reordering writes ahead of the per-dot tick (breaks
tick-then-access + the counter-pinned dispatch); retracting the already-fired
dot-4 edge on the later write would be a curve-fit. = C2 write-side.

**Conclusion stands and is sharpened: the lyc/m1 START slice is read-frame floors
(A) + write-frame floors (B), BOTH the C2 global reclock (read-side + write-side).
No clean tier2 ADD lever. The write-side is a NEW, concrete C2 sub-target: a
leading-edge FF41/FF45 write commit will fix the ~10-row (B) cluster.**

## ADDENDUM 2 — the (B) write-side sub-target BUILD-MEASURED → REFUTED (#11v, 2026-06-27)

The (B) "leading-edge FF41/FF45 write commit" sub-target above was **built and
two-bin measured: net −3/+0, REFUTED.** The (B) ROOT diagnosis ("slopgb tick-then-
access runs the dot-4 compare BEFORE the FF45 write commits cc+4") is the **flag-OFF**
behaviour; on the **flag-on** path the deferred `write_deferred` already commits
writes at the leading edge (`advance_machine_t(before, before+pending+δ)` pays the
PREVIOUS M-cycle's debt THEN `write_no_tick` → value live at ≈`before+pending` =
cc+0). A leading-edge split only moved the commit by the WriteCpu `+1` (one dot):
(B) cluster 6/14→6/14 (0 change), lyc/m1 family 364→361 (−3, zero fixes). The (B)
rows are dispatch/read-frame + STAT line-level continuity across the line boundary,
NOT write timing (`lyc153_late_m1disable_2`: slopgb fires a FRESH `ly153 dot6` LYC
edge where SameBoy holds the line HIGH across the ly152→153 VBlank→LYC handoff; the
SameBoy `SBWH` cfl that grounded the (B) root is lazy-synced = stale line-start, not
the commit dot). **No separable write-side lever; (B) collapses into the same atomic
read-frame/dispatch reclock as (A).** Full: `c2-writeside-leadingedge-refuted-2026-06-27.md`.
