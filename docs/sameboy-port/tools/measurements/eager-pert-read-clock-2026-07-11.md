# Scoped per-T eager READ — REFUTED AT THE PREMISE: the CPU `clock` does NOT carry the sub-M-cycle poll-phase discriminator the bar-19 `_1`/`_2` weld needs. Traced at the decisive FF41 read, both siblings reach BYTE-IDENTICAL `clock.now()` (70992) / `pending` (4) — the read always samples on the 4T M-cycle leading edge, so slopgb's M-cycle-atomic CPU collapses gambatte's 1-T poll shift to the same boundary. bar-19 is the eager clock's TRUE floor; crossing it needs a T-exact CPU core (advance PPU per-T + sub-instruction accesses at their true T), which re-couples the two-frame decomposition #11co proved load-bearing (2026-07-11, #11eb)

Base: `finish-port-halfdot @ ac7c9c3` (= #11ea). **NO CODE SHIPPED — tree
byte-identical @ ac7c9c3** (`git diff HEAD crates/` empty). The one experiment (an
env-gated `CLKTRACE` `probe!` in `Bus::read`, `SLOPGB_S5DBG`, dumping
`clock.now()`/`pending` at every FF41 read) was REVERTED. Measure-only REFUTE (the
#11br / #11bu / #11bw / #11dz / #11ea pattern).

## The load-bearing answer: NO

**Does the CPU `clock` distinguish the `_1`/`_2` poll phase at the decisive read?**
**NO.** Traced under `eager_value` (`SLOPGB_PROBE_EV`), the two welded siblings reach a
**byte-identical** decisive FF41 read — same `clock.now()`, same `pending`, same PPU
dot, same `read_pos_hd`. The clock carries no more phase than the whole-dot PPU frame
already did.

### The trace (two independent weld families, each run solo)

`late_disable_early_scx03_wx11` (native `m 0` at the read; the debt-`bpre` family):

| ROM | want | decisive read | resolved | verdict |
|---|:--:|---|:--:|:--:|
| `_1` | 0 | `ly1 dot252 rphd512 now=70992 pending=4 carr=true` | m=0 | PASS |
| `_2` | 3 | `ly1 dot252 rphd512 now=70992 pending=4 carr=true` | m=0 | **FAIL** (want 3) |

`late_reenable` (native `m 3` at the read; the debt-`reen` family):

| ROM | want | decisive read | resolved | verdict |
|---|:--:|---|:--:|:--:|
| `_1` | 3 | `ly1 dot252 rphd512 now=70992 pending=4 carr=true` | m=3 | PASS |
| `_2` | 0 | `ly1 dot252 rphd512 now=70992 pending=4 carr=true` | m=3 | **FAIL** (want 0) |

In **both** families the `_1`/`_2` pair is byte-identical at the decisive read down to
the CPU clock: `now=70992`, `pending=4`. The entire preceding FF41 read stream on ly0
(dots 16/44/72/100) is byte-identical too — the two ROMs drive slopgb through an
identical CPU+PPU trajectory to the read; only gambatte's reference expectation splits.
`_1` and `_2` want OPPOSITE modes at read state slopgb cannot tell apart.

## Why the clock structurally cannot carry it

`CycleClock::read` (`cycle_clock.rs:98`) pays the previous M-cycle's parked debt
(`clock += pending`), samples at `clock`, then **reparks 4**. Every internal cycle
(`internal`) parks +4. So at any eager `Bus::read`, after `clock.read()`:

- `pending == 4` always (observed: 4 at every trace line), i.e. the byte is sampled at
  the M-cycle **leading edge (cc+0)** — never a sub-M-cycle offset.
- `now` is on the 4T grid (`70992 = 4·17748`, `70300 = 4·17575`, …). The clock advances
  a whole M-cycle (4T) per CPU step.

A gambatte `_1`/`_2` sibling pair differs by a **sub-M-cycle (1-T)** shift of the
critical event across a mode-3→0 boundary that itself falls mid-M-cycle. slopgb's CPU
is **M-cycle-atomic**: an instruction's memory access is modeled at the M-cycle
boundary and the PPU is advanced 4 dots per M-cycle. That 1-T shift therefore lands the
read at the **identical** M-cycle boundary (dot 252) with the **identical** clock
(`now=70992`). There is no sub-M-cycle state anywhere in the eager execution — not in
the PPU `dot` (whole-dot; `read_pos_hd = 2*dot + dhalf(0) + debt`, `dhalf` stays 0 at
SS), and now proven not in `clock.now()`/`pending` either. The task's hypothesis (the
`CycleClock` might hold the phase where the PPU frame doesn't) is **refuted at step 1
of the investigation** — the discriminator is not in `clock`.

This is the same barrier the code already names in prose:
`read_laws.rs:128` — *"The whole-dot frame carries NO other observable — the true split
is the sub-dot poll phase, not resolvable in this frame."* #11eb extends that from the
PPU frame to the CPU clock: **neither** representation carries it.

## Why the scoped per-T read cannot escape (and why #11co is the wrong shape)

The scoped-per-T plan was: for the welded FF41 poll subclass, resolve `read_pos_hd`'s
sub-M-cycle offset **from `clock.now()`** instead of the whole-dot approximation. That
requires `clock.now()` to differ between the fix side and the drop side. It does not
(70992 == 70992). A scope, however tight, has nothing finer to read — so it degenerates
to the whole-M-cycle read it was meant to refine. There is no golden-safe zero-shuffle
lever because there is no lever input.

#11co's wholesale route (send the read through `read_deferred`/the deferred T-machine,
EV CGB 361→425, strictly worse) was the only way to get a genuine sub-M-cycle read
position — by advancing the machine T-granularly to the read's leading edge. It
regresses because the deferred T-read **re-couples** the load-bearing two-frame
decomposition: the eager clock deliberately reads the STAT mode VALUE at cc+4 (the
`+8hd` read-debt) while sampling render STATE at cc+0; a literal true-T read forces both
to the same instant and breaks the rows the decomposition currently wins. #11eb shows
the scoped version cannot even reach #11co's regression — it has no per-T input to act
on, so it is a no-op on the weld, not a trade.

## The definitive floor statement

**bar-19 is the eager clock's TRUE floor.** Crossing it is not a read-frame
calibration; it requires a **T-exact CPU core**: execute sub-instruction memory
accesses at their true intra-M-cycle T-offset and advance the PPU **1 dot per T** so a
mode boundary can fall between two reads 1 T apart. That is a whole-CPU rewrite (the
`M-cycle-atomic tick_machine` → per-T coroutine), and it re-introduces exactly the
frame re-coupling #11co refuted. The `_1`/`_2` weld is therefore **not** reachable by
any scoped, golden-safe, `eager_value`-gated read change. bar-0 is unreachable on the
eager clock without T-exact CPU execution.

## Baselines reproduced (exact, at ac7c9c3)

| metric | value | gate |
|---|---:|---|
| `flagon_probe[ON]` EV DMG | pass 1551 / **fail 52** / skip 1819 | steady-state floor ✓ |
| `golden_fingerprint` (production build, no `port_probe`) | **ok — byte-identical** | THE gate ✓ |

## Gates (all hold — NO code shipped, tree byte-identical @ ac7c9c3)

1. `golden_fingerprint` byte-identical — production build passed (42s); `git diff HEAD
   crates/` empty.
2. EV DMG **52** unchanged (nothing shipped); EV CGB **295** / tier2 **291/116** untouched.
3. Zero regression (nothing shipped).
4. The `CLKTRACE` `probe!` edit on `bus.rs` REVERTED; `git diff HEAD crates/` empty.
5. No file grew (untouched at HEAD: `read_laws.rs` 999, `engine.rs` 589, `bus.rs` 313).

## Do-not-re-chase ledger (add)

- The bar-19 `_1`/`_2` weld is NOT resolvable by any state slopgb's eager clock holds.
  Traced at the decisive FF41 read, both siblings reach byte-identical `clock.now()`
  (70992) / `pending` (4) across TWO weld families (`late_disable_early`,
  `late_reenable`). The CPU clock is M-cycle-quantized at reads (repark 4, sample cc+0),
  so it carries no sub-M-cycle phase. Do NOT re-attempt a scoped per-T read keyed on
  `clock.now()` — there is no discriminator to key on.
- REFUTES the #11eb hypothesis ("the `CycleClock` tracks sub-M-cycle T, so a scoped
  per-T read can distinguish the poll phase"): the clock's per-read granularity is the
  M-cycle leading edge, identical to the PPU whole-dot frame. Both representations lack
  the phase.
- bar-19 → bar-0 on the eager clock requires a T-exact CPU core (PPU 1 dot/T +
  sub-instruction accesses at their true T), which re-couples the two-frame
  decomposition #11co proved load-bearing. It is a whole-CPU rewrite, not a scoped
  read lever. The eager-clock read-frame attack surface for the DMG window-exit weld is
  now EXHAUSTED (adds to #11ea's refute: #11ea = no PPU-side discriminator; #11eb = no
  CPU-clock-side discriminator either).

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd8
cargo test -p slopgb-core --test gbtr --release --features port_probe --no-run
BIN=$(ls -t target/hd8/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# Solo-run each welded sibling; the CLKTRACE probe (reverted) dumped clock.now():
#   _1/_2 both -> "CLKTRACE ... ly1 dot252 rphd512 now=70992 pending=4 carr=true"
printf 'gambatte/window/late_disable_early_scx03_wx11_1_dmg08_cgb04c_out0.gbc [Dmg]\n' > /tmp/w1.txt
printf 'gambatte/window/late_disable_early_scx03_wx11_2_dmg08_cgb04c_out3.gbc [Dmg]\n' > /tmp/w2.txt
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=/tmp/w1.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture   # pass=1
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=/tmp/w2.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture   # fail=1 (want3 got0)
# EV DMG steady-state floor (52):
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['
# Golden (production build):
SLOPGB_REQUIRE_ROMS=1 cargo test -p slopgb-core --test gbtr --release golden_fingerprint
```
