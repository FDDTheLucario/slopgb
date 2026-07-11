# HALFDOT mode/LYC FSM Stage 2 — the coupled landing is REFUTED by DIRECT MEASUREMENT: the one buildable coupled piece (the line-153 LYC re-latch back-date, piece 2) SEPARATES the #11dr target pair CLEAN (both `_3` rows recover, siblings hold, golden byte-identical) but STRICT-SHUFFLES the broader line-153 family — DMG net +3 (recovered 9 / dropped 12), CGB net +17 (recovered 4 / dropped 21). The dropped rows (`lyc153int_m2irq_2`/`_late_retrigger_2`, the `window/late_wy` cluster, CGB `gdma_cycles`) consume the dot-6 latch the `_3` rows needed at dot-4 — proving the family members' LYC rises land on DISTINCT sub-dot phases that a WHOLE-dot window shift cannot serve simultaneously. Stage 1's reasoned "the 4 pieces are coupled; none alone separates the pair without shuffling" is now a MEASURED number. No code shipped; tree byte-identical (`git diff 243870e -- crates/` empty). (2026-07-11, #11dv)

Base: `finish-port-halfdot @ 243870e` (Stage 1 = #11du @ this commit).

## TL;DR — outcome (c), decisive measured refutation

The task's three outcomes were: (a) SHIP wall-1 rows, (b) SHIP byte-identical
infra + map the obstacle, (c) REFUTE (coherent coupled landing still
pair-shuffles — numbers). **This is (c), sharpened to a number.** The one piece
of the coupled FSM that IS buildable byte-identical in a bounded edit — the
line-153 `ly_for_comparison` LYC re-latch back-dated to its coincidence dot
(piece 2) — recovers the target pair but regresses steady-state on BOTH models.
It is NOT shipped (steady-state must only go DOWN; this raises it). The residual
requires the full HALF-dot resolution of the LYC re-latch coupled with the FF41
commit + flip half-dots (pieces 1/3/4) — the multi-session convergence Stage 1
scoped, now with the whole-dot floor measured.

## Baselines reproduced (exact, at 243870e)

| metric | value | gate |
|---|---:|---|
| `golden_fingerprint` | 9020 cases match | byte-identical ✓ |
| EV DMG (`SLOPGB_PROBE_EV`, `dmg_rowlist.txt`) | fail **54** | steady-state floor |
| EV CGB (`cgb_rowlist.txt`) | fail **295** | steady-state floor |

Sibling-pair trace confirmed at THIS base (`run_gambatte --features port_probe`,
`SLOPGB_EAGER=1`, `SLOPGB_S5DBG=1`):

```
TARGET  lyc153_late_m1disable_3 (dmg, want E0): dispatch ly=153 dot=6 mfi=1 lycln=1  → OUT E2 ✗
SIBLING lycdisable_ff41_2       (dmg, want 2 ): dispatch ly=1   dot=254 mfi=0 lycln=1 → OUT 2  ✓
```

The target's spurious edge is the LYC-source rise at **ly153 dot6** — exactly the
first dot `ly_for_comparison_line_153_at` returns 153 (DMG/CGB-C SS window
`6..=7`). The VBLANK-disable FF41 commit lands 2 dots earlier at **ly153 dot4**
(Stage-1 `ff41commit` trace). SameBoy holds the line continuously HIGH: on the
8-MHz grid the LYC=153 re-latch coincides with the VBLANK-disable, so the mode-1
fall fuses into the LYC rise → no fresh 0→1 edge → E0. slopgb's whole-dot engine
separates the drop (dot4) from the rise (dot6) → spurious edge → E2.

## The experiment E1 — the coupled piece that IS buildable (piece 2 alone)

The #11dr sibling pair is on DIFFERENT lines — TARGET on line **153**, SIBLING on
line **1** — so a **line-153-specific** LYC re-latch shift is NOT the uniform
write-commit shift Stage 1 refuted (#11du: "a uniform half-dot write-commit
shift is a strict pair-shuffle"). Back-dating ONLY the line-153 LYC window leaves
the line-1 sibling's mode-3→0 flip at dot254 untouched — the two rises separate
**structurally**, not by a uniform slide. This is the one piece of the coupled
FSM (piece 2, the LYC re-latch) that can be built as a bounded, golden-safe edit.

Edit (`ppu/stat_irq/reclock.rs`, `ly_for_comparison_line_153_at`,
`eager_value`-gated so tier2 + production keep the dot-6 table → byte-identical):

```rust
} else if self.eager_value {
    // back-date the line-153 LYC=153 re-latch to dot 4 (coincide the
    // VBLANK-disable FF41 commit at ly153 dot4)
    match at_dot { 0..=3 => -1, 4..=7 => 153, 8..=11 => -1, _ => 0 }
} else {
    // DMG / MGB / CGB-C SS: GB_SLEEP(14,4) → first set dot 6 (unchanged)
    match at_dot { 0..=5 => -1, 6..=7 => 153, 8..=11 => -1, _ => 0 }
}
```

### The pair SEPARATES clean (piece 2 works FOR the pair)

| row | model | want | base | E1 |
|---|---|---|---|---|
| `lyc153_late_m1disable_3` | DMG | E0 | E2 ✗ | **E0 ✓** |
| `lyc153_late_enable_m1disable_3` | DMG | E0 | E2 ✗ | **E0 ✓** |
| `lycdisable_ff41_2` (sibling) | DMG | 2 | 2 ✓ | 2 ✓ (held) |
| `lyc153_late_m1disable_1/_2`, `..enable_m1disable_1/_2` | DMG | E2 | ✓ | ✓ (held) |

The dispatch moves ly153 **dot6 → dot4**, coincident with the VBLANK-disable
commit → line stays high → no fresh edge → **E0**. `golden_fingerprint` **9020
match, byte-identical** (the edit is `eager_value`-gated; production never sets
it). The pair is separable, confirming Stage 1's premise.

### …but it STRICT-SHUFFLES the broader line-153 family (the refutation)

| model | EV base | EV E1 | net | recovered | dropped |
|---|---:|---:|---:|---:|---:|
| DMG | 54 | **57** | **+3** | 9 | 12 |
| CGB | 295 | **312** | **+17** | 4 | 21 |

**Steady-state goes UP on both models** → violates the "EV may only go DOWN"
gate → NOT shipped.

**DMG dropped (12, baseline-PASS → E1-FAIL):**
```
lyc153int_m2irq/lyc153int_m2irq_2                       (want 2)
lyc153int_m2irq/lyc153int_m2irq_late_retrigger_2       (want 0)
window/arg/late_wy_{10to0_ly1,10to1_ly1,FFto0_ly0,FFto0_ly2,
  FFto1_ly2,FFto2_ly2,FFto2_ly2_scx2,FFto2_ly2_scx3,
  FFto2_ly2_wx00,FFto2_ly2_wx0f}_3                      (want 0)   ×10
```
**DMG recovered (9):** the 2 `_3` targets + `lyc153int_m2irq_ifw_1` +
`lycwirq_trigger_ly00_stat50_2` + 5 `window/late_wy_*_1/_2`.

**CGB dropped (21):** `lyc153int_m2irq_2`, `lyc153int_m2irq_late_retrigger_2`,
`lyc153_late_ff45_enable_4`, `lycwirq_trigger_ly00_stat50_2`, **3 DMA rows**
(`gdma_cycles_short_2`, `gdma_cycles_short_scx5_2`, `gdma_weird_2`), and 13
`window/late_wy_*_1/_2`. **CGB recovered (4):** `lyc153int_m2irq_ifw_1`,
`lyc153_late_enable_m1disable_2`, `lycstatwirq_trigger_ly00_10_50_1`,
`late_wy_FFto2_ly2_scx5_1`.

## Why it shuffles — the family members' rises have DISTINCT sub-dot phases

The dropped rows are exactly the OTHER consumers of the line-153 `dot-6` LYC
latch:

1. **`lyc153int_m2irq_2` / `_late_retrigger_2`** (both models) — line-153
   LYC/mode-2 rows whose STAT edge is calibrated to the **dot-6** latch. Moving
   it to dot-4 kills their coincidence: the `_2` variant's FF41 write lands at a
   DIFFERENT sub-dot than the `_3` variant's, so the coincidence it needs is at
   dot-6, not dot-4. The `_2`↔`_3` split is precisely a sub-dot-phase difference
   that a single whole-dot window cannot encode.
2. **The `window/late_wy` cluster** (both models, the dominant mass — a `_1/_2`
   ↔ `_3` A/B swap per config) — the late-WY window verdict straddles the
   line-153→line-0 frame boundary; `ly_for_comparison`'s line-153 view feeds the
   LYC latch carried into the next frame's line-start reads, and the shift moves
   which timing variant matches. The line-153 schedule is consumed well beyond
   the direct `lyc153*` rows.
3. **CGB `gdma_cycles_short/weird_2`** — the eager HDMA/GDMA cycle count gates on
   the LY view at the frame boundary; the line-153 LY-comparison shift moves the
   DMA's LY-straddle by a dot, mis-counting the transfer. The blast radius
   reaches the DMA engine.

**The decisive structure:** the `_3` rows want the LYC re-latch at dot-4; the
`_2`/`late_retrigger` rows want it at dot-6; the window/DMA neighbors want the
UNSHIFTED dot-6 frame view. These are not reconcilable on the **whole-dot** grid —
they are three different **sub-dot** coincidences. Resolving them simultaneously
requires the LYC re-latch to land at its TRUE half-dot (piece 1: `StatUpdate::level`
eval on the 8-MHz grid) coupled with the FF41 commit half-dot (piece 4) and the
mode-3→0 flip half-dot (piece 3), so each row's write and its LYC rise resolve to
their own sub-dot phase INDEPENDENTLY rather than all snapping to one whole dot.
That is the full coupled FSM — unbuildable byte-identical + steady-state in a
bounded edit (Stage 1 §"golden-safety"), now with the whole-dot floor measured:
the buildable piece nets **+3 DMG / +17 CGB**.

## Per-piece status (the 4 coupled pieces from #11du §"The 4 coupled pieces")

| piece | what | this session |
|---|---|---|
| 1. `stat_update_tick` level on half-dot grid | idempotent odd-half `StatUpdate::level` re-eval | NOT wired — inert without a changed odd-half input (Stage 1 §"idempotent seam"); it goes in WITH pieces 3/4 that make it live. Confirmed dead scaffolding standalone (unchanged from #11du). |
| 2. line-153 LYC re-latch at its 8-MHz tick | `ly_for_comparison_line_153_at` back-date | **BUILT + MEASURED** as a whole-dot back-date (E1). Separates the pair; strict-shuffles the family (+3/+17). The half-dot version is the residual. |
| 3. `m0_flip_events` odd `flip_hd` | mode-3→0 flip on the 8-MHz grid | NOT built — the sibling's line-1 flip stays whole-dot dot254 (correct as-is for the pair; needed for the OTHER family members' coincidences). |
| 4. FF41 write commit at its half-dot | `eager_wr_borrow` extension | NOT built — the borrow already spans a `tick_half` pair but commits `eng_stat` via `write_no_tick` AFTER, at the whole dot. Piece 4 = commit the eng_stat change on the odd half so the odd-half level re-eval (piece 1) sees it. |

## What NOT to re-chase (adds to #11du / #11dr / #11dq)

- **The whole-dot line-153 LYC back-date (E1) as a wall-1 fix** — REFUTED here by
  direct measurement: it separates the pair but nets **+3 DMG / +17 CGB**
  steady-state, dropping `lyc153int_m2irq_2`/`_late_retrigger_2` + the
  `window/late_wy` cluster + CGB `gdma_cycles`. A whole-dot window cannot encode
  the `_2`↔`_3` sub-dot-phase split.
- **A ROM-timing-gated narrow back-date** (distinguish `_3` from `_2` by the
  write dot) — forbidden: "Never special-case test ROMs" (CLAUDE.md). The
  principled discriminator IS the sub-dot phase → the half-dot FSM, not a gate.
- **Piece 2 in ISOLATION at any grain** — the family needs pieces 1+3+4 coupled;
  even the half-dot LYC re-latch alone would shuffle the window/DMA neighbors that
  key on the unshifted frame view unless the FF41 commit + flip also move to their
  half-dots coherently.

## The residual — the coupled half-dot FSM (multi-session, unchanged from #11du)

Build all four together: (1) `StatUpdate::level` re-eval on `tick_half`'s odd half
(idempotent, byte-identical seam); (2) `ly_for_comparison` on the half-dot grid —
requires deriving SameBoy's `GB_SLEEP(14,4)` exact 8-MHz sub-dot phase for the
line-153 LYC re-latch (NOT in the current whole-dot table; wilbertpol `ly_lyc_153-C`
pins the whole-dot dot-6); (3) `m0_flip_events` recording an odd `flip_hd`; (4) the
FF41 `eng_stat` commit on the odd half. Success = the target pair separates AND
`lyc153int_m2irq_2`/`_late_retrigger_2` + the window/DMA neighbors HOLD (EV
monotone-down). Land slice-by-slice with the #11by→#11cb discipline (each slice:
`flagon_probe` two-bin, golden byte-identical, EV monotone-down, mooneye ×3,
intr_2 tripwires). The measured blast radius here (window/late_wy + CGB DMA) sizes
the coherence requirement: the half-dot LYC schedule must preserve the frame-boundary
LY view every window and DMA row consumes.

## Gates (all hold; tree byte-identical, no code shipped)

1. `git diff 243870e -- crates/` **empty** (E1 stashed, not committed).
2. `golden_fingerprint` byte-identical — 9020 match (at base AND under E1:
   `eager_value`-gated, production never reaches it).
3. EV CGB 295 / EV DMG 54 reproduced exactly at base.
4. tier2 / production defaults NOT flipped; no push; parent branch untouched.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd2
# baselines
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt \
  cargo test -p slopgb-core --test gbtr --release -- --ignored --exact \
  gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['   # fail=54
# apply E1 (the ly_for_comparison_line_153_at eager_value arm above), rebuild, re-run:
#   DMG fail=57 (+3), CGB fail=312 (+17)
# per-row diff: grep '^FAIL' <run> | sed -E 's/ want=.*//' | sort  → comm base vs E1
# pair trace:
cargo build -p slopgb-core --example run_gambatte --features port_probe --release
BIN=target/hd2/release/examples/run_gambatte
R=test-roms/game-boy-test-roms-v7.0/gambatte
SLOPGB_S5DBG=1 SLOPGB_EAGER=1 $BIN \
  $R/lycEnable/lyc153_late_m1disable_3_dmg08_cgb04c_outE0.gbc dmg 2>&1 | grep dispatch | grep 'ly=153'
#   base: dot=6 (spurious, E2) · E1: dot=4 (coincident, E0)
```
