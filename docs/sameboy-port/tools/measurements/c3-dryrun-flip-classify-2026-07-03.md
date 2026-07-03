# #11bi phase 2 — the C3 DRY-RUN FLIP CLASSIFY (2026-07-03)

Checklist step-3 dry run executed at census 0 (post-#11bi exit table,
worktree `phase-b-s7` @ `9fe3ddf`). LOCAL temp flip: `GameBoy::new` →
`new_inner(model, rom, true)` (the `new_with_reclock` semantics — C0 DIV +4
at construction). Flip REVERTED after measurement; NOT committed.

## Battery results (defaults flipped)

| suite | result |
|---|---|
| mooneye FULL matrix (both models) | **91/91 PASS** |
| blargg / acid / mooneye2022 / same_suite(dmg+breakpoint-new-fails) / smallsuites / oam/apu/misc | **PASS** (229/237 test fns green) |
| gambatte_matrix | 276 new-fail / **332 now-pass** (vs ratchet) |
| golden_fingerprint | 985 drifted (C4 regen+review item) |
| gbmicrotest (DMG) | **68 new-fail** / 8 now-pass |
| mealybug | **20 new-fail** / 3 now-pass |
| wilbertpol | **10 new-fail** / 44 now-pass |
| age | **3 new-fail** / 3 now-pass |
| same_suite | 0 new-fail / 1 now-pass (`ei_delay_halt` [Cgb] — tighten) |

## Gambatte: the 276 new-fail legs, classified

- **CGB-OCR 37** — identical (set-equal) to the probe two-bin flip-BUGs
  (ON-fail ∧ OFF-pass on the 3422-row list; ON 291 / OFF 486 / shared floor
  254 / flip-FIXES 232). `classify_cgb_regr.py`: **37/37 SameBoy-FAIL →
  rebaseline-OK. ZERO forbidden CGB-OCR drops — the census-0 bar HOLDS at
  the dry-run level.** Lists: worktree `scratchpad/flipregr_cgb_ocr.txt`
  (+ `flip_buglist_11bi.txt`/`flip_floorlist_11bi.txt` = the full-291
  classification: 152 SameBoy-pass / 139 fail — the 152 minus the 37 are
  the STANDING OFF-floor debt, not flip regressions).
- **DMG-OCR 44** — classified with a `--dmg` variant of the classifier
  (built this session: `/tmp/s7_dmg/classify_dmg.py` pattern — OCR x-shift
  0/1 trial + `dmg08_out(…)(?:_cgb|\.gb)` want-regex):
  **37 SameBoy-PASS = FORBIDDEN drops** (`scratchpad/flip_dmgocr_buglist
  .txt`) + 7 rebaseline (`flip_dmgocr_floorlist.txt`). Family split of the
  37: **window 29** · sprites 2 · tima/serial/miscmstatirq/m2enable/
  lycEnable/enable_display 1 each. The window 29 = the tier2 window
  length/shadow laws are `model.is_cgb()`-gated — the DMG port of the
  EXISTING laws is the single biggest flip lever left.
- **Pixel-reference 195** (CGB 44 + DMG 151: bgen/bgtiledata/bgtilemap/
  scy/scx_during_m3/window-pixel…) — NOT classifiable by the OCR tool;
  needs the SameBoy-frame-vs-reference-PNG comparison (palette-tolerant)
  or the C4 golden review. Listed in `scratchpad/flip_gambatte_newfail
  .txt`; the now-pass 332 in `flip_gambatte_nowpass.txt`.

## Non-gambatte new-fails (101 legs — block C3 outright per checklist)

- **gbmicrotest 68** [all Dmg]: `hblank_int_scx0-7_if_a-d` +
  `hblank_int_scx*_nops_*` — the DMG mode-0 IF engine vs the cc+0 read
  frame (the #11e "engine-dispatch core", never drained for DMG).
- **mealybug 20**: `m3_bgp_change[_sprites]`, `m3_lcdc_*_change*` (DMG+CGB
  pixel) — mid-scanline register-change rendering.
- **wilbertpol 10**: `ly_lyc_153_write-C/-GS` (6 legs, all models) +
  `timer_if` (4 legs DMG-class).
- **age 3**: `halt-m0-interrupt` [Dmg], `m3-bg-bgp` [Cgb] px,
  `m3-bg-lcdc` [Dmg] px.

Lists: `scratchpad/flip_{age,gbmicrotest,mealybug,same_suite,wilbertpol}_
{newfail,nowpass}.txt` + `scratchpad/flip_5suites.log`.

## Pre-seeded joiner cross-check

`speedchange2_nop_m2int_m3stat_scx1_1` + `ly0_m0irq_scx0_ds_2` fail ON
**and** OFF (fresh lists) — they are ALREADY-FLOORED shared-floor rows, not
flip joiners; no rebaseline action at flip time. The #11bg 8 floor-losses /
#11am 51-row set are superseded by this dry run's fresh full classification.

## Rebaseline-block DRAFT (`tests/gbtr/baselines/gambatte.txt`)

At the real flip, add ONE dated swap block per the header rules:

```
# 2026-07-XX C3-FLIP swap (dry-run classified 2026-07-03, #11bi):
# +332 gambatte legs removed from the floor (flip-FIXES; probe ON list
#  on_11bi_n.txt is the new gambatte floor content for the CGB-OCR rows)
# −37 CGB-OCR joiners, classify 37/37 SameBoy-FAIL (floor-class letters:
#  speedchange/lcd_offset read-frame + window/cgbpal render classes —
#  assign per row from flipregr_cgb_ocr.txt at flip time)
# −7 DMG-OCR joiners (flip_dmgocr_floorlist.txt, sb!=want verdicts inline)
# PIXEL rows: classify-or-floor at flip via golden review (195 legs listed)
```

## VERDICT

The CGB-OCR flip bar (census 0, zero SameBoy-pass drops) **HOLDS**. The
flip is still blocked by, in lever order:

1. **DMG window law port (29 rows)** — extend the shipped CGB
   `vis_exit_hd` window arms to DMG (guards + constants re-measured on the
   DMG frame).
2. **gbmicrotest hblank_int DMG (68)** — the DMG mode-0 IF delivery on the
   cc+0 frame (S5 engine-dispatch, DMG leg).
3. **wilbertpol `ly_lyc_153_write`/`timer_if` (10) + age halt (1)** —
   engine/FF45-write + timer-IF DMG legs.
4. **mealybug/age m3_* pixel (22)** — mid-scanline render (the atomic
   render reclock's DMG face).
5. Remaining DMG-OCR singles (8) + pixel-row classification debt (195) +
   golden 985 regen/review (C4).

mooneye + blargg + acid + same_suite + smallsuites are flip-CLEAN.
