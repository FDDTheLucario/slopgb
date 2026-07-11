# Eager sub-M-cycle WAKE-CLOCK port — SHIPPED: the 5 CGB halt bar rows recovered (+14/−0), the #11cz wall overturned (2026-07-11, #11dl)

The last bounded hard piece of the eager C3-flip — the 5 CGB halt bar rows —
is **built and shipped**, flag-gated behind `eager_value`, CGB single-speed
scoped, production/tier2/DMG **byte-identical**. EV CGB **318 → 304 (−14, all
SameBoy-PASS BUG, zero drops)**. The three exhausted veins (#11cw entry-peek-
alone, #11cy wake-mask, #11cz read-position) each failed because the
discriminator is the sub-M-cycle **wake instant** the eager whole-M-cycle IF
commit quantizes away. This port recovers it — NOT with a deferred machine, but
with a pure dot-space **value peek** of the mode-0 flip, coupled to the
re-fetch read override.

## Baselines reproduced EXACTLY (gate: STOP if not)

| bin | OFF CGB | EV CGB | tier2 CGB | EV DMG | tier2 DMG |
|---|---:|---:|---:|---:|---:|
| target | 486 | 318 | 291 | 66 | 116 |
| measured | **486** | **318** | **291** | **66** | **116** |

All five reproduced on the current base (`186c6c3`, #11dk). `flagon_probe`,
`SLOPGB_REQUIRE_ROMS=1`.

## The root cause (traced, not conjectured) — the whole-M-cycle IF commit collapses two SCX-shifted flips

The eager halt idle loop samples the wake once per whole M-cycle (after
`fetch_opcode` ticks 4 dots). The PPU commits the mode-0 STAT IF at the **END**
of the M-cycle that contains the flip, so two lines whose projected flip dots
(`projected_flip_dot`, `pfd`) differ by <4 dots — an `SCX&7` delta — commit and
wake at the **same** whole-M-cycle boundary. tier2's 4k+2 half-M-cycle sample +
`stat_vis_from_t` deadline resolves the 1-dot `pfd` difference into a 4-dot wake
gap.

Dual-trace of the #11cz mutual-exclusion pair (idle-loop `pfd` per dot):

| row | want | `pfd` (idle) | eager whole-M-cycle wake | tier2 wake |
|---|---:|---:|---|---|
| `late_m0int_halt_m0stat_scx2_3a` | 0 | **256** | ly2 dot **260** (collapsed) | ly2 dot **256** |
| `late_m0int_halt_m0stat_scx3_3b` | 2 | **257** | ly2 dot **260** | ly2 dot **260** |

Both eager-wake at dot 260 → both FF41-read at ly2 dot 452 (`read_pos_hd` = 912
= `LINE_DOTS*2`, the +8hd cc+4 debt across the line boundary). Identical wake,
identical read, **opposite wants** — the #11cz wall.

## Per-consult-site offset table (the #11cx Step-1 table, completed)

`clock.now()` / dot at each consult, tier2 vs eager, on the pair (SS):

| consult site | tier2 (`scx2_3a` / `scx3_3b`) | eager (`scx2_3a` / `scx3_3b`) | offset |
|---|---|---|---|
| halt entry (`hentry`) | ly1 dot256 clk5108 / dot260 clk5112 | ly1 dot252 clk5100 / dot256 clk5104 | eager −4 dot / −8 clk |
| plain wake (`wake[plain]`) | ly2 dot256 clk5564 / dot260 clk5568 | (whole-M-cycle) ly2 dot260 / dot260 | eager collapses to 260 |

The offset is **not** a single scalar the mask could re-calibrate (#11cx's
hypothesis): the entry sits −4 dot but the wake collapses to a single dot. The
lever is the **wake instant**, and the flip dot is available as a peek.

## The mechanism — two coupled pieces, both pure value peeks (timer-safe)

### 1. Sub-M-cycle wake peek (`Ppu::m0_stat_flip_reached`, `interconnect/speed.rs`)

In the eager CGB-SS halt wake, OR `IF_STAT` into the wake word when
`self.dot` sits in the flip's own M-cycle window `[flip, flip+4)` — where
`flip` is `flip_dot` once the render recorded it, else `projected_flip_dot()`.
This lands the wake at the flip's M-cycle boundary (`pfd256` → wake 256,
`pfd257` → wake 260) — tier2's sub-M-cycle instant — instead of the collapsed
whole-M-cycle IF commit. The `[flip, flip+4)` upper bound is essential: without
it the peek re-fires on the stale, already-passed flip after the IME=1 halt
rewind (measured — woke ly1 dot336). **Pure value peek: no machine advance, no
timer tick** (`int_hblank_halt` TIMA rows stay green).

### 2. Re-fetch read override (`Ppu::halt_refetch_read_override`, applied at `regs.rs` FF41)

The wake arms a one-shot `Ppu::halt_refetch` flag. The IME=1 dispatch's first
FF41 read, once its `read_pos_hd` crosses the line boundary (`>= LINE_DOTS*2`,
mode 0 natively), returns **mode 2** — SameBoy's cc+4 re-fetch view already in
the next line's OAM. Consumed one-shot at the boundary-crossing read
(`Bus::read`/`read_inc`), backstop-cleared at the next halt entry
(`set_cpu_halted`).

### Why coupled (the discriminator the whole port turns on)

The sub-M-cycle wake **separates the read position**: `scx2_3a` (want0) wakes at
256 → reads one M-cycle short of the boundary (`read_pos_hd` 904 < 912, stays
mode 0); `scx3_3b` (want2) wakes at 260 → reads at 912 → the override fires
(mode 2). The entry peek ALONE (#11cw/#11cy/#11cz) dropped `scx3_3b` (int); the
read shift ALONE (#11cz) fired on the want-0 siblings too (−9 SameBoy-pass).
**Together, zero collateral.**

## Result — EV CGB 318 → 304, +14 / −0, all SameBoy-PASS BUG

`comm` A/B (EV CGB fail-set, base vs port). NEW-fails: **EMPTY** (both rowlists).

Recovered 14, `classify_cgb_regr.py` → **BUG=14 / FLOOR=0** (every one
SameBoy-PASS):

- 5 bar targets: `late_m0int_halt_m0stat_scx{2,3}_3a` (want0),
  `late_m0irq_halt_dec_scx{2,3}_2` (want6), `late_m0irq_halt_m0stat_scx3_3b`
  (want2).
- 9 bonus halt m0stat rows the coupling also lands:
  `late_m0int_halt_m0stat_scx3_{1b,2b,4b}` (want2),
  `late_m0irq_halt_m0stat_scx3_{1b,2b}` (want2),
  `{m0int,m0irq}_m0stat_scx{3,4}_2` (want2) — including the row the entry peek
  alone DROPPED (`late_m0int_halt_m0stat_scx3_3b`, int, want2, SameBoy-PASS),
  now recovered by the read override.

The ~10-row halt A/B the #11cx Step-0 slice broke: **zero drops** (the full
3422-row NEW-fails set is empty).

## Gates (all green)

1. `golden_fingerprint` byte-identical (43.5s real run, `SLOPGB_REQUIRE_ROMS=1`).
2. EV CGB **318 → 304**; tier2 CGB **291** (fail-set byte-identical); EV DMG
   **66** (byte-identical); tier2 DMG **116** (byte-identical).
3. Zero-regression A/B: NEW-fails EMPTY on CGB **and** DMG rowlists.
4. mooneye **92/0** OFF **and** `SLOPGB_MOONEYE_EAGER=1` **and**
   `SLOPGB_MOONEYE_RECLOCK=1` (incl. `int_hblank_halt` TIMA + intr_2 — the
   wake peek fabricates no machine time, so timers stay on grid).
5. eager `intr_2_mode0/mode3/mode0_timing_sprites` PASS both models (run_mooneye
   `SLOPGB_EAGER=1`).
6. clippy `-D warnings` clean; every `.rs` < 1000 (`read_laws.rs` stayed 998 —
   the override lives in `stat_irq.rs`, applied at the sole non-probe
   `vis_mode_read` consumer `regs.rs`; `stat_irq.rs` 827→856, `regs.rs`
   897→902).
7. Red-before-green pin `eager_halt_wake_passes` (`tests/gbtr/gambatte/eager_web.rs`):
   neuter the wake peek → `scx2_3a` fails (want0 got2); neuter the read override
   → `scx3_3b` int fails (want2 got0). Both restored → green.

## What this retires

- **"The 5 CGB halt rows need a full deferred-machine wake clock" (#11cx/#11cy/
  #11cz).** They need a sub-M-cycle wake **instant**, which the eager clock
  reaches as a pure dot-space peek of the flip (`projected_flip_dot`/`flip_dot`)
  — the flip dot IS the sub-M-cycle information, available without a machine.
- **"The wake instant is unreachable on the eager read frame" (#11cz's wall).**
  Overturned: the flip-window peek (`m0_stat_flip_reached`) separates the wake
  (256 vs 260) whole-dot, and that separation moves the read position enough
  (904 vs 912) for the boundary override to fire collateral-free.

## Gate state

Shipped flag-gated; tree at HEAD of `finish-port-halfdot`. Production (all flags
off) byte-identical. The eager C3-flip bar loses its last bounded hard piece:
**CGB halt = 0**. Remaining eager residual (per #11cb/#11dk): counter-pinned
dispatch reads (C3-flip), the DS mid-dot floor, HDMA DMA-service — the
half-dot / dispatch-atomic classes, not this bounded wake piece.

## Reproduce

```
export CARGO_TARGET_DIR=target/wake
# EV CGB 304 / DMG 66 / tier2 291/116 via flagon_probe (SLOPGB_PROBE_EV [+ dmg
#   rowlist] / SLOPGB_PROBE_RECLOCK), SLOPGB_REQUIRE_ROMS=1.
# A/B: comm -13 base_fails port_fails  → EMPTY (both rowlists).
# classify: python3 docs/sameboy-port/tools/classify_cgb_regr.py <recovered rels> → BUG=14.
# pin: cargo test -p slopgb-core --test gbtr --release eager_halt_wake_passes.
# trace: run_gambatte --features port_probe + SLOPGB_EAGER=1 SLOPGB_S5DBG=1
#   (wake[]/hentry probes shipped; the idle/pfd + armFIRE probes were reverted).
```
