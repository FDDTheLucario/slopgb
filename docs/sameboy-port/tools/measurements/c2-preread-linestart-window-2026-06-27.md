# C2 #11x — the line-start OAM-window dot-0 exclusion (+1/−0)

2026-06-27, after #11w. A second forward C2 slice in the lcd-offset family
(#11q lineage). The `cgb_linestart_oam_open` window opened dots 0-3 to serve the
lcd-offset variant; it wrongly opened the BASE row's dot-0 read. Excluding dot 0
(single speed) separates them: full-CGB two-bin flag-on **+1/−0**.

## Mechanism

`oam_access/preread_2` (base, `want3`/blocked): slopgb reads OAM at **ly2 dot0**,
ACCESSIBLE (got0) via the `cgb_linestart_oam_open` window — wrong. SameBoy reads it
BLOCKED: SBMODE shows the mode-0→2 transition WITHIN cfl0 (`cfl0 dc2 vis=0` → `cfl0
dc8 vis=2`, where `dc`=`display_cycles` is SameBoy's lazy-advance accumulator), so by
the time the base read lands the mode-2 OAM lock has engaged.

The lcd-offset variant `preread_lcdoffset1_1` (`want0`/open) reads at **ly2 dot2**
(the offset shifts its read off the line start), which the window correctly opens.
So base@dot0 (want blocked) and offset@dot2 (want open) read at DIFFERENT dots — the
window opening dots 0-3 served the offset but wrongly opened the base's dot 0.

## Fix

`ppu/blocking.rs::cgb_linestart_oam_open`: single speed opens `1..CGB_LINESTART_OAM_OPEN`
(dots 1-3, EXCLUDING dot 0) instead of `0..`. Double speed keeps `dot < OPEN_DS`
(dots 0-1) — the DS read grid is 2 dots earlier and the DS lcd-offset variant
(`preread_ds_lcdoffset1_1`) reads dot0 wanting OPEN, so DS must keep dot0 (its base
is the separate S6/S7 grid). `tier2_reclock && is_cgb() && !line0` gated → DMG +
flag-off + double-speed byte-identical.

## Result + gate

- Full-CGB flag-on two-bin: 511 → 510 = **+1/−0** (`preread_2` fixed, zero regr).
- Pin `tier2_oam_preread_lcdoffset1_passes` extended to assert BOTH the lcd-offset
  variant (out0) AND the base `preread_2` (out3).
- gbtr+mooneye OFF byte-identical, flag-on mooneye 91/91, clippy/fmt clean.

## Note — `dc` is the lazy-advance counter, not a sub-dot

SameBoy's tracer `dc` = `gb->display_cycles` (`display.c:534`), the lazy PPU-advance
accumulator within a `GB_display_run` call — NOT a true sub-dot position. So a
`cfl0 dc8` reads "the PPU had advanced 8 cycles when the CPU read interrupted the
batch", a whole-dot-ish OAM-lock-engage question, which the window can express. This
refines the #11w "sub-dc clincher": some residual floors (preread, the line-start
lock geometry) are whole-dot-expressible window/offset fixes; the genuinely sub-dot
ones (the read-collapse where slopgb reads `_1`/`_2` at the IDENTICAL dot, e.g.
`late_scx4`) still need the cc-exact clock. Re-survey each before flooring.
