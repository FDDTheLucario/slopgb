# HALFDOT Part-B WRITE half-dot commit — REFUTED: the DMG FF41/FF45 half-dot write-commit is INERT (reproduces no-borrow); the write path has no meaningful `dhalf` for the STAT/LYC engine, so the ~10 DMG write-frame bar rows are a whole-dot pair-shuffle FLOOR, not a sub-dot straddle (2026-07-11, #11dr)

Base: `finish-port-halfdot @ 951d0b7`. Task (#11dr, build-or-refute): extend the
#11dd whole-dot WriteCpu borrow to a HALF-dot commit (`tick_half` once → D+½)
for DMG FF41/FF45, to separate the two straddling siblings #11dq §3(b2)
exhibited — `lyc153_late_m1disable_3` (recovered by the whole-dot borrow) and
`m0enable/lycdisable_ff41_2` (dropped by it) — predicting +10/−0 vs the whole
dot's +10/−8. **Answer: REFUTED. The half-dot commit is INERT — EV DMG stays
EXACTLY 54 (byte-identical to no-borrow), recovering 0 and dropping 0.** The
eager PPU STAT/LYC engine advances only at whole-dot `tick()`; `dhalf` drives
only the render strobe, never the STAT engine, so committing FF41 at D+½ is
engine-indistinguishable from committing at D. The two siblings do NOT straddle
a sub-dot boundary — they share ONE whole-dot commit boundary with a BINARY
OLD/NEW lever and want OPPOSITE outcomes. No write-commit timing on the
whole-dot STAT engine separates them. Code env-gated + REVERTED; tree
byte-identical (`git diff 951d0b7 -- crates/` empty), `golden_fingerprint` ok
(42.24s).

## Baselines reproduced (exact, at 951d0b7)

`flagon_probe` two-bin, `scratchpad/{cgb,dmg}_rowlist.txt`:

| frame | CGB | DMG |
|---|---:|---:|
| OFF (`SLOPGB_PROBE_OFF`) | 486 | — |
| EV (`SLOPGB_PROBE_EV`) | **295** | **54** |
| tier2 (`SLOPGB_PROBE_RECLOCK`) | **291** | 116 |

`golden_fingerprint` byte-identical.

## The build — a HALF-dot WriteCpu borrow

Extended the #11dd whole-dot borrow (`interconnect/bus.rs::write`,
`eager_wr_borrow`: 2×`tick_half` → commit at D+1, repay a whole dot). For DMG
FF41/FF45 the new `eager_wr_borrow_half` path did ONE `tick_half` (advance to
D+½, `dhalf=1`, dot NOT completed — only `strobe_tick` ran), then
`write_no_tick`, then repaid ½ dot on the next `tick_machine` (cc 1 ticks 1
half-dot instead of 2). Correctly built (verified: the write commits at
`dhalf=1`, phase restored over the M-cycle — 4 dots preserved). The mechanism
IS a half-dot write-commit; it is NOT the refuted #11co half-dot READ (the read
frame / compensation tower / `read_pos_hd` were untouched).

## The sibling-pair trace (`run_gambatte`, EV, DMG, per-mode)

`off` = half-borrow disabled (= no-borrow EV base); `half` = the D+½ commit;
`whole` = the #11dd/#11dq-b2 D+1 commit. OCR digit (16-frame):

| ROM (DMG want) | EV/off | EV/half | EV/whole | tier2 |
|---|:--:|:--:|:--:|:--:|
| `lycEnable/lyc153_late_m1disable_3` (want **E0**) | E2 ✗ | **E2 ✗** | **E0 ✓** | E0 ✓ |
| `m0enable/lycdisable_ff41_2` (want **2**) | 2 ✓ | **2 ✓** | **0 ✗** | 0 ✗ |

FF41 disable-write commit positions (`SLOPGB_DR_TRACE`, half mode):
- target `lyc153_late_m1disable_3`: the mode-1 disable (`FF41=0x40`) commits at
  **ly=153 dot=4**.
- drop-sibling `lycdisable_ff41_2`: the mode-0 disable (`FF41=0x08`) commits at
  **ly=1 dot=252**.

Read across the trace: **`half` == `off` in every column** — the half-dot
commit reproduces the no-borrow result on BOTH siblings (and, globally, EV DMG
= 54 = the no-borrow count, so ALL rows are inert, not just these two). Only
`whole` moves anything, and it moves BOTH siblings the SAME direction
(target E2→E0 recovered; sibling 2→0 dropped) — the #11dd +10/−8 pair-shuffle,
reconfirmed.

## Why the half-dot is inert (code-anchored)

`Ppu::tick_half` (`ppu/engine.rs`): the FIRST half of a dot (`dhalf 0→1`) does
**no structural work** — it only advances `strobe_tick` (the render write
strobe, `eager_value`) and returns 0; the whole-dot body (`tick()`: mode
transitions, LY/LYC compare, STAT-source evaluation, IF rises) runs ONLY on the
SECOND half (`dhalf 1→0`, `dot_completed()`). So:

- **no-borrow (commit at D):** `write_no_tick` at D; the next `tick_machine`
  completes dot D+1 with the NEW `eng_stat` → the D+1 rise uses the **NEW** value.
- **whole-dot borrow (D+1):** 2×`tick_half` completes dot D+1 (rise folded)
  BEFORE `write_no_tick` → the D+1 rise uses the **OLD** value.
- **half-dot borrow (D+½):** 1×`tick_half` (no dot completes, only strobe);
  `write_no_tick` at D+½; the borrowed dot D+1 completes in the next
  `tick_machine`'s cc 1 with the NEW `eng_stat` → the D+1 rise uses the **NEW**
  value = **identical to no-borrow.**

The STAT engine has exactly TWO reachable outcomes — {D+1 rise sees NEW} ≡
{off, half} and {D+1 rise sees OLD} ≡ {whole}. The half-dot creates no third,
intermediate outcome because there are no sub-dot STAT events: `dhalf` is a
RENDER grain (strobe), never a STAT/LYC grain. **The write path has no
meaningful `dhalf` for the interrupt engine** — the escape clause #11dr
anticipated.

## Why it is the floor (the pair-shuffle, restated)

`lyc153_late_m1disable_3` NEEDS the OLD outcome (late commit, E0);
`m0enable/lycdisable_ff41_2` NEEDS the NEW outcome (early commit, 2). Both are
the same FF41 disable write matching the same scope, so any global borrow flag
sets both the same way — the whole-dot forces OLD for both (target ✓, sibling
✗), off/half force NEW for both (target ✗, sibling ✓). #11dq §4's premise that
"each sibling resolves to its own half-dot side" is **empirically false**:
there is no half-dot side. The correct commit phase is line-context-dependent
(the target's line-153 rise wants the T-late commit; the sibling's line-1
mode-0 rise wants the T-early commit), and on SameBoy that context is resolved
because `GB_CONFLICT_WRITE_CPU` lands the value 1 T past the boundary on a STAT
engine clocked at true 8-MHz half-dots, where each write's commit lands at its
exact sub-dot vs THAT line's rise. slopgb's whole-dot STAT engine offers only a
global {early, late} lever and cannot replicate the per-line sub-dot phase.

This is the FIFTH independent refutation of a scoped whole-dot commit/dispatch
retime on the eager clock (after #11br incoherent-fold, #11cq stat_late
pair-shuffle, #11cl inert/corrupt, #11dq dispatch-scoping), now with the
write-commit half-dot directly measured inert.

## The only remaining lever (not this session)

Separating these rows requires the STAT/LYC ENGINE itself on the half-dot grid
(evaluate mode/LYC/IF per 8-MHz half-dot so the two siblings' rises land at
distinct half-dots and a per-write 1-T commit resolves emergently) — a Part-A
STAT-engine reclock, a DIFFERENT and larger rewrite than a write-commit borrow,
and the read-side twin (#11co) is already refuted strictly worse. Until then the
~10 DMG write-frame rows + the 4 targets here are the confirmed **partial-flip
FLOOR** (#11dq); #11dd's whole-dot +10/−8 is the best a whole-dot commit lever
reaches, and its −8 SameBoy-pass drops keep it un-shippable under the golden law.

## Gates (all hold; tree byte-identical, no code shipped)

1. `git diff 951d0b7 -- crates/` **empty** (field + both borrow branches + the
   throwaway trace/mode knobs all reverted).
2. `golden_fingerprint` ok (42.24s, default flags-off build).
3. EV CGB 295 / EV DMG 54 / tier2 CGB 291 / tier2 DMG 116 reproduced exactly;
   the half-dot borrow held EV DMG at 54 (inert, +0/−0) and EV CGB at 295.
4. interconnect/engine reclock defaults NOT flipped; no push; parent branch
   untouched.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/pbw
cargo test -p slopgb-core --test gbtr --release --features port_probe --no-run
BIN=$(ls -t target/pbw/release/deps/gbtr-* | grep -v '\.d$' | head -1)
run(){ SLOPGB_ROWLIST=$PWD/scratchpad/$1 SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 \
       $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['; }
run dmg_rowlist.txt   # 54 (unchanged by the inert half-dot borrow)
run cgb_rowlist.txt   # 295
# sibling trace (re-add the reverted borrow + SLOPGB_DR_MODE=off|half|whole
# + SLOPGB_DR_TRACE knobs in bus.rs, then):
#   run_gambatte --features port_probe <rom> dmg, SLOPGB_EAGER=1 SLOPGB_DR_MODE=<m>
#   target lyc153_late_m1disable_3: off/half=E2, whole=E0 (FF41=0x40 @ ly153 dot4)
#   sibling lycdisable_ff41_2:      off/half=2,  whole=0  (FF41=0x08 @ ly1 dot252)
```
