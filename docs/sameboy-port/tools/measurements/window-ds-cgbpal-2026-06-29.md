# Window DOUBLE-SPEED + cgbpal_m3 map (2026-06-29 #11ag)

Ported the WINDOW family's two shipped single-speed laws — the #11y/#11z length
law and the #11af shadow WY-trigger — to **double speed**, and surveyed the 11
cgbpal_m3 rows. **Shipped a clean `+8/−0` DS window slice; the remaining DS rows +
all cgbpal map to atomic mechanisms with direct SameBoy ground truth (incl. a
framebuffer-decoded cgbpal verdict).** Method: per-row `flagon_probe` ON/OFF/LE +
`SLOPGB_S5DBG` vs `sameboy_tester --cgb --length 4` (SBMODE exit, SBWWY/SBWYTRIG),
full-CGB two-bin (both bins rebuilt over all 3422 CGB rows).

## SHIPPED — the DS window slice (`+8/−0`, flag-gated, byte-identical OFF)

Both `vis_mode_read` laws were `!ds`-gated. Under DS the deferred cc+0 FF41 read
lands **+1 dot vs SS** (the ISR read offset is +3, not the SS +4), so:
- **length law (the `m2int_wx*_m3stat` shorten)**: exit `260 + SCX&7` (`259 + ds`).
  MEASURED — `m2int_wxA6_scx5_m3stat_ds` reads `_1` dot264 / `_2` dot266, so only
  exit 265 (=260+5) separates them (the on-screen scx0 `_2` rows read dot260
  robustly past either, so 259-vs-260 is invisible to them → SS byte-identical).
  FIXED 7 (`m2int_wx03/07/0C/57/Default_m3stat_ds_2` + `wxA6_m3stat_ds_2` +
  `wxA6_scx5_m3stat_ds_2`, all want0).
- **shadow WY-trigger (the late-WY extend)**: exit `264 + SCX&7` (`263 + ds`),
  deadline slack **+4** (the DS wy2-copy lands the trigdot 2 dots later:
  `late_wy_FFto2_ly2_ds` `_1` trigdot 101 / `_2` 103 vs wxmatch 97). FIXED 1
  (`late_wy_FFto2_ly2_ds_1`, want3).
- **DS additionally excludes sprite-laden lines** from BOTH laws (`!ds ||
  n_sprites == 0`): with sprites the real mode-3 end extends past the bare exit and
  the DS read frame straddles it, so the bare shorten drops the want-3 read
  (`sprites/space/10spritesPrLine_wx*_m3stat_ds_1`, a SameBoy-pass — the early
  build scored window +7 / sprites +7/−8). SS keeps allowing on-screen sprites
  (byte-identical).

Full-CGB two-bin **487 → 479 = +8/−0**. Pin `tier2_window_ds_passes` (the 8 fixes +
the off-screen `_1`, the DS-`_2` deadline sibling, and a sprite-space `_1` as
regression guards). 26 tier2 pins; mooneye flag-on 91/91 + OFF 91/91; clippy/fmt
clean. SS legs byte-identical (the `ds` terms are 0 in single speed).

## DS window — the atomic remainder (build-measured)

| sub-family | rows | verdict |
|---|---|---|
| length-law `_1` scx5 (EXTEND) | `m2int_wx03_scx5`/`wx07_scx5_m3stat_ds_1` | **atomic**: slopgb reads native mode 0 at dot264 (UNDER-extends); the shorten law (needs `m==3`) cannot reach them. The render mode-3 length / read-frame (the SS `_1` scx5 EXTEND has no shorten lever either). |
| shadow scx5 deadline | `late_wy_FFto2_ly2_scx5_ds_1` | **atomic**: the SS #11af scx5 near-miss, reproduced — `wx_match_dot` shifts with WX but NOT SCX (97 for scx0 *and* scx5), while SameBoy's deadline does; `scx5_1` trigdot 105 > the scx0-tuned deadline 101 (and a `+SCX&7` slack collapses a `_2` sibling). |
| shadow boundary-WY | `late_wy_FFto0_ly2_ds_1` | **atomic**: the deferred-frame WY-write phase (slopgb places the line-boundary WY write a line off SameBoy → the shadow never latches the trigger line; the SS `FFto0`/`10to0` wall). |
| shorten / abort | `late_disable_*_ds`, `late_reenable_*_ds`, `late_wx_*_ds`, `late_enable_*_ds`, `1toFF_ds`, `late_wy_ds_2` | **atomic**: render-coupled abort (the window aborts but SameBoy keeps the in-flight tile's cost) / WY→FF over-extend — the `line_render_done` SHORTEN direction the add-only `vis_mode_read` cannot express (the #11z/#11af/#11s render-C2 term, confirmed for DS by #11s). |

## cgbpal_m3 (11) → ATOMIC (direct SameBoy framebuffer verdict)

The ON-only fails (`cgbpal_m3end_{scx2,scx3,scx5,ds,scx5_ds}_2`, `m3start_2`,
`read/write_m3start_2`) all **PASS flag-OFF** → tier2 read-frame regressions (the
#11s flavour-1 floor, blocked on the C2 global reclock), NOT missing features.

**The `cgbpal_m3end` read-collapse is now DIRECTLY CONFIRMED** (resolving the #11w
"PARKED: verify SameBoy's `cgbpal_m3end_1` digit"). Decoded SameBoy's rendered
framebuffer (the tester's `<rom>.bmp`, 32-bit top-down) for both legs:
`cgbpal_m3end_1` renders **7** (palette BLOCKED — matches gambatte out7) and
`cgbpal_m3end_2` renders **0** (palette ACCESSIBLE — matches gambatte out0). **Both
are SameBoy-passes**, reading the palette at nearly the same dot with OPPOSITE
results (blocked vs accessible). So the #11w `PalAccess` palette-release that fixes
`m3end_2` (accessible) **drops `m3end_1`** (a SameBoy-pass) — `+3/−1` is a genuine
SameBoy-pass drop, the read-collapse, NOT a gambatte-reference floor. Atomic — the
cc-exact read-phase lift. (`cgbpal_m3start`/`read`/`write` = the #11u A/B-pinned
lcd-offset window + the tier2 read-frame regressions, also C2.)

## Walls (why DS window caps at +8)

1. **EXTEND direction** (the length `_1` scx5 + the native under-extends): the
   shorten law only fires on `m==3`; an under-extended native read is mode 0 and
   needs `line_render_done` lengthened — a render change.
2. **SHORTEN direction** (late_disable/late_wx/1toFF abort): `line_render_done`
   over-extends; shortening the visible mode 3 breaks byte-identical OFF.
3. **SCX-non-linear deadline** (scx5) + **deferred-frame WY phase** (boundary
   writes): the SS #11af walls, reproduced in DS.

All four are the C-stage cc-exact reclock, not whole-dot CGB levers.

Detail/continuation: `docs/hardware-state/ppu-subdot-ladder.md` "#11ag".
