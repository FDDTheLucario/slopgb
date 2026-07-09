# Re-host the DMG §3b read laws onto the eager clock — EV DMG 145→102, the clean read-frame vein DRAINED (2026-07-09, #11ci)

Task (continuation of #11ch): the eager-value (EV) clock — the C3-flip base —
REGRESSED DMG-OCR (DMG OFF 103 → EV **145**, 107 flip-BUGs) because the tier2
DMG §3b verdict laws live inside `read_deferred` (only under `tier2_reclock`)
and never fire under eager. Re-host them onto the eager read path, incrementally,
golden-safe.

**Result: TWO flag-gated read-frame slices SHIPPED (`eager_value`-scoped,
production + tier2 + CGB byte-identical), EV DMG 145→109→102 — now BELOW the
103 production floor. The FF0F re-host and the render/dispatch/halt floors were
BUILD-MEASURED as the DMG mirror of the CGB #11cg residual: the clean whole-dot
read-frame vein is DRAINED at 2 slices (line-boundary mode + coincidence); every
remaining SameBoy-PASS blocker fails EV on a render-length / write-commit-dot /
dispatch-loop / halt-wake / accessibility-half-dot difference the eager whole-dot
clock structurally cannot match — the HALFDOT-Part-A floor, DMG side.**

## Baselines (branch `finish-port-halfdot` @ `51efd7e`, `CARGO_TARGET_DIR=target/ag3`)

Same BIN for all probes; `gambatte::flagon_probe::flagon_probe`.

| bin | rowlist | fail | note |
|---|---|---:|---|
| DMG OFF (production) | `dmg_rowlist.txt` | 103 | reference floor |
| DMG EV (`SLOPGB_PROBE_EV=1`) start | `dmg_rowlist.txt` | **145** | 107 flip-BUGs |
| DMG EV end (2 slices) | `dmg_rowlist.txt` | **102** | −43, below 103 |
| DMG tier2 (default probe) | `dmg_rowlist.txt` | 116 | the deferred clock is WORSE for DMG |
| CGB EV | `cgb_rowlist.txt` | 400 | must not move — held |
| tier2 CGB | `cgb_rowlist.txt` | 291 | must not move — held |

The 107 start flip-BUGs classified vs SameBoy 1.0.2 (`--dmg --length 4`,
`classify_dmg.py`): **BUG(SameBoy-PASS, must-recover)=98, FLOOR(SameBoy-FAIL,
rebaseline-OK)=9, UNK=0.** The 9 FLOORs: `m1/m1irq_m2*`/`m2m1irq_ifw_2` ×4
(SameBoy reads mode 3 where gambatte wants 1), `window(/arg)/late_wy_1` ×2 (sb 3
vs want 0), `lyc153int_m2irq_ifw_1`, `m0enable/lycdisable_ff41_scx3_2`,
`miscmstatirq/lycstatwirq_trigger_m0_late`.

## The 2 SHIPPED slices — the clean whole-dot read-frame vein

Both are the DMG analogue of a CGB #11cg / #11by–#11cg eager read-frame slice: an
FF41 field the eager cc+0 read samples 4 dots (DMG is single-speed) before the
CPU-visible cc+4 value. `eager_value && !is_cgb` scoped → golden + tier2 291 +
CGB EV 400 byte-identical by construction.

| # | slice | site | mechanism | ΔEV DMG | recovered (0 regressions) |
|---|---|---|---:|---|
| 1 | FF41 line-boundary mode back-dates (`vis_mode_read`, `read_laws.rs`) | 3 DMG arms after the CGB eager arms | line-start OAM entry (lines 1-143 dots 0-3, m0→2), VBlank entry (line 144 m0→1), 153→0 wrap (m1→0; DMG line-0 dots 0-3 = mode 0, not CGB's mode 1). `dot<4` + `!glitch_line` scope. | 145→109 (−36) | `halt/*_m0stat`, `lycint_m0stat`, `lycm2int_m0stat`, `m0int_m0stat` (want 2); `enable_display/*_m1stat`, `m1/lycint_m1stat` (want 1); `ly0/lycint152_ly0stat_2` (want 0) — 36 rows |
| 2 | LYC-coincidence back-date (`read_cmp` / `compare_ly_irq_shift`, `lyc.rs`) | DMG arm of `read_cmp` | evaluate the DMG readable-flag table (`compare_ly_irq`, with its line-start forced-invalid gaps) at the +4-dot cc+4 position; the 153→0 wrap folds to line-0 `Some(0)`. | 109→102 (−7) | `ly0/lyc0flag`/`lyc153flag`, `lycint_lycflag`, `enable_display/frame1_m2stat_count_1` — 7 rows; **also recovered mooneye `lcdon_timing-GS` under eager** (the standing exemption). |

Commits: `16bedb0`, `f98a092` (each signed `%G?`=G, golden PASS + CGB EV 400 +
tier2 291 + mooneye 91/91 OFF+ON verified).

### The critical guard lessons (both slices)

- **`stat_lyc_onoff` (mooneye) is NOT `lcdon`.** Slice-1's first cut broke mooneye
  DMG `stat_lyc_onoff` (ALL of Dmg/Mgb/Sgb/Sgb2) — masked by a
  `grep …lcdon` that hid every other failing row. The mooneye ON check MUST
  print the FULL failure list, never filter to one ROM. Root cause: the line-0
  OAM-entry arm (line 0 dots 0-3 m0→2) fired at line 0 **dot 0**, where mooneye
  `stat_lyc_onoff` wants mode 0 but gambatte `ly0/lycint152_ly0stat_3` wants mode
  2 — a **sub-dot ambiguity the whole-dot frame cannot split** (both read dot 0).
  The arm was DROPPED (`ly0stat_3` parked as a HALFDOT floor); `!glitch_line`
  added for the LCD-enable line's own back-date.
- The coincidence slice never regressed `stat_lyc_onoff` (the shift is a no-op at
  its read positions) — the misattribution above was slice-1's break surfacing.

## FLOOR — the FF0F re-host BUILD-MEASURED and REFUTED (+35)

The ~10 SameBoy-PASS FF0F blockers (`ly0/lyc*irq`, `m2int_m0irq/*_ifw`,
`irq_precedence/late_m0irq_retrigger`, `lycEnable/lyc153_late_*`,
`enable_display/ly0_m0irq`) fail EV on the STAT-IF bit (want E0 got E2, or the
reverse). The eager `Bus::read` samples FF0F at **cc+4** (trailing, past the STAT
rise), where `read_deferred` samples cc+0 and folds the imminent rise with the
§3b FF0F laws (`ff0f_stat_peek` / `ff0f_dmg_service_clear` /
`ff0f_dmg_m0_coincident_mask`).

Experiment (reverted): route DMG FF0F through `leading_edge_sample` (cc+0) + the
§3b FF0F block, `|| eager_value` on the DMG gates (`dmg_m0_if_rise`,
`ff0f_dmg_m0_coincident_mask`).

| config | DMG EV fail | reading |
|---|---:|---|
| control (FF0F cc+4) | 102 | — |
| **FF0F → cc+0 + §3b laws** | **137** | **+35 WORSE** |

The deferred cc+0 FF0F frame is broadly wrong for DMG — DMG tier2 (the full
deferred clock) is **116**, already worse than eager 102. Importing the deferred
FF0F frame for FF0F reads regresses far more currently-passing eager cc+4 FF0F
rows than the ~10 it recovers. The eager cc+4 FF0F is a DISTINCT read frame; the
SameBoy value needs the coherent per-T half-dot clock, not the deferred cc+0
frame. **Do NOT re-attempt the deferred-FF0F-frame routing for DMG.**

## The residual — 55 SameBoy-PASS blockers, ALL half-dot / deferred-write / dispatch / wake FLOOR

At EV=102 the flip-BUGs (OFF-pass ∩ EV-fail) = 64; SameBoy classification:
**BUG=55, FLOOR(rebaseline-OK)=9.** The 55 by mechanism (all structurally
un-recoverable on the eager whole-dot clock — the DMG mirror of the CGB #11cg
K≈70):

| mechanism | blk | why (floor) |
|---|---:|---|
| window render / write-commit-dot (`window`, `window/arg`, `m2int_m3stat/scx/late_scx4`) | 29 | the DMG window arms (D1/D6/D3/D-wx) FIRE under eager but the render inputs (`win_active`, `wx_match_dot`, `scx_write_dot`, `win_aborted`) differ — the eager `tick_machine` drains a staged mid-mode-3 write within its M-cycle, landing the fine-scroll/window commit 2-4 dots off the deferred frame. tier2 passes 26 of these. Same floor as CGB window (#11cg). |
| FF0F cc+4-vs-cc+0 frame (`ly0/lyc*irq`, `m2int_m0irq`, `irq_precedence`, `lycEnable`, `enable_display/ly0_m0irq`) | ~10 | re-host REFUTED above (+35). Needs the half-dot clock. |
| halt-wake clock (`halt/late_m0int_halt_m0stat`, `late_m0irq_halt_*`) | 6 | eager has no `wake_skew` repay (that path is `read_deferred`); the post-wake read frame differs. |
| dispatch loop-timing (`enable_display/frame*_count`, `m2enable/late_enable`, `m2int_m0irq_scx3_ifw`) | ~6 | the OCR captures a different dispatch-loop iteration (count E0/E2, mode-enable latch); moving dispatch is forbidden (`intr_2`, #11br). |
| accessibility half-dot (`vram_m3/postread`, `oam_access/postwrite`) | 2 | the deferred `vis_early` back-date; #11cb REFUTED the eager gate-flip (CGB). |
| line-0 sub-dot (`ly0/lycint152_ly0stat_3`) | 1 | dot-0 whole-dot ambiguity vs mooneye `stat_lyc_onoff` (see slice-1 guard lesson). |

## VERDICT — the eager DMG read-frame vein is DRAINED at 2 slices; the residual = the DMG HALFDOT floor

The two whole-dot read-frame back-dates (line-boundary mode + coincidence) were
the whole clean DMG vein — they adjust an FF41 field the eager cc+0 read left
un-shifted, reproducible whole-dot. Every remaining SameBoy-PASS blocker fails EV
on a render-length / write-commit-dot / dispatch-loop / halt-wake /
accessibility-half-dot difference — the SAME structural floor #11cg proved on the
CGB side (K≈70). The C3 flip stays gated SOLELY on the coherent per-T half-dot
retime + deferred write clock (HALFDOT Part A), now confirmed on BOTH models:

- CGB: EV 400, flip bar 70 half-dot floor (#11cg).
- DMG: EV 102 (below the 103 production floor), flip bar ~55 half-dot floor
  (this session) — the DMG §3b length/boot/if laws that the deferred clock
  carries are re-hosted onto eager where whole-dot-reproducible (2 slices), and
  proven un-portable where they need the deferred render/wake/dispatch frame.

## Reproduction

```
CARGO_TARGET_DIR=target/ag3 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/ag3/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1  $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 102
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_OFF=1 $BIN ... | grep pass=  # 103
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1  $BIN ... | grep pass=  # 400
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt                    $BIN ... | grep pass=  # 291 (tier2)
# flip-BUG classify:
export SLOPGB_GBTR_ROOT=$(pwd)/test-roms/game-boy-test-roms-v7.0
python3 docs/sameboy-port/tools/classify_dmg.py flipbugs.txt out  # BUG=55 FLOOR=9
```

Golden-safe verified after every kept slice: `golden_fingerprint --release`
PASS; CGB EV 400 / tier2 291 byte-identical; mooneye `acceptance_ppu` OFF + ON
(`SLOPGB_MOONEYE_EAGER=1`) both 91/91 (slice 2 removed the `lcdon_timing-GS`
exemption under eager); clippy `-D warnings` clean; all `.rs` < 1000 lines
(`read_laws.rs` 948, `lyc.rs` 432).
