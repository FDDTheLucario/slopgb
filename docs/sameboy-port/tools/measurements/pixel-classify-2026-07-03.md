# Pixel-reference flip-blocker classification (#11bj Phase 3, 2026-07-03)

Built `tools/classify_pixel.py` and classified the §3b pixel-reference legs
(the rows the dry run left "unclassifiable by the OCR tool"): **123 gambatte
pixel legs + mealybug 20 + age 2**. These split into SameBoy-PASS (genuine
flip blockers — the render-reclock atomic core) vs SameBoy-FAIL (rebaseline)
vs golden-review (color-confounded).

## The classifier

Runs SameBoy `--dmg`/`--cgb` on each ROM, reads its 160×144 BMP, and compares
against the sibling reference PNG (resolver handles gambatte `_dmg08`/`_cgb04c`
+ mealybug `_dmg_blob`/`_cgb_c` naming). **Match metric: luminance-RANK, not
raw RGB** — the calibration finding: SameBoy renders DMG with a yellow-tinted
palette (`[0,0,0],[85,85,0],[170,170,0],[255,255,0]`) and CGB with its own
colour correction, so a raw-RGB compare of even a perfect geometry match
scored thousands of "mismatches". Mapping each pixel to its shade rank
(position in the image's luminance-sorted palette) folds the tint/gamma so
only genuine per-pixel geometry differences survive. Verified bimodal on the
calibration set: true matches → **mm = 0** exactly (DMG luminance is exact);
real differences → coherent regions (e.g. `bgtiledata_spx0A_1` a 128-px column
band cols 16–143). Threshold 0 for SameBoy-PASS; DMG mm ≥ 64 for a confident
rebaseline; DMG 0 < mm < 64 and ALL CGB mm > 0 held for golden review (the CGB
colour-correction confound makes the luminance rank unreliable there).

## Results

| bucket | count | meaning |
|---|---|---|
| **SameBoy-PASS (mm=0)** | **100** | flip blockers — SameBoy matches the gambatte/mealybug reference; slopgb-flip broke them. The RENDER-RECLOCK atomic core. |
| DMG SameBoy-FAIL (mm ≥ 64) | 13 | rebaseline-OK — SameBoy renders differently (grey, luminance-exact). |
| DMG uncertain (0 < mm < 64) | 2 | `scy_during_m3_spx0A_1/_3` (mm 16) — golden review (don't drop). |
| CGB colour-confound (mm > 0) | 8 | golden review — luminance rank unreliable under CGB colour correction. |
| age m3-bg (model-rev PNG) | 2 | golden review — `m3-bg-bgp`/`m3-bg-lcdc`, the class-H fetch-grid residue. |

## The 100 SameBoy-PASS blockers — by mechanism (all RENDER-RECLOCK atomic)

- **scy_during_m3 (27)** — mid-mode-3 SCY change render position.
- **bgtilemap (26)** + **bgtiledata (21)** — the sprite-X (`spx08/09`) BG
  fetch-grid render (spx0A diverges → the 13 rebaseline).
- **mealybug m3_* (13)** — `m3_lcdc_*`/`m3_bgp`/`m3_obp`/`m3_window`/`m3_wx`
  mid-scanline register-change rendering.
- **dmgpalette_during_m3 (6)** + **scx_during_m3 (5)** + bgen + window (2).

**Every one is the mode-3 pixel-render pipeline** — broken by the flip's render
timing (`vis_early`/`early_lead`/the fetch grid), NOT reachable by the
`vis_mode_read` FF41-read laws (those are read-verdict-only, never touch a
pixel). So NONE are "law-shaped" — they are the render reclock's own atomic
core and fix WITH the production render reclock at the flip (a
`line_render_done`/render change that breaks byte-identical OFF, out of this
session's scope per the goal). NO render code shipped.

## Rebaseline feed

- **13 DMG rebaseline** (`scratchpad/pixel_rebaseline_dmg.txt`): bgtiledata/
  bgtilemap `spx0A` ×8, `scy_during_m3_spx0A_2`, dmgpalette_scx2_1,
  `m3_lcdc_bg_map_change`/`_tile_sel_win_change`/`_win_map_change`.
- **12 golden-review** (2 DMG-uncertain + 8 CGB-colour + 2 age): resolve at
  the C4 golden regen (`scratchpad/pixel_uncertain_dmg.txt` +
  `pixel_fail_cgb.txt` + the age m3-bg legs).
- **100 flip-blockers** stay on the floor until the render reclock lands.

Lists: `scratchpad/pixel_{pass,rebaseline_dmg,uncertain_dmg,fail_cgb}.txt`.
Classifier: `tools/classify_pixel.py` (env-overridable `SLOPGB_GBTR_ROOT`/`SBT`).
