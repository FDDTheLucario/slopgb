# CGB lcd-offset — the lyc-engine dispatch tail (2026-06-26, #11r)

The continuation of #11q's CGB lcd-offset dispatch class (the single-speed
`late_enable` m0/m1/lyc class was DONE there). This session: the deeper
lyc-wrap / spurious / weirdpoint sub-families the goal banked as
"next-session build-measure candidates, NOT floored on reasoning."

**Result: 4 clean Tier-2 levers shipped, +4/−0 flag-on (full 3524-CGB two-bin).**
The build-measure lesson held AGAIN: each row that *looked* like "the read-frame
floor / C2" had — for 3 of the 4 — a clean tier2 write-trigger / engine lever
discriminated by a real slopgb state (the held `lyc_interrupt_line` latch, the
reclock `ly_for_comparison`), NOT the per-config offset. The one genuine floor
(`lycwirq_trigger_ly00_stat50_lcdoffset1_1`) was confirmed a floor only by
build-measure (the fix made the dispatch SET match SameBoy, but the OCR residual
is the legit-LYC-153 dispatch-dot + read-position read-frame).

## SHIPPED (+4/−0, full 3524-CGB two-bin)

| row [Cgb] | want | lever | seam |
|---|---|---|---|
| `lyc153_late_ff41_enable_lcdoffset1_1` | E2 | `lyc_wrap_153` | `stat_irq.rs::stat_write_trigger_cgb` |
| `lyc153_late_ff45_enable_lcdoffset1_1` | E2 | `lyc_write_wrap_153` | `lyc.rs::write_lyc_cgb` |
| `ff45_enable_weirdpoint_lcdoffset1_2` | 0 | `tier2_minus1_gap` | `lyc.rs::write_lyc_cgb` |
| `lyc0_late_ff45_enable_3` | E0 | line-1 carryover hold | `reclock.rs::stat_update_tick` |

Pin: `tier2_lyc_wrap_lcdoffset_passes` (gambatte.rs). 18 tier2 pins; gbtr+mooneye
OFF byte-identical (exit 0); mooneye flag-on 91/91; lib 660; clippy/fmt clean.

### Lever 1 — `lyc_wrap_153` (FF41 LYC enable at the ly153 wrap)

Hard trace (`lyc153_late_ff41_enable_lcdoffset1_1`):
- slopgb: FF41 LYC-enable (val=0x40) lands `ly153 dot11`. The LYC=153 dispatch
  slot is `ly153 dot6` (already passed). `cmp_cgb` at dot11 → `(153,_) => 0`
  (the LY=0 wrap arm), so `lyc_high = (lyc==0) = false` → `lyc_fire` false → no fire.
- BUT the held `lyc_interrupt_line` latch is still TRUE at dots 8-11 (matched 153
  at dot 6, held across the line-153 `ly_for_comparison == -1` gaps, drops at
  dot 12). SameBoy holds `lyc_line` across the gaps too (`display.c:534`, the
  `model<=CGB_C` OR makes the re-eval always run but the `!= -1` guard prevents
  the clear) and fires the fresh enable at `ly153 cfl0 lyc_line=1` (measured
  `SBLEVEL 0->1 stat=c5`, `SBTRACE STAT_IRQ ly153 cfl0 mfi=1`).
- Fix: tier2, a fresh LYC enable at line 153 with the held latch high (`!lyc_high`)
  fires. `cmp_cgb` (the dot-6 base compare) untouched.

### Lever 2 — `lyc_write_wrap_153` (FF45 LYC=153 at the ly153 wrap)

Trace (`lyc153_late_ff45_enable_lcdoffset1_1`): the late FF45 write (LYC=153)
lands `ly153 dot7`. The gambatte `target` table for line-153 dots 4-7 →
`_ => Some(0)` (the LY=0 increment) so `target == Some(153)` fails → no fire. But
the reclock `ly_for_comparison_line_153` is still 153 at dots 6-7. SameBoy writes
LYC=153 at `ly153 cfl0 lyfc=153` → fresh match → `GB_STAT_update` fires (SBWRITE
+ `SBLEVEL 0->1 lyc_line=1`). Fix: tier2, a late FF45 write whose value matches
the held `ly_for_comparison()` fires.

### Lever 3 — `tier2_minus1_gap` (the FF45 "weirdpoint")

Trace (`ff45_enable_weirdpoint_lcdoffset1_2`): LYC=5 (ly3), then the late FF45=6
weirdpoint lands `ly6 dot3` where `ly_for_comparison() == -1` (the line-start gap).
SameBoy writes at `lyfc=-1` → `GB_STAT_update` leaves `lyc_interrupt_line`
unchanged (no re-latch at the -1 gap) → NO fresh edge (SBWRITE `ly6 cfl0 lyfc=-1`,
no STAT_IRQ). slopgb's gambatte `target` treats dots 0-3 as Some(line=6)==value
→ spuriously fires (`got=2`). Fix: tier2, suppress the FF45 fire when
`ly_for_comparison() == -1` on visible lines 1-143. Line 153 EXCLUDED (its -1 gaps
carry the held LYC=153 latch + the lever-2 wrap fire SameBoy delivers — gating
the whole-suite +6/−1 over-broad version dropped `lyc153_late_ff45_enable_3` outE2).

### Lever 4 — line-1 carryover hold (the ly0→ly1 LYC=0 wrap)

This lever was BUILT for the named Row 3 (`lycwirq_trigger_ly00_stat50_lcdoffset1_1`)
and instead fixed `lyc0_late_ff45_enable_3` [Cgb] — the #11l CGB residual the
DMG-only gate gave up. The CGB analogue of #11l's line-start LYC-carryover hold,
restricted to **line 1** (the ly0→ly1 wrap) so it avoids the ly6/ly7 breaks #11l's
ungated CGB hold caused. Mechanism: slopgb's offset-shifted FF45=0 write leaves ly0
unmatched (line stays LOW through ly0 in the measurement frame), then RE-RISES at
the ly1 dot-0 carryover (`ly_for_comparison=line-1=0` matches LYC=0) — a spurious
`ly1 dot0` edge. SameBoy holds the line HIGH across ly153→ly0 (LYC=0 matched at
ly0 cfl0) and FALLS at ly1 cfl0 (`SBLEVEL ly1 cfl0 1->0 lyc_line=0`). A real LYC=0
always matches at ly0 first on SameBoy (lyfc=0 there), so no genuine fresh LYC=0
edge can exist at ly1 → the line-1 hold drops nothing SameBoy delivers.

## FLOORED (build-measured, C2 read-frame) — `lycwirq_trigger_ly00_stat50_lcdoffset1_1`

want E0, got E2. The line-1 carryover hold removed the spurious `ly1 dot0`
dispatch (the dispatch SET now matches SameBoy: ly144 + ly153, no ly0/ly1), but
the OCR stays E2. Root: the persisting `if=02` is the **legit LYC=153 IRQ**, which
slopgb dispatches at `ly153 dot6` vs SameBoy `ly153 cfl0` (6 dots earlier). The
test clears IF in that window, so SameBoy's early fire is cleared (E0) while
slopgb's late fire persists. Compounded: SameBoy reads FF0F NOWHERE at ly0/ly1
(its reads land at ly144); slopgb's deferred read lands `ly0 dot14` (offset-shifted
across the boundary). Both the LYC-153 dispatch dot and the read position diverge
from the lcd-offset CPU↔PPU alignment slopgb does not model → the mech-1 read-frame
/ global reclock = C2. NOT a clean tier2 slice.

## Remaining (next build-measure candidates / out of scope)

- DS line-153 (`lyc153_late_ff4{1,5}_enable_ds_*`, `lycwirq_*_ds_*`) — S6
  double-speed clock (the over-broad -1-gap version's +2 DS bonus rode with it;
  the DS grid needs the S6 unification).
- window `late_enable_afterVblank` / `late_wy` lcdoffset — render-level (#11g/#11p
  C2 floor).
- `dma/hdma` lcdoffset — S6 HDMA.
- VRAM render floor + OAM prewrite genuine floor (#11q).

## Tooling notes (verified live; /tmp was cold this session)

- SameBoy 1.0.2 rebuilt from a cold `/tmp` (git clone v1.0.2 +
  `/tmp/sbpatch.py` applying SBLEVEL/SBTRACE STAT_IRQ/SBMODE/SBREAD ff0f+ff41/
  SBWRITE ff45 per `stat-irq-trace.md`; `make tester`). `--cgb --length 4`.
- slopgb temp tracers (reverted after): `cycle.rs::write_deferred` SLOPGB
  wr{addr} ly/dot/val; `reclock.rs::stat_update_tick` SLOPGB lvl
  ly/dot/mfi/lyc_line/en/lvl (level transitions, the spurious-rise locator) +
  the dispatch tracer un-gated from `ly<144`.
- Two-bin: full 3524-CGB rowlist (`find gambatte -name '*.gb*'`), fixed
  `target/gbtr` vs stashed-revert `target/lint`, `comm` the FAIL row-ids.
