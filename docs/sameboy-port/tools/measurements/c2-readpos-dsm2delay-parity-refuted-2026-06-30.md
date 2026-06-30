# C2 #11ao — the per-ISR read-POSITION model BUILT (DS mode-2 dispatch delay) + REFUTED as whole-M-cycle; the separation is sub-dot, pinned to scx-PARITY

2026-06-30. Executed the goal's lever (b): **build the per-ISR deferred-read
POSITION model** and drive it to a decision. Result = **ESCAPE**, with a sharper
model than #11an, a build-measured new lever, and a conclusive sub-dot
localization. The full per-ISR ISR-T-sequence was traced in both emulators
(`SL2`/`SLOPGB vec` ↔ SameBoy `SB2`/`SBDISP`/`SBVEC`), the read-position model
built (`SLOPGB_DSM2DELAY`), two-binned full-CGB, and **refuted as a whole-M-cycle
lever** (`+29` SameBoy-pass fixed / `−26` SameBoy-pass DROPPED). Defaults NOT
flipped; `pixel-pipe-reclock` core byte-identical; the built model + tracers on
`phase-b-s7`.

## The deepened model — #11an's "opposite-signed +4/−5 whole-line" was a cfl-vs-dot artifact

The #11an localization ("m0int +4 / m2int −5, opposite-signed, whole-LINE
component") mixed two confounds: SameBoy's `cycles_for_line` (cfl) is the render
coroutine's mode-length-inflated position, **not** a clean dot; and the
line-number divergence is a frame-phase artifact (the verdict repeats per line).
The **exit-relative** framing (read dot − the line's own visible mode-3 exit dot)
cancels both. Re-measured both emulators (slopgb `visexit` ↔ SameBoy `SBMODE
vis=0`):

```
ROM (DS)          SL read-exit   SB read-exit   SL read_off   verdict
m0int_ds_1 (w3)      −1             −1              2          pass
m0int_ds_2 (w0)      +1             +2              3          pass
m2int_ds_1 (w3)      −3             −1              4          pass
m2int_ds_2 (w0)      −1 (mode3✗)    +2 (mode0)      5          FAIL
```

`read_off = SameBoy_read_cfl − slopgb_read_dot`. **There is no opposite sign and
no whole-line component.** slopgb's read−exit is *uniformly* a little less than
SameBoy's; the deficit `= read_off − exit_off` (exit_off uniform: 3 SS / 2 DS).
The clean statement:

- **The DS mode-2 (OAM-IRQ) handler read lands at read_off 4-5; the DS mode-0
  (HBlank-IRQ) handler read at 2-3.** slopgb introduces a **2-dot split between
  the mode-2 and mode-0 handlers** that SameBoy does not have (SameBoy reads both
  m0int_ds_1 and m2int_ds_1 at cfl256). `m2int_ds_2` is the sole failing row of
  the 8-row m0/m2int family — the only one where the 2-dot mode-2 deficit flips
  the read across the exit.

## The ISR T-sequence trace (the mechanism, both emulators)

The handler is **identical linear code in both** (the gambatte STAT vector at
0x48 `jp 0x1000`, a NOP slide, then `ld a,(c=0x41)` reading FF41), traced
bus-op-by-bus-op (`SL2` ↔ `SB2`). The **CPU T-accounting matches exactly**: the
5-M-cycle dispatch advances `+18 T` to the vector latch in both (slopgb
`dispatch_vector_retime` ≡ SameBoy `pending-=2; flush; pending=2`), and the NOP
slide is dot-conserving (2 dots/M-cycle DS) in both. So the read divergence is
**not** a CPU-timing bug — it is the PPU-dot-at-the-deferred-read as a function of
the IRQ source's sub-M-cycle dispatch phase, which slopgb's whole-dot PPU
quantizes. The mode-2 OAM IRQ rises at the line start (SameBoy `STAT_IRQ cfl0
dc2`), the mode-0 HBlank IRQ at the mode-3 exit (`cfl257`); their sub-dot rise
phases differ, and slopgb collapses the 2-dot consequence.

## The lever BUILT — `SLOPGB_DSM2DELAY` (DS-only mode-2 dispatch delay)

`stat_irq/reclock.rs::stat_update_halt_masks`: for the mode-2 (OAM) line-start
pulse on lines 1-143, additionally set the **dispatch** mask (`stat_late` →
`if_stat_late`) **in double speed**, delaying the DS mode-2 STAT-IRQ dispatch by
1 M-cycle (= the +2 dots the read needs). SS-EXEMPT — the prior all-speed
`stat_late` attempt (noted in the code) collapsed the SS kernel `m2int_m3stat_1`
(a whole SS M-cycle = 4 dots over-delays it); SS needs +2 dots = half an M-cycle,
unreachable by a whole-M-cycle delay, and already passes. This is the
dispatch-side analogue of the #11an read-side `vis_mode_read` law.

**Want-pair: fixes `m2int_m3stat_ds_2` and holds m0int + the SS kernel (6/6).
mooneye flag-on holds 91/91 — the counter-pinned dispatch is NOT moved** (the
delay holds the IRQ from the dispatch *sampler* a cycle; the DIV/timer counters
are untouched).

## The REFUTATION — full-CGB two-bin (`+29` / `−26`, all SameBoy-pass)

```
ON  (tier2, no DSM2DELAY):   476 fail
ON + DSM2DELAY:              473 fail   (net −3)
  fixed (FAIL→pass):  30  → 29 SameBoy-PASS blockers drained + 1 floor
  regressed (pass→FAIL): 27 → 26 SameBoy-PASS DROPPED + 1 floor   (FORBIDDEN)
```

`classify_cgb_regr.py`: 26 of the 27 regressions are SameBoy-PASSES — a textbook
A/B swap, FORBIDDEN by the never-drop rule. It is the dispatch-side twin of the
#11an read-side BARELAW (`+23/−27`). DSM2DELAY also has IF-delivery side effects
(breaks `m2int_m2irq_ds_1` w1→3, `m2enable/*_ds_1`) — the delay moves the IF bit
the m2irq/m2enable tests sample, not only the FF41 read.

## The DECISIVE localization — the separation is sub-dot, pinned to scx-PARITY

```
FIXED   (m2int_m3stat scx family):  scx2, scx4, scx6, scx8   (EVEN scx) _ds_2
REGRESSED (m2int_m3stat scx family): scx1, scx3, scx5, scx7   (ODD scx)  _ds_1
base scx0 _ds_1: untouched (stays passing)
```

The `+2` whole-dot dispatch delay is **correct for even scx and wrong for odd
scx**. `scx&1` (the fine-scroll low bit) shifts the mode-3 exit by a **half-dot**;
the whole-dot delay aligns to even-scx exits and over/under-shoots the odd-scx
half-dot exits. This is the conclusive sub-dot signature: the `_1`/`_2`
want-pairs of each ROM read ~2 whole dots apart, the exit sits between them only
at half-dot resolution, and **no whole-dot lever — read-side (BARELAW) or
dispatch-side (DSM2DELAY) — can place a whole-dot read/boundary to straddle a
half-dot exit for both scx parities at once.** `DSM2DELAY + BARELAW` together do
NOT rescue (the +2 over-shoots past even SameBoy's exit on the odd-scx legs).

## The single sharpest lever (the ESCAPE deliverable)

**The half-dot (8 MHz) PPU clock — the `dot_phase` pixel-pipe reclock the
`interconnect.rs` `dot_ticks_on_cc`/`dot_phase` field docs already name as "only
a full pixel-pipe reclock uses it".** The per-ISR read-position separation is
sub-dot (scx-parity-pinned), so the PPU must tick on the 8 MHz half-dot grid (as
SameBoy's `cycles_since_last_sync` does) — not the whole-dot grid — to place the
deferred FF41 read and the visible mode-3 exit at the half-dot positions that
straddle correctly for both scx parities. This co-lands with the per-config
render-length port (the exact mode-3 exit per scx/wx, #11am lever a): the read
POSITION (half-dot) and the boundary (render-length) must both be half-dot-exact;
neither whole-dot half alone is a clean slice (build-measured twice now: BARELAW
+23/−27, DSM2DELAY +29/−26, identical parity collapse). The dispatch must NOT
move (counter-pinned; mooneye held 91/91 throughout) — the half-dot clock carries
the IRQ-rise sub-dot phase into the deferred read without moving the dispatch dot.

## Method / data (this session)

- ISR T-sequence tracers (`phase-b-s7`, byte-identical OFF): `SLOPGB vec`
  (vector-latch PPU pos+clock, `interconnect.rs`), `SL2 rd/wr/na/ob`
  (per-bus-op, `cycle.rs::dbg_isr`, `SLOPGB_ISRTRACE`); SameBoy `SBDISP`/`SBVEC`
  (`sm83_cpu.c`) + `SB2 rd/wr/na` per-access (`SB_TRACE2`).
- `scratchpad/isrtrace.sh` (interleaved dispatch→vec→read both emulators),
  `scratchpad/readexit.sh` (exit-relative read measurement both emulators).
- `SLOPGB_DSM2DELAY` lever (`ppu/mod.rs::dsm2delay_on` + `stat_irq/reclock.rs`).
- Full-CGB two-bin `/tmp/ao/{on_base,on_delay}.txt`, `classify_cgb_regr.py`
  (29/26 SameBoy-pass split), `/tmp/ao/{fixed,regr}.txt`.
- mooneye flag-on 91/91 with DSM2DELAY; gbtr OFF byte-identical.
