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
the `cgbpal_m3start/m3end` A/B-pinned window — still the sub-M-cycle read-phase
class (the cc-exact lift). #11w is the boundary-COINCIDENT subset that the whole-dot
stamp could express; the genuinely sub-dot ones remain.
