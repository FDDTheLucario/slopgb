# The tier2 mode-3 pixel-render reclock (#11bo, 2026-07-04)

Ported the RENDER half of §3b — the 100 SameBoy-PASS mode-3 pixel-reference
flip-blockers (`pixel-classify-2026-07-03.md`) — as flag-gated, production
byte-identical render slices. **88 of 100 legs shipped in four mechanisms;
12 residuals classified.** This is the different-subsystem lever the read-frame
vein (#11bk/bl/bm) was drained to reach: the pixel fetcher, not the FF41/FF0F
read laws.

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

## Shipped mechanisms (88 legs)

| # | mechanism | reg | offset | legs | pin |
|---|---|---|---|---|---|
| 1 | SCY / palette | FF42, FF47-49 | dots=3 (survive) | dmgpalette 6 + scy 26 = **32** | `tier2_dmg_m3_render_scy_palette_passes` |
| 2 | LCDC BG addressing | FF40 bit3/4/6 | `render_lcdc` +3 | bgtiledata 21 + bgtilemap 26 + m3_lcdc_tile_sel 1 = **48** | `tier2_dmg_m3_render_lcdc_passes` |
| 3 | SCX double-speed | FF43 (DS) | dots=2 (not 3) | scx_during_m3_ds **5** | `tier2_dmg_m3_render_scx_ds_passes` |
| 4 | BG-priority bit | FF40 bit0 (mixer) | `render_lcdc` +3 | m3_lcdc_bg_en ×2 + bgoff_bgon = **3** | `tier2_dmg_m3_render_bg_priority_passes` |

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
  coupling; OBJ-enable bit1 stays eager). +3 (all CGB).

## The 12 residuals (classified, not shipped)

| leg(s) | count | class | why not a render-defer slice |
|---|---|---|---|
| m3_wx_5/6_change, m3_window_timing, m3_window_timing_wx_0 (Dmg), late_wx_ds (Cgb) | 5 | **WX window-trigger / length** | the WX-match dot IS the window activation = the mode-3 length; a swept FF4B defer that fixed the render broke `tier2_window_late_wx_uncatch` (the un-catch law rides the same eager commit) — lands with the render-length port |
| m3_bgp_change, m3_bgp_change_sprites, m3_obp0_change (Dmg) | 3 | **palette OR-quirk render-atomic** | the DMG "old\|new for one dot" quirk column; no single palette-dots value fixes both the gambatte dmgpalette (wants 3) and the mealybug OR-quirk boundary (swept 1-5, co-temporal) |
| m3_lcdc_win_en_change_multiple (Dmg+Cgb) | 2 | **window-enable / length** | bit5 toggled multiple times mid-mode-3 = the window-length model |
| m3_lcdc_obj_en_change (Cgb) | 1 | **OBJ-enable / length** | bit1 gates the sprite fetch → mode-3 length (eager, must not move) |
| scy_during_m3_spx08_2 (Dmg) | 1 | **sprite-penalty grid** | the sprite stall shifts the SCY refetch sample by a penalty-grid dot, not a uniform frame offset |

The WX + window-enable + obj-enable + sprite residuals are the **render-length /
sprite-grid atomic** class the goal expected to land WITH the length port; the
palette OR-quirk needs the finer one-dot-blend render model.

## Gates (every commit)

Pixel two-bin +N / 0 dropped; CGB two-bin 291/291 IDENTICAL SET (base-diff vs
clean HEAD `6990c09` `flagon_probe`); mooneye 91/91 flag-on (`SLOPGB_MOONEYE_RECLOCK`)
AND flag-off; `tier2_boot_div_passes` + all tier2 pins (55 → 59); lib 660; clippy
`-D warnings`; production byte-identical OFF (pixel probe OFF 100/100). Commits
`cef8471` (mech1) · `c26efdf` (mech2) · `380cbcd` (mech3) · mech4.

## §3b after this class

The RENDER half of §3b is ported (88/100). §3b residual = the 12 render-length /
OR-quirk / sprite-grid legs above + the 43-row engine dispatch-atomic core (the
C3 flip's IRQ-dispatch retime). The render legs that stayed are the same
length-coupled class the engine core lands with — one dispatch-retime session from
the flip.
