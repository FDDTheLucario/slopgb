# Window render-level model + the 24 non-DS window flip-BUG map (2026-06-29 #11af)

Built the tier2 WINDOW render-level **shadow WY-trigger** (the late-WY half of the
#11g window model the prior sessions called "needs a parallel window-length model
— neither exists") and exhaustively build-measured the 24 non-DS window CGB
flip-BUG rows against SameBoy. **Shipped a clean `+5/−0` late-WY slice; the
remaining 19 are mapped to four atomic mechanisms with their SameBoy ground
truth.** Method: per-row `flagon_probe` ON/OFF + `SLOPGB_S5DBG` (the
`ff41`/`winmatch`/new `wytrigset`/`winext` tracers) vs `sameboy_tester --cgb
--length 4` (`SBMODE` measurement-frame exit + `SBWWY`/`SBWYTRIG`). Full-CGB
two-bin (both bins rebuilt from source over all 3422 CGB rows).

## SHIPPED — the shadow WY-trigger (`+5/−0`, flag-gated, byte-identical OFF)

SameBoy latches `wy_triggered` from a **continuous** `WY == LY` compare during the
visible frame (`display.c` `wy_check`); slopgb's production `wy_latch` samples only
at the three gambatte weMaster dots (line 0 dot 2, dots 450/454). So a **mid-line**
late-WY write that SameBoy catches is **missed** by slopgb's discrete sampler →
slopgb renders the line BARE (`vis_mode == 0` at the polled read dot) where
SameBoy's window triggered and extended mode 3 to `263 + SCX&7` (the POLLED read
exit, +0 ISR offset — these reads carry no mode-2 dispatch, #11z).

The fix is a tier2 + CGB **shadow** (`Ppu::wy_trig_sb`, byte-identical OFF — the
fields are never updated nor read on the production path) re-deriving SameBoy's
decision for the FF41-read law only (`vis_mode_read`), NOT `line_render_done`/the
render:
- **sticky latch** set the first dot `win_en && wy2 == ly` on any visible line;
- **WX-activation deadline**: extend mode 3 on a line iff the latch was set
  at/before the WX-comparator match dot (`Render::wx_match_dot`, recorded before
  the `wy_ok` gate so a bare line still pins it) **+ 2** — the +2 is the wy2-copy
  phase: slopgb's `wy2` lags the write by 6 dots (CGB), SameBoy's `wy_check`
  catches it at write + ~4, so the shadow `trigdot` runs 2 dots behind SameBoy's
  detection. The `_1` (extend) trigdot = wxmatch + 1; the `_2`/`_3` (miss) = +5.
- fires ONLY when the trigger latched on THIS line (`trig_line == ly`). The
  cross-line case is deliberately bare: see "boundary-write" below.

Pinned by `tier2_window_late_wy_extend_passes` (the 5 fixes + the `_2` deadline
siblings + the `late_wx`/`late_reenable` cross-line exclusions as regression
guards). FIXED (all `_1`, want 3): `late_wy_10to1_ly1_1`, `late_wy_FFto2_ly2_1`,
`…_scx2_1`, `…_scx3_1`, `…_wx0f_1`. Two-bin full-CGB **492 → 487 = +5/−0**.

## The 24 non-DS rows — mechanism verdicts (build-measured)

### A. late_wy (11) → 5 FIXED + 6 atomic (boundary-write / over-extend)

`_1`/`_2`/`_3` variants differ ONLY in the late-WY write dot; the window extends
mode 3 (`SBex = 263 + SCX&7`) iff the WY-trigger beats the WX activation, else bare
(`257 + SCX&7`). slopgb reads mode 0 at the polled dot for ALL three (renders bare)
→ the `_1` want-3 fails. The want3/want0 split is the WY-write dot (e.g.
`FFto2_ly2`: `_1` write dot 92 → `_2` dot 96, a 4-dot step) — **observable**, so
NOT a hard collapse.

| sub-case | rows | verdict |
|---|---|---|
| mid-line WY write on the measurement line | `10to1`, `FFto2_ly2`(+`scx2`/`scx3`/`wx0f`) | **FIXED** (shadow, +5) |
| `scx5` mid-line | `FFto2_ly2_scx5_1` | near-miss: `wx_match_dot` shifts with WX (97→105 for wx07→wx0f) but NOT with SCX (97 for scx0 *and* scx5), while SameBoy's deadline does. `+SCX&7` slack collapses `scx3_2` (trigdot 102 == deadline 102) → left atomic |
| boundary WY write (latch on an earlier line) | `10to0`, `FFto0_ly0`, `FFto0_ly2`, `FFto1_ly2` | **atomic**: slopgb sees the WY write a full line later than SameBoy (`10to0`: slopgb ly0 dot452 vs SameBoy ly0 dc≈0 → `SBWYTRIG cmp=0`), so the shadow's `wy2` never matches the trigger line. The deferred-frame WY-write phase, not a whole-dot lever |
| WY→FF disable (`_1` over-extends) | `1toFF`, `2toFF` | **atomic**: slopgb's window DRAWS (reads mode 3) where SameBoy's late WY=FF aborted the latch in time (bare) — a SHORTEN the add-only shadow cannot do |

### B. late_disable / late_reenable (9) → atomic (render-coupled abort)

Bidirectional: the `_1` want-0 rows over-extend (slopgb reads mode 3, SameBoy
aborts to bare 257/the abort keeps only part of the +6); the `_2` want-3 rows
under/over by config. SameBoy's window mode-3 **aborts early on the LCDC.5 clear
but keeps the in-flight tile's cost** (`SBex` 263/266 with the abort), where
slopgb's `win_aborted` SUBTRACTS from `lead`. This is the #11z-confirmed
**render-coupled** term — the abort must shorten/keep the *visible* mode 3, a
production-render change (breaks byte-identical OFF), NOT a `vis_mode_read` law.
The add-only shadow is excluded here by design (`!win_active` cross-line latch ⇒
the window was aborted/toggled ⇒ leave bare — this is exactly the guard that
turned an early `−13` over-aggressive build into `+5/−0`).

### C. wxA6 / wxA5 edge-WX wall (3) → atomic (read-frame sub-dot)

`m2int_wxA5_m0irq_2`, `m2int_wxA6_firstline_m3stat_3`, `m2int_wxA6_vrambusyread_3`.
Off-screen WX (≥ 0xA0): the WX comparator (`wx <= 166`) never matches →
`wx_match_dot = 0` → the shadow cannot reach them (confirmed inert). These are the
#11g mech-1 read-frame ↔ boundary sub-dot wall (the `vis_mode_read` `wx ≥ 0xA0`
off-screen-extension law already handles the firstline read-frame; the residual is
the cc-exact read collapse). **Atomic — confirmed, not assumed.**

### D. late_wx (1) → atomic (over-extend)

`late_wx_scx5_1` (want 0): slopgb reads mode 3 at the polled dot (over-extends);
SameBoy's late WX write moves the activation so its window doesn't extend on the
measurement line (bare). A SHORTEN the add-only shadow cannot do — render-coupled,
same class as B.

## Why the shadow is `+5` and not more (the two walls)

1. **The deferred-frame WY-write phase** (boundary writes, `10to0`/`FFto0`/
   `FFto1`): slopgb places the line-boundary WY write a full line off from
   SameBoy, so no continuous-compare shadow latches the right line. This is the
   global deferred-clock frame (the C-stage atomic), not a window lever.
2. **The shorten direction** (`1toFF`/`2toFF`/`late_disable`/`late_wx`): SameBoy
   renders bare/aborts where slopgb's native `line_render_done` over-extends mode
   3. Shortening the *visible* mode 3 is a `line_render_done`/render change
   (production-shared, breaks byte-identical OFF) — the #11z render-coupled
   `late_disable` term, unchanged.

The add-only `vis_mode_read` shadow is exactly the EXTEND-direction lever; it
cleanly captures the mid-line-late-WY missed-trigger rows and nothing else.

## Gate

24 → 25 tier2 pins green (`+tier2_window_late_wy_extend_passes`); full-CGB two-bin
`+5/−0`; mooneye flag-on 91/91 (`SLOPGB_MOONEYE_RECLOCK=1`) + OFF 91/91; gbtr OFF
byte-identical (the shadow is `tier2 && is_cgb` gated; OFF window family two-bin
0-diff); clippy −D clean; rustfmt clean (touched files). Defaults NOT flipped.

Detail/continuation: `docs/hardware-state/ppu-subdot-ladder.md` "#11af".
