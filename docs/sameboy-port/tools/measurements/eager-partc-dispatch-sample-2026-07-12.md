# HALFDOT Part-C — the dispatch-sample experiment: the two `eng_stat=40` siblings do NOT separate on `fetch_sample_hd`; the eager Part-C class is FLOORED (2026-07-12, #11dt)

Base: `finish-port-halfdot @ 293dee6`. Task (#11dt, the decisive Part-C
experiment): add a `pending_if_hd` timestamp for each IF-source rise and A/B
whether gating the STAT bit's dispatch visibility on `rose_hd <=
fetch_sample_hd` SEPARATES the two bit-identical-rise-state siblings that demand
opposite dispatch M-cycles — `m1statwirq_3_dmg08_out2` (want phase A → OCR `2`)
vs `lycEnable/lycwirq_trigger_ly00_stat50_3` (passes eager already → phase B).

**Result: FLOORED (no code shipped, tree byte-identical @ 293dee6).** The two
siblings have a **bit-identical decisive STAT dispatch** — same `fetch_sample_hd`
(139670), same `rose_hd` (139666), same engine rise (`ly153 dot6 lycln=1`), same
dispatched bit (`w=02`). The visibility gate cannot separate them, and it is
structurally the WRONG tool anyway: `m1statwirq_3` needs its single dispatch
EARLIER (phase A), and a `rose_hd <= fetch_sample_hd` gate can only ever DELAY a
dispatch, never advance it. A rom-diff-weld full-trace then pinned the true
failure — a downstream DMG STAT-write-glitch whose fire flips on a pure 4-dot
handler-lateness cascading from the M-cycle-boundary IF sample — and the one
write-frame arm that recovers `m1statwirq_3` was measured to SHUFFLE: **+1 / −12
SameBoy-pass drops**. The discriminator is the CPU M-cycle dispatch phase,
upstream of the half-dot and not representable without moving the dispatch dot
(the refuted `intr_2` B=42 lever). Eager Part-C needs the T-exact CPU-core
per-T IF sample (out of scope), exactly as `eager-partc-stage1` §(c) predicted.

## (a) The sibling A/B table — they do NOT separate on `fetch_sample_hd`

Instrumented (reverted): a monotonic half-dot clock `dbg_hd` (one tick per
`Ppu::tick_half` on the eager loop), a STAT-rise stamp `dbg_stat_rose_hd` (set on
the `intf` STAT 0→1 fold in `fold_ppu_events`), and a probe in
`dispatch_pending_impl` printing `fetch_sample_hd = dbg_hd` (the half-dot the
end-of-fetch dispatch check samples), `rose_hd`, and both PPU positions. Eager,
DMG, `SLOPGB_EAGER=1 SLOPGB_S5DBG=1`, the decisive STAT dispatch:

| ROM | want | got (eager) | dispatch `fetch_sample_hd` | rise `rose_hd` | rise pos | sample pos | `w` |
|---|---|---|---:|---:|---|---|---|
| `miscmstatirq/m1statwirq_3` | 2 | **0** | **139670** | **139666** | ly153 d6 | ly153 d8 | 02 |
| `lycEnable/lycwirq_trigger_ly00_stat50_3` | E2 | **E2** (pass) | **139670** | **139666** | ly153 d6 | ly153 d8 | 02 |

**Bit-identical at the decisive dispatch.** Both engines raise the LYC=153
coincidence at `ly153 dot6 lycln=1` (SameBoy `GB_SLEEP(14,4)`, `6..=7 => 153`),
both fold it at the completing half (`rose_hd` 139666), and both CPU fetch checks
sample at the same M-cycle boundary (`fetch_sample_hd` 139670, `+4 hd = +2 dots`
after the rise). Per the experiment's decision rule (identical `fetch_sample_hd`
⇒ the discriminator is upstream of the half-dot ⇒ FLOORED), the per-T sample gate
is not the lever. `lycwirq_trigger_ly00_stat50_3` already PASSES eager — so the
pair is not a want-opposite weld at the dispatch; the sibling's handler tolerates
the boundary dispatch, `m1statwirq_3`'s does not.

## (b) The direction: the visibility gate can only DELAY; `m1statwirq_3` needs an ADVANCE

`m1statwirq_3` fires exactly ONE STAT dispatch in the whole run (both clocks):

| clock | dispatch sample | rise | verdict |
|---|---|---|---|
| OFF (production `stat_events_tick`) | ly153 **d4** | ly153 **d4** (coincident) | **2** ✓ |
| EAGER (reclock `stat_update_tick`) | ly153 **d8** | ly153 **d6** | **0** ✗ |

The eager dispatch lands 4 dots later than OFF: +2 from the later (SameBoy-exact)
d6 rise and +2 from sampling at the M-cycle boundary (d8) vs OFF's rise-coincident
d4. To match OFF/hardware the eager dispatch must fire EARLIER (phase A). A
`rose_hd <= fetch_sample_hd` gate only HIDES a bit that rose after the sample T —
it can push a dispatch LATER, never earlier — and there is no spurious earlier
dispatch to hide (there is only one). The gate is structurally incapable of
recovering `m1statwirq_3`.

## (c) rom-diff-weld full-trace — the real failure is a downstream DMG STAT-write-glitch

Per the `rom-diff-weld` skill ("identical at the sample ≠ welded; the
discriminator is a WRITE or render-FSM state"), traced every FF-page + VRAM
access of `m1statwirq_3` under OFF vs EAGER (5805 lines each). The frame is
bit-identical up to the STAT dispatch; the verdict is an **FF0F read** (the ISR
disables all STAT sources, clears IF, re-writes FF41=00, then reads FF0F):

```
OFF:   dispatch ly153 d4 → WR ff45=ff d36 → WR ff41=00 d64 → WR ff0f=00 d76
       → WR ff41=00 ly153 d452 → RD ff0f ly0 d12 -> e2  intf=02 lastrose=ly153d4
EAGER: dispatch ly153 d8 → WR ff45=ff d40 → WR ff41=00 d68 → WR ff0f=00 d80
       → WR ff41=00 ly0  d0   → RD ff0f ly0 d16 -> e0  intf=00 lastrose=ly153d6
```

The STAT bit set on OFF (`intf=02`) has `lastrose=ly153d4` — i.e. it was NOT set
by a fresh PPU dot-fold (which would update `lastrose`); it was set by the FF41
**write** itself, the DMG STAT-write glitch (`stat_write_trigger_dmg`). The 4th
FF41 write's glitch eval:

| clock | glitch eval pos | old | data | `fire` | FF0F read |
|---|---|---|---|---|---|
| OFF   | **ly0 d0** (dot<4 ⇒ hblank branch, `old&HBLANK==0`) | 00 | 00 | **1** | e2 → **2** |
| EAGER | **ly0 d4** (dot≥4 ⇒ mode-2/3 branch, `!lyc_high`) | 00 | 00 | **0** | e0 → **0** |

Identical `old`/`data`; the sole discriminator is `self.dot` (0 vs 4) — the
whole ISR runs 4 dots late because the dispatch was caught at the M-cycle
boundary (d8) instead of per-T at the rise (d6), and the fixed-cycle wait loop
between the 3rd and 4th writes preserves that offset across the ly153→ly0 wrap.

## (d) The write-frame arm SHUFFLES — +1 / −12 SameBoy-pass drops

The candidate representable fix: back-date the eager DMG glitch dot by 4 (evaluate
`stat_write_trigger_dmg` at the write's leading-edge frame), `eager_value`-scoped.
It recovers the target — `m1statwirq_3` eager `0 → 2` — but the glitch position is
CALIBRATED (the comment cites `gbmicrotest stat_write_glitch_l0/l1/l143/l154` +
the gambatte `late_enable` family, all DIRECT writes with no late ISR). A/B on the
DMG EV two-bin (`flagon_probe`, `dmg_rowlist.txt`, base EV DMG = **46**):

| | EV DMG fail |
|---|---:|
| base (293dee6) | 46 |
| with −4 glitch arm | **57** (+11) |

- **Recovered (1):** `miscmstatirq/m1statwirq_3`.
- **Newly broken (12, all `_out0` want-no-fire SameBoy-pass):**
  `lycEnable/late_ff41_enable_3`, `m0enable/late_enable_3`,
  `m1/m1irq_late_enable_3`, `m2enable/late_enable_2`,
  `m2enable/late_enable_after_lycint_3`,
  `m2enable/late_enable_after_lycint_disable_3`, `m2enable/late_enable_ly0_2`,
  `m2enable/late_enable_m1disable_ly0_3`, `m2enable/late_m1disable_ly0_3`,
  `m2enable/lyc1_late_m2enable_lycdisable_2`, `miscmstatirq/lycflag_statwirq_4`,
  `miscmstatirq/m0statwirq_2`.

`m1statwirq_3`'s write follows a late-dispatched ISR (needs −4 to compensate the
dispatch lateness); the 12 broken rows are DIRECT writes at the calibrated glitch
position (need no back-date). No representable PPU/write state term separates
"write after a late ISR dispatch" from "direct write" — that IS the CPU dispatch
M-cycle phase, upstream of the half-dot. Confirms the `eager-lyc153-rise-retime`
"+5/−13 shuffle" characterization and the `eager-partc-stage1` §(c) floor.

## The verdict

The eager Part-C dispatch/ENGINE-IF class is **FLOORED** on the current clock.
The one discriminator that would separate the siblings — the CPU M-cycle phase at
which the fetch interrupt-check samples `IF` relative to the PPU dot grid — is not
observable on the PPU/fold/write side and is not representable as a latch or a
render-FSM term. Every realized lever is one of:

- **the visibility gate** (this session): cannot advance a dispatch, the siblings
  are bit-identical, does not move `m1statwirq_3`;
- **a write-frame back-date** (this session): +1 / −12, a uniform shuffle of the
  calibrated glitch grid;
- **moving the dispatch dot** (#11br/#11dq): hangs mooneye `intr_2`/`di_timing`/
  `int_hblank` at B=42 (the invariant tripwire);
- **the deferred clock** (#11cl/#11cj): the tier2 DMG-timer wall (DMG 116 vs
  eager 46).

The honest floor is **1 documented drop** (`m1statwirq_3` + its ~14-row
dispatch/ENGINE-IF cohort, which share the same phase discriminator). Closing it
requires the T-exact CPU-core change: interleave the CPU `IF` sample into the
half-dot PPU advance so a rise folded mid-M-cycle is caught at its true T WITHOUT
relocating the dispatch instruction boundary. That is a `cpu/mod.rs` +
`interconnect/tick.rs` rewrite (a per-half-dot CPU↔PPU coprocess), out of this
session's scope.

## Gates (all hold; tree byte-identical)

- `git diff 293dee6 -- crates/` **empty** (all instrumentation + the test arm
  reverted; only this map added). Defaults NOT flipped; no push.
- `golden_fingerprint` PASS (byte-identical, port_probe env-gated).
- Net-zero reference reproduced: EV DMG **46** (base), EV CGB 287 (unchanged);
  `m1statwirq_3` OFF `2` / EAGER `0`; the −4 arm A/B is +1 / −12 as tabled.

## Reproduction

```sh
git checkout finish-port-halfdot   # @ 293dee6
export CARGO_TARGET_DIR=target/partc2
cargo build -p slopgb-core --example run_gambatte --release --features port_probe
G=target/partc2/release/examples/run_gambatte
R=test-roms/game-boy-test-roms-v7.0/gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb
$G $R dmg                    # OFF   -> 2 (pass)
SLOPGB_EAGER=1 $G $R dmg     # EAGER -> 0 (fail)
# The sibling A/B, the FF0F-read verdict trace, and the DMG-glitch fire A/B were
# taken with temporary probes (dbg_hd / dbg_stat_rose_hd in interconnect.rs +
# tick.rs; dispsample in speed.rs::dispatch_pending_impl; RD/WR FF0F/FF41 in
# bus.rs; ff41wr fire in ppu/regs.rs) — Part-C convention, all reverted.
# The −4 write-frame arm (SHUFFLE, +1/−12): in ppu/stat_irq.rs
# stat_write_trigger_dmg, evaluate the hblank/dot terms at
# `if self.eager_value && self.dot >= 4 { self.dot - 4 } else { self.dot }`.
```
