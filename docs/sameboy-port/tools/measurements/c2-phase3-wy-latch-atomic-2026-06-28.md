# C2 Phase 3 (#11aa) — the WY-latch render fix is ATOMIC with the read-frame (+10/−11, de-masks the polled read-collapse)

2026-06-28. Phase 3's START (the render window WY-latch / `win_active`) build-measured.
The fix is CORRECT (it activates the window SameBoy renders) but +10/−11 — it de-masks
the polled `late_wy` read-collapse, exactly as #11y's window-length law de-masked the
read-frame. Phase 3 is NOT a clean byte-identical-OFF slice; it is part of the atomic
C3 reclock. Experiment REVERTED; HEAD byte-identical (mooneye flag-on 91/91).

## The WY-latch root (confirmed both sides)

The polled `late_wy_*` window rows fail because slopgb's `render.win_active` is FALSE
where SameBoy's is true. ROOT: **SameBoy schedules a `wy_check` on every WY (FF4A) and
LCDC (FF40) WRITE** (`memory.c:1452`/`:1556` set `wy_check_scheduled`; it fires ~8-T
modulo-aligned, comparing the new WY against the CURRENT line, `display.c:519`). slopgb
samples `wy_latch` ONLY at discrete fixed dots (line-0 dot-2 `wy==0` reset + dot-450
`ly==wy` + dot-454 `ly+1==wy`, `ppu/mod.rs`), with NO re-check after a WY write. So a
WY write landing mid-line is missed:
- MEASURED `late_wy_10to0_ly1_1`: WY 10→0, `effwy=0` by dot ~452 of line 0 (`en=true`),
  but slopgb's dot-2 reset already passed (WY was 16 then) and the dot-450/454 checks
  use the wrong comparison → `wy_latch` stays false → line renders bare → ly1 read
  mode 0 (want 3).

## The fix (validated, correct — `wy_recheck_on_write`)

Re-run the latch at the WY/LCDC write commit (`regs.rs::commit_eff`, FF4A + FF40),
tier2-gated: `wy_latch |= win_en && self.ly == self.eff.wy`. Traced
`late_wy_10to0_ly1_1`: `wyrecheck ly=0 dot=452 effwy=0 en=true` → SETS `wy_latch=true`;
ly1 winmatch becomes `wx=7 wy_ok=true en=true` → window ACTIVATES. The render bug is
genuinely fixed.

## Why it can't ship alone — it de-masks the polled read-collapse (+10/−11)

Window-family two-bin (vs #11z's 79): **80 (+10/−11)** — and IDENTICAL whether the read
law is #11z shorten-only, two-sided, or the `in_isr` 259/263 exit (the read law is NOT
the differentiator). The trade, per config:
- FIXED (10, the `_1` want-3 rows): `late_wy_FFto2_ly2_1`, `_scx2/scx3/scx5/wx0f_1`,
  `late_wy_10to1_ly1_1`, `late_wy_FFto0_ly0_1`, `late_enable_afterVblank_{ds,lcdoffset1}_1`.
- REGRESSED (11, the `_2` want-0 SameBoy-pass rows): the `_2`/`_4` siblings of the above.

MECHANISM: currently (window inactive) the line renders BARE, so the `_2` reads land
mode 0 = want 0 — passing BY ACCIDENT. Activating the window (correct) makes BOTH `_1`
and `_2` read mode 3; slopgb reads `_1`/`_2` at INDISTINGUISHABLE dots (the read-collapse
— `_1` wants mode 3, `_2` wants mode 0, but slopgb can't place them on opposite sides of
the window exit), so it trades `_1` (gained) for `_2` (lost). The polled `_2` reads land
at dot < the exit but SameBoy reads them ≥ exit — the same per-config read-frame the
in-ISR family has, now on the polled side.

## Conclusion — the window family is fully entangled = atomic C3

Every angle on the window family de-masks or requires the others:
- window-length law (#11y/#11z): SHIPPED the clean subset (+9), de-masks the read-frame
  for the rest.
- WY-latch render (Phase 3, here): correct, but de-masks the polled read-collapse (+10/−11).
- the read-frame (in-ISR +4 / polled +0): the atomic reclock (breaks ~54 interrupt tests).

So there is NO clean byte-identical-OFF Phase-3 slice — the WY-latch render, the window
length, and the per-config read-frame all CO-LAND in the atomic global reclock (C3 +
the gambatte rebaseline). The `wy_recheck_on_write` code is validated and ready to land
WITH that atomic step (it is a real bug fix, just not separable). The clean read+length
slices (#11w/x/y/z, +14) are exhausted; the window residual is the C3 atomic reclock.
