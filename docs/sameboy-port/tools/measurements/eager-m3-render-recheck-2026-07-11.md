# The EAGER m3_* render write-commit recheck — 5/7 cracked, DMG SCX refuted (2026-07-11, #11ej)

Task: re-examine the 7 render `m3_*` non-gambatte eager flip-regressions that
#11eg refuted as "genuine render-LENGTH (a read-law can't move a pixel)". That
verdict was true for a READ-law but never tried a RENDER-side arm. Apply
`rom-diff-weld`: the eager clock and tier2 share the SAME whole-dot render code,
so a row that PASSES tier2 but FAILS eager is not the mode-3 length — it is the
eager clock's mid-mode-3 **write-commit position** (`Ppu::stage_write` cc+0 vs
tier2 cc+4), a calibratable per-register debt.

**Result: 5/7 recovered (all CGB palette + WX), zero drops; 2/7 (DMG SCX)
REFUTED as a genuine render/length shuffle with the eager-frame A/B proof. All
hard gates hold.**

## The fork (skill step 1): do the rows PASS tier2?

Ran `mealybug_matrix` + `age_matrix` under OFF / `SLOPGB_GBTR_TIER2=1` /
`SLOPGB_GBTR_EAGER=1`. **All 7 targets PASS tier2, FAIL eager** → the eager
`stage_write` frame, not the render length.

| row | model | eager Δpx | tier2 | register |
|---|---|---|---|---|
| age m3-bg-bgp | Cgb | 108 | PASS | FF47 BGP |
| m3_bgp_change | Cgb | 864 | PASS | FF47 BGP |
| m3_window_timing | Cgb | 138 | PASS | FF4B WX |
| m3_window_timing_wx_0 | Cgb | 144 | PASS | FF4B WX |
| m3_wx_4_change_sprites | Cgb | 11 | PASS | FF4B WX |
| m3_scx_high_5_bits | Dmg | 159 | PASS | FF43 SCX |
| m3_scx_low_3_bits | Dmg | 320 | PASS | FF43 SCX |

The CGB eager write-commit debt (`Ppu::stage_write`) was a uniform `8` for ALL
registers, while DMG already had a per-register split (palette `2+parity`-based,
WX `12`, SCX/LCDC `0`). The CGB rows are DMG ROMs run in **DMG-compatibility
mode** (no CGB flag) — they use the FF47-4B render path — so the uniform 8 lands
the palette/WX change at the wrong pixel column.

## The fix (skill steps 2-4): CGB per-register debt, `eager_value`-scoped

Made the CGB single-speed branch per-register, mirroring DMG:

* **Palette (FF47-49): `6 + 2*(scan_pos().1 & 1)`.** The CGB stage is the flat
  `3` (`stage_write_dots`, no CGB parity term) → `3*2 + debt`. `6 + 2*parity`
  reproduces the DMG palette even/odd-M-cycle anchor (12hd even / 14hd odd).
  **Swept unique-optimal: WCOMMIT ±2 regress both m3_bgp_change + age m3-bg-bgp.**
  Recovers **m3_bgp_change [Cgb]** + **age m3-bg-bgp [Cgb]**.
* **WX (FF4B): `12`** (like the DMG WX arm — smallest render stage, largest
  debt). Recovers **m3_window_timing**, **m3_window_timing_wx_0**,
  **m3_wx_4_change_sprites [Cgb]**. Window comparator has slack (debt 10-16 all
  pass, no drops) → 12 matches DMG.
* SCX/SCY/LCDC keep `8` (`_ => 8`, unchanged).

`eager_value`-scoped (inside `if self.eager_value`) → OFF + tier2 byte-identical
by construction (they take the `else { dots }` branch).

## DMG SCX — genuine render/length shuffle, REFUTED with the eager A/B

`vis_hold_until`/`vis_exit_hd` arm 8 = the EMERGENT bare-line length `2*flip+2`
(`flip_projection`), and eff.scx feeds the fine-scroll discard → the flip. A DMG
SCX render debt shifts eff.scx which shifts the FF41 mode-3-length OCR the
gambatte m3stat rows read. Eager-frame A/B (uniform SCX debt sweep, gambatte
`dmg_rowlist`):

| SCX debt | m3_scx_low | m3_scx_high | gambatte drops |
|---|---|---|---|
| 0 | 320 (fail) | 159 (fail) | — |
| 2 | 320 | 51 | `ly0_late_scx7_m3stat_scx0_2`, `late_scx4_1` |
| 4 | PASS | 41 | + `late_scx_late_disable_0` |

m3_scx_low needs debt≥4, m3_scx_high never clears (best ~41px), but the gambatte
length rows DROP at debt≥2 — **before either target even improves**. The
mealybug write (dot 87/111) and the gambatte write (dot 82, traced) are too
close for a write-dot discriminator, and window/sprite state don't separate them
either (2 of 3 breakers are window-free + sprite-free, same as mealybug). No
clean split exists — eff.scx IS the length. DMG SCX stays zero-debt (confirms +
strengthens the existing code comment's refutation, now with eager-frame data).

## Gates (all hold)

* **golden byte-identical** (twice — the hard gate; render path).
* **EV eager mealybug+age 15 → 10 fails: exactly the 5 CGB rows recovered, ZERO
  new drops.**
* **Gambatte EV unchanged: CGB 287 / DMG 38** (both rowlists, zero drops, zero
  shuffles — the CGB palette/WX debt touches only the pixel view, never an OCR
  verdict; DMG untouched).
* **tier2 gambatte two-bin CGB 291 / DMG 116 unchanged.**
* **mooneye 93/93 × 3** (OFF / `SLOPGB_MOONEYE_EAGER=1` / `SLOPGB_MOONEYE_RECLOCK=1`).
* clippy clean; every touched `.rs` < 1000 (regs.rs 991).
* Red-before-green pins: `mealybug_eager_cgb_m3_writecommit_passes` (4 ROMs) +
  `age_eager_cgb_m3_bg_bgp_writecommit_passes` — both FAIL with the CGB
  per-register debt reverted to uniform 8, PASS with it.

## Lesson

#11eg's "render-LENGTH, a read-law can't move a pixel" was true — but the arm
was never a read-law; it was the RENDER-side write-commit position, which IS
representable and calibratable per-register. 5/7 fell to it. The 2 that survived
are the ONE real skill exception (render-LENGTH), and the refutation is now
backed by the eager-frame drop A/B, not an assumption.
