# DMG window-bar 5-row recalibration — the #11ec method applied to the reenable / scx-abort / late-WY-trigger levers (#11ed)

Base: `finish-port-halfdot @ 4698458` (= #11ec). **CODE SHIPPED** — three
`eager_value`-gated threshold recalibrations in `read_laws_exit.rs` (the
`vis_exit_hd` arm family, split out of `read_laws.rs` in a preceding
byte-identical commit for the 1000-line cap). EV DMG **50 → 45**, clean
+5/−0 both models. The measurement scaffolding (a `SLOPGB_WDBG` window-latch
dump + `SLOPGB_D5K`/`SLOPGB_D3BARE`/`SLOPGB_D3KSCX` sweep knobs) was all
REVERTED; only the fixes + comments remain.

## TL;DR — #11ec's method cracks all 5, none is an absolute floor

#11ec cracked `late_disable_*_wx11_2` by ROM-binary `cmp -l` → full-trace diff
→ representable-latch threshold recalibration. The other 5 of #11ea's residual
(the reenable pair, the scx-abort `_0`, the late-WY-trigger pair) are the SAME
shape one lever over — each `_1`/`_2` (or `_0`/`_2`) sibling differs by a
**whole-M-cycle NOP** that shifts a WRITE, which slopgb latches as a
representable render dot; the consuming arm's threshold was off vs the eager
cc+0 read frame.

## The ROM-binary diffs (`cmp -l`) — every pair is a whole NOP

| family | `cmp -l` | shifted write |
|---|---|---|
| `late_reenable_{1,2}` | 1-byte `00` at 0x1006 | `LD A,$91;LDH($40);LD A,$B1;LDH($40)` (disable→reenable) +4 dots |
| `late_reenable_wx0f_{1,2}` | 1-byte `00` at 0x1008 | same |
| `late_scx_late_disable_{0,1,2}` | +1 / +2 NOPs at 0x1012 | `LD A,$91;LDH($40)` (window disable) +4/+8 dots |
| `late_wy_FFto2_ly2_scx{2,3}_{1,2}` | 1-byte `00` at 0x137F | `LD A,$02;LDH($4A)` (WY:=2 write) +4 dots |

All representable on slopgb's M-cycle-atomic CPU (not sub-M-cycle poll phase).

## Family 1 — the reenable pair (arm D5, `win_reenable_dot`)

`SLOPGB_WDBG` dump at the decisive ly1 read (dot 252, rp 512):

| | reen | wxm | scx7 | want | arm-D5 `reen+3 > wxm+scx7` |
|---|---|---|---|---|---|
| `late_reenable_1` | 90 | 97 | 0 | 3 | 93>97 F → no-fire → extend ✓ |
| `late_reenable_2` | **94** | 97 | 0 | 0 | 97>97 F → no-fire → extend ✗ (want bare) |
| `wx0f_1` | 98 | 105 | 0 | 3 | 101>105 F → extend ✓ |
| `wx0f_2` | **102** | 105 | 0 | 0 | 105>105 F → extend ✗ |

The eager cc+0 read records `reen` one M-cycle before the tier2 cc+4 read the
`+3` was calibrated against (`_2` eager reen 94 vs the tier2-frame 95 in the
old comment). **Fix: `reen + (eager?4:3) > wxm + scx7`.** `_2` 94+4>97 → bare ✓,
`_1` 90+4≯97 → extend ✓. Sweep `SLOPGB_D5K` over the 17-row reenable set:
3→2 recovers / **4→both targets, 0 drops** / 5 same plateau / 6→−1 / 7→−2. The
already-failing `late_reenable_scx5_2` (want3, a separate scx5 lever) is
untouched at every K. `+4` = the principled +1 read-debt (mirrors #11ec's D3
+4).

## Family 2 — the scx-abort `_0` (arm D3, `win_predraw_abort_dot`, scx-rewrite)

Arm D3 EXCLUDED the scx-rewrite case (`scx_write_dot == 0` guard) because
`late_scx_late_disable` rewrites SCX 0→4 mid-line. `SLOPGB_WDBG` (guard relaxed
to `|| eager_value`):

| | abd | wxm | fscx | want | note |
|---|---|---|---|---|---|
| `_0` | 122 | 133 | 4 | 0 (bare) | reads dot 252, rp 512 |
| `_1` | 126 | 133 | 4 | 3 (extend) | |
| `_2` | 130 | 133 | 4 | 3 (extend) | |

Two calibrations needed vs the general eager predraw K=4/bare-253:
- **Deadline K = 8**: extend iff `abd + K >= wxm=133`. Want boundary between
  `_0` 122 (bare) and `_1` 126 (extend) → K∈[7,11); the fine-scroll (fscx=4)
  pushes the fetch-ship. K=4 wrongly bares `_1`.
- **Bare exit base = 252** (not 253): `_0`'s bare exit must sit ≤ the read
  rp 512; `2*(253+4)=514 > 512` reads mode 3. The eager cc+0 bare exit
  back-dates one dot (the +1 read-debt) → `2*(252+4)=512`, `512<512` false →
  mode 0. `_1`/`_2` read later so 514 was fine for them.

Joint `SLOPGB_D3KSCX`×`SLOPGB_D3BARE` sweep over the 37-row late_scx set:
K=4→25/4 (no recover); **K∈{7,8,9} × B∈{251,252} → 26/3, +1 clean, 0 drops**
(a robust plateau, not a knife-edge). Shipped K=8 (=4+fscx), B=252 (eager −1).
The 3 residual late_scx fails (`late_scx4_2`, `late_scx_late_wy_FFto4`,
`enable_display/ly0_late_scx7`) are pre-existing baseline fails, untouched.

## Family 3 — the late-WY first-window-line trigger (arm 2, `wy_trig_sb`)

| | win_active | wy_trig_dot | wxm | wy2==ly | want | arm |
|---|---|---|---|---|---|---|
| `late_wy_FFto2_ly2_scx2_1` | **true** | 94 | 97 | yes | 3 | none → native 0 ✗ |
| `..._scx2_2` | false | 98 | 97 | yes | 0 | arm 2 → 530, reads later → 0 ✓ |

`_1` has `win_active=true` (the render triggered on the first window line):
arm D1 excludes the trigger line (mode 3 extends later than the steady 259),
native mode has flipped, so it falls to native 0 where SameBoy still extends
(read dot 260, rp 528 < arm-2 exit 530). **Fix: admit arm 2 when
`eager_value && !is_cgb && wy2==ly && eff.wx<0xA0`** even with `win_active`.
The `eff.wx<0xA0` bound is load-bearing: it excludes the off-screen
`m2int_wxA6_firstline` (renders nothing → bare; a first attempt without it was
the ONE new regression).

## Gates (all hold)

| gate | value |
|---|---|
| `golden_fingerprint` (production, no port_probe) | **ok — byte-identical** (44s) |
| EV DMG | **50 → 45** (−5 clean: the 5 targets; 0 new) |
| EV CGB | 295 (unchanged — all arms `!is_cgb`/DMG-scoped) |
| tier2 DMG / CGB | 116 / 291 (unchanged — every change `eager_value`-gated) |
| mooneye OFF / RECLOCK / EAGER | **93 / 93 / 93**; eager intr_2 explicit ✓ |
| clippy `-D warnings` | clean |
| file cap | `read_laws.rs` 384, `read_laws_exit.rs` 656 (post-split) |
| pin | `eager_dmg_window_latch_recalib_passes` (red before → `late_reenable_2` shows 3) |

## Do-not-re-chase ledger

- None of the 5 is an absolute floor / needs a T-exact CPU core. Each `_1`/`_2`
  weld's discriminator is a whole-M-cycle NOP that lands a WRITE at a
  representable, latched dot (reen / abd / wy_trig), and the consuming exit arm
  was mis-thresholded for the eager cc+0 read frame. The #11ec "diff the ROM
  binary → full trace → representable latch" method applies to every window
  `_1`/`_2` pair; #11ea/#11eb's read-debt-only sweep moved both siblings equally
  → false weld.
- Method note: `late_scx_late_disable_1` is `dmg08_out3` (want 3), NOT out0 —
  read the `_dmgNN_outX` tag, not the sibling index, for the DMG expectation.
