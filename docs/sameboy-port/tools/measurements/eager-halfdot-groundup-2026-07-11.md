# HALFDOT ground-up STAT/mode/LYC engine — the odd-half `GB_STAT_update` engine SHIPS its first wall-1 recovery: the DMG line-153 FF41 write-commit half-dot recovers the EXACT #11dv target pair CLEAN (EV DMG 54→52, −2/−0), overturning #11dv's "+3 DMG / +17 CGB shuffle" refutation — because the lever is the WRITE-COMMIT sub-dot (piece 4, engine `eng_stat` only) resolved on a new odd-half engine (piece 1), NOT the LYC re-latch (piece 2) the whole family consumes (2026-07-11, #11dw)

Base: `finish-port-halfdot @ 3943e76` (= #11dv). SHIPPED (defaults NOT flipped;
`eager_value`-gated, production byte-identical). Commit: see end.

## TL;DR — SHIP (partial), the ground-up approach CONVERGES

The task's outcomes were SHIP (recovered wall-1 rows, zero drops) / PARTIAL
(map the residual) / REFUTE (+N shuffle). **This is SHIP for the #11dv target
pair — the exact rows #11dv recovered only at a +3/+17 family cost.** The
ground-up odd-half engine recovers them with ZERO shuffle, proving the
coupled-half-dot approach CONVERGES where the whole-dot back-date (#11dv) and
the uniform write-borrow (#11du) both refuted. It is *partial* only in extent:
the pure line-153 FF41-write-disable class is 2 rows; the broader wall-1
(retrigger / m2irq / line-152 / CGB line-153-write) needs further odd-half
pieces (§Residual).

## Baselines reproduced (exact, at 3943e76)

| metric | value | gate |
|---|---:|---|
| `golden_fingerprint` | 1 pass (byte-identical) | THE gate ✓ |
| EV CGB (`cgb_rowlist.txt`) | **295** | steady-state floor |
| EV DMG (`dmg_rowlist.txt`) | **54** | steady-state floor |
| mooneye default / eager / tier2 | 93 / 93 / 93 | ✓ |
| `eager_construction_intr_2_timing` (intr_2 ×3 ×2 models) | pass | ✓ |

## The engine built — a NEW eager-only odd-half `GB_STAT_update` path

`Ppu::tick_half` (`engine.rs`) runs the whole-dot `tick()` on the completing
(even) half; under `eager_value` the odd half (`dhalf 0→1`) ran only
`strobe_tick`. Added: `stat_update_half()` on that odd half. The SameBoy STAT
line is now recomputed on the 8-MHz ODD half-dot, so a coincident FF41
write-commit resolves at its true SUB-dot phase instead of snapping to the
whole-dot even-half tick. **The IF it raises persists in `pending_if` and folds
at THIS dot's completing (even) half** — no interconnect fold-path change (the
odd half returns 0; the even-half `tick()` returns `pending_if`).

Per-piece status (the #11du 4-piece decomposition):

| piece | what | #11dw |
|---|---|---|
| 1. `StatUpdate::level` on the odd half | `stat_update_half` | **BUILT.** GATED to run its `update()` ONLY when piece 4 commits a write this half-dot — a bare re-eval every odd half is NOT idempotent (it re-runs the edge WITHOUT the even-half squash/`eng_stat_pending` logic → measured +1 DMG / −1 CGB shuffle), so it stays inert (EV byte-identical) until armed. |
| 2. `ly_for_comparison` LYC re-latch at 8-MHz tick | — | **NOT moved.** SameBoy's line-153 re-latch is at WHOLE dot 6 (`GB_SLEEP(14,4)` = 2 dots + 4 dots, all even half-dots; verified against `~/.cache/sbbuild/.../display.c:2252-2262`). It has NO sub-dot phase → the separator was never piece 2 (why #11dv's piece-2 back-date shuffled the family). |
| 3. `m0_flip_events` odd `flip_hd` | — | **NOT needed for this pair.** The target's join is a LYC re-latch (whole-dot 6); the flip half-dot matters only for the line-1 sibling class (§Residual). |
| 4. FF41 write-commit at its half-dot | `eng_stat_half` | **BUILT.** The DMG line-153 FF41 `eng_stat` write is deferred ~2 dots (`Some((data, hd))`, counted down by the odd half) so it lands COINCIDENT with the LYC=153 re-latch (dot 6) instead of the eager cc+4 commit (dot 4). |

## The physics (why it works, why it is line-153-scoped, why NOT uniform)

Target `lycEnable/lyc153_late_m1disable_3` (DMG, want E0): the ROM enables
VBLANK STAT + LYC=153, then late-disables mode-1. On hardware LYC=153 already
matches when mode-1 falls → the LYC source holds the STAT line HIGH → **no fresh
0→1 edge → E0**. slopgb's eager frame committed the disable's `eng_stat` at cc+4
= **ly153 dot 4**, but `ly_for_comparison=153` (hence `lyc_interrupt_line`)
first latches at **dot 6** — so between dot 4 and dot 6 the line dipped (mode-1
gone, LYC not yet matched) then RE-rose at dot 6 → spurious edge → **E2** (fail).

Fix: defer ONLY the engine `eng_stat` view to dot 6 (coincident with the LYC
re-latch), via the odd-half commit. Then at the commit the line is already high
(LYC matched) → removing mode-1 keeps it high → no edge → **E0**.

- **Why line-153-scoped (principled, NOT a ROM special-case):** line 153 is the
  documented LYC/LY side-effect zone (the same zone `ly_for_comparison_line_153`
  and `write_lyc_cgb`'s dot-≥452 hold already special-case). The FF41 write
  landing 2 dots later relative to the line-153 `GB_SLEEP` micro-sequence is the
  same class of line-153 write quirk. The sibling `m0enable/lycdisable_ff41_2`
  (line **1**) is untouched.
- **Why NOT the uniform write-borrow #11du refuted:** the sibling's join is the
  mode-3→0 HBLANK flip at dot 254; slopgb commits its LYC-disable at dot 252 (2
  before → dip-then-rise → edge = correct). A UNIFORM +2 write-borrow (which
  `interconnect/bus.rs:116-119` already records as inverting `lycdisable_ff41_2`
  on DMG) would move the sibling's disable to dot 254 = coincident with the flip
  → line stays high → no edge → BREAKS it. The line-153 scope moves ONLY the
  target's write, so the sibling holds — the coupling #11du/#11dv said the whole
  engine needs, delivered by scoping the write-commit to the quirk zone rather
  than a uniform slide.
- **Why NOT #11dv's piece-2 back-date:** #11dv moved the line-153 LYC re-latch
  dot 6→4 (piece 2) and separated the pair, but the `window/late_wy` cluster +
  `lyc153int_m2irq_2` + CGB `gdma_cycles` all consume that re-latch's frame view
  → +3 DMG / +17 CGB shuffle. #11dw moves the WRITE-COMMIT (piece 4) instead,
  leaving the LYC re-latch schedule the family consumes byte-identical → ZERO
  shuffle.

## Measured result — EV steady-state before → after (SHIP)

| model | EV base | EV #11dw | net | recovered | dropped |
|---|---:|---:|---:|---|---:|
| DMG | 54 | **52** | **−2** | `lyc153_late_m1disable_3`, `lyc153_late_enable_m1disable_3` | **0** |
| CGB | 295 | **295** | **0** (DMG-scoped; untouched) | — | 0 |

Per-row diff (`comm` of the `FAIL` lists, base vs #11dw): recovered exactly the
2 rows above; **zero dropped** on both models. These are the EXACT #11dv target
pair (its Stage-2 target) — recovered here with zero family cost.

## Gates (all hold; SHIPPED, defaults NOT flipped)

1. `golden_fingerprint` byte-identical — 1 pass (the odd-half path is
   `eager_value`-gated; production `eager_value=false` never enters it).
2. EV DMG 54→52 (−2/−0), EV CGB 295 unchanged.
3. mooneye 93 default / 93 eager / 93 tier2; `eager_construction_intr_2_timing`
   pass (intr_2_mode0/mode3/sprites ×2 models); di_timing green under all three.
4. lib 760/760; clippy `-D warnings` clean; every `.rs` < 1000 (reclock 912,
   regs 927, mod 914).
5. Red-before-green pin `eager_dmg_lyc153_m1disable_passes`
   (`tests/gbtr/gambatte/eager_web.rs`) — the 2 rows on `Model::Dmg` under
   `boot_eager`; red at base (both in the base FAIL list), green after.

## The residual — the broader wall-1 (the next odd-half pieces)

The pure line-153 FF41-write-DISABLE class is EXHAUSTED at 2 DMG rows. The
remaining wall-1 DMG rows are DIFFERENT mechanisms, each needing a further
odd-half piece (traced, not assumed):

- `ly0/lycint152_lyc153irq_late_retrigger_2` (want E0) — a LYC=153 SOURCE
  re-edge that fires every frame at ly153 dot6 (`mfi=1 lycln=1`, traced), NOT a
  disable-coincidence. Needs the LYC-source retrigger suppression, not piece 4.
- `lyc153int_m2irq/*`, `m0enable/lycdisable_ff41_scx3_2` (line-1 scx variant),
  the `m1/*` late-m2enable-lycdisable rows — the line-1 sibling class whose join
  is the mode-3→0 FLIP; these need **piece 3** (the flip's true `flip_hd` odd
  half) coupled with piece 4, so a line-1 write-commit resolves against the
  flip's sub-dot phase (the `2*flip+2` exit-model half-dot).
- CGB line-153 writes (`ly_lyc_153_write ×6`, #11dt) — the CGB twin, on the
  two-phase `eng_stat_pending` frame; the odd-half `eng_stat_half` is
  DMG-scoped (CGB's write frame is owned by the two-phase). Extending needs the
  two-phase resolved on the odd half.

The infrastructure (`stat_update_half` + `eng_stat_half`) is the reusable
substrate for all three — each is a further arm on the same odd-half engine,
landed slice-by-slice with the #11by→#11cb discipline (golden byte-identical,
EV monotone-down, mooneye ×3, intr_2 tripwires).

## What this overturns / adds to the "do not re-chase" ledger

- **OVERTURNS #11dv's refutation** for the target pair: the coupled half-dot
  landing is NOT condemned to a family shuffle. #11dv measured the WRONG lever
  (piece 2, the LYC re-latch the family consumes); the write-commit (piece 4) on
  the odd half separates the pair with zero shuffle. The ground-up approach
  CONVERGES.
- **Confirms #11du:** a UNIFORM write-commit shift IS a strict pair-shuffle
  (`bus.rs:116` records it). The non-uniformity comes from the line-153 quirk
  SCOPE, not a per-ROM gate — principled, per the CLAUDE.md no-special-case rule.
- **NEW (do not re-chase):** the bare odd-half `StatUpdate::update` every dot is
  NOT byte-identical (it re-fires without the even-half squash/pending logic →
  +1 DMG / −1 CGB). The odd-half engine must be gated to fire only on an armed
  coincident commit — the seam is safe ONLY when it has a genuine odd-half input
  change to resolve.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd3
# EV (SHIP): DMG 52, CGB 295
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt \
  cargo test -p slopgb-core --test gbtr --release -- --ignored --exact \
  gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['   # fail=52
# pin (red-before-green): revert the regs.rs eng_stat_half arm → the 2 rows FAIL
cargo test -p slopgb-core --test gbtr --release eager_dmg_lyc153_m1disable_passes
# gates
cargo test -p slopgb-core --test gbtr --release golden_fingerprint            # byte-identical
SLOPGB_MOONEYE_EAGER=1 cargo test -p slopgb-core --test mooneye --release     # 93/93
# deferral sweep (port_probe): SLOPGB_ENGCOMMIT=<hd> (default 2 = the dot-4→6 shift)
```
