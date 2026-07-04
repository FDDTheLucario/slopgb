# DMG window-law port — the §3b DMG-OCR window blockers (2026-07-03, #11bj)

Ported the CGB `vis_mode_read` window arms to DMG. Goal Phase 1: the 37
DMG-OCR flip blockers, of which **window = the biggest lever**. Reclassifying
the shared-want (`dmg08_cgb04c_out*`) rows with the rebuilt `--dmg` classifier
(`scratchpad/classify_dmg.py`) revealed the true DMG window blocker count is
**62** (not 29 — the #11bi census used a want-regex that missed 33
shared-want rows, mis-bucketing them as pixel legs). **56 of 62 fixed**, 0
CGB drift, 0 SameBoy-pass DMG drop.

## Method

- **Probe accepts `[Dmg]`** (verified — `gambatte_flagon_probe` reads the
  model tag; a 5-row DMG list flips verdicts with the flag). No harness patch
  needed.
- Full non-DS DMG window family (209 legs) two-binned flag-on/off:
  baseline **ON 115 pass / 70 fail**, OFF 156/29. The 64 flip-regressions =
  the dry-run's 64 DMG window new-fails (reconciled exactly).
- Dual-traced every leg + its `_1`/`_2`/`_3` siblings: slopgb
  `SLOPGB_S5DBG` (`ff41`/`winmatch`/`visexit` + new `wwy`/`wlcdc` write-commit
  + abort-state `wpa`/`wpad`/`wxm`/`wxscx`/`wend`/`wren`/`wxwd` tracers) ↔
  SameBoy `--dmg SB_TRACE=1` (`SBMODE`/`SBREAD`/`SBWWY`/`SBWLCDC`/`SBWSCX`).
  Harness `scratchpad/win_measure.sh`; fit table `scratchpad/p1_fit_table.txt`.

## The DMG-vs-CGB divergence (the core finding)

The DMG deferred FF41 read shares the SS **−4 polled offset** with CGB, but
two constants differ, and they make the model **diverge**:

1. **`wy2` lag +2 (DMG) vs +6 (CGB).** The shadow WY-trigger latches 2 dots
   after the WY write on DMG (measured: `FFto2_ly2_2` write 96 → latch 98),
   6 on CGB. So the SAME `wx_match + 2` shadow deadline extends a mid-line
   late-WY write on DMG that stays bare on CGB — the `_2` legs
   (`dmg08_out3_cgb04c_out0`) flip **extend on DMG, bare on CGB**.
2. **Per-WX/SCX ship deadlines shift** (the fine-scroll fetch phase is 1 step
   ahead on DMG): the WX-rewrite un-catch fires at `scx&7 ≥ 3` on DMG vs
   scx5-only on CGB; the reenable deadline carries an SCX term
   (`reen + 3 > wx_match + SCX&7`) absent on the SCX-flat CGB arm; the
   pre-draw abort keeps the SCX penalty (bare `257 + SCX&7`) where CGB drops
   it (`253`).

## Shipped arms (all `!is_cgb()`-scoped → CGB byte-identical)

| arm | rows | law |
|---|---|---|
| D1 length | m2int_wx*/wxA5/wxA6 `_2` (16) | exit `259+SCX&7` (wx<0xA6) / `253+SCX&7` (wxA6 bare) / `259` (wxA6+spr) |
| D2 shadow | late_wy mid-line `_2` (7) | arm-2 dropped `is_cgb`; same +2 deadline, DMG `wy2` lag splits |
| D3 pre-draw abort | late_disable `_1`/`_2` (12) | bare `253+fscx` / extend `259+fscx`; ship = `wx_match−3+min(fscx,2)`; low-WX SCX-delay kill; `wx_match_scx` (fetch SCX) |
| D3-spr | late_disable_spx10 `_2` (1) | sprite extends → `270` |
| D5 reenable | late_reenable `_2` (2) | bare `253`, deadline `reen+3 > wx_match+SCX&7` |
| D-wx uncatch | late_wx_scx3/scx5 (2) | scx≥3 write≤match → bare `253+SCX&7` |
| D6 untrigger + WY→FF | late_wy 1toFF/2toFF `_2` (2) | `!wy_trig_sb_raw` bare `257+SCX&7`; WY→FF at dot≤4 releases the raw latch |
| D7 boundary head-latch | late_wy 10to0/FFto0/FFto1 `_2` (3) | WY head-write (dot<4) `value+1==line` → xline trigger (arm-7 DMG) |

New recording (tier2, byte-identical OFF): shadow/abort/wx_match/reenable/
wx_write now record on DMG too (was CGB-only); `Render::wx_match_scx` (fetch
SCX for the pre-draw exit), `Render::scx_write_dot` (mid-line SCX-rewrite
flag → skips the pre-draw arm on `late_scx_late_disable`).

## Two-bin results

- Full DMG window family: **ON 115→171 pass** (56 fixed / 0 new-fail on the
  185-leg OCR set).
- Full-CGB two-bin (3422 rows): **291/291 identical, ZERO drift** (every arm
  `!is_cgb`).
- gbtr OFF battery 236/0; mooneye flag-on 91/91 + flag-off 91/91; lib 660;
  clippy clean. Pin `tier2_dmg_window_passes` (21 legs incl. guards); 52 pins.

## The 6 residual (parked — same atomic classes CGB parks)

Classified SameBoy-PASS (must-fix) but require mechanisms the whole-dot model
can't represent, identical to the CGB-side parks (`c2-flip-blocker-
classification-2026-06-30.md`):

- **wxA6/wxA5 carried-read sub-dot wall (5):** `m2int_wxA5_m0irq_2`,
  `wxA6_m0irq_2`/`_m0irq2_2`/`_oambusyread_2`/`_vrambusyread_2` — the
  off-screen-WX carried mode-0 ISR read lands sub-M-cycle past the (correct)
  boundary (the #11g mech-1 read-frame wall; needs the S7 read clock).
- **scx5 non-linear deadline (1):** `late_wy_FFto2_ly2_scx5_2` — the +SCX&7
  shadow slack collapses at scx5 (the #11af CGB `scx3_2` collapse, DMG face).
- **mid-frame SCX rewrite (1):** `late_scx_late_wy_FFto4_ly4_wx20_3` — a −5
  read frame + line-start-SCX exit the whole-dot arm mis-frames (same class
  as the parked `late_scx_late_disable_1`, `scx_write_dot`-excluded).
- **render-trigger extend (2):** `late_enable_afterVblank_2`/`_4`,
  `late_reenable_scx5_2` — the window never enters slopgb's render
  (`win_active=false`, `win_enable_dot=0`); extending them is a production
  `line_render_done` change (breaks byte-identical OFF), the #11af/#11g
  render-coupled class.

3 more window rows classify SameBoy-FAIL → rebaseline at flip: `late_wy_1`
(×2, `arg/` + top-level; sb=3 want=0) + `m2int_wxA6_spxA7_m0irq_2` (sb=0
want=2).
