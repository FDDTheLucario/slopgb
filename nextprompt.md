slopgb — APU duty family CLOSED (class G); next is the merge + the PPU clusters

## Repo state (verified 2026-08-05)

`main` + branch `feat/apu-granule-grid` (the APU granule grid, below). Clean
tree at the branch tip.

gbtr **221/221**; mooneye **93/93** suite tests (439/439 rom×model); core lib
**910**; frontend **676**; clippy clean; `golden_fingerprint` current.

gambatte baseline **250 keys**; **349** baselined floor cases across all suites
(read the floor-class index header in `tests/gbtr/baselines/gambatte.txt` before
touching any baselined row). Remaining APU rows: twelve
`ch1_duty0_pos6_to_pos7_timing` variants (eleven in `speedchange/`, one in
`sound/`). The six `ch2_nr52` rows are GONE.

The project is GPL-2.0-only, so gambatte's source is readable on compatible
terms and *was* read to derive both this task and the last one. Clone it outside
the repo tree; nothing is vendored.

## READ FIRST

- `docs/hardware-state/apu.md` § **"The granule grid"** — the model that shipped,
  the three-row measured bracket on the leaving re-anchor, and why a constant
  shift cannot satisfy both families. Do not re-derive any of it.
- The APU block in `tests/gbtr/baselines/gambatte.txt` (same content, denser).

## What shipped last session

The APU advances in whole 2 MHz **granules** off a grid that can sit one CPU
cycle away from the CPU's own counter (`Apu::lag`, 0 or 1), and a DIV-APU edge
raised inside a granule is deferred to the next boundary (`Apu::pending_edge`).
That is gambatte's `PSG::generateSamples` truncation. The lazy *call sites* the
old plan called for turned out to be unnecessary: the advance is monotone in
`cc` and idempotent, so advancing once per machine cycle gives every observer at
a machine-cycle boundary — which is every slopgb access — the same state.

Score **+7 / −0**: all six `speedchange*_ch2_nr52_*a` rows plus age
`spsw-ch2-lc-delay-cgbBCE`. With both grid re-anchors disabled the model is
byte-identical to the eager clock (verified by running the matrix that way).

## The twelve duty rows are CLOSED — do not chase them

Both references were built, instrumented and run on the same ROMs
(recipes in `docs/hardware-state/apu.md`; gambatte is one `g++` line, SameBoy is
`make tester` and needs RGBDS, which this box has). Frozen ch1 duty position on
`speedchange3` — duty 0 is high on index 7 only, and all three share a duty
table:

| | `_1` rung (wants != 7) | `_2` rung (wants == 7) |
|---|---|---|
| gambatte | 6 pass | 7 pass |
| **SameBoy** | **7 FAIL** | 7 pass |
| slopgb | 6 pass | **6 FAIL** |

**SameBoy fails these ROMs too**, one 2 MHz cycle in the opposite direction from
us. Class G (upstream tie-break needed), not a timing floor and not ours to fix.
Our value is provably SameBoy's: SameBoy's `delay = 6 + lf_div * (model <
CGB_D && double_speed ? 1 : -1)` sampled at the machine-cycle START equals the
`base + 6 - lf_div` we sample at the END, once the granule already run is
removed. Adopting gambatte's value instead costs seven same-suite rows SameBoy
passes.

Re-open only on hardware evidence. Everything measured is tabulated in apu.md;
the sweep does not need repeating.

## What is actually next

1. **Merge this branch to `main`** — it is +7 rows, verified, six commits.
2. **The remaining gambatte baseline is 250 keys**, biggest clusters `dma` 44,
   `m1` 20, `lcd_offset` 19, `lycEnable` 17, `sprites` 13. Mostly class A/B
   (double-speed sub-cycle phase) — the same structural shape the granule grid
   just cracked for the APU, so the differencing rig is worth pointing at the
   PPU side next.
3. Optional substrate: the granule-accounting corrections (below) make the APU
   match gambatte segment-for-segment and are corpus-neutral. Ship them only if
   a future lever needs them; they cost a full golden recapture.

## Constraints

- **Zero regressions.** Growing a baseline is a regression (harness law). A
  +10/−4 shape is not a landing.
- Verify in order: `cargo test -p slopgb-core --lib apu` → blargg `dmg_sound` +
  `cgb_sound` → the gambatte matrix → `same_suite` → full gbtr +
  `golden_fingerprint` → mooneye → frontend.
- **Never drop a row SameBoy passes** for a gambatte-derived change. That rule
  already cost the single-speed half of the edge deferral: it broke same-suite
  `channel_1_sweep_restart_2`, so the deferral is double-speed-scoped.
- Standing repo law: no new deps, no unsafe, files <1000 lines, SSH-signed
  commits, `/rust-diff-review` per iteration.

## Method notes that earned their place

- **A no-op control is worth building first.** Running the matrix with the
  re-anchors zeroed proved the restructure byte-identical before any row moved;
  without it a +10/−4 is uninterpretable.
- **Diff censuses, don't read row counts.** `SLOPGB_GBTR_CENSUS=<file>` per
  variant, then diff per row — that is what showed the breaks were `_1` rungs of
  4/5 and the fixes `_2` rungs of 2/3, which is the whole shape of the problem.
- **Check the ROM before trusting the naming.** `speedchangeN` really is N
  switches (one extra `10 00` per N, `cmp -l` on the kernels), and `_1a`/`_1b`
  really is one inserted NOP before `LDH A,(FF26)`.
- **A `_1`/`_2` pair is two granules apart in single speed**, one in double. A
  one-granule move therefore flips only one of a single-speed pair — which is
  why "+1 step" broke four `_1` rows without touching their `_2` siblings.
- **Read the frozen state, not the verdict.** The pass/fail bit hides the
  distance; `ch1.duty_pos` gives it directly and killed a whole class of
  candidate fixes in one run.
- **Restore baselines with `git checkout --`, never a `/tmp` copy.**

## Last session

| commit | law | rows |
|---|---|---|
| (this branch) | the APU observes itself on a 2 MHz granule grid that a power-on or a leaving speed switch can offset by a cycle | +7 |
