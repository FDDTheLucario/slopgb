# The tier2 mode-3 pixel-render reclock (#11bo, 2026-07-04)

Ported the RENDER half of §3b — the 100 SameBoy-PASS mode-3 pixel-reference
flip-blockers (`pixel-classify-2026-07-03.md`) — as flag-gated, production
byte-identical render slices. **ALL 100 legs shipped; pixel two-bin 100/100.**
(89 shipped #11bo whole-dot; +5 #11bp the DMG palette half-dot commit pop-grid;
+6 #11bq the SCY parity + WX render-view defer/split + window-abort split — the
top update.) This is the different-subsystem lever the read-frame vein
(#11bk/bl/bm) was drained to reach: the pixel fetcher, not the FF41/FF0F read
laws. The #11bp/#11bq legs the #11bo classification called "half-dot precision"
all landed WITHOUT a half-dot FSM — recovered by a whole-dot PARITY term, a
render-view register survive-defer, or an eager-flag/deferred-render SPLIT.

## #11bq update (2026-07-04) — the last 6 residuals SHIPPED (94→100)

The 6 legs #11bp parked as "need the genuine WX/window-length/sprite half-dot
render FSM" all landed as flag-gated, production byte-identical slices — the same
disciplined whole-dot/parity/split levers as #11bp, three mechanisms
(`phase-b-s7` `09a9f5e` + `d3d7d40`):

**1. SCY parity (+1: `scy_during_m3_spx08_2` Dmg).** Same EVEN-dot parity anchor
as the #11bp palette: `dots = 2 + (leading_edge & 1)` for FF42 SS
(`cycle.rs::write_deferred`). The sprite prefill stall (X=8 OBJ, ~11-dot fetch)
shifts the BG fetch grid so tile fx=17's Lo data read (`bg_tile_addr`, fine row
= LY+SCY & 7) lands EXACTLY on the deferred SCY→0 commit dot. Dual-traced:
OFF/production commits the write at the M-cycle mid-point (dot 238, LE=236 even →
+2), so the data read at dot 239 re-samples the NEW scroll (fine=1) while the
already-latched tile NUMBER keeps the old (the mealybug m3_scy_change mixed-fetch
behaviour); the flat defer=3 committed at dot 239 → the read saw the OLD scroll
(fine=0), one column late. The objectless `scy_during_m3_{1,4,5,6}` writes land
ODD leading edges (want +3, held — a flat +2 dropped all 8). Render-only.

**2. WX render-view defer + un-catch SPLIT (+3: `late_wx_ds_1` Cgb, `m3_wx_5`,
`m3_wx_6` Dmg).** In tier2 `eff.wx` committed eagerly at cc+0, 2-4 dots early of
the render's per-dot WX comparator. Fix: `eff.wx` now SURVIVES the arch write
(`regs.rs` `staged_pending` += FF4B) and strobe-commits at leading+2 (== the
production render frame; `cycle.rs` FF4B → dots 0, +1 for the FF4B palette-class
offset → strobe logs leading+1, visible to `render_step` from leading+2 because
`strobe_tick` runs at tick-start BEFORE `dot += 1`). Dual-traced: `late_wx_ds`
(DS) — the eager cc+0 WX=255 committed at dot 96 pre-empted the wx=7 activation at
dot 97 → the window never drew (bare cols 0-7); deferring to dot 98 (visible from
99) lets the wx=7 activation catch. `m3_wx_6` (SS) — the un-catch straddle: a WX
6→5 rewrite must split the `pos_dot==wx+6` compares (wx=6 at pos_dot 11 no-match /
wx=5 at pos_dot 12 no-match), which needs the change visible to `render_step` at
dot 96 (== leading+2). The SPLIT keeps the un-catch READ law's `wx_write_dot`
(FF41 mode-3 length) at its cc+0 leading edge (moved to `regs.rs::Ppu::write`
FF4B, not `commit_eff`) → `tier2_window_late_wx_uncatch` unperturbed.

**3. Window-abort render/read-law SPLIT (+2: `m3_lcdc_win_en_change_multiple`
Dmg+Cgb).** A mid-mode-3 LCDC.5 clear ended the drawn window at the eager cc+0
(dot 148), 2 dots / 2 pixels early of production (dot 150); the abort lands at
lx≈51 → cols 50-51 showed BG instead of window. Split `window_abort`:
`window_abort_flags` (`win_predraw_abort` / DMG `win_aborted`, the FF41 read-law
inputs calibrated to cc+0) stays EAGER in `commit_eff`; `window_abort_render` (the
drawn-window end + BG-fetch tile-boundary re-anchor) fires at the `render_lcdc`
bit5 1→0 catch-up (`ppu/mod.rs`), the deferred fetch view mech2 already carries.
**The activation gate + `win_reenable_dot`/`win_enable_dot` stay eager — a
render-view activation defer (`win_en_now` via `render_lcdc`) was BUILT + REFUTED,
dropping `late_enable_ly0_ds_2` / `late_reenable_scx2_2` (SameBoy-passes: the
activation dot IS the mode-3 length). ONLY the drawn-window END was separable.**

Gates (both commits): pixel two-bin ON 94→98→100 (+6 / 0 dropped), OFF 100/100
byte-identical; CGB two-bin **291/291 IDENTICAL SET** (0 new / 0 fixed vs clean
HEAD, twice); mooneye 91/91 flag-on AND flag-off; 63 tier2 pins
(+`tier2_dmg_m3_render_scy_spx08`, `_wx`, `_win_abort`); lib 660; clippy clean;
gbtr OFF 0 failed. **§3b's RENDER half is COMPLETE (100/100).** The residual §3b
work is the engine dispatch-atomic core (the C3 flip's IRQ-dispatch retime), not
the pixel fetcher.

## #11bp update (2026-07-04) — the DMG palette half-dot pop-grid SHIPPED (+5)

## #11bp update (2026-07-04) — the DMG palette half-dot pop-grid SHIPPED (+5)

The 5 palette-timing legs #11bo classified as needing "half-dot precision"
(`m3_bgp_change`, `m3_bgp_change_sprites`, `m3_obp0_change`, `m3_window_timing`,
`m3_window_timing_wx_0`) SHIPPED without any actual half-dot FSM — the half-dot
resolution is recovered by a **whole-dot parity term** on the commit defer.

Dual-traced (slopgb OFF/ON pop+strobe+stage tracers vs SameBoy SBPOP/SBWPAL,
`build_sameboy_tracers.sh` + a temporary `render_pixel_if_possible` pop tracer):
- SameBoy commits the palette at the write M-cycle's exact fp and pops per dot;
  for `m3_window_timing` ly0 the BGP=ff write lands fp=335479436 == the lcdx=3
  pop's fp, so lcdx=3 reads the NEW value → boundary at column 3 (== slopgb OFF).
- slopgb OFF (defer=2) reproduces that boundary; the flip's mech1 defer=3
  rendered it one column late (lcdx=4) — EVERY mealybug BGP/OBP boundary shifted
  +1 (m3_bgp_change all 6 transitions +1). `dmgpalette_during_m3` (defer=3) is
  correct and MUST stay.
- The discriminator: **the write's leading-edge dot parity.** All the mealybug
  BGP/OBP writes land EVEN leading edges (m3_window_timing LE=104,
  m3_bgp_change 80/96/108/168/180/240/252), the gambatte dmgpalette writes ODD
  (LE=183). Single speed is whole-dot aligned so SameBoy's commit sits on an
  EVEN (CPU-M-cycle) dot, visible +2 from the pop; an ODD leading edge means the
  M-cycle boundary rounds up one dot → visible +3 (round_up_even(LE)+2). So
  `dots = 2 + (leading_edge & 1)` (was fixed 3) — EVEN→+2 (mealybug), ODD→+3
  (dmgpalette held).

Mechanism (`interconnect/cycle.rs::write_deferred`, the FF47-49 dots calc):
`2 + (self.ppu.scan_pos().1 & 1)`, gated tier2 + `!is_cgb` + `!glitch_active`.
DMG only (CGB palettes are FF68-6B, no FF47-49 render path, no BGP OR-quirk —
keeps the plain +3). Render-only (pure colour selection, no mode-3-length or
FF41-read-law coupling). Gates: pixel two-bin ON 89→94 (+5 / 0 dropped), OFF
100/100 byte-identical; CGB two-bin 291/291 IDENTICAL SET (0 new / 0 fixed);
mooneye 91/91 ON+OFF; 60 tier2 pins; lib 660; clippy clean; gbtr OFF 0 failed.
Pin `tier2_dmg_m3_render_palette_halfdot_passes` (phase-b-s7 `f45ab02`).

The 6 remaining residuals (WX reactivation `m3_wx_5/6_change` + `late_wx_ds`,
window-enable `m3_lcdc_win_en_change_multiple` ×2, sprite-penalty grid
`scy_during_m3_spx08_2`) are NOT palette — they need the WX/window-length or
sprite-penalty half-dot grid, a genuine half-dot render FSM (the C3 flip's own
work), not a parity term.

## The bug (why the flip renders mid-mode-3 register changes at the wrong column)

Production (flag-off) renders all 100 legs correctly. The flip breaks them. Root
cause: the tier2 **deferred write path** (`write_deferred`, `interconnect/cycle.rs`)
advances the PPU/render to the write's **leading edge (cc+0)** *before* the write
commits, then the eager `Ppu::write → commit_eff` lands the new register value
into the pixel-view `eff` right there — 4 dots **early** of the render's
cc+4-calibrated fetch grid. So a mid-mode-3 SCY/SCX/BGP/OBP/LCDC change reached
the fetcher/mixer at the wrong dot, shifting the rendered boundary column.

Production commits the same register via `stage_write` + `strobe_tick` *during*
the write M-cycle's machine tick (mid-cycle), so the render sees it at the right
dot. SCX already carried a `dots=3` survive-and-defer (the `late_scx4` FF41 read
law, #11bb). The fix generalises that per register class.

**Separability (why this is a flag-gated slice, not the atomic flip):** the FF41
read laws sample the ARCH registers (`self.scy`/`self.scx`/`self.lcdc`), while the
render samples the pipeline view `eff`. Adjusting the `eff` render-commit dot is
render-only — it never touches a read verdict, the mode-3 length, or the IRQ
dispatch. Every mechanism two-binned CGB 291/291 IDENTICAL SET (zero-drift vs
clean HEAD `6990c09`), mooneye 91/91 flag-on AND flag-off, production
byte-identical OFF.

## Tooling — the pixel two-bin

`crates/slopgb-core/tests/gbtr/gambatte_pixel_probe.rs` (`#[ignore]`,
`SLOPGB_ROWLIST`): boots each pixel leg on the flag-on reclock
(`boot_with_reclock`) and compares the 160×144 framebuffer to the reference PNG
with the suite's own comparator (`harness::expect_frame_png`), so a probe PASS is
a real suite PASS. `SLOPGB_PROBE_OFF=1` for the production baseline. Handles both
gambatte (`_dmg08`/`_cgb04c`) and mealybug (`_dmg_blob`/`_cgb_c`) legs. Baseline:
OFF 100/100, ON 0/100 (the flip-blocker set).

## Shipped mechanisms (89 legs)

| # | mechanism | reg | offset | legs | pin |
|---|---|---|---|---|---|
| 1 | SCY / palette | FF42, FF47-49 | dots=3 (survive) | dmgpalette 6 + scy 26 = **32** | `tier2_dmg_m3_render_scy_palette_passes` |
| 2 | LCDC BG addressing | FF40 bit3/4/6 | `render_lcdc` +3 | bgtiledata 21 + bgtilemap 26 + m3_lcdc_tile_sel 1 = **48** | `tier2_dmg_m3_render_lcdc_passes` |
| 3 | SCX double-speed | FF43 (DS) | dots=2 (not 3) | scx_during_m3_ds **5** | `tier2_dmg_m3_render_scx_ds_passes` |
| 4 | BG-priority bit | FF40 bit0 (mixer) | `render_lcdc` +3 | m3_lcdc_bg_en ×2 + bgoff_bgon = **3** | `tier2_dmg_m3_render_bg_priority_passes` |
| 5 | OBJ-enable draw-side | FF40 bit1 (mixer, CGB) | `render_lcdc` +3 | m3_lcdc_obj_en **1** | (same pin) |

- **Mech 1** — SCY/palette are pure colour/row selection (no length, no read-law
  coupling). Give them SCX's `dots=3` survive-and-defer (`cycle.rs` +
  `regs.rs::staged_pending` skip). Measured: +32 / 0 dropped.
- **Mech 2** — LCDC bit3 (BG map) / bit4 (tile data) drive the BG fetch. A *full*
  LCDC defer regressed 5 window pins (the bit5 abort/reenable laws are calibrated
  to the eager cc+0 control commit — #11bb "LCDC +4 net-negative"), so this
  **splits the view**: `eff.lcdc` still commits eager (window bit5 + FF41 read
  laws + OBJ-enable/length), while a new `eff.render_lcdc` — read only by the
  BG/window tile fetcher (`render/mode0.rs`) — lags `RENDER_LCDC_DELAY`=3 ticks
  via `render_lcdc_pending`. +48.
- **Mech 3** — SCX's defer was dots=3 at both speeds. In double speed the M-cycle
  is 2 PPU dots (vs 4), so the offset halves: dots=2 fixes the 5 `scx_during_m3_ds`
  fine-scroll legs AND holds `late_scx4`'s DS read law (dots=1 broke the read law,
  dots=3 broke the render — dots=2 is the single value that straddles both). +5.
- **Mech 4** — LCDC bit0 (BG/window priority) in the sprite↔BG mixer
  (`render/sprite.rs::output_pixel`) reads `render_lcdc` too (bit0 has no length
  coupling). +3 (all CGB).
- **Mech 5** — LCDC bit1 (OBJ enable) has two effects: it gates the sprite FETCH
  (a stall = length, stays eager in `render.rs`) and the sprite pixel DRAW at the
  mixer (render-only). On CGB the draw-side mixer read takes `render_lcdc` (DMG
  keeps its eager one-dot-ahead mixer calibration). +1 (m3_lcdc_obj_en, CGB).

## The 11 residuals (classified, not shipped) — all HALF-DOT precision

Deep-traced each one (`gambatte_pixel_probe` frame dump + `wpop`/`palwr`/`winmatch`/
`windisc` tracers). Every residual needs SUB-DOT (half-dot) render/write
precision that a whole-dot flag-gated defer cannot provide — exactly the
`HALFDOT-BUILD-PLAN` Part A-render / A-D class. Traced findings:

| leg(s) | count | traced root | why no whole-dot slice fits |
|---|---|---|---|
| ~~m3_bgp_change, m3_bgp_change_sprites, m3_obp0_change, m3_window_timing, m3_window_timing_wx_0~~ **SHIPPED #11bp** | 5 | **palette pop-grid half-dot → parity term** | m3_window_timing is a BGP test, NOT a window test — traced: its window render (activation dot, discard, pops, colour indices) is BYTE-IDENTICAL flag-on/off; only `eff.bgp` at the pop dot differs (OFF `ff` / ON `00` at the col-9 pop). The render's pixel-pop samples the palette at a half-dot SameBoy commits at; dmgpalette wants whole-dot defer 3, the mealybug legs want 2 (swept PALD) — but both write at the SAME phase (cycmod4=3, dhalf=0 aligned in SS), so the difference is the render POP grid being sub-dot, not the write. m3_bgp adds the rapid per-M-cycle "old\|new for one dot" torture (swept ORQ 0-2 doesn't fix it). |
| m3_wx_5/6_change (Dmg), late_wx_ds (Cgb) | 3 | **WX reactivation / length** | the mid-mode-3 WX rewrite's reactivation inserts zero-pixels (`output_pixel(0)`+`advance_lx` = +1 dot each), so the reactivation COUNT = the mode-3 length; a swept FF4B defer that fixed the render dropped `tier2_window_late_wx_uncatch` (the un-catch law rides the same eager commit). |
| m3_lcdc_win_en_change_multiple (Dmg+Cgb) | 2 | **window-enable / length** | bit5 toggled multiple times mid-mode-3 = the window-length model (activation/abort). |
| scy_during_m3_spx08_2 (Dmg) | 1 | **sprite-penalty grid** | the sprite prefill stall shifts the SCY refetch sample by a penalty-grid dot, not a uniform frame offset. |

**The decisive finding (m3_window_timing):** the window render is byte-identical
flag-on/off — the ONLY difference is the palette value the pop grid samples, off
by one whole-dot because the deferred clock commits the palette at a whole-dot
while SameBoy commits it at the write's exact half-dot AND the render pops at a
half-dot. dmgpalette (defer 3) and these (defer 2) can't both be satisfied at a FIXED
whole-dot defer. **RESOLVED #11bp (see the update at the top): the sub-dot
information is the write's leading-edge dot PARITY — `dots = 2 + (LE & 1)`
recovers which side of the even CPU-M-cycle grid the commit sits on, so
dmgpalette (odd LE, +3) and mealybug (even LE, +2) both land without any half-dot
FSM.** The other 6 residuals (WX/window-enable/sprite grid) genuinely need the
coordinated half-dot render reclock (the C3 flip's own work —
`HALFDOT-BUILD-PLAN.md` Part A-render + A/D + C), which breaks byte-identical OFF
and re-derives the read laws, so they are NOT a flag-gated slice. **94/100 is the
new flag-gated ceiling.**

## Gates (every commit)

Pixel two-bin +N / 0 dropped; CGB two-bin 291/291 IDENTICAL SET (base-diff vs
clean HEAD `6990c09` `flagon_probe`); mooneye 91/91 flag-on (`SLOPGB_MOONEYE_RECLOCK`)
AND flag-off; `tier2_boot_div_passes` + all tier2 pins (55 → 59); lib 660; clippy
`-D warnings`; full gbtr OFF 244/0; production byte-identical OFF (pixel probe OFF
100/100). Commits `cef8471` (mech1) · `c26efdf` (mech2) · `380cbcd` (mech3) ·
`e1cd243` (mech4) · `5fe88d5` (cleanup) · `04d4425` (mech5).

## §3b after this class

The RENDER half of §3b is ported (94/100 after #11bp). §3b residual = the 6
render-length / WX / window-enable / sprite-grid legs above + the 43-row engine
dispatch-atomic core (the C3 flip's IRQ-dispatch retime). The render legs that
stayed are the same length-coupled class the engine core lands with — one
dispatch-retime session from the flip.
