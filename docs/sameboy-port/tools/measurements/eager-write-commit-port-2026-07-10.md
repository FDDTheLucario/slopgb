# The eager write-conflict commit port — 12 CGB SS dispatch/IRQ web BUG rows SHIPPED (2026-07-10, #11dd)

Executes bucket 2 of the #11db reachability map
(`eager-dispatch-web-reachability-2026-07-10.md`): the 14 write-commit-frame
CGB single-speed dispatch/IRQ web rows. **12 of the 14 recovered (all 12 are
SameBoy-PASS BUG rows), + 2 FLOOR flip-gains, ZERO regressions on either
model.** The 2 unrecovered rows are an ack-squash miss, not a write-commit
miss (out of scope, see below). EV CGB **344 → 330**.

## Baselines reproduced (exact, on the #11dc base `1eb6cf4`)

`flagon_probe` two-bin: OFF CGB **486** (trusted from #11db), EV CGB **344**,
tier2 CGB **291**, EV DMG **78**. All exact.

## The mechanism — a whole-dot write-commit borrow

The eager `Bus::write` ticks the full M-cycle (`tick_machine`, 4 PPU dots at
single speed) and *then* runs `write_no_tick`, landing a register's
engine-visible commit at the **M-cycle boundary (dot D)**. SameBoy's
`GB_CONFLICT_WRITE_CPU` (FF41 STAT / FF0F IF / FF45 LYC on the CGB SS map)
commits the CPU value **one T into the M-cycle** — at single speed that is
**dot D+1**, exactly where the tier2 deferred clock lands it (`clock.write` =
`pending+1`, repark 3).

Fix (`interconnect/bus.rs`, `eager_value` + CGB + `!double_speed` +
`!lcd_shift_active` + addr ∈ {FF41,FF0F,FF45}): after `tick_machine`, **borrow
the next M-cycle's first PPU dot** — tick one whole dot (2 `tick_half`s),
`fold_ppu_events(_, 1)` so any co-instant STAT rise folds into `intf` FIRST,
then `write_no_tick` at D+1. A `eager_wr_borrow` flag makes the next
`tick_machine` skip cc 1 (tick 3 PPU dots) to restore CPU/PPU phase. Timer /
APU / serial / `cycles` are per-M-cycle (4 cc) and untouched by the PPU-dot
borrow. FF0F clears also `arm_ff0f_if_squash` (the tier2 twin) so a rise inside
the window merges into the write. Production (both flags off) and tier2
(early-returned) never reach this → **golden byte-identical, tier2 291
unchanged**.

### Trace tables (dot-by-dot, verified this session)

`ff41_disable_2` (want 2 — the LYC-latch straddle):

| event | EV before | EV after (borrow) | tier2 |
|---|---|---|---|
| LYC=6 latch | ly6 dot 0 | ly6 dot 0 | ly6 dot 0 |
| FF41=0x00 disable commit | ly6 **dot 0** | ly6 **dot 1** | ly6 **dot 1** |
| LYC STAT dispatch | — (killed) | ly6 dot 4 ✓ | ly6 dot 4 ✓ |
| digit | 0 ✗ | **2** ✓ | 2 ✓ |

`m2int_m0irq_scx3_ifw_2` (want 0 — the IF-clear straddle):

| event | EV before | EV after (borrow) | tier2 |
|---|---|---|---|
| mode-0 rise (`dispatch`) | ly1 dot 257 | ly1 dot 257 | ly1 dot 257 |
| FF0F=0x00 clear commit | ly1 **dot 256** | ly1 **dot 257** | ly1 **dot 257** |
| digit | 2 ✗ | **0** ✓ (rise folds then clears) | 0 ✓ |

The mode-0/LYC rise dots are byte-identical EV↔tier2 in every trace (confirming
#11db: NOT a render-frame or dispatch-frame miss — purely the write-commit dot).

## Results — 14 recovered, ZERO regressions

EV CGB **344 → 330** (−14). EV DMG **78 → 78** (CGB-scoped). tier2 CGB **291**
unchanged. Golden byte-identical.

Zero-regression A/B diff (`comm`): recovered 14, **new-fails EMPTY on both CGB
and DMG**. `classify_cgb_regr.py`: **12 BUG (SameBoy-pass, must-fix bar) + 2
FLOOR (SameBoy-fail, flip-gain)**.

12 BUG rows recovered (the true C3-flip bar):

```
lycEnable   ff41_disable_2 · late_ff41_enable_2 · lyc0_ff41_disable_2 ·
            lyc153_late_ff41_enable_2 · lyc153_late_m1disable_3      (5)
m0enable    lycdisable_ff41_2 · lycdisable_ff45_3                    (2)
m2enable    lyc0_late_m2enable_lycdisable_2                          (1)
m2int_m0irq m2int_m0irq_scx3_ifw_2 · m2int_m0irq_scx3_ifw_4          (2)
miscmstatirq lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_4         (1)
ly0         lycint152_lyc153irq_ifw_2                                (1)
```

2 FLOOR (flip-gain, fine): `m0enable/disable_2`, `m0enable/disable_scx4_2`.

## Scope calibration (measured, not guessed)

- **`!lcd_shift_active`**: the first cut (unscoped) recovered 16 but REGRESSED
  `lycEnable/lycwirq_trigger_ly00_stat50_lcdoffset1_1` — a SameBoy-PASS row.
  On an LCD-enable sub-dot offset line (`lcd_shift_dots != 0`) the CPU/PPU
  whole-dot grid is shifted, so the whole-dot borrow maps a co-instant rise
  onto the wrong side of the write. Traced: the FF45 borrow, not FF41, was the
  culprit; a per-register `lcd_shift` split still regressed, so the whole
  borrow is scoped to the aligned grid. Cost: the bonus BUG row
  `ff45_enable_weirdpoint_lcdoffset1_2` (an lcdoffset FF45 write that happened
  to land right with the borrow) stays open — an acceptable trade for zero
  regression.

## The 2 unrecovered map rows — an ack-squash miss, NOT write-commit

`irq_precedence/late_m0irq_retrigger_2` and `_scx1_2` (want E0, EV E2) stay
open. They are a STAT bit that re-fires AFTER a dispatch ACK: tier2 recovers
them via `arm_ack_squash` (the `ack_squash_ppu` window) armed on the deferred
`ack` path. The eager `ack` does not arm it — a separate lever (the eager
ack-squash port), not a write-commit-frame miss. Correctly out of this slice.

## Gates (all green)

1. `golden_fingerprint` — byte-identical (42s).
2. EV CGB 344→330 ↓; tier2 CGB 291 unchanged; EV DMG 78 unchanged.
3. Zero-regression A/B — new-fails EMPTY on CGB AND DMG.
4. mooneye `ppu` green on all three clocks (off / `RECLOCK` / `EAGER`).
5. eager intr_2 (mode0/mode3/mode0_sprites/oam_ok/intr_2_0) PASS both models.
6. clippy `-D warnings` clean; every touched `.rs` < 1000 lines.
7. Red-before-green pin `gambatte::eager_web::eager_write_conflict_commit_passes`
   (12 rows, both directions + FF0F squash) — FAILS with the borrow neutered,
   PASSES with it.

## Files

- `interconnect/bus.rs` — the borrow + FF0F squash arm in `Bus::write`.
- `interconnect/tick.rs` — the cc-1 repay in the eager `tick_machine` branch.
- `interconnect.rs` — the `eager_wr_borrow` field.
- `ppu/access.rs` — `lcd_shift_active()` accessor.
- `tests/gbtr/gambatte/eager_web.rs` — the pin (new module).
- `examples/run_gambatte.rs` — `SLOPGB_EAGER` support (tracing companion).

## Endgame after #11dd

The dispatch/IRQ web bucket 2 is done bar the ack-squash pair. Remaining C3-flip
work (per #11db): the eager ack-squash port (the 2 retrigger rows + siblings),
L1 CGB DS re-host of the SS eager slices, the 5 CGB halt rows (the one genuine
sub-M-cycle wall), HDMA `defer_steal`.
