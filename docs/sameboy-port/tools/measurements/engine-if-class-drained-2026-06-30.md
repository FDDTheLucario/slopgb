# The ENGINE-IF class — build-measured (2026-06-30 #11al): 1 clean engine mechanism (3 rows shipped) + the sharpened atomic-engine-core boundary

The goal: DRAIN the 64-row ENGINE-IF flip-BUG class — complete the S5
`stat_update_tick` IF-bit lifecycle, shipping clean flag-gated +N/−0 slices from
the named roots (lycEnable late-write / miscmstatirq m0-late re-arm / m1 m2-disable
/ m0+m2enable lyc-disable / ly0+lyc153 wrap), holding mooneye 91/91 flag-on +
byte-identical OFF. Build-measure every root (the #11ae/#11ah "atomic" verdict to
overturn).

## RESULT — 1 clean engine mechanism found + DRAINED (3 rows); the rest are read-frame

The ENGINE-IF class is **not a separable IF-lifecycle bug for most rows**. Live
3-mode probe (`flagon_probe` OFF/LE/ON) + SameBoy SBLEVEL/SBWRITE/STAT_IRQ ground
truth across all 7 named root families shows: the `stat_update_tick` rising-edge
**dispatch DOTS are ~right but a few dots off** SameBoy's (the LYC-153 match lands
slopgb `dot6` vs SameBoy `cfl0`; the bare-line mode-0 lands `dot254` vs `cfl257`;
the mode-2 line-start lands `dot0` == `cfl0`), and the measurement read straddles
that gap → wrong OCR. **That is the read-frame, NOT the engine IF-lifecycle.** The
one genuinely-engine-side bug — slopgb firing a STAT edge SameBoy *entirely lacks*
— is the **last-M-cycle LYC-write spurious re-latch**, now drained (SS).

### SHIPPED (#11al, flag-gated, byte-identical OFF, +3/−0 full-CGB two-bin)

The CGB last-M-cycle LYC-write hold (`reclock.rs::stat_update_tick`
`line_start_carryover`): `(line==1 && dot<=2) || (dot>=452 && !ds)`. A late FF45
write in slopgb's leading-edge frame commits 1 M-cycle EARLIER than SameBoy, on the
current line's last M-cycle (dot≥452), where the freshly-matching just-written LYC
re-latched `lyc_interrupt_line` → a spurious last-dot STAT edge. SameBoy's write
lands the NEXT line's `cfl0` (the held carryover / `lyfc=-1`) → no fresh edge
(measured SBWRITE/SBLEVEL). Hold the latch across the last M-cycle so the
just-written LYC carries into the next line unchanged; a write before dot 452 still
re-latches and fires (the A/B `_1` sibling).

| row | model | want | mechanism (SBWRITE) |
|---|---|---|---|
| `lycEnable/lyc0_late_ff45_enable_2` | Cgb | E0 | write `ly1 cfl0` (no edge); `_1` writes `ly0 cfl0` (fires) — slopgb fired `_2` at ly0 dot453 |
| `lycEnable/lyc153_late_ff45_enable_2` | Cgb | E0 | write `ly153 cfl0 lyfc=-1` (no edge); slopgb fired the wrap last M-cycle |
| `m2enable/lyc1_m2irq_late_lyc255_2` | Cgb | 0 | late LYC=255 disables the match; slopgb re-latched the line-1 last M-cycle |

Pin `tier2_lyc_carryover_late_ff45_cgb_wrap_passes` (3 rows). Two commits:
`69abb81` (line-0 wrap, +1), `115b01a` (broadened to all lines' last M-cycle, SS,
+3 total). 26→27 tier2 pins. mooneye flag-on 91/91, OFF 91/91, gbtr OFF
byte-identical.

**Why SS-only (DS = S6, build-confirmed atomic):** at double speed the last M-cycle
is 2 dots (the leading-edge write offset is +1, not +4). `dot>=452` over-covers the
DS grid → inverts the SameBoy-passing `_ds_1` siblings (`lyc153_late_ff45_enable_ds_1`
E2, `lyc1_m2irq_late_lyc255_ds_1` out2). A tighter `dot>=454 && ds` was BUILT +
measured: it STILL breaks both `_ds_1` AND fixes ZERO `_ds_2` rows — the DS rows are
not a threshold shift, they are the S6 DS read-grid. Reverted.

**Why line-START stays REFUTED on CGB lines 2-143:** the #11l/#11r line-START
carryover hold (`dot<=2`) is the symmetric lever, but on CGB the lcd-offset shifts a
REAL edge onto the START carryover dot (`late_ff45_enable_3` fires `ly7 dot1`, a
mis-dotted REAL edge SameBoy delivers — measured negative, reclock.rs comment). The
line-END (last M-cycle) is clean precisely because `ly_for_comparison` is FIXED
there (set at dot 4, held) — only a fresh LYC write moves the latch, never a
carryover/offset edge.

## THE SHARPENED ATOMIC-ENGINE-CORE BOUNDARY (the #11ae/#11ah verdict, refined)

The #11ae map called the lycEnable/m1/m2enable/miscmstatirq/ly0 ENGINE-IF rows
"dispatch dot correct / IF-lifecycle wrong / atomic." Build-measure SHARPENS this:
the **divergence is the dispatch/match DOT (a few dots), not the IF bit's
presence/blocking-level**, for every named root except the last-M-cycle write.
Per-family, with the decisive trace:

| family | rows | verdict | decisive measurement |
|---|---|---|---|
| **lycEnable late-write** | 3 | **SLICED** (above) | slopgb fires an edge SameBoy lacks (last M-cycle) |
| lycEnable disable/enable | ~10 | read-frame | `ff41_disable_2` / `ff45_disable_2`: slopgb + SameBoy BOTH fire ly152+ly153, slopgb ly153 `dot12` vs SameBoy `cfl0`; the FF41/FF45 disable at ly153 races the dot-12 fire. `late_ff45_enable_2`: the dot-453 spurious is now suppressed but OCR is set by the ly5/6 LYC dispatch (dot4) vs read — still read-frame |
| **m1** vblank/m2-disable | 14 | read-frame | every `want1 got3`: slopgb + SameBoy fire the SAME ly143+ly144 lines (`m1irq_m2disable_lycdisable_3`, `m1irq_m0disable_2`); the residual is the deferred-read placement (#11k confirmed). The ly144 LYC-latch drop (#11j) is already CGB-applied |
| **m0enable** lyc-disable | 9 | read-frame | mode-0 (HBlank) fires EVERY line, slopgb `dot254`/`255` vs SameBoy `cfl257`; the disable-commit timing vs the match dot |
| **m2enable** lyc-disable | 5 | read-frame (1 sliced) | mode-2 (OAM) fires EVERY line, slopgb `dot0` == SameBoy `cfl0` (IDENTICAL dispatch) → the want/got is PURELY the read protocol straddle |
| **miscmstatirq** m0-late | 6 | read-frame | per-line mode-0, slopgb `dot254` vs SameBoy `cfl257`; `lycstatwirq_trigger_ly00_10_50_1`: SameBoy fires `ly0 cfl0` (serviced→E0), slopgb's residual STAT is the ly144/153 delivery |
| **ly0 / lyc153int_m2irq** wrap | 10 | read-frame | the `_2` / `_ifw_2` pairs have IDENTICAL slopgb+SameBoy dispatch (`dot0`==`cfl0`, or `dot6` vs `cfl0`) and OPPOSITE wants — the `ifw` read protocol straddles the dispatch = textbook read-frame A/B |

**The boundary:** ENGINE-IF splits into (a) the **last-M-cycle LYC-write spurious
re-latch** (engine-side, SLICED, SS) and (b) **dispatch/match-dot read-frame** (the
LYC-153 match `dot6`↔`cfl0`, the bare-line mode-0 `dot254`↔`cfl257`, the
disable-commit-vs-match-dot race) — all atomic with the C-stage dispatch reclock
(moving them shifts the counter-pinned kernel/`intr_2`/`int_hblank` frame; the
`ly_for_comparison_line_153` dot is pinned exactly by wilbertpol ly_lyc_153-C).
The #11ae "IF-lifecycle blocking-level" framing is REFINED: there is no separable
blocking-level/edge-presence bug in (b) — slopgb and SameBoy fire the same edges,
just a few dots apart, and the ISR read lands between them.

## Method / tooling

- 3-mode probe: `flagon_probe` (`scratchpad/binpath_temp.txt`), `SLOPGB_PROBE_OFF`/
  `_LE`, per-family rowlist (`scratchpad/lycenable_rows.txt`, `ds_rows.txt`).
- slopgb level/dispatch trace: temp `SLOPGB lvl ly/dot/prev->now/mfi/lycln/en` in
  `stat_update_tick` (reverted) + the committed `SLOPGB dispatch`.
- SameBoy: `sameboy_tester --cgb --length 4` `SB_TRACE` → `SBLEVEL`/`SBWRITE ff45`/
  `SBTRACE STAT_IRQ`. Helper `scratchpad/trace_row.sh`.
- Two-bin: `flagon_probe` ON (`target/gbtr`) vs fresh HEAD (`target/gbtr_head`),
  `comm` over `scratchpad/cgb_rowlist.txt` (3422 rows).

## Next (the residual 61 ENGINE-IF rows)

All read-frame / dispatch-dot — land with the C-stage atomic reclock (the deferred
read frame + the dispatch retime), NOT a slice. The DS last-M-cycle write is the S6
DS-grid. The slice vein for ENGINE-IF is the last-M-cycle SS write, now drained.
