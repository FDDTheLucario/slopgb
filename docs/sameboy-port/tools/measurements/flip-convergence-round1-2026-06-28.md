# Flip convergence round 1 (2026-06-28 #11ac) — landed +6/-0, AGREE-floor insight

## Shipped this round (tier2-gated, byte-id OFF, 23 pins green, mooneye 91/91)
- **#11ac wxA6/wxA5 window length-law extension (+6/-0)**: full-CGB regressions
  206 -> 200. The #11z read-side law (visible window mode-3 exit 259+SCX&7)
  extended from wx<0xA0 to wx<=0xA6 (off-screen-trigger window, SameBoy extends
  to the same exit), sprite-free-gated. Fixed 6 m2int_wxA5/wxA6 m3stat rows.

## KEY INSIGHT — many 'regressions' are AGREE-floors, NOT bugs (real fix-count < 200)
Classified late_disable against SameBoy (SBREAD ff41 measurement read):
- late_disable_scx{2,3,5}_1: SameBoy reads mode3 == slopgb-tier2's got -> AGREE-floor
  (slopgb-tier2 is CORRECT; gambatte out0 is the outlier). NOT a bug -> baselines
  at the flip, needs NO fix.
- late_disable_early_scx03_wx*, late_reenable_*: SameBoy reads mode0 == want -> BUG.
So the late_disable cluster is MIXED. The real fix-count is the BUG subset only;
the AGREE rows self-resolve at the flip + rebaseline. cgb-groundtruth's 248/39 was
the OLD (pre-sprite-fix) snapshot; sprites are gone from the current 200, and the
AGREE fraction is higher than 39. A full BUG/AGREE re-classification (SameBoy OCR)
is the next tooling step to get the precise fix-list.

## Remaining clusters (current 200 CGB regr) + tractability
- window late_wy (~16): WY-trigger + LCDC frame-phase RENDER. Deep.
- window late_disable/reenable (~13): MIXED AGREE-floor + render-abort BUG. Per-row.
- lycEnable/m1/m0enable/ly0/misc ENGINE (~50): the stat_update_tick mode-0 DELIVERY
  core (frame0_m0irq_count=0: engine fires ~144 m0 edges/frame but the deferred cc+0
  read-frame mis-samples the IF bit -> running CPU sees 0). The hard atomic core;
  needs a dot-level SameBoy GB_STAT_update IF-write tracer + slopgb CPU-delivery
  trace. localized to interconnect/tick.rs m0_rise + if_stat_late + reclock.rs.
- cgbpal_m3end (3): #11w-REFUTED (palette unblock physically lags). Leave.
- S6/S7 DS (~32): lcd_offset/speedchange/dma/tima/serial. Separate port stage.

## Read-law clean-slice space: now EXHAUSTED (window length fully extended wx<=0xA6).
The rest needs render (abort/trigger) + the engine delivery core + DS = multi-session.

## Engine m0-delivery core — localized concretely (frame0_m0irq_count=0)

Traced `frame0_m0irq_count_scx2_1` (want90 got00): the flag-on engine
(`stat_update_tick`) FIRES **2299 mode-0 dispatches** (mfi=0, ~144/frame × ~16
frames) — `pending_if |= IF_STAT` on every mode-0 STAT rise. So the engine is NOT
silent; the CPU just never COUNTS them. ROOT (confirmed): the engine raises
`pending_if` at the dot END (`stat_update_tick`, after `step_dot`), but the CPU's
deferred read samples IF at cc+0 (the M-cycle leading edge) — a ~4-dot/1-M-cycle
read-phase gap, plus the `if_late`/`second_half` halt masking. This is the
"dispatch↔read-phase miss" — the hard atomic core (the ~40-row engine cluster:
halt/lycEnable/m2enable/ly0/misc). Fixing it must NOT break the counter-pinned
int_hblank (mode-0 halt, passing) / mooneye2022 / gbmicrotest tests that pin the
5-M-cycle service + the halt-wake grid — so it needs a dot-level SameBoy
`GB_STAT_update` IF-write tracer + a slopgb CPU-IF-delivery trace, then a careful
re-frame of the engine's IF-raise vs the deferred read (not a blind nudge).
Localized: `interconnect/tick.rs` (`m0_rise`/`if_late`/`if_stat_late`) +
`reclock.rs::stat_update_tick` (the `pending_if` raise) + the dispatch retime.

## SCX-latch fixed-dot read-law — BUILD-MEASURED NEGATIVE (do-not-re-chase)

Attempted (#11ad, REVERTED): a bare-line read-law that latches `SCX&7` at a fixed
mode-3-entry dot and reads mode 0 at the latched-SCX exit when `eff.scx&7` changed
mid-line (the `late_scx4`/`late_disable_early` over-extension). Implemented (field
`scx_at_m3` + dot-88 capture + `vis_mode_read` law). Measured: dot-84 latch regressed
`late_scx4_1` (want3, a SameBoy-pass — both _1/_2 write SCX after dot-84 via the 2-dot
stage delay → both latch 0). Dot-88 latch fixed `late_scx4_2` + `late_scx_late_disable_0/1`
(+3) but REGRESSED `late_scx_late_disable_2` (want3, a SameBoy-pass, classifier-confirmed
BUG=sb==want3) — its SCX latches LATER than dot-88. **No fixed latch dot is clean: late_scx4
needs the deadline EARLY (miss _2's write), late_scx_late_disable needs it LATE (catch _2's
write) — they conflict.** This is the sub-dot SCX-latch (the discard-point deadline is
sub-M-cycle, config-dependent on the exact write timing + window geometry). A fixed-dot
version always drops a SameBoy-pass → reverted. Needs the genuine sub-dot fetcher SCX
latch (the render port), not a read-law. The clean +7 (window length law) stands.
