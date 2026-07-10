# EAGER DMG m3-render re-host — 9 mealybug DMG rows recovered; SCX refuted; CGB residual = Part-B (2026-07-09, #11cm)

Task (direct continuation of #11ck's render-strobe): the tier2 DMG mode-3 RENDER
laws (`tier2_dmg_m3_render_*`) fire under `eager_value` too (they were widened to
`|| eager_value` earlier), yet a dry-run default-flip REGRESSED the mealybug/age
DMG render suites — the `#11ci` DMG re-host only covered the gambatte DMG-OCR
rowlist, never the pixel suites. Re-host the render laws so they land on the
eager clock.

## Result — 2 clean flag-gated slices shipped; 9 DMG pixel rows recovered

| slice | commit | DMG pixel rows | EV DMG two-bin |
|---|---|---|---:|
| baseline (flip, pre-slice) | — | 0 (11 regressed) | 102 |
| 1. palette/SCY render-frame debt | `4a969ca` | +5 (m3_bgp_change / _sprites / m3_obp0_change / m3_window_timing / _wx_0) | 102 |
| 2. WX render-frame debt (+ SCX refuted) | `aee5e09` | +4 (m3_wx_4/5/6_change + _sprites) | **96** |

EV CGB **365** and golden **byte-identical** across both slices (all changes
`eager_value`-gated + `!is_cgb()`; production/tier2 untouched). tier2 CGB
**291**. mooneye **92/92** flag-off. clippy `-D warnings` clean, all `.rs` < 1000.

## The mechanism — the eager write stage starts one M-cycle EARLIER than tier2

The tier2 render laws widened to `|| eager_value` were INERT for DMG because the
DMG debt was 0. Root cause of the frame miss:

- **tier2** stages the mid-mode-3 write in `interconnect/cycle.rs::write_deferred`,
  which runs `advance_machine_t` (cc+0 → cc+4) FIRST, so the stage starts at the
  **cc+4 leading edge**.
- **eager** stages in `interconnect::Bus::write` BEFORE `tick_machine`, so the
  stage starts at the **cc+0 leading edge** — ~4 dots (8hd SS / 4hd DS) earlier.

`stage_write` already adds the CGB render-frame debt (+8hd SS / +4hd DS, #11ck)
to close exactly this gap, but only for `is_cgb()`. The fix: give the DMG render
registers the same debt, per-register-scoped in `Ppu::stage_write` (`regs.rs`,
inside `if self.eager_value`). Render-only — the debt shifts only the pixel-view
`eff` commit; the FF41 mode-3-length OCR reads sample ARCH state (`self.scy`),
and WX's un-catch read law records `wx_write_dot` at cc+0 in `Ppu::write` (the
#11bq split), so EV DMG does not regress.

### Per-register debt (DMG SS)

| reg | render stage (`stage_write_dots`) | ×2 grid | debt | absolute | rows |
|---|---|---:|---:|---:|---|
| FF42 SCY / FF47-49 palette | `2 + parity` ≈ 2 | 4hd | **8hd** | ~12hd | m3_bgp_change/_sprites, m3_obp0_change, m3_window_timing/_wx_0 (all BGP tests) |
| FF4B WX | 0 (+1) | 2hd | **12hd** | ~14hd | m3_wx_4/5/6_change + _sprites |
| FF40 LCDC | — | — | **0** | — | (zero-debt; a debt breaks `late_enable_afterVblank`, #11ck) |
| FF43 SCX | 3 | 6hd | **0 (REFUTED)** | — | m3_scx_low/high (see below) |

WX debt swept: 12hd lands all 4 rows, 10hd lands 2, ≤8hd lands 0. The debt only
moves the render view because `wx_write_dot` (the un-catch length read law) is
recorded at cc+0 in `Ppu::write`, not `commit_eff` — the tier2 #11bq split
carries onto eager unchanged. Measured EV DMG 102 → 96 (+6/−0, verified strict
subset of the pre-slice fail set: the 6 fixed OCR rows are `late_wx_*`).

## REFUTED — SCX (FF43): `eff.scx` IS the mode-3 length, no clean split

Adding the debt to FF43 recovers both mealybug SCX pixel rows (m3_scx_low_3_bits
+ m3_scx_high_5_bits, swept optimum 6hd) and improves the EV-DMG count, **but it
is a forbidden A/B sibling swap on the OCR set**:

| config | scx pixels | EV DMG | OCR rows fixed | OCR rows BROKEN (SameBoy-PASS) |
|---|---|---:|---|---|
| FF43 debt=6 | 2/2 | 95 | `late_scx4_2`, `scx_m3_extend_1`, `late_scx_late_disable_0`, +6 wx | `late_scx_late_disable_**1**`, `ly0_late_scx7_m3stat_scx0_2` |

Both broken rows **pass in production (OFF)** → SameBoy-pass → real flip-BUGs.
`late_scx_late_disable_0` (fixed) and `_1` (broken) are a SIBLING PAIR — the
classic one-sided A/B swap the floor-class discipline forbids.

**The render/read split does NOT save it.** I built the #11bq WX split for SCX
(record `scx_write_dot` at cc+0 in `Ppu::write`, defer only `eff.scx`): the SAME
2 rows still broke. Root cause: unlike WX (whose read law is a discrete
`wx_write_dot` latch), SCX's render view `eff.scx` fine-scroll discard **IS** the
mode-3 length — a longer/shorter discard shifts when mode 3 ends, which the FF41
mode-3-length reads observe directly. There is no dot at which `eff.scx` can
carry the render debt without shifting the length verdict. Refuted; DMG SCX stays
zero-debt (reverted, tree carries only palette+WX).

## Out of scope — the CGB residual is Part-B (read/accessibility frame), not a render law

The flip also regresses 5 CGB render rows:
`m3_bgp_change` [Cgb], `m3_window_timing` [Cgb], `m3_window_timing_wx_0` [Cgb],
`m3_wx_4_change_sprites` [Cgb], and `age-test-roms/m3-bg-bgp` [Cgb] (108px).

**These pass under BOTH production AND the tier2 reclock** (measured:
`pixel_probe[ON] 4/4`, `pixel_probe[OFF] 4/4`), failing ONLY under eager
(`pixel_probe[EV] 0/4`). Since tier2 carries the same CGB render debt as eager
but passes, the eager-specific break is the eager READ/ACCESSIBILITY machine
(CGB palette-RAM FF69 accessibility + window activation), not a render commit —
exactly #11ck's Part-B ("the eager cc+4 access vs the cc+0 grid, distinct from
the render commit, not solvable by the write-strobe") and the #11ck residual
"window / sprites / m2int / ly0 / m1" bucket. That is another agent's lane
(`interconnect/cycle.rs` read path, `ppu/blocking.rs`, `ppu/stat_irq`), not the
DMG render laws. Left for the Part-B read-frame session.

`age m3-bg-lcdc-ds` [Cgb] + siblings (a named target) RECOVERED under the flip —
the CGB LCDC render laws already fire on eager (`|| eager_value` + the CGB debt).

## Tooling

- Added `SLOPGB_PROBE_EV` (eager) mode to the pixel two-bin
  (`gambatte_pixel_probe.rs`, `PixelMode` enum + `harness::boot_eager`) — the
  render analogue of the `flagon_probe` EV two-bin. No default-flip needed to
  measure eager pixels.
- Pins: `eager_dmg_m3_render_palette_passes` (5 legs) +
  `eager_dmg_m3_render_wx_passes` (4 legs), via `assert_pixel_leg_eager`
  (`gambatte.rs`). Persistent gates — the default gbtr suite runs production only,
  so without these the eager render recovery had no committed gate.

## Reproduction

```
git checkout eager-dmg-render-rehost
CARGO_TARGET_DIR=target/agM cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agM/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# eager pixel two-bin (no flip needed):
printf 'mealybug-tearoom-tests/ppu/m3_bgp_change.gb [Dmg]\nmealybug-tearoom-tests/ppu/m3_wx_5_change.gb [Dmg]\n' > /tmp/rows.txt
SLOPGB_ROWLIST=/tmp/rows.txt SLOPGB_PROBE_EV=1 SLOPGB_REQUIRE_ROMS=1 \
  $BIN --ignored gambatte::pixel_probe::pixel_probe --nocapture | grep pass=   # 2/2
# EV two-bins:
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 96
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 365
```

## Gate state (both slices SHIPPED)

golden_fingerprint PASS (byte-identical, `eager_value`-gated); tier2 CGB two-bin
**291**; EV CGB **365**; EV DMG **96** (from 102, strict subset); mooneye
`acceptance_ppu` + full 92/92 flag-off; clippy `-D warnings` clean; all `.rs`
< 1000 (regs.rs 870). Render pins: `eager_dmg_m3_render_palette`/`_wx` +
all `tier2_dmg_m3_render_*` green (12 total).

## What remains (eager DMG render)

- **DMG SCX** (2 mealybug rows) — refuted (length-coupled, above). Needs the C3
  flip's coherent length reclock, not a render debt.
- **CGB residual** (5 rows incl. age m3-bg-bgp) — Part-B read/accessibility
  frame, another lane.
