# SameBoy cycle-exact core port — staged plan (the multi-session work package)

Status: **port begun** (2026-06-21). The spec is complete and verified; the
implementation is an atomic, multi-session rewrite. This file is the durable
roadmap that sequences it.

## Why a port (verified finding)

SameBoy passes ~420 of slopgb's baselined-failing gambatte rows via a
cycle-exact (T-cycle) timing model. Independently re-verified this session:
SameBoy's headless tester renders **`3`** (non-blank, OCR `300000`) for the
kernel `m2int_m3stat_1` — the exact value slopgb baseline-fails — and `0` for
`m0int_m3stat_2`, while also passing mooneye `intr_2_mode0_timing`. So the
prior "class-A/B floor is CPU-context-irreducible" verdict is **wrong**: the
floor is a coarseness of slopgb's whole-dot / tick-then-access model, not a
hardware contradiction. Evidence + methodology: `sameboy-parity-plan.md`,
gap list `/tmp/sb_gap_list.txt` (332/456 rows have non-zero expectations →
real signal, not blank-screen false-positives).

## The kernel resolver (the mechanism the port reproduces)

From `ppu-timing-map.md` §6 — SameBoy separates the two identical
`ldh a,(FF41)` reads with **no call-stack inspection**, via three cooperating
DOF slopgb lacks:

1. **Leading-edge sampling** (`cpu-timing-map.md` §2): a read latches the byte
   at the M-cycle's leading edge (cc+0), deferring its own 4 cycles
   (`pending_cycles`); slopgb samples at cc+4 (trailing edge).
2. **Decoupled `mode_for_interrupt`** (`ppu-timing-map.md` §2): the CPU-visible
   STAT mode and the IRQ-facing mode are two fields updated on different dots.
3. **Opposite-sign anchor offsets**: mode-2 IRQ fires **1 dot before** visible
   mode→2 (`display.c:1787` vs `1792`); mode-0 IRQ fires **1 dot after** visible
   mode→0 (`display.c:2091` vs `2108`). The 2-dot relative swing + cycle-exact
   mode-3 length (`167 + SCX&7` + sprite/window penalties, `display.c:1493`)
   lands the two reads on opposite sides of the visible 3→0 edge.

## Why it is atomic / not single-session

slopgb's boundary dots are a self-consistent hand-fit around **cc+4** reads
(e.g. `m0_flip_events` puts the mode-0 flip "2 dots before pipe end" precisely
so a cc+4 read passes mooneye `intr_2_mode0_timing`). SameBoy's are
self-consistent around **cc+0** reads. You cannot move one piece of SameBoy's
model into slopgb's frame without the 4-dot read-phase change dragging the whole
recalibration with it — which is why every single-lever attempt measured an A/B
swap: R3 (cc+2 MID read) +19/−23; S3-boundary-event (DS decouple) +5/−12; G2c
(dispatch tag) +57/−34-breaks-canonical (`ppu-subdot-ladder.md`). The foundation
(deferred-commit reads + decoupled mode) and the first boundary set must land
**together**; intermediate states are red until convergence.

## Stage order (each gbtr-baseline + golden-frame-hash + mooneye-439 gated, revert-on-regression)

The TDD task list (`/tdd-test-plan` output, this session) maps onto these:

- **S0 — executable red spec.** `#[ignore]`'d gbtr tests pinning the SameBoy
  kernel targets (m2int=3, m0int=0) + the mode-2/mode-0 IRQ split dots. (TDD #1)
- **S1 — CPU deferred-commit scaffold.** `pending_cycles` debt counter + a
  `flush_pending` hook at the instruction boundary, behaviorally inert (sample
  point unchanged). Conserved per-instruction T-count. Net-zero gate. (TDD #2)
  - Seam: `Bus::read`/`write`/`tick` (`interconnect.rs:671`); flush at the CPU's
    per-instruction boundary (`cpu/execute.rs` dispatch loop). SameBoy
    `sm83_cpu.c:85` (`cycle_read`), `:336` (`flush_pending_cycles`), `:321`
    (`cycle_no_access` = `pending += 4`).
- **S2+S3 — ATOMIC: leading-edge reads + boundary re-derivation.** Switch
  FF41/OAM/VRAM/palette reads to cc+0 sampling; decouple visible mode from
  `mode_for_interrupt`; place the mode-2 IRQ 1 dot before / mode-0 IRQ 1 dot
  after their visible edges; mode-3 length = `167 + SCX&7` + penalties. Un-ignore
  S0. This is the convergence point — kernel pair lifts, mooneye + golden hold.
  (TDD #3+#4) Seams: `ppu/stat_irq.rs:vis_mode`, `ppu/render/mode0.rs:m0_flip_events`.
  **Executable recipe with the measured dot offsets: [`atomic-reclock-recipe.md`](atomic-reclock-recipe.md)** (2026-06-23 #11e — the read frame lands ~4 dots before SameBoy's; read + every tier2 boundary move +4 together or the kernel pin fails / the suite is red).
- **S4 — accessibility back-dating.** OAM/VRAM unblock at the visible boundary,
  CGB palette 2-dot HBlank pulse (`display.c:2090-2121`); retire the
  `m0_access_flip`/`pal_access_flip`/`stat_mode_edge` stamps. Lifts cgbpal_m3end
  + vramw + oam_access `_ds`. (TDD #5)
- **S5 — STAT engine swap.** Replace the gambatte-derived `stat_irq.rs` event
  engine with SameBoy `GB_STAT_update` rising-edge on `mode_for_interrupt` | LYC
  (`display.c:523-560`). Largest mutating stage (~123 rows). (TDD #6)
- **S6 — cycle_write conflict table.** Port the per-model conflict-staging map
  (`sm83_cpu.c:131-318`) replacing `stage_write`. Lifts speedchange/hdma rows;
  mealybug photos byte-identical. (TDD #7)
- **S7 — unify double speed + delete scaffold.** Re-unify DS onto the back-dated
  model; delete `event_phase`/`lead_eighths`/`ACCESS_PHASE` + cc-reclock
  `dot_phase` once subsumed; reproduce the INC-DS-1(+43)/task6(+84) trades. (TDD #8)

## Tooling (rebuildable)

SameBoy tester `/tmp/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester`
(`make tester`); gap-finder `/tmp/sb_gaps.py` + OCR `/tmp/sb_ocr.py`
(gambatte-glyph) → `/tmp/sb_gap_list.txt`. Re-run after each stage; the gap
count is the progress metric. Source maps: `cpu-timing-map.md`,
`ppu-timing-map.md`, `slopgb-core-map.md` (all `file:line`-grounded against
SameBoy 1.0.2).
