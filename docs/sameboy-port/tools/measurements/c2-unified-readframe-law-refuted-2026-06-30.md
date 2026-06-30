# C2 #11an — the UNIFIED bare-line read-frame law BUILT + REFUTED; the read-straddle is config-dependent sub-M-cycle (the C2/S7 core, sharpened)

2026-06-30. The goal's two levers, build-measured to a decision: (a) drain the
genuine-LENGTH subset of the 56 RENDER-LENGTH blockers as clean `+N/−0` slices,
and (b) build/sharpen the architectural ISR-READ-POSITION model. **Result =
ESCAPE.** The 56 split by read type (FF41-mode 16 / accessibility 17 / IRQ-count
5 / window 18); the FF41-mode subset's "genuine length" reduces to a single
**unified read-frame law** that was BUILT, two-binned full-CGB, and **REFUTED as
a whole-dot lever** — it is a pure curve-fit A/B swap (`+23` SameBoy-pass fixed /
`−27` SameBoy-pass DROPPED, all 27 classified SameBoy-pass). The read-straddle is
**config-dependent sub-M-cycle**, freshly localized: the m0int/m2int want-pair
reads land at the IDENTICAL slopgb dot but **opposite-signed SameBoy offsets**
(m0int +4 / m2int −5). Defaults NOT flipped; `pixel-pipe-reclock` core
byte-identical to HEAD; the built reclock + tracers pushed to `phase-b-s7`.

## What was built (the unified read-frame law — `phase-b-s7`, env-gated `SLOPGB_BARELAW`)

The dual-emulator trace (kernel `m2int_m3stat_1`/`_2` + the DS `_2` blocker +
`late_disable`, enriched with `win_active`/`vis_early`/`line_render_done` AT the
deferred FF41 read) established the exact invariant:

```
slopgb deferred FF41 read at dot D   ⟺   SameBoy read_high_memory at cfl D + read_offset
read_offset = 4 (single speed) / 3 (double speed)
```

So slopgb reproduces SameBoy's FF41 mode verdict by comparing its OWN read dot D
against SameBoy's CONFIG render exit minus the offset — anchored to SameBoy's
config geometry, NOT slopgb's own (possibly wrong) render. This UNIFIES levers
(a)+(b) into one `vis_mode_read` branch and GENERALIZES the shipped window law:

```
triggering window  (shipped):  return 0 when dot >= 263 + SCX&7 − read_offset   (= 259+SCX&7 SS)
BARE / disabled-win (#11an):   return 0 when dot >= 257 + SCX&7 − read_offset   (= 253+SCX&7 SS / 254 DS)
```

The bare branch is anchored to SameBoy's bare exit (`SBex = 257 + SCX&7`,
measured: SBMODE vis 3→0 at cfl 257 for scx0), so it corrects even
`late_disable` — where slopgb's render OVER-extends (the window was active then
disabled) but SameBoy renders the line BARE (exit 257). Restricted to bare
non-sprite CGB lines for this cut.

### The dual-emulator evidence (the invariant, exact dots)

| row | want | slopgb read dot / mode | SameBoy read cfl / mode | SameBoy SBMODE exit |
|---|---|---|---|---|
| `m2int_m3stat_1` (kernel, SS) | 3 | 252 / 3 (ve=F) | 256 / 3 | 257 |
| `m2int_m3stat_2` (kernel, SS) | 0 | 256 / 0 (ve=T) | 261 / 0 | 257 |
| `m2int_m3stat_ds_2` (blocker) | 0 | 254 / 3 | ~258 / 0 | 257 |
| `late_disable_..._1` (blocker) | 0 | 256 / 3 (render over-extends, ve=F) | 260 / 0 | 257 (SameBoy bare) |

The SS kernel pair PASSES in slopgb's frame because its 254 visible boundary sits
between its 252/256 reads, exactly as SameBoy's 257 sits between its 256/261 —
the −3 boundary / −4 read shifts stay consistent. The blockers fail where the
read lands within the offset window on the opposite side.

## The REFUTATION (full-CGB two-bin, fresh HEAD bins, SLOPGB_BARELAW on vs off)

```
ON  (tier2, no barelaw):   476 fail
ON + BARELAW:              480 fail        (NET +4 WORSE)
  fixed (FAIL→pass): 23     ALL SameBoy-pass blockers (5 m2int_m3stat_ds_2, 8 late_disable, 2 dma, …)
  regressed (pass→FAIL): 27 ALL SameBoy-pass  (classify_cgb_regr.py: BUG=27 / FLOOR=0)
```

**Every one of the 27 regressions is a SameBoy-PASS** (`classify_cgb_regr.py`
`BUG=27 FLOOR=0`). The law trades 23 SameBoy-pass fixes for 27 SameBoy-pass
drops — a textbook curve-fit A/B swap, FORBIDDEN by the goal's never-drop rule.
The 27 are the A/B SIBLINGS of the 23 fixes:

- `late_disable_*_2`/`_3` (want 3) — siblings of the `late_disable_*_1` (want 0) fixes.
- `m2int_wxA6_scx*_m3stat_2` (want 3) — window read-straddle siblings.
- `m0int_m3stat_ds_1` (want 3) + `speedchange ..._scx1_1` (want 3) — bare m0int/m2int collisions.

Mooneye flag-on held **91/91 WITH barelaw** (the law touches only the FF41
register read, never the counter-pinned dispatch — confirmed).

## The decisive new localization — the read offset is config-dependent AND opposite-signed

The collision is sub-M-cycle and NOT a uniform offset. `m0int_m3stat_ds_1` (want
3) and `m2int_m3stat_ds_2` (want 0) both read at slopgb **dot 254** (different
lines, same dot), yet:

```
m0int_ds_1:  slopgb dot 254  =  SameBoy cfl 250 + 4   (read mode 3, before exit 257)  → want 3
m2int_ds_2:  slopgb dot 254  =  SameBoy cfl 259 − 5   (read mode 0, after  exit 257)  → want 0
```

SameBoy spreads these two reads **9 dots apart** (cfl 250 vs 259); slopgb's
deferred clock collapses them onto a single dot (254). The read offset is **+4
for the mode-0-IRQ ISR but −5 for the mode-2-IRQ ISR** — opposite-signed,
config-dependent on the dispatch type and the handler's M-cycle depth. The
frame even diverges by whole LINES: m2int's read lands slopgb ly135 / SameBoy
ly137 (a 2-line DS/C0 frame offset), while m0int is line-aligned (ly136 both).

This is exactly the goal's lever (b): *"SameBoy's post-dispatch ISR read lands a
config-dependent T-count after the dispatch; slopgb's deferred-commit clock
collapses it."* It refines #11ab: the SAME-ROM want-pairs are co-temporal, but
DIFFERENT-ROM collisions (m0int vs m2int) sit at genuinely different SameBoy
positions that slopgb mis-frames — the deferred clock does not reproduce the
per-ISR T-position.

## Why there is NO clean slice (re-confirmed, freshly build-measured)

- **Whole-dot read law** = A/B swap (the m0int/m2int and `_1`/`_2` want-pairs
  collapse to one slopgb dot; +23/−27 SameBoy-pass).
- **Boundary-only** (extend bare render 254→257 to match SameBoy) breaks the
  kernel `_2` (read 256 < 257 → mode 3, want 0) — the read frame still −4/−5.
- **Read-only** (cc+4) = production = fails the kernel `_1` (refuted, recipe).
- No threshold separates the dot-254 want-pairs; no local discriminator exists
  in `vis_mode_read` (the ISR type is known only at dispatch, but the read is
  arbitrarily deep in the handler).

## The single sharpest lever (the ESCAPE deliverable)

**The interrupt-service read-POSITION model in `interconnect/tick.rs`
(`advance_machine_t`) + `cycle_clock.rs`** — make slopgb's deferred FF41/FF0F
read land at SameBoy's per-ISR absolute dot (m0int 250 / m2int 259), reproducing
the config-dependent T-count after the counter-pinned dispatch, WITHOUT moving
the dispatch. The unified read-frame law above is the CORRECT boundary half
(it places the read-frame mode-3 exit at SameBoy's config exit − offset, and
holds mooneye 91/91); what it lacks is the sub-M-cycle / per-ISR read POSITION
that separates the dot-254 want-pairs. The two co-land in the flip:

1. the unified `vis_mode_read` read-frame boundary law (built, `phase-b-s7`), AND
2. the per-ISR deferred-read T-position (the genuinely-open S6/S7 architectural
   rewrite — the read offset is +4/−5/whole-line, a function of the dispatch
   type and handler depth, not a constant).

A whole-dot lever cannot do step 2 (the want-pairs collapse); it needs the
deferred clock to carry SameBoy's per-ISR read dot. This is the atomic
multi-session reconciliation, now localized to the read-vs-dispatch T-accounting.

## Method / data (this session)

- visexit tracer (`ppu/mod.rs`, thread-local edge of `vis_mode_read` 3→0) +
  enriched FF41 read trace (`dbg_read_state`: wa/ve/lrd/vh/vm/ns) +
  `SLOPGB_BARELAW` law gate — all `phase-b-s7`, byte-identical OFF.
- `scratchpad/measure_renderlen.sh` (the 56-row Es/Eb/Rs/Rb batch),
  `scratchpad/dtrace.sh` (dual-emulator enriched per-row), `bl_full_{on,bl}.txt`
  (the full-CGB two-bin), `classify_cgb_regr.py` (the 27/27 SameBoy-pass split).
- SameBoy `--cgb --length 4` SBMODE/SBREAD ff41; the measurement read is the
  count-1 mode-matching read (isolate from the setup poll loop at cfl=0).
</content>
