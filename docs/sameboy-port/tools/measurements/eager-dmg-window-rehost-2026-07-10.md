# EAGER L2 — the DMG window `late_wy_*_1` re-host: 5 clean BUG rows SHIPPED, EV DMG 74 → 69 (2026-07-11, #11di)

Task (#11dh L2 tail): re-host the 16 DMG window rows the #11dh Part-A scoping
RECLASSIFIED from "Part-A render-weld" to cheap `|| eager_value` re-hosts
(`scratchpad/dmg_window_rehost.txt`). Base: `finish-port-halfdot @ 31761a7`.
Reproduced the baseline exactly before touching a line: **EV DMG 74, EV CGB 318,
tier2 CGB 291, OFF DMG 103**.

## Result — 5 of 16 SHIPPED (all SameBoy-PASS BUG rows), EV DMG 74 → 69 (−5, 0 drops)

| sub-family | rows | shipped? | mechanism |
|---|---:|---|---|
| `late_wy_{1toFF,2toFF}_1` (un-trigger, out0) | 2 | **YES** | regs.rs SS un-latch also clears the spurious `wy_trig_sb` shadow + commits wy2 |
| `late_wy_{10to0_ly1,FFto0_ly2,FFto1_ly2}_1` (extend, out3) | 3 | **YES** | read-law arm D1 uses the cross-line 263 (not steady 259) when `wy_xline_trig` under eager |
| `late_wy_FFto2_ly2_scx{2,3}_1` (extend, out3) | 2 | no — REFUTED | different seam (ly2, `wy_xline_trig` NOT set); render/read-frame |
| `late_disable_{early,late}_scx03_wx11_2` | 2 | no — REFUTED | render-recorder frame (arm D3 abort dot at cc+0 render dot, not the cc+4 read frame) |
| `late_reenable_2` / `late_reenable_wx0f_2` / `late_scx_late_disable_0` | 3 | no — REFUTED | render-recorder frame (arm D5 reenable dot / carried-read frame divergence) |
| `m2int_wxA5/A6_m0irq(2)_2` (want2) | 3 | no — REFUTED | line-start mode-2 back-date, no clean read-time discriminator (#11dc) |
| `m2int_wxA6_firstline_m3stat_2` (want0) | 1 | no — REFUTED | over-hold read/render-frame |

All 5 gains classified BUG=5 FLOOR=0 UNK=0 (`classify_dmg.py`, SameBoy `--dmg`).

## What SHIPPED — two eager-DMG-scoped read-frame ports

Both `late_wy_*_1` sub-families are the `_1` boundary siblings of the #11dc `_2`
pairs. The `_2` writes commit BEFORE the render draws; the `_1` writes commit
one dot past the head, so on the eager whole-dot clock the wy2-lagged render
behaves differently than tier2's deferred frame, and the #11dc write-latch alone
(`eager_dmg_late_wy_passes`) does not recover them.

### 1. The un-trigger shadow-clear (`regs.rs::write` FF4A block 4)

`late_wy_1toFF_1`/`2toFF_1` write WY→FF at **dot 0**. Trace: the render never
draws (`win_active` false), `wy_trig_sb_raw` never latches (dot-0 write → WY=FF
by the dot-4 raw sample), BUT the dot-0 write spuriously latched the wy2-lagged
SHADOW `wy_trig_sb` at line start (wy2 still = old_wy = ly). That sticky shadow
gates OFF the arm-8 emergent bare exit (`!wy_trig_sb`), so on every later line
the read over-holds mode 3 → `out=3`. The existing SS un-latch (`old_wy == ly &&
value != ly && dot <= 4`) cleared only `wy_trig_sb_raw`; it now ALSO releases the
shadow (`if wy_trig_sb && wy_trig_sb_line == ly`) and commits wy2 immediately
(`wy2 = value; wy2_delay = 0`) so the next dot's compare cannot re-set it — a
verbatim mirror of the CGB/DS un-latch block just above. `!is_cgb() && !ds`,
fires only `tier2 || eager` → production/CGB byte-identical.

### 2. The cross-line trigger-line extend (`read_laws.rs` arm D1)

`late_wy_{10to0_ly1,FFto0_ly2,FFto1_ly2}_1` are WY tail-writes at ly0/ly1 dot452
matching the current line → `wy_xline_trig` set (block 1, model-agnostic). On
tier2 the render MISSES the seam (`win_active` false) and arm 7 folds the polled
`263` extend at `m == 0`. On the eager clock the wy2-lagged render OVER-triggers
the seam line (`win_active` true, native `m == 3` held), so arm 7 (`m == 0`) is
blocked and arm D1 (the triggering-window length, `m == 3`) fires with the
STEADY-STATE `259` instead of the trigger-line `263` — the read (rpos +8hd
read-debt) lands just past 259 → `out=0`. The arm D1 comment already documents
that the trigger line extends +4 (the `wy2 == ly` first-window exclusion). Give
the cross-line seam the same 263 when `wy_xline_trig` under `eager_value`:
`base = if eager_value && wy_xline_trig { 263 } else { 259 }`. `!is_cgb()`,
extra disjunct only under eager → tier2/CGB byte-identical (their render never
triggers the seam, so arm D1 never fires for these rows).

## The #11ck tension RESOLVED

#11dc/#11ck mapped `late_wy_*_1` + `late_disable`/`late_reenable` as "DMG
render-commit DEBT, refuted (a gate flip breaks 5 SameBoy-PASS rows)." That
refutation was about MOVING THE RENDER-COMMIT POSITION (the write-strobe debt),
a different lever. The two mechanisms shipped here are pure READ-FRAME ports (a
shadow release + a read-law constant), touch neither the render commit nor the
dispatch, and drop ZERO SameBoy-pass rows on either model — so they do not
reproduce #11ck's net-negative. The `late_disable`/`late_reenable`/`scx2/scx3`
extend / `m2int` rows DO depend on the eager render-recorder dots
(`win_predraw_abort_dot`/`win_reenable_dot`/`wx_match_dot`) or a line-start
mode-2 back-date — those stay refuted for Part-A render / a sub-dot read
discriminator, NOT forced.

## The 5 gains (all SameBoy-PASS TRUE-bar rows)

```
late_wy_1toFF_1     want0  (un-trigger, block 4 shadow-clear)
late_wy_2toFF_1     want0  (un-trigger, block 4 shadow-clear)
late_wy_10to0_ly1_1 want3  (cross-line extend, arm D1 263)
late_wy_FFto0_ly2_1 want3  (cross-line extend, arm D1 263)
late_wy_FFto1_ly2_1 want3  (cross-line extend, arm D1 263)
```

New-fail set: EMPTY on both models (EV DMG comm -13 empty; EV CGB fail-set
`diff` IDENTICAL, 318/318).

## Gates (all green on the shipped tree)

- `golden_fingerprint` byte-identical (production = flags false).
- EV DMG 74 → 69; EV CGB 318 fail-set byte-identical (`diff` clean); tier2 CGB
  291 unchanged.
- mooneye `acceptance_ppu` 92 flag-off AND `SLOPGB_MOONEYE_EAGER=1` AND
  `SLOPGB_MOONEYE_RECLOCK=1`.
- eager tripwires (`SLOPGB_EAGER=1`, both models): `intr_2_mode0/mode3/oam_ok/
  0_timing/mode0_timing_sprites` all PASS.
- clippy `-D warnings` clean; `regs.rs` 896 / `read_laws.rs` 998 / `window.rs`
  1010-legs (< 1000 src is 998; window.rs is a test file) — under cap; no new
  deps; no `unsafe`. Defaults NOT flipped.
- Pin `eager_dmg_late_wy1_rehost_passes` (window.rs) red-before-green verified
  (revert the two src files → 5 legs FAIL, `late_wy_1toFF_1` got=3).

## What's left on the DMG window cluster

After L2 (#11di): EV DMG 69. The window residual (11 rows) is
- the `scx2/scx3` extend siblings (a non-cross-line seam, `wy_xline_trig` unset);
- the `late_disable`/`late_reenable`/`late_scx_late_disable` render-recorder frame
  (Part-A render, the write-commit debt is the #11ck refuted A/B trade);
- the `m2int_wxA5/A6` line-start mode-2 reads (need a sub-dot read discriminator);
- `m2int_wxA6_firstline_m3stat` over-hold.

The clean READ-FRAME vein (shadow release + cross-line-latch read constant) for
the DMG `late_wy` family is now exhausted; the rest is render-frame (Part-A) or
read-accessibility.
