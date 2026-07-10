# EAGER HALFDOT Part A-render — the write-strobe SHIPPED: EV CGB 400 → 365, the render-length class recovered (2026-07-09, #11ck)

Task (the FINISH KEYSTONE, direct continuation of #11cj): host the tier2
half-dot write STROBE on the eager path so a mid-mode-3 register commit lands
at its true half-dot instead of a whole-dot boundary — the render-length class
(window / scx / cgbpal / sprites) that the whole-dot eager commit floored.

## Result — TWO clean flag-gated slices shipped, EV CGB 400 → 365 (−35)

| slice | commit | EV CGB | mechanism |
|---|---|---:|---|
| baseline (`705c3d7`) | — | 400 | Part A-infra only (half-dot PPU advance, strobe still whole-dot) |
| 1. half-dot write-strobe + CGB render-frame debt | `659829d` | 373 | strobe per half-dot + the +8hd SS / +4hd DS read-debt on the commit |
| 2. eager WY cross-line + DS un-latch | `1cc64db` | 365 | the write-side WY latches fire on the eager clock |

EV DMG **102 unchanged** (the DMG debt is a refuted A/B trade — see below).
tier2 CGB **291** and golden **byte-identical** across both slices (all changes
`eager_value`-gated; production/tier2 untouched). mooneye `acceptance_ppu`
**91/91** flag-off AND `SLOPGB_MOONEYE_EAGER`. clippy `-D warnings` clean, all
`.rs` < 1000 lines.

## The mechanism — the eager commit was in the WRONG frame, not the wrong dot

#11cj's model ("the eager `tick_machine` drains the strobe within the M-cycle →
commit 2-4 dots early") is CONFIRMED but the fix is subtler than a half-dot
nudge:

1. **The half-dot strobe alone is INERT** (grid conversion). `strobe_tick` now
   runs on the non-completing `tick_half` under `eager_value`, and
   `stage_write` doubles `dots_left` (×2) — a run of aligned half-dots still
   commits at the same whole dot. Measured byte-identical (EV 400/102).
2. **The render-dot recorder flip alone is INERT on CGB.** `scx_write_dot`
   (DMG-only in the read laws), `wx_write_dot` (scx5/DMG-only), `staged_pending`,
   `render_lcdc` split, `window_abort_render` → `|| eager_value`: no CGB row
   moved, because the eager commit ALREADY lands at the same dot D+3 as tier2
   (both stage at cc+0, the strobe drains over the write's 4 dots) — the
   recorders were just never set.
3. **The lever is the READ-DEBT on the write commit.** The FF41/accessibility
   reads observe the mode-3 length in the **cc+4 frame** (`read_pos_hd`'s +8hd SS
   / +4hd DS), but the eager write commits the render latches
   (`wx_match_dot`/`win_predraw_abort_dot`/…) at the **cc+0 render dot**. Adding
   the read-debt to the staged commit (`stage_write`: `dots*2 + debt`, debt = 8hd
   SS / 4hd DS) puts the write commit in the SAME frame the reads sample the
   length in — so the render-length `_1`/`_2` pairs separate on the eager clock.
   With the debt, `dots_left` exceeds the write's own `tick_machine` half-dots,
   so the stage survives to `write_no_tick` exactly like tier2's leading-edge
   stage → the existing `staged_pending` survive-check holds with no eager
   bypass (DMG debt-0 stays byte-identical: its stage drains in-M-cycle, so the
   M-cycle-END commit still runs).

Global-offset sweep (uniform, pre speed-split): +2→384, +4→381, +8→**374**
(local min), +10→375. The +8hd (=+4 dots = one M-cycle read-debt) is the SS
optimum; DS wants +4hd (its M-cycle is 2 dots) — a uniform +8 broke the
SameBoy-PASS `late_scx4_ds_1`, the speed-split (8 SS / 4 DS) preserves it → 373.

### Slice-1 recovery (29 fixed / 2 broken)
27 of the 29 are SameBoy-PASS blockers; both broken are SameBoy-FAIL floor
(`late_disable_late_scx00_wx10_ds_1`, `late_disable_late_scx03_wx0f_2`, sb=3
want=0). Families: window (`late_wx*`, `late_reenable*`, `late_disable*`,
`late_scx_late_disable*`), sprites (`late_sizechange*`), `scx_m3_extend`,
`m2int_m3stat/late_scx4`, one `enable_display/ly0_late_scx7`.

### Slice-2 recovery (8 fixed / 0 broken)
6 of 8 SameBoy-PASS. The boundary-WY cross-line latch (`wy_xline_trig`) + the
CGB-DS trigger-line un-latch fire on the eager clock (`|| eager_value`). The
eager arch commit lands at the M-cycle END; the write-side WY latches pair with
the read-frame WY laws already ported (#11by). Recovers the whole `late_wy`
render-length class (`late_wy_FFto0/FFto1/10to0/1toFF` + `late_wy_ds`/`_1toFF_ds`
DS pairs). ALL recoverable `late_wy` CGB rows cleared.

## REFUTED / dead-end levers (do not re-chase)

- **The DMG write-commit debt is an A/B trade — NOT shippable.** DMG best is +2hd
  (94, −8) but it breaks **5 SameBoy-PASS rows** (`late_enable_afterVblank_2/4`,
  `enable_display/ly0_late_scx7`, …). Excluding FF40 from the debt kills the
  A-side gain too (late_disable is FF40) → +5/−2 at best, still breaking rows.
  The DMG window/palette render-length laws (arm D1/D3/D6 fetch phase + the
  palette pop-grid) are calibrated one fetch-step ahead of CGB; the write-commit
  frame is a separate DMG calibration. DMG debt = 0 (CGB-scoped) shipped.
- **The cgbpal_m3end accessibility exit is NOT a debt shift.** Back-dating the
  eager `pal_ram_blocked` compare by the read-debt (`self.dot − 4 SS / −2 DS`) is
  WRONG-direction: 0 fixed / **7 broken** (the `_4` variants). The palette
  write/read accessibility at the mode-3 exit is a Part B read/write-FRAME
  problem (the eager FF69 write commits at cc+4 vs the cc+0 `pal_open_dot`
  grid), distinct from the render commit — not solvable by the write-strobe.

## What remains (87 recoverable CGB rows at 365, by family)

The render-LENGTH class is EXHAUSTED (Part A done). The residual is other
subsystems:

| class | rows | owner |
|---|---:|---|
| halt-wake (`halt`, `int_hblank_halt`) | 19 | the halt-wake clock port (unported eager wake clock) |
| dispatch-coupled (`enable_display`, `lycEnable`, `m2int_m0irq`, `m0enable`, `irq_precedence`, `lcd_offset`) | ~30 | the counter-pinned C3-flip floor (lands WITH the flip, needs the dispatch move — forbidden here) |
| accessibility exits (`cgbpal_m3`, `vram_m3`, `oam_access`, `vramw_m3end`) | ~15 | Part B read/write-frame (the eager cc+4 access vs the cc+0 grid) |
| residual window / sprites / m2int / ly0 / m1 | ~10 | mixed (off-screen window `m2int_wxA5`, per-row) |

## The precise next lever

1. **Part B accessibility frame** (`cgbpal_m3`/`vram_m3`/`oam_access` ≈ 13
   blockers): the eager FF69/VRAM/OAM read+write sample at cc+4 while the
   `pal_open_dot`/mode-3 blocking grid is cc+0. NOT a uniform debt (measured
   wrong-direction). Needs the per-access straddle law the tier2
   `interconnect/memory.rs::stamp_blocks` cc+4 edge carries, re-hosted on the
   eager whole-M-cycle commit.
2. **Halt-wake clock port** (19 blockers): the eager analogue of the tier2
   `stat_vis_from_t` / `m0_halt_hold` wake grid — a separate clock, per #11cb's
   "next: the halt-wake clock port".
3. The dispatch-coupled ~30 are the C3-flip floor (land with the −2 dispatch
   move itself, which eager forbids by design — count-safety).

## Reproduction

```
git checkout halfdot-render   # at 1cc64db
CARGO_TARGET_DIR=target/agR cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agR/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 SLOPGB_REQUIRE_ROMS=1 \
  $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 365
# the write-commit sweep knob (port_probe): SLOPGB_WCOMMIT adds hd to the CGB debt.
```

## Gate state (both slices SHIPPED + pushed)

- EV CGB **365** (−35 from 400) / EV DMG **102** (byte-identical — DMG debt
  refuted). tier2 CGB **291**. golden_fingerprint PASS. mooneye acceptance_ppu
  91/91 flag-off + `SLOPGB_MOONEYE_EAGER`. clippy `-D warnings` clean. All
  `.rs` < 1000 lines.
- Flip bar: CGB SameBoy-PASS blockers **253 → 220** (−33: 27 write-strobe +
  6 WY), 0 dropped (classified over the full EV fail list vs SameBoy 1.0.2 —
  365 EV fails = 220 blockers + 145 floor). DMG unchanged (byte-identical EV).
