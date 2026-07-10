# EAGER L2 — the DMG window / late_wy re-host: the write-latch pair SHIPPED, EV DMG 83 → 78, TRUE bar 39 → 34 (2026-07-10, #11dc)

Task (L2 from the #11cs bar composition): re-host the DMG window / `late_wy`
render-length laws onto the eager clock (`|| eager_value`), the DMG twin of the
#11ck CGB slice-2 cross-line WY latch which was CGB-scoped. The DMG `window`
family is 23 of the ~38 DMG TRUE-bar rows; 12 of those are `arg/late_wy_*`.

Base: `finish-port-halfdot @ dd40980` (#11db). Reproduced the baseline before
touching a line: **EV DMG 83, EV CGB 344, tier2 CGB 291** (all exact).

## Result — ONE clean sub-family SHIPPED (`4204b2b`), EV DMG 83 → 78 (−5)

| sub-family | shipped? | EV DMG | rows | note |
|---|---|---:|---:|---|
| `late_wy` write-latch (`_2` pairs) | **YES** (`4204b2b`) | 83 → 78 | +5 | write-side latch, read-frame-paired |
| `late_wy_*_1` | no — mapped | — | 7 | render-recorder frame (refuted DMG debt) |
| `late_disable_*_2` / `late_reenable_*_2` / `late_scx_late_disable_0` / `late_scx4_2` | no — mapped | — | ~6 | render-recorder frame (refuted DMG debt) |
| `m2int_wxA5/6_m0irq(2)_2` (want 2) | no — mapped | — | 3 | entangled read-frame, no clean discriminator |
| `m2int_wxA6_firstline_m3stat_2` (want 0) | no — mapped | — | 1 | render/read-frame |
| `m2int_wxA6_{oam,vram}busyread_2` (want 5) | no — floor | — | 2 | accessibility (Part B); tier2 ALSO fails |

**DMG TRUE flip bar 39 → 34** (`classify_dmg.py` on OFF-pass ∩ EV-fail: BUG
39→34, FLOOR 9 unchanged, UNK 0). All 5 gains verified SameBoy-PASS (in the
before-BUG set). CGB byte-identical (EV 344, tier2 291 both unchanged).

## What SHIPPED — the two DMG write-side WY latches, `|| self.eager_value`

The DMG read-frame WY laws (`vis_exit_hd` arms D1/D6/7 in `read_laws.rs`) are
model-agnostic and already run under `eager_value` (`vis_mode_read` gates the
whole web on `tier2_reclock || eager_value`). The gap was purely the two
DMG-scoped **write-side** latches in `regs.rs::write` (FF4A), still
`tier2_reclock`-only:

1. **Block 2 — the HEAD-write cross-line EXTEND** (`regs.rs:~598`). A head write
   (`dot < 4`) matching the just-finished line (`value + 1 == line`), window
   enabled, sets `wy_xline_trig` → feeds read-law arm 7 (`263 + SCX&7` polled
   extend). Recovers `late_wy_10to0_ly1_2`, `FFto0_ly2_2`, `FFto1_ly2_2`
   (commit ly1/ly2 dot0, want out3).
2. **Block 4 — the SS trigger-line UN-latch** (`regs.rs:~665`). A WY write that
   matched then flips away by dot 4 (`old_wy == ly && value != ly`) clears
   `wy_trig_sb_raw` → lets read-law arm D6 fire the bare exit. Recovers
   `late_wy_1toFF_2`, `2toFF_2` (FF at dot 4, want out0).

Both are `!self.model.is_cgb()`-scoped (CGB byte-identical) and fire only when
`tier2_reclock || eager_value` (production byte-identical). Pin
`eager_dmg_late_wy_passes` (window.rs), red-before-green verified (stash the
regs.rs change → 5 legs FAIL).

## The 5 gains (all SameBoy-PASS TRUE-bar rows)

```
late_wy_10to0_ly1_2   want3  (extend, block 2)
late_wy_FFto0_ly2_2   want3  (extend, block 2)
late_wy_FFto1_ly2_2   want3  (extend, block 2)
late_wy_1toFF_2       want0  (un-trigger, block 4)
late_wy_2toFF_2       want0  (un-trigger, block 4)
```

Broken-row set: **empty** (0 regressions on either model, both keylists diffed
before/after; CGB EV byte-identical).

## What stayed MAPPED, and why

### `late_wy_*_1` (7 rows) + `late_disable_*_2` / `late_reenable_*` / `late_scx_late_disable` / `late_scx4_2` (~6) — the refuted DMG render frame

tier2 DMG PASSES all the `_1` variants; EV DMG fails them. Root cause: these
depend on the render recorders `wx_match_dot` / `win_active` (arm 7),
`win_predraw_abort_dot` (arm D3), `win_reenable_dot` (arm D5) — which under the
eager whole-dot commit sit at the **cc+0 render dot**, not the tier2 **deferred
frame** the arm constants are calibrated to. Adding the read-debt to the DMG
render commit is the **DMG write-commit debt REFUTED at #11ck** (best +2hd but
breaks 5 SameBoy-PASS rows: `late_enable_afterVblank_2/4`,
`enable_display/ly0_late_scx7`). The DMG window render-length laws are
calibrated one fetch-step ahead of CGB (#11bp/#11bq); the render frame is a
separate DMG calibration that needs the atomic Part A-render, not a write-latch
gate flip. Confirmed the `_1` route fires block 1 (already eager) but the render
never sets `wx_match_dot` under the eager commit → arm 7 stays inert.

`late_wy_1` (arg + plain, want 0) is a **tier2 DMG floor** (tier2 also fails it
`got=3`) — not recoverable at all.

### `m2int_wxA5/6_m0irq(2)_2` (want 2, 3 rows) — entangled read-frame, no clean discriminator

tier2 DMG passes these (the deferred read advances +4 into the next line's OAM,
reads mode 2); EV DMG reads 0. This is a line-start mode-2 back-date, but the
existing DMG arm (`read_laws:221`) already covers `dot < 4`. Trace
(`SLOPGB_EAGER` + `SLOPGB_S5DBG`, temp probe on `vis_mode_read`): the verdict
reads are `carr=true roam=false`, indistinguishable from the mode-0 poll rows,
and span the frame (no isolated line-tail read with a next-VISIBLE line — a tail
back-date at line 143 lands on VBlank, want ≠ 2). No read-time discriminator
separates fix from the DMG `m0stat` want-0 boundary → a naive extension is an
A/B trade. Left mapped rather than forced (HARD rule: no SameBoy-PASS drop).

### `m2int_wxA6_{oam,vram}busyread_2` (want 5) — Part B floor

tier2 DMG ALSO fails these (accessibility read of a busy OAM/VRAM at the mode-3
seam) → not a flip-bar regression, the accessibility read/write frame (Part B).
`m2int_wxA6_spxA7_m0irq_2` is OFF-fail floor (production fails too).

## What's left on the DMG window cluster

After L2: the DMG TRUE bar is 34. The window residual (~15 rows) is
- the DMG render-length frame (`late_wy_*_1`, `late_disable_*_2`,
  `late_reenable_*`, `late_scx_late_disable`) — the atomic Part A-render, NOT a
  gate flip (the write-commit debt is a refuted A/B trade);
- the entangled `m2int_wxA` line-start mode-2 reads — need a sub-dot discriminator;
- the `busyread` accessibility rows — Part B.

The clean write-latch vein for the DMG window family is now exhausted; the rest
is render-frame (Part A-render) or read-accessibility (Part B).

## Gates (all green on `4204b2b`)

- `golden_fingerprint` byte-identical (production = flags false).
- EV DMG 83 → 78; EV CGB 344 unchanged; tier2 CGB 291 unchanged.
- mooneye `acceptance_ppu` 92/92 flag-off AND `SLOPGB_MOONEYE_EAGER=1` AND
  `SLOPGB_MOONEYE_RECLOCK=1`.
- eager tripwires (`SLOPGB_EAGER=1`, both models): `intr_2_mode0/mode3/oam_ok/
  0_timing`, `di_timing-GS`, wilbertpol `intr_2_0/intr_2_mode0_sprites/
  intr_1_timing` all `B=03 C=05 D=08 E=0D H=15 L=22`.
- clippy `-D warnings` clean; `regs.rs` 879 / `window.rs` 902 (< 1000); no new
  deps; no `unsafe`. Defaults NOT flipped.
- Pin `eager_dmg_late_wy_passes` red-before-green verified.

## Reproduction

```sh
CARGO_TARGET_DIR=target/agL2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agL2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1 SLOPGB_PROBE_DMG=1 \
  SLOPGB_REQUIRE_ROMS=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 78
# TRUE bar: build OFF-pass ∩ EV-fail rels, then
#   SLOPGB_GBTR_ROOT=$PWD/test-roms/game-boy-test-roms-v7.0 \
#     python3 docs/sameboy-port/tools/classify_dmg.py <rels> <outprefix>   # BUG=34
```
