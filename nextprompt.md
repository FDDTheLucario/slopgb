slopgb — the twelve `ch1_duty0_pos6_to_pos7_timing` rows are BLOCKED (read below first)

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

## TARGET — the twelve duty rows (blocked on a cross-oracle conflict)

**Read `docs/hardware-state/apu.md` §"The blocker" first.** The cause is fully
localized, by differencing against a built + instrumented gambatte (build recipe
in that file — one `g++` line): the whole family is one 2 MHz cycle in the
INACTIVE TRIGGER's reload. gambatte lands the first duty step `period + 4 -
align` granules after the trigger; we land at `period + 5 - lf_div`. Every
measured lever:

| lever | duty fixed | duty broken | same-suite broken |
|---|---|---|---|
| inactive delay −1, single speed only | 6 | 4 | 1 ([Agb]) |
| inactive delay −1, double speed only | 5 | 2 | **7** |
| both speeds | 11 | 6 | **7** |
| `lf_div` flipped in double speed (SameBoy's CGB ≤ C rule) | 4 | 2 | **7** |
| gambatte's full alignment form | 4 | 2 | **7** |
| granule-accounting corrections | 0 | 0 | 0 |
| double-speed PCM read offset | 0 | 0 | 0 |

The seven are `channel_1/2_align`, `_align_cpu`, `_duty` and
`channel_1_freq_change_timing-A` — all SameBoy-passing, so the cross-oracle rule
keeps SameBoy's value. Compensating the delay after flipping `lf_div` recovers
three and leaves `channel_1/2_align` + `_duty`, which proves what they pin is the
double-speed trigger delay itself — the quantity gambatte wants one shorter.

**Do not re-run this sweep.** The open question is a third quantity that makes
one of the two hardware-verified suites read wrong here; the two testable
candidates (the alignment bit, the register-read observation point) are both
excluded above. Hardware evidence, or SameBoy's and gambatte's handling of the
same ROM traced side by side, is what moves this next.

Two traps worth carrying forward. `lf_div` derives from the APU's granule
parity, so a one-granule clock change flips it and moves the trigger delay the
other way — **levers do not compose**, always re-measure a combination. And the
granule accounting has two compensating errors (the enter grants one extra
granule, the leave withholds the debt gambatte pays); correcting both is exact
against gambatte and precisely neutral on the corpus, and is the right substrate
for any further attempt even though it buys no row on its own.

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
