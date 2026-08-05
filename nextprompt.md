slopgb — next task: the nine `ch1_duty0_pos6_to_pos7_timing` rows

## Repo state (verified 2026-08-05)

`main` + branch `feat/apu-granule-grid` (the APU granule grid, below). Clean
tree at the branch tip.

gbtr **221/221**; mooneye **93/93** suite tests (439/439 rom×model); core lib
**910**; frontend **676**; clippy clean; `golden_fingerprint` current.

gambatte baseline **250 keys**; **349** baselined floor cases across all suites
(read the floor-class index header in `tests/gbtr/baselines/gambatte.txt` before
touching any baselined row). Remaining `speedchange` APU rows: the nine
`ch1_duty0_pos6_to_pos7_timing` variants. The six `ch2_nr52` rows are GONE.

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

## TARGET — the nine duty rows

They turn on whether the leaving re-anchor also hands the APU the granule
**debt** gambatte's `lastUpdate_ -= ds` implies. Measured, two-sided:

| leave re-anchor | score |
|---|---|
| moves the grid only (shipped) | +6 / −0 |
| with the debt (literal `lastUpdate_ -= 1`) | +10 / −4 |
| debt alternating in sign per leave | +10 / −1, no source, NOT shipped |

The wanted granule delta per switch count is +1 for `speedchange2`/`3` and 0 for
`speedchange4`/`5`, and **no constant (enter, leave, power-on) shift delivers
that** while keeping the nr52 rows: those pin the grid to trail after enters 1
and 3 and to be in step after enter 2, which forces the leave to flip parity,
and every parity-flipping shift gives speedchange2..5 the same delta. Swept
enter 0..4 × leave 0..3 (20 points, full matrix each): best net +6.

**The lever is whatever separates a second leave from a first.** The unmodelled
candidate named in the source is the frame-sequencer re-base gambatte does on
the ENTERING side — `cycleCounter_ = cc - divCycles/2 - lastUpdate_ % 2`
(`PSG::speedChange`) — which is keyed on the grid's parity and has no
counterpart here because slopgb derives the frame sequencer from DIV rather than
from the PSG's own counter. Start there, not on another shift sweep.

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
- **Restore baselines with `git checkout --`, never a `/tmp` copy.**

## Last session

| commit | law | rows |
|---|---|---|
| (this branch) | the APU observes itself on a 2 MHz granule grid that a power-on or a leaving speed switch can offset by a cycle | +7 |
