slopgb — next task: the twelve `ch1_duty0_pos6_to_pos7_timing` rows

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

## TARGET — the twelve duty rows

**Measured per ROM, not inferred** (probe: 16 frames on CGB, read
`ch1.duty_pos` at the end — the kernel freezes it, so it IS the verdict; duty 0
is high on position 7 only). All 22 `_1` rungs sit on 6 and every one is
correct; every `_2` rung is on 7 (pass) or 6 (fail). Each failure is therefore
the same single 2 MHz cycle, and **every uniform shift is refuted a priori** —
advancing the duty unit a granule everywhere puts all 22 `_1` rungs on 7.

The entering pace IS a live lever on this family, measured across the matrix:

| entering pace | score |
|---|---|
| one granule less (gambatte's literal `cc + 8` jump) | −6 / +0 |
| shipped (`Apu::set_double_speed_lag`) | — |
| one granule more | **+9 / −8** |
| one granule more + the leave debt | +9 / −9 |

The +9/−8 row reads exactly as one granule per ENTERING switch: `k`=1,2 (one
enter) move the expiry from after `_2` to between the rungs — pure fix; `k`=3
(two enters) move it two and lose `_1`; `k`=4,5 were already between their rungs
so any advance only costs them. So the corpus wants **+1 for speedchange 1/2/3
and 0 for 4/5**, and no per-switch accumulating term generates that — `k`=3 and
`k`=4 share their two enters and differ only by a trailing leave, while `k`=2
and `k`=3 differ by an enter yet want the same delta.

**The cause is localized — read `docs/hardware-state/apu.md` §"Differential
trace against gambatte" before anything else.** gambatte builds standalone in
one `g++` line (recipe in that section) and can be instrumented, which turns
this family from fitting into differencing. What it showed on `speedchange3`:
the granule counts agree EXACTLY at every switch and over the whole
trigger→retrigger span, the alignment bit agrees — and the duty position at the
deciding retrigger is 7 in gambatte, 6 in slopgb. The entire difference is the
**inactive trigger's reload**: gambatte's first step lands `period + 4 - align`
granules after the trigger, ours lands at `period + 5 - lf_div`, one cycle
later, uniformly.

`base + 5 - lf_div` (gambatte's exact value, and NOT one of the refuted forms)
scores **+11 / −6** on gambatte but breaks seven same-suite rows that SameBoy
passes, so it is out as a uniform change. Tracing the six it breaks separates
two further defects: `sound/*_ds_1,_ds_3` have the right interval but our
`lf_div` reads 1 where gambatte's `align` reads 0, while `speedchange5*_1` has
the right alignment but one granule too many in its interval (229827 vs
229826). Three separable bugs, not one — and the reload's +1 was cancelling the
`lf_div` disagreement on exactly the rows that broke. Settle the reload against
SameBoy first (same-suite pins it, and gambatte does not overrule that), then
the `lf_div` disagreement, then the five-switch interval.

Older framing, still true but superseded — and the kernels are already
disassembled for you in `docs/hardware-state/apu.md` § "The kernel,
disassembled": one code body at `$0150`, whose only free variables are the
switch sequence, the rung's padding NOP and a per-family `ld b,NN` delay (108 /
99 / 105 / 102 / 103 / 95 / 94 / 100 / 91 / 90). Two things follow. The delay
loop cannot drift — it runs at a fixed granules-per-machine-cycle rate in
whichever speed it sits in — so the entire error comes from the switches. And a
family's wanted correction is not a function of its switch count alone, because
each family starts at a different distance from its own expiry, which is why
fitting "delta per k" keeps failing.

The measurement that would settle it: trace one failing family and one passing
family with the same switch count (`speedchange3` vs `speedchange4`) and record
the duty expiry's position against each STOP — per switch, not per ROM. The
probe to build it on is `gb.bus.apu().ch1` from an in-crate test.

The leave-side lever turns on whether the re-anchor also hands the APU the
granule **debt** gambatte's `lastUpdate_ -= ds` implies. Measured, two-sided:

| leave re-anchor | score |
|---|---|
| moves the grid only (shipped) | +6 / −0 |
| with the debt (literal `lastUpdate_ -= 1`) | +10 / −4 |
| debt alternating in sign per leave | +10 / −1, no source, NOT shipped |

No constant (enter, leave, power-on) shift delivers the wanted vector while
keeping the nr52 rows: those pin the grid to trail after enters 1 and 3 and to
be in step after enter 2, which forces the leave to flip parity, and every
parity-flipping shift gives speedchange2..5 the same delta. Swept enter 0..4 ×
leave 0..3 (20 points, full matrix each): best net +6.

**The lever is whatever separates a second leave from a first**, and it is finer
than the granule the model runs on. Before spending a session on gambatte's
entering-side FS re-base (`cycleCounter_ = cc - divCycles/2 - lastUpdate_ % 2`,
`PSG::speedChange`): the DIV reset immediately preceding it leaves `divCycles`
at the four granules the `cc + 8` jump just ran, so the expression reduces to
pulling the PSG counter back two granules plus the grid's parity, and it moves
the frame sequencer *relative to the channels*. The duty rows measure the
channels alone, so that is probably the length/envelope side's lever, not
theirs — and that side is already green. Rebuild the duty-position probe first
(it is ~50 lines against `gb.bus.apu()` from an in-crate test) and measure where
the expiry sits against each rung's retrigger; do not sweep a scalar again.

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
