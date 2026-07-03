# ENGINE-IF asm-method run #11bg (2026-07-03) — the CGB FF41 TWO-PHASE engine write + the DS line-153 lyfc table

Goal item 1 (the ENGINE-IF read-frame core, asm-first). The #11al/#11am
"read-frame A/B atomic" verdict for the lycEnable/ly0/m1/misc families is
**partially REVERSED** by the same method that converged halt (#11bf): the
gambatte hwtests asm (7 fan-out constraint analyses, `scratchpad/asm_*.md`) +
the new **SBWRITE** tracer (FF41/FF45 write instants with `lyfc` + `fp`,
banked in `build_sameboy_tracers.sh`) exposed two discrete mechanisms no
prior session had:

## Mechanism 1 — the CGB FF41 write is TWO-PHASE (`GB_CONFLICT_STAT_CGB`)

SameBoy `sm83_cpu.c:168-188`: a CGB single-speed FF41 write commits
`(old & 0x40) | (new & ~0x40)` at T0 and the full value one T later — the
**LYC-enable bit lags the mode bits by 1 T-cycle** (double speed: bit 3
instead). Dual-traced on `ff41_disable_2`: SBWRITE prints the phase pair
(val=40 then val=00) with the ly6 LYC-latch STAT_IRQ **between** them — the
disable's phase-1 keeps bit6 armed while `ly_for_comparison` latches, so the
edge fires (want 2); slopgb killed bit6 at the commit → got 0.

**The port** (`Ppu::eng_stat` + `eng_stat_pending`, consumed only by
`stat_update_tick` → byte-identical OFF): the engine's FF41 view transitions
OLD → phase-1 (mode new, bit6 old) at commit+2 (= SameBoy T0, dual-traced
Δ=+2) → final at commit+4, with:
- phase-1 rises FIRE (mode enables at their effective instant), falls silent;
- externals at commit+2..3 edge against the armed phase-1 (`ff41_disable_2`,
  `lyc0_ff41_disable_2`);
- the final value evaluates against **`mfi_at_t0`** (the mode saved at the
  phase-1 tick) — the T0+1T sub-dot dip: `lyc1_m2irq_late_lycdisable_1`'s
  line falls before the next line's OAM carryover and the ly2 mode-2 rise
  re-fires (want 2);
- a final RISE (the bit6-late enable) fires iff the pre-write line was LOW
  (the m1→LYC handoff `lyc153_late_enable_m1disable_3` is hazard-free on
  hardware where SameBoy's intersection phase dips and reads E2 — a
  measured hardware-vs-SameBoy divergence, SameBoy FAILS that row), through
  the CGB delivery delay `lyc_if_delay` (=3, the FF41 twin of the FF45-write
  delay; swept 3..8 on the `lyc_ff41_trigger_delay` pair);
- **m0-flip fast-forward**: a stage past T0 at the line's mode-3→0 flip
  resolves to final immediately (the flip sits later than T0+1T in SameBoy's
  frame) with a forced dip when the final value cannot hold the line —
  `m0enable/lycdisable_ff41_scx1/2/3_1`'s dying LYC hold re-edges the mode-0
  rise; the k<1 guard keeps `m0enable/disable_2` (scx0)'s dying enable
  catching its own rise.

The write-instant gambatte LYC arms (`lyc_fire`/`lyc_carryover`/
`lyc_wrap_153`) are suppressed **only in the line-boundary region**
(`law_pos` outside 16..448, unshifted CGB SS): the mid-line
`lyc_ff41_trigger_delay` pair collapses to ONE deferred commit dot (both
legs dot 77, measured) — only the calibrated write-instant arm can split it;
the engine owns the boundary where the staged view + lyfc schedule decide.
The fresh-m0-enable else-arm fire also moves to the engine (phase-1 rise);
the ly143-vs-normal-line want-split (`m1irq_m0enable_1` fires,
`late_enable_2` doesn't) falls out of the mfi at the application tick (the
ly144 hblank carryover vs the next line's OAM carryover).

## Mechanism 2 — the CGB DOUBLE-SPEED line-153 `ly_for_comparison` table

slopgb's DS table was a documented SS placeholder. The four
`lyc153_m1disable_ds` / `lyc0_m1disable_ds` dip-vs-seamless m1→LYC handoff
constraints (asm-derived) have a UNIQUE whole-dot solution: **153 latches at
dot 4** (not 6), live through dot 7, **the [8,12) window is the -1 GAP**
(held for a latched match; a fresh LYC write there does NOT re-latch —
`lyc153_late_ff45_enable_ds_6` E0), 0 from dot 12 — with the DS engine view
immediate (the two-phase window is sub-dot at 2 dots/M; the write-after-tick
order gives display-step-first collision semantics).

**The dot-4 wake cascades through every LYC=153-anchored DS test** (the ISR
runs 2 dots earlier): fixes the gdma_cycles pair (read frame), the
lcd_offset offset1 scx1 count legs (first-poll mode), and the late_wy trio
(write instants) as side effects. Two recalibrations rode along:
- the #11ag shadow-WY DS deadline slack +4 → +2 (`win_extends_sb` — the `_1`
  99 / `_2` 101 trigdots moved −2 with the wake);
- the **DS trigger-line WY un-latch** (`regs.rs` FF4A): an un-matching WY
  write at commit dot ≤ 4 of the fresh trigger line beats SameBoy's per-line
  `wy_check` (~dot 2-5) that slopgb's production `wy_latch` pre-latches at
  the previous line's 450/454 samples — releases `wy_latch` + the #11af
  shadow + commits `wy2` immediately (the stale copy re-latched the shadow
  the next dot, measured). Splits `late_wy_1toFF_ds_1`/`_2` (both
  SameBoy-passes — was an A/B swap risk mid-build, threaded).

## Results (all gates green at commit)

- 47-blocker list: **30 remain (−17)** — lycEnable 5/5, m2enable 2/2,
  ly0 2/4 (DS legs), miscmstatirq 2/3, plus the gdma pair, the late_wy trio
  and the lcd_offset scx1 pair via the wake cascade (m0enable's 2 blockers
  remain).
- Full-CGB two-bin: 373 → **357** flag-on = +24 fixed / −8 new, the 8 all
  classify-FLOOR (SameBoy fails them too) → ZERO SameBoy-pass drops. DMG
  two-bin: 154 → 154 (all levers CGB-gated). The base-373 list was rebuilt
  from a stashed base build (not preserved from #11bf); two apparent
  regressions (`offset1_lyc99int_m2irq_count_ds_1`, `late_wy_1toFF_ds_2`)
  were base-failing all along (full-line comm artifact — name-level diffs
  only, method lock).
- Documented floor-losses (all classify-FLOOR — SameBoy fails them too):
  `lycwirq_trigger_ly00_stat50_ds_lcdoffset1_2`,
  `lycstatwirq_trigger_ly00_10_50_ds_lcdoffset1_2`,
  `lycint152_lyc153irq_late_retrigger_ds_1`,
  `m1irq_late_enable_ds_lcdoffset1_2` (DS-shifted/ack-race rows whose law
  tables sit on the old dot-6 frame), `late_wy_ds_1` + `_lcdoffset1_1`
  (SameBoy also triggers the window — sb=3), and
  `offset1_lyc99int_{m2stat,m3stat}_count_ds_2` (sb=00).
- 42 tier2 pins (+`tier2_ff41_twophase_engine_passes`,
  `tier2_ds_line153_lyfc_passes`); mooneye 91/91 ON+OFF; gbtr OFF 227/0
  byte-identical; lib 660; clippy clean.

## Built + REFUTED this run (do not naively retry)

- phase-1 = mode-UNION (make-before-break): breaks the m2enable
  disable-before-pulse rows (+5) — mode bits BREAK at T0.
- phase-1 = bit6-UNION (enable immediate): breaks the late-enable want-0
  rows — bit6 enables land at commit+4 with the continuity gate.
- SILENT phase-1 (no rise fires): eats the m2enable line-start enable fires.
- The line-end mfi=2 write-view (dots 454+): double-fires `disable_1` — the
  SameBoy line-end `mfi=2` set runs WITHOUT `GB_STAT_update` (engine-
  invisible); the natural mfi at the application tick suffices.
- DS staged views k∈{2,3}: over-delay the DS `_1` legs (the DS window is
  sub-dot; immediate + the table is the solution).
- `lyc_if_delay` sweep 4..8 on the trigger-delay pair: no constant passes
  both legs from the engine fire instant — the pair is a deferred-commit
  COLLAPSE (identical commit dots), only the write-instant arm splits it.
- wy2_delay +2 (tier2-DS): breaks `late_wy_FFto2_ly2_ds_1` + a pin — the
  un-latch is a discrete race, not a copy-phase shift.

## Tooling banked

`SBWRITE` (FF41/FF45 write instants, `lyfc=` + synced `fp=`) in
`build_sameboy_tracers.sh` (guard updated); the 7 asm constraint analyses in
the worktree `scratchpad/asm_*.md` (lycEnable, ly0+lyc153int, m1+misc,
m2int+irq_precedence, enable-family, lcd_offset+speedchange, window+gdma) —
the remaining families' constraint tables are ready for the next slices.
