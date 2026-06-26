# m2int_m3stat read-observer — the eighth-grid bare-line slice (#11n)

2026-06-25 (#11n). First MECH-1 (read-observer) CODE slice. The `m2int_m3stat`
sub-family converges flag-on via a 1-dot `vis_early` anticipation on bare lines
whose mode-0 dispatch lands at cc2 of its M-cycle — the eighth-grid read-observer
expressed as a per-config `early_lead`. Flag-gated (`tier2_reclock`),
byte-identical OFF.

## Ground truth (SameBoy `SBREAD ff41` + `SBMODE`, slopgb `SLOPGB ff41`/`visflip`)

DMG, the `_1` (want mode 3) / `_2` (want mode 0) read straddle vs the per-config
mode-3→0 boundary. Reads land on the CPU M-cycle grid (dot ≡ 0 mod 4); the
boundary (dispatch) = 254 + SCX&7 (slopgb) ≈ 257 + SCX&7 (SameBoy).

| cfg | slopgb read_1 | read_2 | slopgb bd (dispatch) | bd cc | flag-on |
|---|---|---|---|---|---|
| scx0 (kernel) | 252 | 256 | 254 (≡2, cc3) | cc3 | PASS |
| scx2 | 252 | 256 | 256 (≡0, cc1) | cc1 | PASS |
| **scx3** | 252 | 256 | **257 (≡1, cc2)** | cc2 | **FAIL→FIX** |
| scx5 | 256 | 260 | 259 (≡3, cc4) | cc4 | PASS |
| late_scx4 | 256 | **256** | 258 (≡2, cc3) | cc3 | FAIL (floor) |
| nobg_scx7 (ly2) | 256 | 260 | **261 (≡1, cc2)** | cc2 | **FAIL→FIX** |

SameBoy reads all these mode 0 on `_2` (passes every config). slopgb's deferred
FF41 read lands ~4–5 dots earlier than SameBoy AND on the M-cycle grid; the
boundary extends smoothly +SCX&7, so for some SCX the boundary overshoots the
quantized read.

## The mechanism (eighth-grid read-observer)

A leading-edge (cc+0) FF41 read samples at its M-cycle START (dot ≡ 0 mod 4). The
CPU observes the mode-0 flip at the cc+2 phase (`ACCESS_PHASE`), so a read landing
IN the dispatch's M-cycle should see mode 0 iff the dispatch is at cc1 or cc2:

- **cc1** (dispatch ≡ 0 mod 4 = the M-cycle start): already caught — the read
  ticks THROUGH that dot and `line_render_done` is set. (scx2)
- **cc2** (dispatch ≡ 1 mod 4): the dispatch commits ONE dot past the read's
  M-cycle start, so whole-dot `line_render_done` leaves the same-M-cycle read at
  mode 3. Needs `vis_early` anticipated 1 dot to the M-cycle start. (scx3 257,
  nobg_scx7 261)
- **cc3/cc4** (≡ 2/3): the same-M-cycle read PRECEDES the boundary → mode 3
  (correct: kernel m2int@252 with dispatch 254 ≡2, and lcdon's 253 read); the
  NEXT M-cycle's read sees mode 0 via `line_render_done`. NO anticipation — and
  the kernel/lcdon REQUIRE el=0 here.

So: **bare-line `early_lead` = 1 when the predicted dispatch dot ≡ 1 mod 4, else
0** (`ppu/render/mode0.rs::m0_flip_events`, gated `tier2_reclock`).
`dispatch_dot = self.dot + proj - lead`; SCX&7 ∈ {3,7} → dispatch ≡ 1.

The IRQ side (`mode_for_interrupt`/`prev_done`, reclock.rs) keys on
`line_render_done`, NOT `vis_early`, so the counter-pinned dispatch dot is
untouched (kernel, int_hblank, intr_2, hblank_ly_scx hold).

## The window-line exclusion (the `clean_bare` gate — critical)

First full-suite two-bin (`/tmp/allgb.txt`, target/gbtr fix vs target/lint reverted)
exposed an A/B swap: el=1 also fires on `window/late_disable_*` lines (bare at flip
time, `!win_active`) with SCX&7=3, FIXING the `_1` rows but DROPPING the `_2`.
Ground truth: `late_disable_early_scx03_wx0f` SameBoy reads `_1`=mode 0 (out0) AND
`_2`=mode 3 (out3) at the SAME `cfl260` — BOTH SameBoy-passing, a sub-dot
distinction. slopgb collapses both test reads to dot 256 (the two reads are 1 cycle
apart → same M-cycle), so it can render only ONE digit for the pair; the cc2
anticipation merely flips WHICH sibling passes → drops the SameBoy-passing `_2`.
This is a read-COLLAPSE A/B, NOT a true fix (contrast m2int_scx3, whose reads land
4 dots apart at distinct dots 252/256).

Fix: gate el=1 on `clean_bare = !wy_latch && wy2 != ly && !win_stalled &&
!win_aborted`. `wy_latch`/`wy2==ly` stay set across a window-DISABLE (only the
LCD-on path clears `wy_latch`), so they mark every window-involved line; m2int/scx
are window-free. Window length is its own sub-family (parallel model + vis-HOLD).

## Result

m2int_m3stat single-speed flag-on: 5 fails → 2. Fixed: `m2int_scx3_m3stat_2`
(DMG+CGB), `m2int_nobg_scx7_m3stat_2` (CGB). Floored: `late_scx4_2` (DMG+CGB) —
slopgb collapses read_1 and read_2 to the SAME dot 256 (SameBoy reads 260/261),
a read-side sub-dot resolution limit, not a boundary issue (the same collapse as
the window late_disable pairs).

Full-suite two-bin (clean_bare gate): clean +N/−0, the window family untouched.
Fixed beyond m2int: 6 `speedchange/*_m2int_m3stat_scx3_2` (CGB) +
`gdma_cycles_short_scx3_2` (CGB) — all bare scx3 read-observers riding the same
lever. Zero SameBoy-passing rows dropped.

Verified: scx3 `vis_early` → dot 256 (el=1), dispatch stays 257; kernel scx0
`vis_early` stays 254 (el=0). Read `m2int_scx3_m3stat_2` now mode 0.
