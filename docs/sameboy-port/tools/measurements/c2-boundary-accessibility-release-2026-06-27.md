# C2 #11w — the boundary-coincident accessibility release (+4/−0, first forward C2 slice)

2026-06-27, after the #11v write-side refutation + frame-relationship sharpening.
The sharpening pointed at the sub-M-cycle read↔edge interaction; this is the first
piece of it that yields a clean tier2 slice. **+4/−0 full-CGB two-bin flag-on**;
the first forward C2 progress (every prior #11v step was refute/sharpen).

## Mechanism (build-measured, dot-exact)

The `vram_m3`/`oam_access` `postread_scx2/scx5_2` rows (`want0 got3`) read VRAM/OAM
and check accessible (mode 0) vs blocked (mode 3). Traced both oracles:

- `postread_scx2_1` (want blocked): slopgb reads at **ly1 dot252**, blocked ✓.
- `postread_scx2_2` (want accessible): slopgb reads at **ly1 dot256**, blocked ✗
  (SameBoy reads accessible). The reads are **4 dots apart** (different M-cycles —
  NOT the late_scx4 read-collapse the survey grouped them with).
- In the measurement frame scx2 shifts ly1's `line_render_done` to **dot256**, and
  the `_2` read lands EXACTLY on it. The read returns 0xFF via the
  `m0_access_edge` STAMP in `memory.rs` (`0x8000..=0x9FFF if stamp_blocks(...)`),
  NOT `vram_read_blocked` (which the trace showed was never reached).

The stamp models the production cc+2-MID accessibility: an unblock committing in
the M-cycle's SECOND HALF (`event_phase` commit eighth > `ACCESS_PHASE`) reads as
still-blocked. But under the cc+0 deferred read **SameBoy unblocks AT the
boundary** — a read landing on `line_render_done`'s dot reads accessible. The `_1`
read (dot252, 4 dots / one M-cycle earlier) sees no stamp and stays blocked, so
releasing ONLY the boundary M-cycle's stamp is a clean separation, not an A/B swap.

## Fix

`render/mode0.rs`, the `line_render_done` rise: under tier2 single speed, set the
`m0_access_flip` lead to `-8` (clamps the `M0Access` `event_phase` to phase 0, which
never pre-empts an `ACCESS_PHASE` observer) instead of `0`. Bare lines only
(`bare_flip`), `tier2_reclock && !ds` → production byte-identical OFF.

## Why single speed only

The same release in double speed unblocks the DS VRAM-**WRITE** path too (the
`m0_access_edge` stamp also gates writes at `memory.rs:213`
`0x8000..=0x9FFF if stamp_blocks => {}`). Full-CGB two-bin showed the universal
(both-speed) form is **+7/−2**: it additionally fixes 3 DS reads
(`oam/vram postread_scx5_ds_2`, `oam postwrite_scx1_ds_2`) but regresses 2 DS
VRAM-write-end floors (`vramw_m3end_scx5_ds_{2,4}`). Gating to `!ds` drops the 3 DS
fixes AND the 2 regressions → **+4/−0, zero SameBoy-pass dropped**. The DS read
grid is its own S6/S7 reclock (the DS accessibility windows are already a separate
`CGB_LINESTART_OAM_OPEN_DS` family); the DS read-accessibility slice is deferred
there.

## Result + gate

- Full-CGB flag-on two-bin: 515 → 511 fail = **+4/−0** (oam+vram postread
  scx2_2/scx5_2 [Cgb]). DMG render family clean (1 pre-existing `late_scx4_2`
  floor, no new regression). mooneye flag-on 91/91.
- New pin `tier2_oam_vram_postread_scx2_scx5_passes` (gambatte.rs); the scx3 pin's
  "scx2/scx5 stay floored" note updated.
- Production byte-identical OFF (gated `tier2_reclock && !ds` + `bare_flip`).

## Remaining render floors (post-#11w, non-ds)

`late_scx4_2` (the genuine read-collapse, _1/_2 same dot), `oam_access/preread_2`
(want3 got0, the opposite direction), `prewrite_lcdoffset1_1`, `vram_m3/preread`,
the `cgbpal_m3start/m3end` window — still the sub-M-cycle read-phase class (the
cc-exact lift). #11w is the boundary-COINCIDENT subset that the whole-dot stamp
could express; the genuinely sub-dot ones remain.

## The clean boundary-coincident render slice is EXHAUSTED at #11w (full triage)

Build-measured every remaining non-ds render floor → #11w is the ONE clean
boundary-coincident lever; the rest are genuine floors needing bigger C2 machinery:
- **scx2/scx5 postread (oam+vram)** — #11w, CLEAN +4/−0 (shipped).
- **cgbpal_m3end_2** — palette analogue +3/−1 (palette unblock physically LAGS the
  pipe end, so the boundary read correctly BLOCKS; wrong-direction, see below).
- **preread_2** (want3 got0) — slopgb reads OAM at `ly2 dot0` ACCESSIBLE (the
  `cgb_linestart_oam_open` window, #11q) where SameBoy reads blocked. The window
  serves the lcd-offset variant (`preread_lcdoffset1_1`, wants open) but over-opens
  the BASE `preread_2` (wants blocked) — the #11q **lcd-offset A/B floor**: both read
  at line start, want opposite; only the lcd-offset model (shifting the offset read
  off dot0) resolves it. NOT a clean slice.
- **DS postread_scx5_ds / postwrite_scx1_ds** — +3 but the same `m0_access_edge`
  stamp gates the DS VRAM-WRITE path → `vramw_m3end_scx5_ds` regresses (−2), and the
  DS writes are themselves mixed (`postwrite` wants release, `vramw` wants block) →
  no read/write split helps; = the DS S6/S7 reclock.
- **late_scx4_2** — the genuine read-collapse (`_1`/`_2` same slopgb dot) → cc-exact
  sub-dot read sample (the big lift).

So the next C2 forward work is the bigger machinery (the lcd-offset render model, the
cc-exact T-granular read sample, the DS reclock), NOT more whole-dot boundary slices.

### CLINCHER — the residual is UNIFORMLY sub-M-cycle `dc`, proven on `preread_2`

`oam_access/preread_2` (want3/blocked): slopgb reads OAM at `ly2 dot0` ACCESSIBLE
(the `cgb_linestart_oam_open` window). SameBoy's SBMODE at the SAME dot shows the
mode transition WITHIN the dot: `cfl0 dc2 vis=0` (mode-0 carryover) → `cfl0 dc8
vis=2` (mode-2 lock). slopgb reads at cfl0's START (≈dc2, open); SameBoy's read lands
at the later `dc8` (mode-2, blocked). **Same dot (cfl0), different `dc`** — a
sub-M-cycle distinction slopgb's whole-dot read sample cannot make. This is the same
`dc`-level resolution `late_scx4` (sub-dot SCX boundary) and the read-collapse class
need. #11w was the ONE floor where the whole-dot boundary stamp happened to align
with SameBoy's `dc`; every other residual floor genuinely needs the **dc-resolved
clock** — the PPU mode flips at a `dc` (not a dot) AND the read samples at its true
`dc` (from `cycle_clock.rs`'s T-position, not the M-cycle-rounded leading edge). That
is the atomic reclock: a fundamental sub-dot upgrade to the PPU mode timeline + the
read path, not incremental tier2 slices.

## Tried + parked: the PALETTE analogue (pal_access_flip release) = +3/−1, NOT clean

Applied the same `-8` lead release to `pal_access_flip` (`render/mode0.rs` lx==160,
the `PalAccess` END_PHASE stamp) under `tier2 && !ds`: cgbpal family **+3/−1** —
fixes `cgbpal_m3end_scx2/scx3/scx5_2` (want0/accessible) but REGRESSES
`cgbpal_m3end_1` (want7/blocked → got0). Unlike OAM/VRAM, the palette boundary read
wants BLOCKED (the unblock physically lags the pipe end — INC-G3 task5's reason for
END_PHASE), so a blanket release over-unblocks. `cgbpal_m3end_1` is in the floor
baseline, so the −1 MAY be a gambatte-reference floor (SameBoy renders 0, not 7) —
in which case +3/−1 is +3/0-SameBoy-drop and shippable. NOT verified (needs SameBoy
direct OCR of `cgbpal_m3end_1`; the tester renders the palette read result). PARKED:
verify SameBoy's `cgbpal_m3end_1` digit; if 0 → ship the pal release as a separate
slice; if 7 → it's a SameBoy-pass, keep the END_PHASE block and the m3end_2 rows
need the sub-dot lift. The `cgbpal_m3start` read/write rows are the #11u A/B-pinned
lcd-offset window (pin `tier2_cgbpal_m3start_lcdoffset1_passes`), untouched here.
