# C2 #11z' — the per-read frame-offset model IMPLEMENTED + measured: it converges the window, atomically

2026-06-28. The goal's START ("a per-read frame-offset model on `read_deferred`,
converging the window/DMG rows") implemented as an experiment and build-measured. The
model is CORRECT and converges the window; it is ATOMIC (the read frame and the
boundary must co-move, which breaks 2 counter-pinned interrupt tests until their
boundaries co-move too). Experiment REVERTED (partial atomic lever, can't ship);
result documented. Defaults NOT flipped; HEAD byte-identical.

## The model (implemented)

The read offset is the INTERRUPT-SERVICE FRAME: slopgb's post-dispatch ISR reads land
+4 (1 M-cycle) before SameBoy's (kernel dispatch→read 252 dots vs 256). The fix is one
M-cycle of clock at the dispatch — `cpu/execute.rs::dispatch_interrupt`, after
`bus.dispatch_retime()`, add `bus.tick()` (a deferred internal M-cycle) on the reclock
path (`SLOPGB_ISR_TICK`-gated for the experiment). This re-frames EVERY post-dispatch
read +4 to SameBoy's dot (the polled reads, no dispatch, are already aligned at +0).

## Measured (correct probe-internal frame)

- **Kernel read shifts to SameBoy's dot:** `m2int_m3stat_1` ly1 dot252→**dot256**
  (SameBoy reads cfl256). Exactly the +4 the offset table predicted.
- **The window family CONVERGES (the goal's target)** — with the boundary co-moved to
  the aligned +0 frame (`vis_mode_read` exit `259`→`263`, the SameBoy `SBex` directly,
  since the reads are now at +0 not +4):

  | config | window fails (276-row CGB) |
  |---|---|
  | HEAD #11y (exit 260, no tick) | 81 |
  | shipped #11z (exit 259, no tick) | 79 |
  | **tick ON + exit 263 (read frame + boundary BOTH at +0)** | **68** |
  | tick OFF + exit 263 (boundary moved, reads stale) | 88 (WORSE) |

  The atomic model is **−11 vs #11z / −13 vs HEAD**. The `tick OFF + exit 263` = 88
  (worse than HEAD) is the PROOF of atomicity: moving the boundary without the read
  frame mis-frames everything; they MUST co-move.

## The atomic cost (why it can't ship as a slice)

The same +4 read shift hits the INTERRUPT-driven counter-pinned tests:
- **mooneye flag-on 91 → 89** (`acceptance_ppu` + `acceptance_root` = the
  `intr_2_mode0_timing` / kernel class). The kernel read at dot256 now lands AT its
  bare-line mode-3 boundary (`line_render_done` ~256, unmoved) → mode 3 → **mode 0** →
  fails. To recover them the BARE-LINE read exit (and the halt-wake LY read, and the
  dispatch dot) must ALSO move +4 — i.e. the whole cc-phase cluster re-clocks to
  SameBoy's frame, and the ~thousand gambatte rows the shift moves get rebaselined
  (`cgb-groundtruth.md`: 248 BUG-fix / 39 floor / 6 DIFF).

So the per-read frame-offset model is **proven correct and convergent** (window −11),
and its scope is now exact: it is the **atomic global reclock** — the dispatch +4
(implemented here) + the bare-line/window/halt boundary co-move + the C-stage
rebaseline + the counter-pinned mask re-derivation. NOT a flag-gated read-only nudge
(the dispatch tick alone breaks the gate flag-on). This is the C2-atomic → C3 lift the
goal stages; #11y (+7) / #11z (+2) shipped the boundary-law families whose offset was
uniform, and this experiment implements + measures the read-frame core that converges
the rest.

## Next session — the bare-line boundary co-move (recover the 2 mooneye groups)

With `SLOPGB_ISR_TICK` ON, the kernel read is at dot256 (SameBoy cfl256, mode 3 wanted)
but slopgb's bare-line `vis_mode` exit (`line_render_done`/`vis_early`) is ~256 →
mode 0. Move the bare-line read exit +4 (a `vis_mode`-read-side back-date for
interrupt-frame reads, analogous to `vis_mode_read`'s window law) so the kernel read
at dot256 sees mode 3 again, and re-derive the halt-wake (`halt_ly_phase`) + int_hblank
masks for the +4 frame. Then the window exit 263 + the dispatch tick co-land cleanly,
mooneye returns to 91/91, and the gambatte rebaseline (C-stage) proceeds. The lever is
now CONCRETE and the convergence is MEASURED (window 68); the remaining work is the
coordinated boundary re-clock, not another characterization.
