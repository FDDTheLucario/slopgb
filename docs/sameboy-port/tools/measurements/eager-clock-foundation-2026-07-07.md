# The EAGER-CLOCK foundation — the census NO-GO overturned (2026-07-07, #11bv)

Task: "fix the architectural problem" (the thrice-refuted DMG dispatch/timer
wall that blocks the C3 flip). **Result: the wall is an artifact of the DEFERRED
CPU clock. Measured proof: the EAGER clock recovers the entire DMG blocker set
the census (#11bt) declared unfixable.** No code shipped yet — this map pins the
architecture + the build plan.

## The insight

The tier2 reclock defers the WHOLE CPU clock (`read_deferred`,
`interconnect/cycle.rs`): every read AND the interrupt dispatch sample at the
M-cycle leading edge (cc+0). This fixes the CGB leading-edge read-frame rows but
moves dispatch off cc+4 → the DMG dispatch-COUNT + timer-completion rows break
(they want production's cc+4 frame). All three refutations (#11ai C2ADV, #11br
fold, #11bs eager-split) tried to move DISPATCH back to cc+4 while keeping the
deferred cc+0 READS → incoherent (dispatch and ISR reads in different frames).

**The un-tried foundation:** the EAGER clock (production `tick_machine`, dispatch
+ WRAM reads at cc+4 — count-safe, mooneye-safe) + the CGB read-frame fix as a
cc+0 VALUE PEEK per PPU register (the `leading_edge_sample` pattern, cycle.rs:16,
already proven for FF41). Dispatch NEVER leaves cc+4 → the DMG counts are safe by
construction; only the PPU-register read VALUES back-date. This is SameBoy's
actual model (exact-T CPU + `GB_display_sync` returns the value; the CPU clock is
never shifted).

## The measurement (branch `finish-port-halfdot` off main = #11bu tree)

Three-way gambatte-OCR two-bin, `flagon_probe` OFF (production) / ON (tier2
deferred) / LE (`set_leading_edge_reads`, the eager clock + StatUpdate engine).
Rowlists `scratchpad/{cgb,dmg}_rowlist.txt` (3422 rows). Reproduces the census:
CGB OFF 486 / ON 291; DMG OFF 103 / ON 116.

**DMG: the eager clock (LE) recovers 86 rows the deferred clock (ON) breaks —
== the census's 79 "unfixable" blockers:**

| family recovered by eager clock | n | census class |
|---|---:|---|
| tima | 45 | S6 timer-completion (the whole class) |
| m2int_m0irq | 16 | dispatch-chained read-frame |
| enable_display | 6 | dispatch-COUNT |
| window | 5 | render/window |
| m0enable / m0int_m0irq / lyc0int_m0irq | 8 | line-start STAT read-frame |
| sprites / serial / oamdma / miscmstatirq / lycEnable | 6 | IF-lifecycle |

The deferred clock reads FF0F/counters at cc+0 (one M-cycle early → timer IF not
yet landed, dispatch mis-counted); the eager clock reads at cc+4 = production =
PASS. **This is the entire "thrice-refuted" DMG wall, recovered by NOT deferring
the clock.**

**The cost (LE's wrong peek):** LE also BREAKS 134 rows production passes
(window 51, halt 28, enable_display 9, ly0 8, m1 5, accessibility vram_m3/
oam_access 6, …) — these want the tier2 cc+0 read frame, but LE's peek uses the
DMG/mooneye debug back-date (mode3_entry 80, StatUpdate engine) which is WRONG
for them. CGB LE = 578 fail (+92 vs OFF, 247 flip-BUGs) for the same reason.

## The architecture (eager clock + CORRECT cc+0 peeks)

LE proves effect (1): the eager clock recovers the DMG blockers. LE's flaw is
effect (2): the naive peek frame. The build = eager clock + the tier2 READ LAWS
applied as cc+0 value peeks (extend `leading_edge_sample` from FF41 to
mode/LY/OAM/VRAM/palette/FF0F; keep the render/window/accessibility laws, hosted
at the eager pre-`tick_machine` cc+0 point where the PPU sits at this M-cycle's
leading edge — the SAME position the deferred read samples). Dispatch stays cc+4.

**Targets:** CGB two-bin → 291 (match tier2, via correct peeks); DMG two-bin →
≤103 (recover the 86, keep production counts); mooneye 91/91 ON+OFF; gbtr golden
byte-identical OFF. The 134 LE-breaks are the CGB-style rows the correct peeks
fix; the 86 recoveries are kept because dispatch stays eager.

## EV v0 measured — the read frame must be 80 (LE), NOT 84 (tier2)

Built `set_eager_value` = the eager clock + the FULL ppu tier2 laws (frame 84 +
accessibility/render), a cc+0 FF41 peek, dispatch cc+4 (`SLOPGB_PROBE_EV` /
`SLOPGB_MOONEYE_EAGER`). Measured:

- **DMG recovers 87 (incl. all 45 tima)** — the eager clock foundation confirmed
  a third time.
- **CGB 608 fail (WORSE than LE 578)** — the tier2 accessibility/render/window/
  speedchange laws are calibrated to the DEFERRED machine position + write-commit;
  on the eager clock they misfire (breaks by family: window 67, speedchange 31,
  sprites 29, halt 28, lcd_offset, m2int_m3stat).
- **mooneye EV 90/91 — `intr_2_mode0/mode3_timing` HANG (B=42, all models).**
  DECISIVE: the tier2 read frame (mode3_entry **84**, un-back-dated) makes the
  cc+0 FF41 read 4 dots off the cc+4 dispatch → incoherent → `intr_2` detects it.
  This is the refutations' incoherence, mirror-imaged (they broke COUNTS with
  cc+4-dispatch ∧ cc+0-reads; EV breaks intr_2 with the same split at frame 84).
- golden_fingerprint PASS (production byte-identical — scaffold is golden-safe).

**The frame law (the key correction):** on the EAGER clock (dispatch cc+4), the
read frame must be the LE **back-dated 80** frame (`mode3_entry_dot`
`leading_edge && !tier2` branch — "observationally neutral", `intr_2` passes
LE-only, stat_irq.rs:73-92), NOT the deferred-clock **84**. The 84 is a
DEFERRED-clock accounting artifact (the read pays the previous M-cycle's debt).
SameBoy's FF41 read genuinely sits 4 dots before the following dispatch (different
M-cycle positions) = exactly the LE back-date. So:

**Refined architecture — the correct foundation is LE (frame 80), and the port =
add the tier2 accessibility/render/window laws to the LE base, re-calibrated to
the frame-80 (eager) frame.** NOT a new flag on top of tier2. The tier2 laws are
the REFERENCE for WHAT to compute, but their frame constants (84, deferred
write-commit) must be re-derived to the LE frame. This is the multi-session
re-host; EV v0 (frame 84) is REVERTED (the wrong base). LE already carries the
frame-80 read laws it has (mode3_entry 80, glitch 74); it LACKS the
accessibility/render/window laws — that gap (the 134 DMG / ~290 CGB LE-breaks) is
the port surface. Each family lands as an `leading_edge_reads && !tier2_reclock`
law arm (the existing LE fork), measured on the LE two-bin, dispatch never moving.

**Why this converges where the deferred clock could not:** the DMG dispatch/timer
rows are production-correct on the eager clock (nothing to fix — just don't break
them); the CGB fixes port onto the eager/80 frame; dispatch never leaves cc+4 so
intr_2 + the counts + mooneye hold. The flip becomes CGB +232 / DMG +0 (a pure
gain) instead of tier2's CGB +232 / DMG −98 (net loss) → GO instead of NO-GO.

## The 80-vs-84 conflict → the true architecture is EAGER CLOCK + HALF-DOT READ

Attempting the re-host surfaced the deep constraint. On the eager clock the FF41
read-frame faces a genuine conflict:
- **intr_2 (mooneye) wants frame 80** — the production-coherent back-date; the
  cc+0 read must reproduce the cc+4 dispatch's mode detection (EV frame 84 hung
  intr_2_mode0/mode3, B=42).
- **the CGB two-bin rows want frame 84** — SameBoy's actual mode boundary, 4 dots
  later than production; this is what the deferred clock gives them (→ 291).

Whole-dot cannot be both 80 and 84 for the same read. This is NOT a dead end — it
is the precise statement of why the port ultimately needs the **half-dot read**
(`Ppu::read_pos_hd`, HALFDOT Part B): SameBoy reads at the exact HALF-DOT, a
sub-dot position slopgb's whole-dot grid rounds to 80 OR 84. The half-dot read
resolves the exact value that is coherent with BOTH intr_2 (dispatch) AND the CGB
mode boundary.

**The synthesis (supersedes the deferred-clock HALFDOT-BUILD-PLAN):** the true
SameBoy model = **EAGER clock (dispatch cc+4) + half-dot-resolved PPU reads**. The
prior HALFDOT plan built Part B (half-dot read) on the DEFERRED clock — which
self-inflicts the DMG regression the half-dot read then can't undo (dispatch is
in the wrong frame). On the EAGER clock the DMG side is free (production-correct),
and the half-dot read resolves the CGB 80/84 conflict. **Eager clock + half-dot
read is the coherent target; the deferred clock was the wrong base all along.**

OPEN (measure next, do not assume): whether a *mixed whole-dot* frame suffices for
a useful subset — intr_2-relevant boundaries (mode-2→3 entry, mode-0 exit) at 80
while CGB-relevant boundaries at 84, IF they are distinct reads. If they overlap,
the half-dot read is required. Position-based families (accessibility, render
write-commit, window length) do NOT hit the 80/84 conflict — they re-host onto the
eager clock directly (port the tier2 stage_write dots to `Bus::write`; gate the
blocking/render laws `tier2 || eager_value`) and are the tractable first slices.

## Build plan (family order, each an eager_value `leading_edge && !tier2` arm)

1. **Plumbing:** `eager_value` flag (interconnect + ppu), eager clock + dispatch
   cc+4, `set_eager_value`, probe `SLOPGB_PROBE_EV` + mooneye `SLOPGB_MOONEYE_EAGER`.
2. **Write-commit (render):** port `write_deferred`'s per-register stage_write dots
   to the eager `Bus::write` under eager_value → recovers window/speedchange/
   sprites (the 51+31+29 render breaks). No read-frame/dispatch touch → intr_2 safe.
3. **Accessibility peek:** route OAM/VRAM/palette reads through the cc+0 blocked
   verdict (pre-`tick_machine`) → oam_access/vram_m3 (~16). Position-based, no 80/84.
4. **Read-frame:** the FF41 mode / LY reads — the 80/84 crux. Measure mixed-frame
   first; if it doesn't separate, land the half-dot read (Part B) on the eager clock.
5. **DS + halt + speedchange:** re-host the DS/wake laws to the eager frame.
6. **Flip C3:** CGB +232 / DMG +0, rebaseline the ~37 CGB SameBoy-fail legs.

## Reproduction

`BIN=target/probe/release/deps/gbtr-*`; `SLOPGB_ROWLIST=<abs>/scratchpad/
{cgb,dmg}_rowlist.txt $BIN --ignored gambatte::flagon_probe::flagon_probe
--nocapture` (+ `SLOPGB_PROBE_OFF=1` / `SLOPGB_PROBE_LE=1`). ALWAYS the exact test
path (`--ignored flagon_probe` matches 3 probe tests → they race + corrupt
counts). Fail lists: `scratchpad/{dfail,fail}_{off,on,le}.txt`.
