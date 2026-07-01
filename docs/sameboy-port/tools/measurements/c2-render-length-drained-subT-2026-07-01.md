# C2 #11ax — the SS render-length slice method DRAINED (7 families build-measured atomic); the barrier SHARPENED to sub-T operation-order; cgbpal atomic proven via SameBoy palette ground truth

2026-07-01, `phase-b-s7` (base `3e7714f`). Executed the goal's PRIMARY lever — hunt
EVERY remaining SS render-length family for the resolvable-discriminator shape (the
#11at/au/aw write-side shadow method). **Result: ESCAPE. Every remaining SS
render-length candidate is build-measured ATOMIC — no clean +N/−0 slice remains.** The
3 write-side slices shipped this branch (#11at pre-draw abort, #11au reenable, #11aw
late-WY un-trigger) are the clean extent of the method. Fresh two-bin re-confirmed the
base: **flag-on 455 / off 486 → 165 flip-BUGs = 115 SameBoy-pass + 50 rebaseline**
(`classify_cgb_regr.py`), mooneye flag-on 91/91.

## The definitive drained-slice census (build-measured, both emulators)

| family | rows | slopgb reads | SameBoy | VERDICT |
|---|---|---|---|---|
| `m2int_m3stat/scx/late_scx4` | 2 | BOTH legs dot256 mode3 | BOTH at half-dot **520** (`_1` cfl260dc0 / `_2` cfl261dc-2), split mode3/mode0 at the m3→0 transition | **sub-T co-temporal** |
| `window/late_wx_scx5` | 1 | BOTH legs dot260 mode3 (`wa=true`) | co-temporal read | **co-temporal read** |
| `window/m2int_wxA5_m0irq` | 1 | mode0 (FF0F, not FF41) | read-frame | **read-frame (FF0F)** |
| `window/late_disable_spx10_wx0f` | 1 | BOTH legs dot264 mode0, ns=1 (sprite) | exit **268** (`_1`) vs **274** (`_2`); LCDC.5-disable dot **100** vs **104** (whole-M-cycle apart, slopgb resolves) | **write-discriminated EXTEND, but per-config post-draw length** (see below) |
| `window/arg/late_wy_{FFto0_ly0,FFto0_ly2,FFto1_ly2,10to0_ly1}` | 4 | mode0 (bare, window didn't trigger) | window triggered (extends) | **deferred-frame WY-write collapse** (see below) |
| `enable_display/ly0_late_scx7_m3stat_scx1` | 1 | BOTH legs BYTE-IDENTICAL (glitch STAT collapses to a fixed dot-250 exit, SCX-blind) | BOTH at cfl260, split mode3/mode0 via a glitch-line `257+SCX&7` fine-scroll latch | **glitch co-temporal** |
| `cgbpal_m3/{read,write,}m3start_2` + `enable_display/ly1_late_cgbpw_2` | 4 | palette access dot84 | blocked@**176** (see cgbpal below) | **lcd_offset frame INVERSION** (A/B, see below) |
| `halt/*_m0stat_scx{2,3}` | 12 | BOTH legs byte-identical | BOTH read `ly2 cfl0 dc0` (half-dot 0), split mode0/mode2 | **sub-T WAKE co-temporal** (halt_mode_phase refuted #11av) |

## The three deep-dives (why each is atomic, not a mislabeled write-discriminated pair)

### 1. `late_disable_spx10_wx0f` — write-discriminated, but a per-config post-draw LENGTH

This is the ONE family whose discriminating WRITE slopgb resolves (LCDC.5-clear dot
**100** `_1` / **104** `_2`, a whole M-cycle apart) — the #11at shape, EXTEND
direction. SameBoy's mode-3 exit tracks the disable dot: disable100 → exit cfl268,
disable104 → exit cfl274 (+6 exit per +4 disable). slopgb's render aborts and collapses
both to mode0 at the read dot264.

**Why NOT a clean slice:** it is a POST-draw abort on a SPRITE line (ns=1). To force
mode3 (`_2`) the shadow must know SameBoy's mode-3 exit = a function of (disable_dot,
wx, scx, sprite) — the window-tile-completion LENGTH. #11at build-measured that this is
non-monotonic ACROSS configs (early_scx03 abort104→exit257, non-early late_scx0
abort100→exit>260 at the same read), so a general formula is the atomic render reclock,
and a formula scoped to `(spx10, wx0f, sprite)` is a test-ROM special-case (forbidden).
1 row, high-risk, no principled shadow → parked to the render reclock.

### 2. `late_wy` TRIGGER (FFto0/FFto1/10to0) — the deferred-frame WY-write collapse

SameBoy's `wy_check` (`display.c:508`) latches `wy_triggered` at ANY dot where
`WY == current_line` (CGB SS) & WIN_ENABLE — continuous. The 4 rows write WY to match LY
at a LATE dot (FFto1: WY→1 at slopgb ly1 dot452 when LY=1; FFto0_ly0: WY→0 at ly0 dot92;
10to0/FFto0_ly2: WY→0 at ly0 dot452) — a compare SameBoy catches but slopgb's line-start
`wy_latch` sampler misses. The WRITE dot + LY ARE traced (resolvable in principle), BUT:

- The write lands at slopgb dot452 (deferred frame) = effectively the line boundary; the
  #11af shadow reads the 6-dot-lagged `wy2` so its `wy_trig_sb_line` is the NEXT line —
  the cross-line latch the shadow deliberately excludes (over-fires +1/−27, goal-refuted).
- The extend is FRAME-ALIGNMENT-sensitive: SameBoy's per-line mode-3 exit for the read
  line is cfl257 (bare) on most frames and only extends on the specific measurement
  frame — the OCR-capture-frame the deferred write phase mis-places. This is the
  `s7-readclock` deferred-frame WY-write POSITION lever (atomic), not an on-line shadow.

### 3. `cgbpal_m3` + `ly1_late_cgbpw` — the lcd_offset frame INVERSION (proven via SameBoy palette ground truth)

New tooling this session: a SameBoy palette-access tracer (`SBPALR`/`SBPALW`, printing
`cgb_palettes_blocked` at every FF69/FF6B access) + a slopgb palette tracer (`pal`/`palw`
FF68-6B, `SLOPGB_S5DBG`). Direct ground truth:

| row | want | slopgb access dot | SameBoy pos (cfl*2+dc) | SameBoy blocked |
|---|---|---|---|---|
| `read_m3start_2` (offset0) | FF (blocked) | dot84 | 89*2−2 = **176** | **1** |
| `write_m3start_2` (offset0) | 00 (blocked) | dot84 | **176** | **1** |
| `ly1_late_cgbpw_2` (offset0) | 55 (blocked) | dot84 | **176** | **1** |
| `read_m3start_lcdoffset1_1` (offset1) | 00 (accessible) | dot86 | 87*2+0 = **174** | **0** |
| `write_m3start_lcdoffset1_1` (offset1) | 01 (accessible) | dot86 | **174** | **0** |

SameBoy's palette lock engages between pos 174 and 176. The offset0 accesses land at 176
(blocked); offset1 at 174 (accessible). **But slopgb reads offset0 EARLIER (dot84) and
offset1 LATER (dot86) — the ordering is INVERTED.** The lcd_offset shifts slopgb's
deferred read frame the WRONG sign relative to SameBoy, so NO dot-threshold on
`pal_ram_blocked` can put offset0 (block) below offset1 (accessible). slopgb has no
`lcd_offset` field, and both are the same post-glitch line-1 context (both LCD-on at ly0
dot0, glitch signature cfl76) → no scope separates them.

**A/B build-measured** (env-gated `SLOPGB_PALLOCK84`, tier2 palette lock `84+3 → 84`):
the cgbpal+cgbpw subset went **6 fail → 4 fail = +4 offset0 / −2 offset1**
(`read_m3start_lcdoffset1_1` + `write_m3start_lcdoffset1_1`, BOTH SameBoy-passes dropped).
Confirmed A/B swap — forbidden. The `enable_display` triage agent's "resolvable" verdict
for `ly1_late_cgbpw` was WRONG: it measured the row in isolation and missed the offset1
rows that share `pal_ram_blocked` and break — exactly the survey-overturn the goal warns
of; build-measure caught it. Reverted to green.

## The SHARPENED barrier — the terminal lever is sub-T operation ORDER, not the half-dot grid

The co-temporal render-length pairs (`late_scx4`) AND the WAKE pairs (`halt *_m0stat`)
read at the IDENTICAL half-dot (`cfl*2+dc`): `late_scx4` `_1`/`_2` both land half-dot
**520** (the m3→0 transition); `halt` `3a`/`3b` both land `ly2 cfl0 dc0` = half-dot **0**
(the line-2 mode-2 raise). SameBoy returns OPPOSITE modes for reads at the SAME half-dot.

**So the half-dot (8 MHz) PPU clock the goal/#11ao/#11ap name as the lever is NECESSARY
BUT INSUFFICIENT — not just for S6-DS, but for the render-length and WAKE co-temporal
pairs too.** The discriminator is the SUB-T operation ORDER within SameBoy's per-8MHz-tick
main loop (`GB_advance_cycles`): whether `read_high_memory` samples FF41 BEFORE or AFTER
`GB_STAT_update` flips the visible mode / raises mode 2 / the CPU resume completes — all
co-located in the same tick. slopgb's tick-then-access M-cycle model collapses this order.

This UNIFIES the four residual atomic classes under one terminal lever: replicate
SameBoy's exact per-tick operation sequence (read ↔ STAT_update ↔ wy_check ↔ wake ↔
render-FSM step), the cycle-exact main-loop rewrite. It is NOT a finer read SAMPLE clock
(co-temporal, #11ab), NOT a whole-dot/read-law verdict (refuted ×12), NOT the half-dot
tick alone (the co-temporal pairs share the half-dot). This is the "atomic multi-session
rewrite" the port has named since 2026-06-21, now pinned to the operation-ORDER grain.

## Residual (115 SameBoy-pass blockers — UNCHANGED; all atomic)

RENDER-LENGTH 41 · ENGINE-IF 30 · S6-DS 20 · READ-FRAME 12 · WAKE-CLOCK 12. Every SS
render-length candidate is build-measured atomic (this doc); the DS/ENGINE-IF/READ-FRAME
are the #11al/#11am/#11ar read-frame + FF0F IF-delivery classes (structurally unreachable
by any FF41 verdict law — the #11ar peek exhausted them at +9). All converge only under
the sub-T operation-order reclock.

## Gate (END CLEAN)

No behavior change: the only code is two `SLOPGB_S5DBG`-gated palette tracers
(`interconnect/cycle.rs`, byte-identical unset) — the reusable cgbpal/cgbpw tooling.
mooneye flag-on 91/91; gbtr OFF full battery byte-identical (golden + gambatte_matrix +
pins clean); clippy `-D` clean. 115 blockers unchanged; defaults NOT flipped. ESCAPE.
