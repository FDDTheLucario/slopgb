# S7 (sub-M-cycle READ clock) is REFUTED — the smoking-gun reads are CO-TEMPORAL; the lever is the window-render trigger

2026-06-28 (#11ab). The goal's START — "S7: a sub-M-cycle (T-granular / eighth-grid)
READ clock so the smoking-gun pair `late_wy_FFto2_ly2_{1,2}` reads distinct modes" —
is **build-measured FALSE**. Direct SameBoy `cfl`+`dc` measurement proves the two reads
occur at the **IDENTICAL sub-dot instant**; the mode difference is entirely the
**window mode-3 extension** (the trigger-timing / render lever), NOT the read position.
A read clock — at ANY resolution, including the eighth-grid — cannot separate two reads
that happen at the same instant. This is the 5th wrong-direction lever the branch's
build-measure discipline has caught; do NOT chase the S7 read clock.

## The measurement (SameBoy tester, `--cgb --length 4`, `SBREAD ff41` cfl+dc)

| ROM (want) | SBREAD | unified = cfl*2+dc | SameBoy mode |
|---|---|---|---|
| `late_wy_FFto2_ly2_1` (3) | cfl=260 dc=0  | **520** | 3 |
| `late_wy_FFto2_ly2_2` (0) | cfl=261 dc=-2 | **520** | 0 |

SameBoy's display clock composes as `unified_half_dots = cycles_for_line*2 +
display_cycles` (validated against the steady SBMODE transitions for `_1`:
`(0,2)→2`, `(0,8)→8`, `(84,8)→176`, `(257,6)→520`, all monotonic). **Both reads land
at unified 520.** They are the SAME instant — `dc` is just the sub-dot remainder
(8 MHz ticks = half-dots), so `(cfl=260,dc=0)` and `(cfl=261,dc=-2)` are one point.

## What actually differs — the window mode-3 EXTENSION (per-frame SBMODE)

The measurement-frame (1-occurrence) SBMODE `vis=0` flip:

| ROM | measurement-frame mode-0 flip | unified |
|---|---|---|
| `_1` | cfl=263 dc=2 | **528** (window extended +8 half-dots = +4 dots) |
| `_2` | — (no measurement extension; steady cfl=257 dc=6) | **520** |

`_1` reads at 520 < 528 → mode 3 (window extended past the read). `_2` reads at 520 ≥
520 → mode 0 (no extension). **The discriminator is whether the window TRIGGERED and
extended mode 3 on the measurement line — not the read clock.**

## The window-trigger root — a WHOLE-M-CYCLE WY-write timing difference

`SBWWY`/`SBWYTRIG` traces (added to SameBoy `memory.c` GB_IO_WY + `display.c` wy_check):

| ROM | WY=2 write | wy_triggered | renders ly2? |
|---|---|---|---|
| `_1` | ly2 cfl=92 dc=0 | ly2 cfl=96  | YES → mode3 257→263 |
| `_2` | ly2 cfl=96 dc=0 | ly2 cfl=100 | NO  → mode3 stays 257 |

The WY writes are a **whole M-cycle apart** (cfl 92 vs 96, dc=0 both) — NOT sub-dot.
SameBoy's `wy_check` fires `cycles_to_check = 8 − (wy_check_modulo & 7)` after the
write (`display.c:1555`), then `wy_check()` sets `wy_triggered` iff `WY == current_line`
(`display.c:519`). `_1`'s earlier write makes `wy_triggered` true before the window
fetcher passes WX on ly2 → the window renders + extends mode 3; `_2`'s later write
sets `wy_triggered` too late for ly2 (renders from ly3). This is **whole-dot
resolvable** — no half-dot grid needed.

## Why the prior #11aa "smoking gun" mis-framed it as a read collapse

#11aa applied a BLANKET WY-latch fix (`wy_latch |= win_en && ly==eff.wy` on every
WY/LCDC write). That set `wy_latch` true for BOTH `_1` and `_2` (both write WY=2 at
ly2), so slopgb over-triggered the window for both → both slopgb reads then saw mode 3
at the same dot, and the ONLY remaining separator looked like a sub-dot read clock.
But that was an artifact of the over-aggressive fix: SameBoy does NOT trigger `_2`'s
window on ly2. The correct fix is a **precise** window trigger (so `_2` does not
extend), which is whole-dot, not a read clock.

## slopgb's actual failure (winmatch trace, both ROMs, current reclock HEAD)

At the ly2 window-match dot (slopgb dot 97, lx=0, wx=7), the WY-latch (`wy_ok`) and
window-enable (`en`) are **never both true simultaneously** for either ROM
(`wy_ok=false en=true` XOR `wy_ok=true en=false` across frames) → the window never
activates → both read mode 0. slopgb's discrete dot-quantized WY-write / LCDC-enable
commits land in a state the rising-edge `win_match && wy_ok && win_en` activation
misses. The fix is to make those commits' ORDERING relative to the match dot precise
(the C3 render-frame), so `_1` lands `wy_ok && en` true at the match and `_2` does not.

## The corrected path (supersedes the goal's S7 START)

1. **NO eighth-grid read clock.** The reads are co-temporal; the scaffold
   (`event_phase`/`lead_eighths`/`ACCESS_PHASE`) cannot and need not separate them.
   (The kernel pair already separates via the `mode_timeline` 2-dot anchor swing, also
   not a read clock.)
2. **The window-render trigger (C3 render):** make slopgb's `wy_ok`+`win_en`+
   window-fetch ordering match SameBoy's per-write `wy_check` so `_1` activates the
   window on ly2 and `_2` does not. Whole-dot.
3. **The +4 read frame (C3 frame):** slopgb reads BOTH at dot 260; SameBoy reads at
   unified 520 (≡ slopgb dot ~256 at the +4 interrupt-service offset). Even with the
   trigger fixed, `_1`'s extended exit (cfl263 → slopgb dot ~259) means slopgb's read
   at dot 260 ≥ 259 still reads mode 0. The read must land at the SameBoy-frame dot —
   the PPU-advance +4 (NOT `bus.tick`, di_timing) the goal already localizes.

Both (2) and (3) are whole-M-cycle render/frame levers — the C3 atomic reclock — with
NO sub-half-dot read clock. This removes S7 from the critical path entirely.
