# HALFDOT Part-C STAGE 1 (eager) — the net-zero half-dot IF-fold infra ALREADY EXISTS at base; the m1statwirq / dispatch residual is blocked at the CPU SAMPLE, not the PPU fold (2026-07-12, #11ds)

Base: `finish-port-halfdot @ 7720913`. Task (#11ds, STAGE 1 of the multi-session
Part-C build): wire the net-zero half-dot IF-fold onto the eager inline
`tick_machine` path (mirroring the tier2 A-infra #11ba), keeping every transition
whole-dot so flag-on eager stays byte-identical, as the substrate the two-latch
`mode_for_interrupt` convergence needs.

**Result: NO CODE SHIPPED — the STAGE-1 net-zero infra is ALREADY PRESENT in the
base (and exceeded).** The task's "current eager path" premise ("the eager inline
path does NOT use `tick_half`") is FALSE at 7720913: the eager `tick_machine`
already advances the PPU per 8-MHz half-dot via `Ppu::tick_half`, folding on the
dot-completing half (commit `3454e14`, 2026-07-09, "eager tick_machine half-dot
PPU advance (HALFDOT Part A-infra)" — the literal EAGER analogue of #11ba). The
odd-half STAT-engine re-eval that Part-C convergence hangs on is ALSO already
wired (`Ppu::stat_update_half`, #11dw). There is no net-zero increment left to
add; the next lever is non-net-zero AND refuted (the counter-pinned CPU dispatch
sample). Tree byte-identical (`git diff 7720913 -- crates/` empty); this session
adds only this map.

## Net-zero reference measured at THIS base (7720913)

| metric | value | note |
|---|---:|---|
| `golden_fingerprint` | PASS (42.72s) | production byte-identical (port_probe env-gated) |
| EV CGB (`flagon_probe`, `cgb_rowlist.txt`, `SLOPGB_PROBE_EV`) | **287** | (task's "287" ✓) |
| EV DMG (`dmg_rowlist.txt`, `SLOPGB_PROBE_EV`) | **46** | task's "38" is stale — measured on branch `9233110`, not this base |
| `m1statwirq_3_dmg08_out2.gb` (`run_gambatte`) | OFF `2` (pass) / EAGER `0` (fail) | the target row, still red as expected |

(The EV-DMG delta 38→46 is the base difference: `9233110`
[`crack-m1statwirq`/`c3-flip`] measured 38; `7720913` = `e2c07de` + 2 measures 46,
matching the `eager-lyc153-rise-retime` map. Use **287 / 46** as the net-zero
reference for this base.)

## (a) The m1statwirq whole-dot-fold-miss — the trace

`run_gambatte --features port_probe`, DMG, `SLOPGB_EAGER=1 SLOPGB_S5DBG=1`, grep
`SLOPGB dispatch|m0rise`, line 153 / line 0:

```
EAGER: SLOPGB dispatch ly=153 dot=6 mfi=1 lycln=1     (exactly one; the LYC=153 rise)
OFF:   (no SLOPGB dispatch — production uses stat_events_tick, a different engine)
```

Cross-referenced with the two prior maps' full `Bus::read/write` + dispatch traces
(`eager-write-halfdot-2026-07-12.md` §1, `eager-lyc153-rise-retime-2026-07-12.md`),
the mechanism is exact:

| config | STAT rise reaches `intf` | STAT-ISR dispatch fetch | verdict FF41 write | verdict |
|---|---|---|---|---|
| OFF   | cyc 69832, ly=153 **dot 4** (`stat_events_tick`) | `0232` | `ly=0 dot=0` (hblank glitch, `dot<4`) → fires | **2** ✓ |
| EAGER | cyc 69836, ly=153 **dot 6→8** (`stat_update_tick`) | `0233` (**+1 M-cycle**) | `ly=0 dot=4` (mode 2/3) → no fire | **0** ✗ |

**Where the eager half-dot fold folds the rise vs where the CPU sample looks.**
The eager reclock engine (`stat_update_tick`) raises the LYC=153 coincidence at
**dot 6** (SameBoy's DMG `GB_SLEEP(14,4)` `ly_for_comparison` schedule,
`reclock.rs::ly_for_comparison_line_153_at`: `6..=7 => 153`). The half-dot advance
(`tick_half`) advances the PPU 0.5-dot at a time, but `fold_ppu_events` OR's the
rise into `intf` only on the **completing (even) half** of dot 6 — i.e. at
half-dot position `2*6+1 = 13`. The CPU dispatch check
(`interconnect/speed.rs::dispatch_pending_impl` → `pending() = intf & ie &
!if_stat_late`) samples at the **fixed M-cycle boundary** — it never re-observes
the odd-half rise at its sub-M-cycle T. So whether the STAT ISR dispatches in the
M-cycle that spans dot 6 or the NEXT one depends on the CPU's M-cycle phase at
line 153; for m1statwirq's phase it slips one M-cycle → the verdict write drifts
from `ly=0 dot=0` to `ly=0 dot=4` → misclassified.

SameBoy avoids the slip because `GB_display_run` and `GB_cpu_run` share ONE per-T
(per-half-dot) interleaved advance: `GB_STAT_update` sets `IF|=2` at the exact T of
the rise, and the M-cycle's trailing-T interrupt sample re-reads `IF` in lockstep,
so a dot-6 rise is caught coherently regardless of phase. slopgb's eager clock
advances the whole PPU M-cycle inside `tick_machine`, THEN the CPU samples — the
fold is at the completing half-dot, but the SAMPLE is at the fixed M-cycle
boundary. **The miss is on the CPU-sample side, not the fold side.**

## (b) What "STAGE 1" is, and the net-zero proof

STAGE 1's deliverable is the eager half-dot substrate. It is present in three
already-committed pieces, all `eager_value`-gated (production/tier2 byte-identical):

1. **`3454e14` — eager `tick_machine` half-dot advance** (`interconnect/tick.rs:62-94`).
   The `for cc in 1..=4` loop runs `self.ppu.tick_half()` (2 half-dots/dot SS, 1 DS)
   and folds via `fold_ppu_events` only on `self.ppu.dot_completed()` (the even
   half). On the aligned grid `dhalf` stays 0 → byte-identical to the whole-dot
   advance. **This is exactly the task's STAGE-1 shape** ("reuse
   `tick_half`/`dhalf`/`fold_ppu_events`-on-completing-half, keep all transitions
   whole-dot").
2. **`Ppu::tick_half` odd-half body** (`ppu/engine.rs:254-277`): under `eager_value`
   the odd half (`dhalf 0→1`) already runs `strobe_tick()` (Part-A-render write
   strobe) AND `stat_update_half()` (#11dw), returning `0` IF so the fold is inert
   there.
3. **`Ppu::stat_update_half`** (`ppu/stat_irq/reclock.rs:451-493`, #11du/#11dv/#11dw):
   the odd-half STAT-engine level re-eval — "a coincident FF41 write-commit,
   LYC re-latch, or mode-0 source rise resolves at its true SUB-dot phase." **This
   IS the Part-C two-latch convergence mechanism**, already built and already used
   for the DMG write-commit case (armed via `eng_stat_half`).

Plus `read_pos_hd` (`engine.rs:297`, the read-side half-dot, task acknowledges DONE)
and `strobe_tick` (the half-dot write strobe).

**Net-zero proof (by construction + measurement):** no `crates/` bytes changed this
session (`git diff 7720913 -- crates/` empty), so `golden_fingerprint` (PASS),
mooneye, tier2 291, and EV CGB 287 / EV DMG 46 are all unchanged by definition.

**Why there is no additional net-zero increment.** The one conceivable delta —
calling `fold_ppu_events` (or its `intf |=` half) on the ODD half too — is either
(i) a pure no-op (`tick_half` returns `0` on the odd half, so `intf |= 0`; the
edge-consumers `take_stat_late`/`take_m0_rise`/… and the `hdma_trigger_level`
detector must then be guarded to stay completing-half-only, reproducing current
behavior byte-for-byte with zero effect), or (ii) if it instead makes the odd half
PRODUCE a non-zero IF, that MOVES the rise's fold phase — a transition move, NOT
net-zero, and explicitly out of STAGE-1 scope. `stat_update_half` already captures
an armed odd-half rise into `pending_if`, folded at the same dot's completing half,
so even (i) delivers nothing Part-C needs. Per the reclock.rs:456-461 comment, a
BARE unconditional odd-half re-eval "would just re-run the whole-dot engine's edge
WITHOUT its squash/pending logic and shuffle verdicts" — so it is gated behind
`eng_stat_half` precisely to stay net-zero.

## (c) The concrete staged sub-plan for the convergence — and the structural blocker

Part-C's goal (task): the two-latch `mode_for_interrupt` dispatch coherence recovers
the ~15 counter-pinned dispatch/ENGINE-IF bar rows (`m1statwirq_3` + the lycEnable/
m2enable/ly0/lyc153int/miscmstatirq dispatch web). The visible-side collapse
(deleting the `mode0.rs` `early_lead` case-tower + the seven `vis_mode_read` shadow
laws, HALFDOT §3-C) is a separate render-side cleanup and is NOT what unblocks
these rows (per `eager-partA-buildplan` §5.2: the shadow laws are window-LENGTH
closed forms, not flip-granularity paper — they do not gate the dispatch class).

### The atomicity group (converge together or not at all)

Per `eager-lyc153-rise-retime` §"A/B WELD TABLE" and `eager-write-halfdot` §3, the
dispatch class is ONE welded group: `m1statwirq_3` (want dispatch phase A) and its
siblings `lycEnable/lycwirq_trigger_ly00_stat50_3`, `lyc153int_m2irq_2`,
`late_retrigger_2`, `m2enable/late_enable_2` (want phase B) present a **bit-identical
engine rise state** `(line, dot, mfi, lyc_interrupt_line, eng_stat, stat_en)` and
demand OPPOSITE dispatch M-cycles. Every uniform PPU-side lever (rise-dot 6→4,
write-debt `-N`, `eng_stat_half` arm) shifts them EQUALLY → +5/−13 shuffles (never
net-positive, drops SameBoy-pass). Expected RED→GREEN: the whole group flips
together ONLY when the discriminator — the CPU M-cycle dispatch phase — becomes
observable; nothing partial converges.

### The one lever that separates them (and why it is blocked)

The discriminator is NOT representable on the PPU/fold side; it is the CPU's
`intf` sample phase. The fix is the **coherent per-T CPU/PPU dispatch sample**: the
CPU must read `pending()` at the true T the PPU set the rise, not at the fixed
M-cycle boundary. Executable seam:

- `cpu/mod.rs:129 pending_dispatch` / `interconnect/speed.rs:661 dispatch_pending_impl`
  — today returns `pending()` at the boundary (eager) or the tier2 deferred view.
  The needed change: sample `intf` at the sub-M-cycle half-dot the fetch M-cycle's
  interrupt check lands on, re-observing an odd-dot rise folded by `tick_half`
  inside that M-cycle.
- This requires `tick_machine` to expose the intra-M-cycle fold position (it already
  advances half-dot-by-half-dot; the seam is to let the dispatch check run AGAINST a
  mid-M-cycle `intf` snapshot rather than the post-M-cycle one) — i.e. interleave the
  CPU interrupt sample into the half-dot loop, or record the half-dot at which each
  IF bit rose (`pending_if_hd`) and gate the dispatch on the fetch M-cycle's sample T.

**THE STRUCTURAL BLOCKER (refuted 4× + twice today):** doing this without moving the
dispatch DOT is the open problem. Every realized form so far either
- **moves the dispatch** (the `-2`/imminent-rise fold, #11br; `dispatch_reclock`/
  `dispatch_retime` on eager, #11dq) → hangs mooneye `intr_2`/`int_hblank`/`di_timing`
  at B=42 (the invariant tripwire), because a whole-M-cycle dispatch move on the
  eager clock is not sub-dot-separable (`eager-dispatch-retime-scoping` §5); or
- **routes through the deferred clock** (#11cl/#11cj) → double-advances the
  `tick_machine`-ticked PPU / breaks the +16 ISR reads, and IS the tier2 DMG-timer
  wall (tier2 DMG 116 vs eager 46); or
- **moves the rise dot / write-commit** (a uniform PPU lever) → the +5/−13 /
  +10/−8 shuffle (`eager-lyc153-rise-retime`, `eager-write-halfdot`).

The map-cited SameBoy mechanism (`GB_cpu_run` sampling `IF` at the M-cycle's
trailing T on the SAME per-T interleave that drives `GB_STAT_update`) is a genuine
per-T CPU↔PPU coupling. slopgb's `tick_machine` advances the whole PPU M-cycle then
samples — the half-dot substrate exists on the PPU side (folds are per-completing-
half) but the CPU sample is not interleaved into it. **That interleave — a per-T
dispatch-sample coherence that catches the odd-dot rise WITHOUT relocating the
dispatch instruction boundary — is the real, unbuilt Part-C lever, and it is a CPU-
core change, not a PPU-fold change.** STAGE 1 as scoped (PPU-side half-dot fold) was
already complete; it does not, and cannot, move this class.

### Recommended next-session shape (if pursued)

1. **Instrument the sample side, not the fold side.** Add a temporary
   `pending_if_hd` (the half-dot each IF bit rose within the current M-cycle) and,
   in `dispatch_pending_impl`, A/B whether gating the STAT bit on
   `rose_hd <= fetch_sample_hd` separates `m1statwirq_3` from `lycwirq_trigger_
   ly00_stat50_3` at bit-identical rise state (the two `eng_stat=40` siblings). If
   they separate, the per-T sample is the lever; if not, the discriminator is
   upstream of even the half-dot (DIV/opcode phase) and Part-C on the eager clock is
   genuinely floored.
2. **Do NOT move the dispatch dot** (`intr_2`/`di_timing`/`int_hblank` B=42) and do
   NOT re-run any uniform PPU lever (rise-dot, write-debt, bare `stat_update_half`
   arm) — all measured, all shuffle. Gate every candidate on the flagon EV two-bin
   (287/46) with `classify_cgb_regr`/`classify_dmg`: zero SameBoy-pass drops or it is
   the same wall.
3. If the per-T sample IS separable, the whole atomicity group (§above) converges
   together, `m1statwirq_3` `0→2`, expected +~15 with 0 drops. That is the C3-flip's
   last class.

## Gates (all hold; tree byte-identical)

- `git diff 7720913 -- crates/` **empty** (no source changed; only this map added).
- `golden_fingerprint` PASS (42.72s); EV CGB 287 / EV DMG 46; `m1statwirq_3` OFF 2 /
  EAGER 0 reproduced. Defaults NOT flipped; no push.

## Reproduction

```sh
git checkout finish-port-halfdot   # @ 7720913
export CARGO_TARGET_DIR=target/partc
cargo test -p slopgb-core --test gbtr --release --features port_probe --no-run
BIN=$(ls -t target/partc/release/deps/gbtr-* | grep -v '\.d$' | head -1)
run(){ SLOPGB_ROWLIST=$PWD/scratchpad/$1 SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 \
       $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture 2>&1 | grep flagon_probe; }
run cgb_rowlist.txt   # 287
run dmg_rowlist.txt   # 46
cargo build -p slopgb-core --example run_gambatte --release --features port_probe
G=target/partc/release/examples/run_gambatte
R=test-roms/game-boy-test-roms-v7.0/gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb
$G $R dmg                                   # OFF   -> 2 (pass)
SLOPGB_EAGER=1 $G $R dmg                     # EAGER -> 0 (fail)
SLOPGB_EAGER=1 SLOPGB_S5DBG=1 $G $R dmg 2>&1 | grep 'SLOPGB dispatch'  # ly=153 dot=6 mfi=1 lycln=1
# The infra is pre-existing: `git show 3454e14 -- crates/slopgb-core/src/interconnect/tick.rs`
#   and `ppu/stat_irq/reclock.rs::stat_update_half` (#11dw).
```
