# #11bh item 7 — the POST-SWITCH bare-exit law, built + measured, PARKED (2026-07-03)

The last 4 must-fix blockers: `speedchange2_lcdoff[_nopx2]_m2int_m3stat_scx2_2`
+ `speedchange2_nop_lcdoff[_nopx2]_m2int_m3stat_scx2_2` (want 0, got 3).

## Measured geometry (dual-traced)

- SameBoy (scx2_2): LCD ON in DS fp=26441980 → STOP#2 DS→SS fp=26442132
  (`SBSTOP dsa=32 dsa7=0` — NOT the briefed dsa7=4); m2 dispatch SBACK
  ly134 cfl0 fp=26704628; the FF41 read `cfl259 → mode 0` (the exit at/before
  the read).
- slopgb: leave advance k=2 hd (the #11bd default, `lcd_shift_dots += 1`);
  read ly134 **dot 253**, rp = 2·253 = 506, ISR carry 4; native flip 256 →
  emergent exit `2·256+2−4 = 510` → 506 < 510 → **3**. The emergent
  `2·flip+2` is frame-INVARIANT to the leave advance (read and flip shift
  together) — it cannot express the freeze's read↔exit displacement.
- The pair pins the post-switch exit: `_1` rp 498 (want 3) / `_2` rp 506
  (want 0) → **E(scx2) ∈ (498, 506]**.

## The law built

`vis_exit_hd` bare arm, scoped `stop_leave_lcd_on` (a new PPU flag set at a
tier2 DS→SS STOP leave with the LCD enabled — the SameBoy freeze path;
cleared with the LCD): `fold(exit, 502 + 2·(SCX&7))` (the recipe's
`E(scx)=510+2·scx` mapped onto the rp = 2·dot frame).

Also swept: `SLOPGB_LCDPH=2` (the carried `lcd_phase_hd` = 4−k law-side
surplus): speedchange family 61 → 63 fails (+3 ly44 `_2` / −5 `_1`) — the
law-side-phase single knob re-refuted, consistent with the standing
do-not-retry.

## The A/B (the park evidence)

Speedchange family (242 rows) two-bin, name lists
`scratchpad/spd_base.txt` vs `spd_law2.txt` (worktree):

- **FIXED 9**: all four scx2_2 blockers + `speedchange2[_nop]_ly44_m3[_nopx2]_m3stat_scx2_2`
  ×3 + `speedchange4[_nop]_ly44_m3_m3stat_scx1_2` ×2.
- **BROKE 14**, `classify_cgb_regr.py`: **14/14 BUG (SameBoy-pass, forbidden)**:
  `speedchange2_frame1/…_m3stat_scx3_1`, `speedchange2[_nop]_lcdoff_nop_…_scx1_1`,
  `speedchange2_m2int_m3stat_scx3_1`, `speedchange2_nop_m2int_m3stat_scx4_1`,
  the ly44 `_1` family (scx2/3/4 + `stat_1`), `speedchange4*_ly44_scx2_1`…

The blanket is the #11bb (+21/−8) / #11bc (+14/−11) half-dot A/B swap
re-measured at a third operating point. The broke set spans BOTH scx
parities and both nop-slide classes → no single (scx)-linear exit fits; the
per-leg resolution needs the **(nop-slide s, SCX, `sb_dsa8`-at-leave,
ISR-carry) 4-variable exit table** — the leave k already branches on
dsa7 (2 vs 6), and the polled ly44 reads carry 0 where the m2int ISR reads
carry 4, so the four variables are all live. That is the full S6 co-land
(items (i) machine half-dot skew + (iii) per-row rebase landing together);
parked per the goal's rule with these numbers. The scaffold was reverted;
the park note lives at the `vis_exit_hd` bare arm.

Note for the next attempt: the SBSTOP `dsa7` values contradict the
asm_lcdoffset_speed brief for the s-even class (measured 0, briefed 4) —
re-derive the class map from SBSTOP before building the table.
