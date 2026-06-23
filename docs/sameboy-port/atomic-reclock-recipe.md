# The atomic read-frame reclock — implementation recipe (the S5 core)

The C-stage flip is blocked on one thing: the bulk of the flag-on gambatte
regressions (DMG ~84, CGB 248 — `tools/cgb-groundtruth.md`) are slopgb-LE bugs
where the **FF41-mode read lands at the wrong dot**, not where the IRQ
dispatches. This recipe turns the 2026-06-23 #11e dot-level measurements
(`tools/stat-irq-trace.md`) into the concrete reclock to execute. It is the
atomic S2+S3 step (`PORT-PLAN.md`) with hard numbers.

## The measured invariant

The dispatch dots ALREADY match SameBoy (mode-2 @ cfl 0, mode-0 @ cfl 257). The
read frame does not:

| read | slopgb dot | SameBoy cfl | Δ |
|---|---|---|---|
| kernel m2int scx0 (→3) | 252 | 256 | +4 |
| m0int scx0 (→0) | 256 | 261 | +5 |
| m2int scx3 (→0) | 256 | 260 | +4 |

slopgb's deferred FF41 read lands **~4 dots before** SameBoy's. Both frames are
internally self-consistent: slopgb reads at dot D and its mode-3 boundary is at
D+4-ish; SameBoy reads at D+4 and its boundary at D+8-ish. The kernel pin holds
in EITHER frame (read before boundary → mode 3) — which is exactly why you
**cannot move the read OR the boundary alone** (moving the read +4 to 256 with
the boundary left at 256 flips the kernel m2int to mode 0 → pin fails). They must
move together. That is the atomicity.

## The reclock (do all together, in the worktree, converge before committing)

1. **Read frame +4.** Make the deferred read (`Interconnect::read_deferred`,
   `interconnect/cycle.rs`) sample 4 dots (1 M-cycle, single speed) later — at
   SameBoy's cc, not slopgb's current cc+0-after-debt. Equivalently: the
   leading-edge read should land where SameBoy's `read_high_memory` reads, which
   the tracer pins per ROM. (DS = +2; deferred with the rest of the DS frame, S7.)
2. **Every tier2 boundary +4, re-derived against the tracer, NOT hand-nudged.**
   The C1.x calibrations (`mode3_entry_dot`=84, `vis_early`/`early_lead` in
   `render/mode0.rs`, the glitch back-dates) are all fitted to the CURRENT read
   frame. Re-derive each so that, at the NEW read dot, the visible mode matches
   SameBoy's `STAT&3` at the same cfl (instrument `display.c` `mode_for_interrupt`
   + the visible-mode byte alongside the existing `:558`/`:629` traces). The
   per-SCX mode-3 length (`proj`/`lead`) is already correct — the issue is only
   WHERE the read samples it.
3. **Re-validate the 7 tier2 pins + mooneye 91/91 (`SLOPGB_MOONEYE_RECLOCK`)
   continuously.** The pins are the kernel/intr_2/lcdon/hblank/boot anchors; they
   must hold at the new frame (they will, if read+boundary moved together).
4. **The halt `*_m0stat_*` family is a SEPARATE residual** — the sub-M-cycle wake
   clock (`tools/stat-irq-trace.md`: scx3_2/cc1/want0 and scx3_2b/cc1/want2
   collapse). It needs the deferred halt-wake recorded at the IRQ's T-phase, not
   the M-cycle boundary (an S7 finer-grid follow-on, not part of this read reclock).
5. **THEN C2+C3+C4 as one unit:** with the BUG rows fixed, the only remaining
   flag-on failures are the genuine floor (the 39 CGB AGREE + DMG agree set,
   already triaged). Flip both defaults (`interconnect/cycle.rs` `new` defaults +
   `ppu/mod.rs`), rebaseline `baselines/gambatte.txt` to the new floor (the
   baseline + flip MUST land together — a baseline entry written while OFF is a
   passing/stale entry that fails the gate), recapture the 146 golden (drift =
   STAT/mode-0 mode-bit ONLY), verify every oracle zero-drop.

## Why it can't be staged smaller (and why intermediate is RED)

Step 1 alone fails the kernel pin (measured). Step 2 alone (boundaries without
the read move) is the current state. Steps 1+2 together, but for only SOME
boundaries, leaves the un-moved boundaries' reads on the wrong side → red. So the
whole tier2 boundary set + the read frame converge together or the suite is red
throughout — hence uncommittable until convergence, hence the worktree
(`/tmp/slopgb-flip`) + the dot tracers are the vehicle. The window slice
(`a084116`) was the one corner already at the right relative phase (a +2→0
`vis_early` nudge sufficed without crossing the kernel frame); everything else
needs the full move.
