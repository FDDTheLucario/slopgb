# C2 #11ap — the HALF-DOT exit PARITY term BUILT (resolves the #11ao scx-parity) + the residual ISOLATED to the m0int/m2int read-POSITION collision

2026-06-30. Executed the goal's lever: **build the half-dot (8 MHz) read-vs-exit
resolution** and drive it to a decision. Result = **ESCAPE**, but with the
half-dot exit now SOLVED as a clean `scx&1` parity term, and the residual
sharply decomposed away from the exit onto the per-ISR read POSITION. The
surgical half-dot-exit law was built (`SLOPGB_HDEXIT`), co-landed with the
read-position lever (`SLOPGB_DSM2DELAY`), two-binned full-CGB, and the wall
localized: **the half-dot exit is not the blocker — the m0int/m2int read
COLLISION is, and it is co-temporal (unseparable by any read clock).** Defaults
NOT flipped; `pixel-pipe-reclock` core byte-identical; the built law + tracers on
`phase-b-s7`.

## The half-dot ground truth (SameBoy, fresh — cfl=DOTS, dc=8MHz half-dots)

`cycles_for_line` (cfl) is in DOTS; `display_cycles` (dc) in 8 MHz HALF-dots; the
true half-dot line position = `cfl*2 + dc` (display.c:1584 `cfl*2 + cycles + dc >
LINE_LENGTH*2`). Prior sessions read cfl only (whole-dot) — the half-dot info is
in dc. SBMODE reports the visible 3→0 exit at a constant `cfl 257` for ALL scx,
but that is COARSE (GB_STAT_update is M-cycle-granular); the TRUE exit is
bracketed by the read verdicts and moves with scx.

### m2int_m3stat DS, both emulators, exit-relative (8 MHz = cfl*2 + dc)

```
        SameBoy read_1(m3) read_2(m0)   exit∈        slopgb read_1 read_2  native exit
scx0       512        516          (512,516]          252      254        255   (even: read_2+1)
scx1       516        520          (516,520]          254      256        256   (odd:  read_2)
scx2       516        520          (516,520]          254      256        257
scx3       520        524          (520,524]          256      258        258
scx4       520        524          (520,524]          256      258        259
scx5       524        528          (524,528]          258      260        260
scx6       524        528          (524,528]          258      260        261
scx7       528        532          (528,532]          260      262        262
```

Both emulators: read_1 (want3) is BELOW the exit, read_2 (want0) AT/ABOVE it; the
exit sits in the 4-half-dot (2-dot) read bracket. **slopgb's native FF41 exit =
`255 + SCX&7` (LINEAR), its deferred DS reads land on EVEN dots (DS M-cycle = 2
dots).** For EVEN SCX the linear exit is ODD (`255+even`), one above the read
grid, so `read_2` (even) reads mode 3 where it should read 0 — the even-scx
`_ds_2` FAIL. For ODD SCX the exit is EVEN (= read_2) and resolves. SS is
unaffected (every SS m2int_m3stat leg passes; SS reads step by 4).

## The half-dot exit = a `scx&1` PARITY term on the even read grid (BUILT, clean for m2int_m3stat)

SameBoy's CPU-visible exit, sampled by an even-dot read, rounds to the read grid:
`exit = 255 + SCX&7 + (SCX&1)` (= SCX&7 rounded UP to even). The `+(SCX&1)` is
the half-dot resolution expressed on slopgb's whole (even) read grid — it is a
no-op for even SCX and raises the odd-SCX exit one dot. `ppu/stat_irq.rs`
`vis_mode_read`, env-gated `SLOPGB_HDEXIT`, DS bare non-sprite CGB lines.

**Co-landed with `SLOPGB_DSM2DELAY` (the read-position separator), the combo
passes ALL m2int_m3stat DS (16/16) AND m0int_m3stat DS (2/2)** — spot-checked
scx0-7 both legs. The half-dot exit is correct and necessary for the odd-scx
parity once the reads are separated.

## Why the exit ALONE is not a clean slice — the m0int/m2int read COLLISION

The exit-side lever ALONE (`SLOPGB_HDEXIT`, lower-even variant, no DSM2DELAY)
two-bin: **+12 SameBoy-pass blockers fixed / −8 SameBoy-pass DROPPED** (all 8
classified BUG by `classify_cgb_regr.py`). The 8: `m0int_m3stat_ds_1` (1) +
`window/late_disable_*_ds_2` (7). The late_disable 7 are excludable
(`!render.win_aborted`); the irreducible one is **the m0int/m2int collision**:

```
m2int_m3stat_ds_2  reads slopgb ly135 dot254  native m3  want 0   clk=1260 (≡0 mod4)
m0int_m3stat_ds_1  reads slopgb ly136 dot254  native m3  want 3   clk=2172 (≡0 mod4)
```

Both ISR reads land the **identical dot 254 AND the identical sub-M-cycle phase**
(clk ≡ 0 mod 4). SameBoy spreads them **9 dots** (cfl 250 vs 259) via the per-ISR
sub-M-cycle DISPATCH phase (mode-0 HBlank IRQ vs mode-2 OAM IRQ rise at different
line positions). **No exit threshold and no finer read SAMPLE can give one dot
two opposite verdicts** — this generalizes #11ab (co-temporal) across DIFFERENT
ISRs, not just same-ROM `_1`/`_2`. The half-dot exit is irrelevant to the
collision: the bug is the read POSITION, not the exit.

## Why the read-position lever (DSM2DELAY) drags forbidden drops — the atomic entanglement

The collision's ONLY separator is moving the DISPATCH: `SLOPGB_DSM2DELAY` delays
the DS mode-2 OAM STAT IRQ +1 M-cycle (matching SameBoy's "delay the OAM STAT IRQ
a cycle on all lines"), pushing m2int's read +2 dots clear of m0int. Co-landed
with the raise-odd half-dot exit, full-CGB two-bin: **+28 fixed / −24 regressed
(23 BUG = SameBoy-pass DROPPED, 1 FLOOR)**. The 23 drops are NOT exit-fixable
(they read other registers / IF): cgbpal_m3 (3), dma scx5 (2), m2enable (3),
m2int_m2irq (3), m2int_m0irq (1), oam_access (1), irq_precedence (1), window
m2int_wx*_ds_1 (8, win_active so the FF41 exit law excludes them), late_reenable
(1). DSM2DELAY is a DISPATCH delay → it moves the **IF delivery** the m2irq/
m2enable tests sample and shifts the deferred read frame for the palette/oam/dma
access tests. The IF-delivery move is plausibly CORRECT (SameBoy does delay the
IRQ) but every per-test deferred read must re-frame WITH it — the atomic reclock.

## Per-lever residual map (this session, build-measured)

| lever | what it does | m2int_m3stat | full-CGB two-bin |
|---|---|---|---|
| `HDEXIT` lower-even (exit ↓ even) | FF41 exit `254+scx&7+scx&1` | fixes even `_ds_2` | +12 / −8 (m0int collision + late_disable) |
| `HDEXIT` raise-odd (exit ↑ odd) + `DSM2DELAY` | read +2 (m2int) ∧ exit `255+scx&7+scx&1` | 16/16 + m0int 2/2 | +28 / −23 BUG (DSM2DELAY IF/access side effects) |
| `DSM2DELAY` alone (#11ao) | read +2 (m2int) | even `_ds_2` fixed, odd `_ds_1` over-shot | +29 / −26 BUG (scx-parity + IF) |
| `BARELAW` (#11an) | FF41 exit whole-dot `253+scx&7` | A/B swap | +23 / −27 BUG |

## The single sharpest lever (the ESCAPE deliverable — refined)

**The per-ISR deferred FF41-read POSITION, decoupled from the IF dispatch.** The
half-dot EXIT is solved (the `scx&1` parity term, FF41-clean). The wall is that
slopgb collapses the mode-0-IRQ and mode-2-IRQ ISR reads onto one dot+phase;
SameBoy places them at per-ISR positions 9 dots apart via the sub-M-cycle
dispatch phase carried into the deferred read. The fix must move the read to
SameBoy's per-ISR dot WITHOUT a blunt dispatch delay's IF-delivery side effects —
i.e. the deferred-commit clock (`cycle_clock.rs` / `interconnect/tick.rs`) must
carry the IRQ-source sub-M-cycle phase into the FF41 read sample while the IF
delivery and counter-pinned dispatch dot stay put. This is the S6/S7
interrupt-service read-position reconciliation — the atomic multi-session rewrite
the port has named — now isolated to exactly this read↔dispatch decoupling, with
the half-dot exit parity term ready to co-land on top. The half-dot PPU TICK
(last resort) does NOT crack it: the collision is at the CPU-driven deferred read,
not the PPU dot grid.

## Tooling rebuilt this session (the /tmp wipe recovery)

`/tmp/sbbuild` (SameBoy source + tracers) and `/tmp/ao` were wiped mid-session by
a tmpfiles cleanup. Reconstructed the tester from the pinned yay-cache tarball +
re-applied the tracers; persisted as **`docs/sameboy-port/tools/build_sameboy_tracers.sh`**
(idempotent, re-runnable) so the loss cannot recur. Tracers: SBMODE / SBREAD
ff41+ff0f / SBLEVEL / STAT_IRQ (`SB_TRACE=1`). Measurement helpers:
`scratchpad/hd_measure.sh` (SameBoy exit+read 8MHz), `scratchpad/sl_measure.sh`
(slopgb read+exit), `scratchpad/twobin.sh` (fixed/regressed comm).

## Gate (END CLEAN — no production change)

mooneye flag-on 91/91 (`SLOPGB_MOONEYE_RECLOCK`); gbtr OFF byte-identical;
`pixel-pipe-reclock` core byte-identical; `HDEXIT`/`DSM2DELAY` env-gated OFF. The
built half-dot exit law + the SameBoy build script on `phase-b-s7`.
