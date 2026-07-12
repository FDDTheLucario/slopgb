# eager `m1statwirq_3` — the per-T interleave PoC: the class is NOT CPU-T-atomic; the lever is the line-153 LYC **emission dot**, but the minimal slice shuffles the LYC-153 cluster (2026-07-12)

Base: `finish-port-halfdot @ a0436bc` (isolated worktree `pert-interleave-poc`;
no push, no default flip; tree byte-identical after this session — only this map
+ probe scaffolds, all reverted, `git diff` on `crates/` empty).

## Task

Decisive feasibility PoC for the TRUE per-T CPU/PPU interrupt-sample interleave
(SameBoy `GB_cpu_run` coprocess model) on the eager clock, targeting
`gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb` (DMG; OFF `2` / eager `0`) —
the row six prior investigations (#11br, #11dq, #11dt, the ISR-context re-attack,
+2 more) each declared "counter-pinned dispatch / CPU-T-atomic, resolvable only
by a per-T CPU IF sample that moves the dispatch → breaks `intr_2`."

## Verdict: the CPU-T-atomic premise is REFUTED; the true lever is a PPU emission-dot correction (no dispatch move). But the minimal slice is a net-negative cluster shuffle (−8 SameBoy-pass) → FLOORED as an isolated slice; the clean fix is the bounded LYC-153 cluster re-host.

The decisive finding overturns every prior verdict's premise: **this is NOT a CPU
dispatch-recognition problem at all.** The eager `stat_update` engine (the SameBoy
`GB_STAT_update` port, run under `leading_edge_reads`) emits the line-153 LYC=153
STAT IRQ at slopgb **dot 6** — the READ frame (`ly_for_comparison` `GB_SLEEP(14,4)`,
wilbertpol `ly_lyc_153-C`) — while the dispatch/IF instant is **dot 4** (SameBoy
sets IF at `display_cycles == 4`; the production gambatte `stat_events_tick` engine
fires at dot 4 and PASSES). The +2 is the read-debt (read frame = cc+4). The
dot-6 fold lands mid-M-cycle, so the eager CPU recognizes it at the *next* boundary
(dot 8) — one M-cycle late — and the ISR's fixed-cycle wait carries that offset to
the decisive FF41 glitch write.

Correcting the emission to dot 4 recovers `m1statwirq_3` (`0→2`) with **ALL
mooneye tripwires green** — the CPU dispatch boundary never moves (mooneye 93×3,
`intr_2`/`di_timing`/`int_hblank`/`ie_push`/`rapid_di_ei` all pass under eager),
`golden_fingerprint` byte-identical. The prior "moving the recognition → the
`intr_2` tripwire" fear does NOT apply: the lever is a PPU IF-emission dot, not a
CPU-T sample.

But the minimal isolated slice is **net −8 SameBoy-pass** (−5 recovered / +13
dropped): the eager whole-dot frame's compensating offsets for the *sibling*
line-153 LYC-153 cluster (window `late_wy` WY-write timing, `lyc153int_m2irq`
blocking, `lycwirq_trigger` retrigger) are calibrated for the un-shifted dot-8
recognition. The drop is proven to be the **dispatch-dot shift itself** (Test B,
below), not an implementation side-effect — so there is no PPU-representable
discriminator (the sibling ROMs dispatch the *same* LYC=153 rise to the same
handler-entry; only what each ISR does *after* differs, which the PPU cannot see).
The clean ship = re-host that cluster's eager-frame compensations onto the
dispatch-frame emission (a bounded multi-row re-calibration, tractable, but beyond
a minimal PoC and not the full S7 rewrite).

## SameBoy `GB_cpu_run` IF-sample point (the model the task cited)

`sm83_cpu.c:1610 GB_cpu_run`: interrupt recognition is `interrupt_queue =
interrupt_enable & io_registers[GB_IO_IF] & 0x1F` at **line 1633**, the
instruction-boundary sample (before the `cycle_read(pc++)` fetch at :1721). IF is
set earlier, lazily, by `GB_STAT_update` during the `pending_cycles` hardware
advance (`GB_advance_cycles` inside `cycle_read`/`cycle_write`, :85-146). So
SameBoy ALSO recognizes at an M-cycle boundary — it is **not** a per-T CPU sample.
The physical timing lives in `GB_STAT_update` (`display.c:523`): the LYC-coincidence
`stat_interrupt_line` rises (`:557-559`) and `io_registers[GB_IO_IF] |= 2`
(`:577`) at the point `lyc_interrupt_line` latches — for line 153, at
`display_cycles == 4` (traced `SBIF su ly=153 dc=4`). **The interleave the class
needs is the PPU IF-set dot, not the CPU recognition granularity.**

## The decisive trace (rom-diff-weld step 1b/2, full CPU-state + PPU-dot probes)

Instrumented (all reverted; tree byte-identical): a `Bus::write` FF41 dump, a
`fold_ppu_events` STAT-rise dump `{ly,dot,cc,cyc}`, a `dispatch_pending_impl` dump,
and a completing-dot `{ly,dot,cc,cyc}` window trace. SameBoy: `SB_TRACE=1` on the
1.0.2 tester (`SBIF su` = the `GB_STAT_update` IF|=2 point).

| clock | LYC=153 STAT IF fold | cc | dispatch recognized | decisive FF41 write | glitch-eval | verdict |
|---|---|---:|---|---|---|---|
| OFF (gambatte engine) | ly153 **dot 4** | cc 4 (M-cycle boundary) | ly153 dot 4 | ly153 d452→wrap | **ly0 d0** (hblank) | **2** ✓ |
| EAGER (stat_update engine) | ly153 **dot 6** | cc 2 (mid-M-cycle) | ly153 **dot 8** (next boundary) | ly0 d0 | **ly0 d4** (mode 2/3) | **0** ✗ |
| SameBoy (`GB_STAT_update`) | `dc=4` (SBIF su), dispatch `cfl=0` (SBDISP) | — | line-153 start | — | — | **2** ✓ |

The completing-dot pipeline is **byte-identical** OFF vs eager (dot D completes on
the same cc/cyc in both) — so the divergence is NOT a CPU/PPU phase drift; it is
purely **which internal dot the STAT engine fires**: OFF `stat_events_tick` at
`dot==4`, eager `stat_update_tick` at the `ly_for_comparison`-153 dot 6. SameBoy
agrees with OFF (`dc=4`).

## The fix + the shuffle (the two arms, both measured)

A 6-line `eager_value`+`!is_cgb()`+`line==153`+`dot==4`+`lyc==153`+LYC-enabled arm
in `stat_update_tick` folding the IRQ at the dispatch frame (dot 4):

- **Arm A** (`lyc_interrupt_line = true` at dot 4): m1statwirq `0→2`; DMG EV two-bin
  **46 → 54** (−5 recovered / +13 dropped).
- **Arm B** (bare `pending_if |= IF_STAT` at dot 4, no latch touch, no
  `force_level`): **IDENTICAL** −5/+13. → The drops are the **dispatch-dot shift**
  (the shared LYC=153 ISR firing 1 M-cycle early), NOT the persistent-latch
  side-effect. `stat_update.update()` re-derives the line level from
  `lyc_interrupt_line` every tick, so emission and the readable latch are welded —
  but that weld is irrelevant: even the bare intf poke shuffles the cluster.

Recovered (−, base-fail → after-pass): `m1statwirq_3`,
`lyc153int_m2irq_ifw_1`, `late_wy_1` (×2), `late_wy_FFto2_ly2_scx5_2`.
Dropped (+, base-pass → after-fail, all SameBoy-pass): `lyc153int_m2irq_2`,
`lyc153int_m2irq_late_retrigger_2`, `lycwirq_trigger_ly00_stat50_3`, and 10
`window/arg/late_wy_*_3` (out0). The `_1`/`_2` recover while `_3` drop = the
canonical uniform-boundary sibling swap: moving the shared dispatch flips which
output leg passes. SameBoy fires the LYC=153 IRQ at `dc=4` for the DROPPED rows
too (traced `SBIF su ly=153 dc=4`, stat=45/65) — so dot-4 is directionally
correct; the drops are the eager whole-dot frame's cluster compensation, not a
SameBoy disagreement.

## Gates (Arm A/B; both reverted, tree byte-identical @ a0436bc)

- `m1statwirq_3` eager **0→2** ✓ (both arms).
- `golden_fingerprint` byte-identical ✓ (`eager_value`-gated; OFF never enters).
- mooneye **93/93 ×3** (OFF / `SLOPGB_MOONEYE_EAGER=1` / `SLOPGB_MOONEYE_RECLOCK=1`)
  ✓; every tripwire green under eager (`intr_2_*` incl. `_sprites`, `di_timing-GS`,
  `int_hblank_*`, `ie_push`, `rapid_di_ei`) — **the dispatch boundary did NOT move.**
- flagon_probe EV two-bin: CGB **287 → 287** (DMG-scoped, zero CGB drift, zero
  drops) ✓; DMG **46 → 54** (−5/+13) ✗ — **13 SameBoy-pass drops** → the gate
  "ONE SameBoy-pass drop = FLOORED" is not met for the isolated slice.
- tier2 unchanged (`eager_value` ≠ `tier2_reclock`; mooneye RECLOCK 93/93).

## Root cause + the (tractable) build-out

The eager `stat_update` engine hosts the line-153 LYC=153 event ENTIRELY at the
READ frame (dot 6, `ly_for_comparison` `GB_SLEEP(14,4)`, pinned by wilbertpol
`ly_lyc_153-C`), but the IF/dispatch instant is the dispatch frame (dot 4 = SameBoy
`dc=4` = the +2-read-debt-earlier position). Every OTHER LYC line already latches
at dot 4 (`ly_for_comparison` returns the line at `dot>=4`); line 153 is the sole
line with the special dot-6 table, and it is the sole LYC line where the eager IRQ
lands mid-M-cycle. `m1statwirq_3` is the one row sensitive to the resulting 1-M
dispatch lateness; the sibling `late_wy`/`lyc153int_m2irq`/`lycwirq_trigger`
cluster is calibrated (in the whole-dot eager frame) for the un-shifted dot-8
recognition, so the shared-dispatch move shuffles them.

**Build-out (not CPU-T-atomic — no S7 CPU rewrite):** re-host the line-153 LYC-153
eager frame — split the IF EMISSION onto the dispatch frame (dot 4) while the
`ly_for_comparison` READ latch stays dot 6 (the split already exists for the
disable direction: the C015 early-delivery at `reclock.rs:449`, `pending_if` +
`force_level`) — AND re-derive the `late_wy` window WY-write / `lyc153int_m2irq`
blocking / retrigger compensations for the dot-4 dispatch. Bounded ~13-row
cluster; each leg is a standard eager-frame recalibration, not a structural
rewrite. The prior "per-T CPU IF sample, moves dispatch → `intr_2`" scope is wrong:
the lever never touches the CPU dispatch.

## Reproduction

```sh
git checkout finish-port-halfdot   # @ a0436bc (isolated worktree)
export CARGO_TARGET_DIR=target/pert
cargo build -p slopgb-core --example run_gambatte --release --features port_probe
BIN=target/pert/release/examples/run_gambatte
R=test-roms/game-boy-test-roms-v7.0/gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb
$BIN $R dmg                    # OFF  -> 2
SLOPGB_EAGER=1 $BIN $R dmg     # EAGER -> 0
# SameBoy IF-set dot: SB_TRACE=1 <sameboy_tester> --dmg --length 2 --boot dmg_boot.bin $R | grep 'SBIF su ly=153'
#   -> SBIF su ly=153 cfl=0 dc=4   (dispatch frame; slopgb eager engine emits at dot 6)
# The fix arm (reverted) — stat_irq/reclock.rs before `let mfi = ...`:
#   if eager_value && !is_cgb && line==153 && dot==4 && enabled && !glitch_line
#      && lyc==153 && eng_stat & STAT_SRC_LYC != 0 { self.lyc_interrupt_line = true; }
#   -> m1statwirq 0->2, golden+mooneye93x3 hold, CGB EV 287/287, DMG EV 46->54 (−5/+13).
```
