# C2 frame-relationship sharpening — the lever is sub-M-cycle READ-PHASE, not a +3 render shift (#11v)

2026-06-27, after the #11v write-side refutation. The goal frames C2 Phase 3 as a
"render mode-0 boundary +3 (slopgb dot254 → SameBoy cfl257)" shift. Direct
measurement shows that framing is **incomplete and partly misleading**: the
mode-0 boundary is already CPU-cycle-aligned, and the residual is a **non-uniform,
per-mechanism sub-M-cycle read-phase** offset that no single frame shift resolves.

## Evidence

### 1. The dispatch/mode-0 boundary is ALREADY aligned (not a +3 physical shift)
slopgb fires the mode-0 IRQ at `line_render_done` = **dot254**; SameBoy at
**cfl257** (SBMODE `ly=1 cfl=257 vis=0`). Both pass the counter-pinned
`int_hblank_halt_scx0-7` (gbmicrotest) + `hblank_ly_scx_timing` (mooneye), so
dot254 and cfl257 are the **same CPU cycle** — a +3 dot-LABEL offset of one
physical timeline, not a 3-dot physical lag. The B2 dispatch retime was measured
INERT for exactly this reason. So "shift the boundary +3" describes a relabeling,
not a behavioural fix.

### 2. The read-collapse floors are sub-M-cycle read-phase (scx2 `_1`/`_2`)
`vram_m3/postread_scx2_1` (want out3/blocked) and `_2` (want out0/accessible)
differ by a 1-T-cycle (sub-M-cycle) alignment in the test's read instruction.
SameBoy reads them on opposite sides of the mode-3→0 boundary (one blocked, one
accessible). **slopgb reads BOTH as mode 3 (`30`)** — `_1` correct, `_2` wrong.
slopgb's deferred read samples at the M-cycle leading edge (`clock.read()` →
`before + pending`, pending reset to 4 each access), which QUANTIZES the read to
the M-cycle and discards the sub-M-cycle phase that separates `_1` from `_2`. The
#11n eighth-grid `vis_early` lever cannot separate them (already tried, floored):
they land at the same `(dot, eighth)`.

### 3. The per-mechanism offsets are NON-UNIFORM (kills the single-shift hypothesis)
- mode-0 boundary: slopgb dot254 ≡ SameBoy cfl257 → **+3**.
- LYC dispatch (lyc153 family, #11v trace): slopgb `ly153 dot6` vs SameBoy
  `cfl0` → **+6 / different**.
- m2 OAM line-start: SameBoy leads the visible edge by 1 dot; mode-0 IRQ lags by
  1 dot (the documented "2-dot swing", `mode_timeline.rs`).
A single global frame offset cannot reconcile a +3 boundary, a +6 LYC dispatch,
and a ±1 mode-2/mode-0 swing. The reads land at DIFFERENT sub-M-cycle phases
relative to each mechanism's edge, and slopgb's whole-M-cycle leading-edge sample
collapses them all to one dot.

## Consequence for the C2 plan

The remaining C2 lever is **the cc-exact sub-M-cycle READ SAMPLE PHASE**, not a
render-grid shift:
- The render mode-0 boundary, the dispatch dot, and the visible-mode flip are
  already at SameBoy's CPU cycles (counter-pinned). Moving the *render grid* is the
  wrong target — it would relabel an already-aligned boundary and regress the
  counter-pinned masks.
- What must change is WHERE within the M-cycle the deferred read samples the PPU.
  SameBoy's `read_high_memory` samples at a `pending_cycles`-precise sub-M-cycle
  point (the conflict-class phase); slopgb's `read_deferred` rounds to the M-cycle
  leading edge. The `cycle_clock.rs` deferred-commit machine TRACKS T-cycles, but
  the read SAMPLES at `clock.now()` after `pending` is reset to 4 — so the
  sub-M-cycle phase is computed-then-discarded.
- The fix is a **cc-exact read sample** that reports `vis_mode`/accessibility at the
  read's true sub-M-cycle T-position, so two reads 1 T apart (the `_1`/`_2` pairs)
  land on opposite sides of the boundary. This needs the boundary stored as a dot
  (not just the `line_render_done` bool) so a sub-dot compare is possible, OR the
  half-dot/eighth-grid resolution applied to the read↔boundary compare with the
  read's true T-phase (not the M-cycle-rounded one).

This is the S7 "sub-M-cycle clock" the ladder has pointed at (#11m wake-clock,
#11n read-collapse, #11p window all reduced to it). It is a multi-session
architectural change (the whole read/dispatch/accessibility path moves from
whole-dot to T-granular), NOT a render-grid rebaseline. The Phase 3/4 framing
should be updated: **Phase 3 = the cc-exact sub-M-cycle read sample (T-granular
read↔edge compare), Phase 4 = rebaseline the rows the finer read frame moves.**

## Method/tooling notes
- SBMODE (`vis=` per `GB_STAT_update`) and SBREAD (`ff0f`/`ff41` cfl) are NOT
  lazy-synced (cfl increments per read) — reliable for read-dot alignment, unlike
  SBWH (lazy, always cfl=0 at line start). Use SBREAD/SBMODE cfl for read-frame
  measurement.
- slopgb read dots via `SLOPGB ff41/ff0f/oam/vram` + `visflip` (`SLOPGB_S5DBG`).
