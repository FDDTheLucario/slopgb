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

## Follow-up — the slopgb failure is render + FRAME-PHASE, both whole-dot (no read clock)

Ordered traces (slopgb `wff40`/`wff4a` write tracer added to `write_deferred`;
SameBoy `SBWLCDC`/`SBWWY`/`SBWYTRIG`) pin slopgb's two whole-dot failures on the
`late_wy_FFto2_ly2_1` measurement frame:

**1. LCDC window-enable frame-phase (off-by-one).** Both emulators toggle LCDC
`0xb1` (win ENABLED, ly151 of a setup frame) ↔ `0x91` (win disabled, ly0). SameBoy's
`0x91`-disable lands on the NEXT setup frame, AFTER the measurement read — so the
measurement frame stays `0xb1` (window enabled) through ly2, and `wy_check` (gated on
WIN_ENABLE) fires. slopgb applies the `0x91`-disable on ly0 of the SAME measurement
frame, BEFORE the read → window DISABLED at ly2 → `win_match` fires at dot97 with
`en=false` → never activates → reads bare mode 0. This is a **frame-phase off-by-one**
(the test's setup↔measurement loop is one frame out of phase under the current
reclock frame) = the goal's "CGB-OCR frame-alignment", not a window-machine bug.

**2. WY-write→trigger→which-line render (the `_1`/`_2` separator).** Given the window
enabled, the SameBoy `_1`/`_2` split is purely the WY-write M-cycle timing:
`_1` writes WY=2 @ly2 cfl92 → `wy_triggered` cfl96 → renders ly2 (mode3 +6);
`_2` writes WY=2 @ly2 cfl96 → `wy_triggered` cfl100 → renders ly3 (ly2 unextended).
Both have the window enabled and both trigger; the 1-M-cycle write difference decides
whether the trigger beats the ly2 window-fetch dot (≈ slopgb dot97). Whole-M-cycle.

**Conclusion reinforced:** the smoking-gun config is gated by (1) a frame-phase
off-by-one (LCDC-enable on the wrong frame) and (2) a whole-M-cycle WY-write→render-line
trigger — BOTH render/frame, both whole-dot. There is no sub-dot read component
anywhere in the chain. The C3 atomic frame reclock (which co-moves the whole-frame
phase) is the lever; S7 is not on the path.

## GENERALIZED — the sub-dot read-clock premise collapses across EVERY measured family

The S7 refutation is not specific to `late_wy`. Measured `SBREAD ff41` cfl+dc for the
canonical "read-collapse" families the branch attributed to a sub-M-cycle read clock
(unified = `cfl*2 + dc`):

| family (geometry) | `_1` want3 (cfl,dc)→unified | `_2` want0 (cfl,dc)→unified | relation |
|---|---|---|---|
| kernel m2int/m0int (scx0) | (256,0)→**512** | (261,-2)→**520** | 1 M-cycle apart |
| m2int_scx2 (bare) | (256,0)→**512** | (261,-2)→**520** | 1 M-cycle apart |
| late_scx4 (bare) | (260,0)→**520** | (261,-2)→**520** | co-temporal |
| late_wy (window) | (260,0)→**520** | (261,-2)→**520** | co-temporal |

**Every want-0 read lands at unified 520; `dc` is only ever 0 or −2** — the lazy-advance
remainder (`gb->display_cycles`, exactly as #11x already established: "dc is a
LAZY-ADVANCE accumulator, NOT a sub-dot"). The two reads of a pair are EITHER:
- **co-temporal (both 520)** — the mode difference is a RENDER mode-3 length/extension
  difference (late-SCX write timing, window trigger), driven by a whole-M-cycle
  register-write timing; OR
- **a whole M-cycle apart (512 vs 520)** — slopgb collapses them only because its
  deferred ISR read frame is mis-framed by the +4 interrupt-service offset; the fix is
  the whole-M-cycle +4 PPU-advance read frame.

**No measured family is sub-half-dot.** The eighth-grid scaffold
(`event_phase`/`lead_eighths`/`ACCESS_PHASE`/`edge_eighth`) is NOT needed for the C2/C3
residual and can be retired. The branch's recurring "read-collapse / sub-M-cycle read
clock" diagnosis (INC1 2026-06-13 onward, and the goal's S7 START) is a **cfl-only
measurement artifact** — reading `cfl` 260 vs 261 as a "1-dot read difference" while
`dc` (0 vs −2) makes them the SAME instant. #11x flagged dc-as-lazy-advance; this
generalizes it to the whole read-collapse class.

## Net consequence for Phase B

The remaining C2/C3 work is entirely WHOLE-M-CYCLE:
1. the **+4 interrupt-service read frame** (PPU-advance, NOT `bus.tick` — di_timing),
   which separates the kernel/m2int_scx2 reads (512 vs 520);
2. the **render mode-3 length / window trigger / frame-phase** co-move, which fixes the
   co-temporal pairs (late_scx4 / late_wy);
3. the counter-pinned mask re-derive + gambatte rebaseline + flip.

There is no sub-half-dot architecture on the path. S7 (the eighth-grid read clock) is
removed from Phase B entirely — a significant de-risking of the atomic lift.

## slopgb-side confirmation — the residual is RENDER mode-3 length, not a read frame

slopgb reads (current reclock HEAD, `flagon_probe` + `SLOPGB_S5DBG`):

| ROM | slopgb read | result |
|---|---|---|
| `m2int_scx2_m3stat_1` (3) | ly1 **dot252** mode 3 | PASS |
| `m2int_scx2_m3stat_2` (0) | ly1 **dot256** mode 0 | PASS |
| `late_scx4_1` (3) | ly1 **dot256** mode 3 | PASS |
| `late_scx4_2` (0) | ly1 **dot256** mode 3 | FAIL (wants 0) |

- **m2int_scx2 already PASSES** — slopgb reads `_1`/`_2` a whole M-cycle apart
  (dot252 vs dot256), matching SameBoy's 512 vs 520. The whole-M-cycle case is
  resolved (the memory's "m2int_scx2 collapses" note is stale, pre-#11n/#11z).
- **late_scx4 collapses** at dot256 (both mode 3) — the CO-TEMPORAL case. SameBoy
  (SBMODE measurement frame): `_1` extends the mode-3 exit to unified **528**
  (cfl261/dc6, +4 dots = SCX&7=4 latched), `_2` stays **520** (cfl257, no latch).
  Both reads at 520 → `_1` mode 3 (520<528), `_2` mode 0 (520≥520). slopgb applies the
  SAME mode-3 length to both → both mode 3.

The fix is a **render mode-3 LENGTH** that responds to the late-SCX write timing
(latch SCX&7 only if the write lands before the fetch samples it) — the exact analogue
of the late-WY → window-trigger lever, both whole-M-cycle register-write timing. This
is the render half of C3; no read clock, no eighth grid. The complete read-collapse
residual is now mapped: whole-M-cycle apart (read frame, mostly already resolved) OR
co-temporal (render length/trigger, the remaining work).

## late_scx4 — the precise render-length spec (SCX&7 latched at mode-3 START)

SameBoy `SBWSCX` (FF43) write timing for the SCX=4 write on the measurement line:

| ROM | SCX=4 write (cfl,dc)→unified | mode-3 start | latched? | exit |
|---|---|---|---|---|
| `late_scx4_1` (3) | (89,-2)→**176** | 176 | YES (write AT mode-3 start) | 528 (+4) |
| `late_scx4_2` (0) | (92,0)→**184** | 176 | NO (8 half-dots = 4 dots late) | 520 |

Mode-3 starts at unified 176 (SBMODE vis=3 @ cfl84/dc8 = 176). **SameBoy samples SCX&7
for the mode-3 length (the fine-scroll discard) AT mode-3 start.** `_1`'s write lands at
176 (latched → +4); `_2`'s at 184 (after → SCX&7=0, no extension). The writes are a
whole M-cycle apart at the boundary.

slopgb OVER-extends `_2`: it uses the 2-dot-delayed `eff.scx` (which captures `_2`'s
late write) for the mode-3 length → both `_1`/`_2` flip at slopgb dot258 (measurement
frame, +4 vs steady dot254) → both read dot256 mode 3. **Fix:** latch SCX&7 for the
mode-3 length at mode-3 start (so a write after mode-3 start does not extend the current
line). This shifts `line_render_done` → counter-pinned (int_hblank/hblank_ly_scx) →
breaks byte-id OFF → atomic with C3 (the render half). Precise, whole-M-cycle, no read
clock. This is the render-length analogue of the late-WY → window-trigger lever; both
co-temporal-read families reduce to "latch the late register write at the
fetch-relevant dot."

## FINAL refinement — the +4 frame is ALSO not a lever; the residual is PURELY render (+DS)

Two further build-measured findings collapse the goal's C3 "frame co-move (+4 PPU-advance)"
as well:

**1. The +4 read frame is REDUNDANT (already absorbed by #11z).** The kernel passes because
slopgb's whole frame is internally consistent — its in-ISR read AND its bare-line mode-3
exit are BOTH 4 dots earlier than SameBoy's, so the read-vs-exit RELATIONSHIP (and thus the
mode) is correct (kernel m2int_1 reads dot252 < exit dot254 → mode 3 ✓). The +4 is a
LABEL offset, not a bug. The single-speed window m2int_wx<A0 rows that DID need accounting
are already handled by #11z's exit law (`259+SCX = SBex−4`), which back-dates the boundary
into slopgb's read frame. Implementing the +4 PPU-advance would shift the read out of that
consistent frame and BREAK the kernel — it is not a remaining lever.

**2. The 79 window fails bucket as PURE RENDER + DS (NOT frame):**
| bucket | ~count | mechanism |
|---|---|---|
| `late_wy_*` | ~18 | render: WY-trigger timing + LCDC window-enable frame-phase |
| `m2int_wxA6/wxA5_*` | ~12 | short/off-screen window render (excluded by #11z's `wx<0xA0` gate) |
| `m2int_wx*_ds` | ~8 | double-speed read grid (S6/S7) |
| `late_scx_*` / `late_disable_*` | ~6 | render: SCX&7-latch / window-abort mode-3 length |

The single-speed `m2int_wx<A0` rows are GREEN (#11z). The residual is the RENDER MODEL
(window activation + mode-3 length responding to late WY/SCX/LCDC writes) plus the DS read
grid — exactly the "tier2 parallel window-length model + vis-HOLD primitive" #11g flagged
as not-yet-built.

## Phase B residual — the COMPLETE, build-measured map (no false levers left)

| former "lever" | verdict |
|---|---|
| S7 sub-M-cycle read clock | REFUTED (reads co-temporal / whole-M-cycle; #11ab) |
| +4 ISR read frame (PPU-advance) | REDUNDANT (absorbed by #11z; breaks the kernel) |
| eighth-grid scaffold | UNNECESSARY (retire) |

The ONLY remaining work to the flip is the **render model** (window activation: late-WY
trigger + LCDC frame-phase; mode-3 length: late-SCX latch at the discard point, window
abort) for the ~248 CGB + DMG "BUG-fix" rows, plus the **DS read grid** (S6/S7), then the
gambatte rebaseline (39 genuine floors + 6 DIFF) + flip + C4. This is a render-model port,
multi-session in scope, but now with ZERO frame/sub-dot architecture and a precise per-bucket
work list. Both the goal's prescribed START (S7) and its C3 frame lever (+4) are
build-measure removed; the path is purely render + rebaseline.
