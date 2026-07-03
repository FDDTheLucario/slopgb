# #11bh — the FF0F per-event ORDER levers + the slice sweep (2026-07-03)

Session goal: the 27-blocker census → 0. Result: **27 → 4** (+23 fixed, ZERO
SameBoy-pass drops across every lever; final flag-on two-bin 354 → 323 (31 fixed, ZERO new of any class)).
The remaining 4 = the `speedchange2*_m2int_m3stat_scx2_2` quartet, parked
with a fresh measured A/B (see
`speedchange-postswitch-law-2026-07-03.md`).

New tracers banked (`build_sameboy_tracers.sh`): **SBACK** (the dispatch
IF-acknowledge instant — printed after the `pending−2` flush, so its `fp` is
synced, unlike SBDISP), **SBIF** (every display.c IF raise with synced fp:
`su` = GB_STAT_update, `m1oam` = the ly144 dot-2 raise, `vbl`/`vbloam` = the
vblank-entry pair), and `SBREAD ff0f` gained `fp=`.

The goal's key held everywhere: the discriminator is a per-event ORDER
comparison (peek / write-race / ack-deadline / co-instant mask) that never
moves the machine, the dispatch, or the IF lifecycle.

## Group A — the FF0F read PEEK (`Ppu::ff0f_stat_peek`, +2)

Verdict-only OR into the deferred cc+0 FF0F read (the #11ar FF41-peek shape).
Dual-traced fp: SameBoy's events-first `read_high_memory` frame has already
folded a rise slopgb's machine stops a hair short of.

- DS mode-0 flip +1 dot (`m2int_m0irq_ds_2` read 254 / rise 255; SameBoy
  reads 2 dots AFTER its rise). Anchored `stat_rise_oam` (the mode-2-ISR
  read frame): `lyc0int_m0irq_ds_1` reads the IDENTICAL dot-254/rise-255
  geometry from an LYC-anchored ISR with the OPPOSITE want. Unshifted only
  (`offset1_lyc99int_m0irq_count_scx1_ds_1` polls rise−1, must stay clear).
  The SS m0 window is 0 (the scx3 rows sit at the same +1 geometry with the
  opposite verdict — measured first, gated).
- LYC latch half-M ahead (2 dots SS / 1 DS) via the pure
  `ly_for_comparison_at` forecast (`lycint152_lyc153irq_2` read ly153 dot 4 /
  latch dot 6; `_1`/`_ds_1` read the lyfc gap).

## Group B — the FF0F write-race squash (`stat_if_squash`, +3)

A bit1-clearing deferred FF0F write consumes a rise within the per-source
window (`GB_CONFLICT_WRITE_CPU`, +1 T commit; the spent edge never
level-re-raises). Windows (dots), 15 legs measured: **DS mode-0 = 2**
(scx3/scx4 `_ifw_ds_2` consume at Δ 1-2, `_ds_1` survive 3-4) · **SS LYC =
1** (`lyc153irq_ifw_2` Δ1, `_ifw_1` survives Δ5) · **all else 0** (SS m0
`scx4_ifw_1` survives Δ1 · DS LYC `_ifw_ds_1` survives Δ2 · mode-2
`m2int_m2irq_ifw_ds_1`). First cut (uniform 2) broke 9 — the per-source
narrowing resolved all.

## Group C — the dispatch-ack squash reclock (+10)

The production gambatte `ackIrq` window (bit-0/1 `ack_squash_dots = 2`,
cc+4-calibrated) ate the post-ack re-rise SameBoy delivers. Tier2 replaces
it with PPU-side per-SOURCE windows (`ack_squash_ppu`): **mode-0 (SS 0, DS
1) · mode-2 pulse (0, 0) · LYC / mode-1 / vblank-IF (2, 0)**. The vblank-IF
raise takes the same window for bit-0 acks. Fixed all 6 retrigger blockers
+ 4 DS twins; zero drops (first cut = plain dots-0, +10/−4; the per-source
windows resolved the 4).

## Group D + late_enable — the carryover-tail write-fire scope (+2)

- `lyc143_late_m0enable_lycdisable_2`: the tier2 carryover-tail m0-enable
  fire missed the held-LYC pre-write-HIGH suppression (`cmp_cgb` switched
  lines while the engine latch still names the old match) — no 0→1 edge on
  hardware.
- `late_enable_2`: on UNSHIFTED CGB SS the same fire double-covers the
  two-phase ENGINE view (`eng_lyc`), whose phase-1 at commit+2 already lands
  on the line-start OAM carry — hardware's `ttnl > 4` dead-tail. Gated
  `!eng_lyc`; the shifted (`late_enable_lcdoffset1_1`) + DS frames keep it.

## Group E — the ly0-dot4 pulse co-instant read-view mask (+1)

`lyc153int_m2irq_1`: the asm_ly0 §2 static prediction ("SameBoy fails at
every phase") is REFUTED by the run — classify shows SameBoy PASSES. The
LYC-153 ISR's FF0F read lands BEFORE the line-0 dot-4 OAM pulse in SameBoy's
frame; slopgb collapses onto the pulse dot. CPU-read-first at the shared
instant (measured: `SBREAD ff0f` AT the rise fp reads clear). Verdict-only
mask, LYC==153-anchored: the LYC-152 ISR's same-dot-4 collapse lands 4 dots
AFTER the rise on SameBoy and must SEE it (unguarded build = +1/−2 A/B,
measured).

## The window/glitch/misc slices (+6 blockers, +5 bonus)

- **W1 root cause (3 slices)**: the win-line render clock sits +2 late in
  slopgb's frame; the FF41 laws compensate but three OTHER flip consumers
  read the raw clock: the m0 ENGINE rise (now projection-led 2 dots on
  win-lines, `m2int_wxA5_m0irq_2` + bonus `enable_wxA6_2x_spxA7_1`), the
  wxA6 VRAM lock (release at `259+SCX&7`, wxA6-scoped — the m0 IF rises
  while VRAM is still locked, never key on it; `m2int_wxA6_vrambusyread_3`),
  and the sprite-at-window-X abort slot (an object at OAM X = WX+1 occupies
  the post-restart GET_TILE_T1, removing the late CGB abort slot —
  `late_disable_spx10_wx0f_2`, exit 270).
- **(c) glitch hunt re-open**: the CGB glitch-line SCX sample deadline is
  `83 + scx_init` INCLUSIVE; a same-dot FF43 commit lands post-tick where
  hardware's live comparator still honors it (`Render::hunt_match_dot`;
  `ly0_late_scx7_m3stat_scx1_1`).
- **(d) DS line-start carryover level hold**: a DS dots-0-1 fresh LYC enable
  with old HBlank joins a line still latched HIGH (SameBoy's natural 1→0 at
  dot 2) — suppression + `force_level(true)` seed
  (`miscmstatirq …08_40_ds_2`; `_ds_3` dot-2 write still fires).
- **(e) WY per-trigger-line deadline**: lines ≥1 un-latch iff commit ≤ 2,
  line 0 iff ≤ 6 (the old single `<= 4` split both wrong);
  `late_wy_1toFF_ds_2` + `_lcdoffset1_2` + the SameBoy-beating
  `late_wy_ds_1`/`_lcdoffset1_1`.
- **The count-row deadline** (item 7's separable slice): the shifted-frame
  first poll lands ON the rise/flip dot where hardware is a half-dot past
  (F1 = L + 1.5). Verdict-only: the FF0F poll masks a same-dot shifted m0
  rise; the FF41 poll holds 3 on `dot == flip_dot`
  (`offset1_lyc99int_m0{irq,stat}_count_scx2_ds_1` + `offset3_…_scx1_1`).

## Ledger

354 → 323 flag-on (final list `scratchpad/on_11bh_final3.txt`, worktree);
blockers 27 → 4; pins 42 → 50; ALWAYS re-run the full two-bin after the LAST commit, not only per lever (the 9th-commit lesson); mooneye 91/91 ON+OFF at every commit; gbtr
OFF byte-identical throughout; lib 660; clippy clean. Commits `c9f9621` →
`873b2e9` (9) on `phase-b-s7` — the 9th anchors the count-row FF41 hold
after the final-tree two-bin caught 5 forbidden drops a family probe
missed (`!read_carried` + the `lyc == 0x99` count anchor; the ly44 poll
`speedchange3_nop_…_scx2_2` sits whole-dot-identical at dot 257 == flip,
dsa 6 — the sub-dot phase is the S6 co-land's).
