# Stage 4 — ENGINE-IF re-measure on the collapsed engine (2026-07-02)

Follows the Part-C collapse + palette/accessibility batch (same session).
Method per goal: dual-trace ONE want-pair per family on `fp` BEFORE touching
any law. Result: **two-bin 397 → 388 committed slice (−9/+0) + a further −3
(lycEnable edge-only FF45 fire), all zero-drop.**

## Slice 1 — the CGB line-START carryover hold, GENERALIZED (−6)

Dual-trace `late_ff45_enable_2/_3` (want out1, got out3): slopgb's per-dot
engine re-latches `lyc_interrupt_line` during the CGB dots-0-2 carryover
(`ly_for_comparison` = line−1), so a late FF45 write whose new LYC equals the
PREVIOUS line (LYC:=6 committing ly6 dot453 / ly7 dot1) catches the ly7
carryover value 6 → spurious edge. SameBoy re-latches ONLY at the state-6/-7
steps (dot 3 → −1/hold, dot 4 → line). The #11l refutation of this hold
predates `law_pos`/#11bd — re-measured: the damage confines to STOP-SHIFTED
rows (`late_ff45_enable_lcdoffset1_1` + `ff45_enable_weirdpoint_lcdoffset1_1`,
both SameBoy-passes, dropped by the ungated hold and recovered by the
`lcd_shift_dots == 0` gate — the shifted write's law position in this window
is one poll quantum ambiguous). Lines 2-143 hold gated unshifted; line 1
keeps the unconditional #11r wrap hold. Fixed: `late_ff45_enable_2/_3/ds_2`,
`ff45_enable_weirdpoint_2` (lycEnable) + `lycwirq_trigger_m0_late_ly44_4/ds_2`
(miscmstatirq).

## Slice 2 — the LYC DISABLE direction rides the delayed FF45 copy (−3)

`ff45_disable_2` (want out3, got out1): the deferred FF45 write commits ~4
dots EARLY of SameBoy's instant, so a LYC rewrite landing in dots 0-3 kills
the dot-4 match slopgb-side while SameBoy's edge fires first. The engine's
latch compare ORs `lyc_event` (the production engine's delayed FF45 copy,
protected dots 1-4) — disables delayed, fresh matches live. **Scope measured:
SS + unshifted only** (the DS write frame is +1 dot — the unscoped OR broke
`*_ff45_disable_ds_1` ×3; shifted frames mis-map — `weirdpoint_lcdoffset1_1`).
Fixed: `ff45_disable_2`, `lyc0_ff45_disable_2`, `lyc_ff45_disable2_2`.

## Slice 3 — the FF45-write fire is EDGE-ONLY under the engine (−3)

`lycwirq_trigger_ly00_stat50_1` (want E0, got E2): gambatte's FF45-write
trigger is an EVENT ("fires even while another source holds the line high");
SameBoy's `GB_STAT_update` raises IF only on the line's 0→1 edge. LYC:=0
commits ly0 dot1 with STAT=$50 — SameBoy's line is continuously HIGH across
the ly153→ly0 wrap (VBlank source, mode-1 carried through ly0 dots 0-3), so
the fresh LYC match joins a high line → no edge. Guard the tier2 fire on
`stat_update.line()` low. Fixed: `lycwirq_trigger_ly00_stat50_1` + the
`lcdoffset1`/`ds` variants (3 of 4).

## Built + REFUTED this session (do not naively retry)

- **Blanket delayed-enable view `stat_en | stat_ev`** for the engine's level:
  fixes `ff41_disable_2` but over-delays the mode-source disables — m2enable
  +5 / m1 +1 (the m2enable late-disable cells are pinned LIVE). Only the
  LYC-source disable rides the delayed copy.
- **The FF41-write edge-only guard** (the FF45 guard's twin in
  `stat_write_trigger_cgb`): m2enable +3 — the FF41 retro/m2 pulse reach is
  event-like in the pinned cells.

## The wake-instant class (halt, 13 DMG rows) — confirmed atomic, parked

DMG rowlist probe: the `*_m0stat` want-0 legs (`1a/2a/3a`) read mode 2 (wake
lands late), the want-2 legs (`2b/3b`, `m0int/m0irq_m0stat_scx5_2`) read mode
0 (wake early) — the #11ay 8-hd wake-instant split, unreachable by any
position-keyed law (WAKEPEEK +3/−13, halt_mode_phase +5/−13, full-swap
+4/−20 all burned here). Needs the sub-M-cycle wake clock extension (PORT 2's
sampler at the CGB head-sample grid + the `_3a` IF-clear race + the `_3b`
skip-path M) — the S6/S7 core.
