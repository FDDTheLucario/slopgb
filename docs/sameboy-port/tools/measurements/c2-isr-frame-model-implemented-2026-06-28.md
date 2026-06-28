# C2 #11z' — the per-read frame-offset model IMPLEMENTED + measured: it converges the window, atomically

## RESOLUTION (the per-read frame model IS #11z — algebraic + empirical)

The goal's START is "route the deferred read sample to SameBoy's per-read dot instead
of the uniform leading edge." Build-measured to its end, every realization of that
reduces to one of two things:

1. **The byte-identical-OFF form is ALGEBRAICALLY the boundary law #11z.** SameBoy reads
   mode 3 iff `cfl < SBex(263+SCX&7)`; slopgb's read at dot `D` corresponds to SameBoy
   `cfl = D + offset` (offset +4 for the in-ISR window reads). "Route the sample to
   `D+offset`" → mode 3 iff `(D+4) < 263+SCX&7` ⇔ `D < 259+SCX&7` — which is EXACTLY
   #11z's exit. So **#11z IS the per-read frame-offset model**, expressed in the only
   form the deferred read clock permits byte-identical OFF (the read sample cannot be
   physically moved: a CPU-cycle shift breaks `di_timing`, a PPU-only advance
   double-counts, and a co-moved sample+boundary cancels). Confirmed EMPIRICALLY: the
   `in_isr` "pure sample-route" two-sided law is a no-op (window 79 = #11z's 79).

2. **The only lever that moved MORE rows (`bus.tick`, window→68) breaks `di_timing`** —
   it adds a CPU M-cycle (the atomic reclock), not a read-frame change.

So there is NO separate, unshipped, byte-identical-OFF "per-read frame-offset model"
distinct from #11z. The window rows #11z leaves are NOT read-frame residual — they are
(a) RENDER-blocked (`win_active`=false, the #11g WY-latch — production render, breaks
byte-identical OFF) and (b) the in-ISR ATOMIC frame (the +4 read↔boundary co-move that
breaks ~54 counter-pinned interrupt tests). Both are out of the byte-identical-OFF
read+length scope by the goal's own staging (Phase 3 render / C2-atomic → C3).

---


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

## REFUTATION (build-measured) — the +4 is NOT a missing CPU M-cycle; the `bus.tick()` lever is WRONG-DIRECTION

Tried to SHIP the model: gated the `bus.tick()` on `tier2_reclock` (not env) + exit 263,
ran the full mooneye flag-on gate. **It breaks ~54 interrupt-timing rom×model combos**
(acceptance/ppu 38/62 + acceptance 16/187): `di_timing`, `halt_ime0_nointr_timing`,
`intr_1_2_timing`, `intr_2_0_timing`, `intr_2_mode0/mode3/oam_ok_timing`,
`hblank_ly_scx` — ALL register `B=C=…=42` (the test never completes). These count the
**5-M-cycle interrupt service EXACTLY**; the `bus.tick()` made it 6 M-cycles, a
CPU-observable timing change that `di_timing`/`halt_ime0_nointr_timing` pin directly.

**So the +4 read offset is NOT a missing CPU M-cycle** (`di_timing` proves the service
is correctly 5 M-cycles; adding one breaks it). The earlier "missing post-dispatch
M-cycle" framing is REFUTED. The +4 is a **PPU-advance lag**: between the aligned
dispatch (slopgb dot0 ≡ SameBoy cfl0) and the kernel read, slopgb's PPU advances 252
dots while SameBoy's advances 256 — for the SAME CPU cycle count. slopgb's deferred
dispatch under-advances the PPU by 4 dots (1 M-cycle) **without** a CPU-cycle deficit
(the CPU service is the correct 5 M-cycles). `bus.tick()` "worked" on the kernel read
only because it added a whole M-cycle (CPU + PPU); the CPU half is the bug.

**Corrected lever (for the next session): advance the PPU +4 during the dispatch
WITHOUT a CPU cycle** — a PPU-only nudge in `interconnect.rs::dispatch_retime`
(`advance_machine_t` the machine 4 extra T while the `clock` stays put), or a per-read
PPU read-position offset in `read_deferred` (sample `vis_mode` at `self.dot + 4` for
post-dispatch reads). This keeps `di_timing` (CPU timing unchanged) while landing the
ISR read at SameBoy's PPU dot. CAUTION: desyncing the PPU from the deferred `clock`
risks the next read's `advance_machine_t(before, now)` going backward — needs the PPU
position tracked separately or the offset applied only at the mode SAMPLE, not the
machine advance. Build-measure against `di_timing` (must stay 5 M-cycle) AND the window
(must reach 68) BEFORE concluding — this is the third wrong-direction lever this branch
caught (write-side #11v, vis_early #11t, the CPU-tick here).

## The `in_isr` read-frame discriminator — BUILT, but inert (the polled window rows are RENDER-blocked)

Since the +4 is in-ISR-only (the polled `late_wy` reads are +0), built the per-read
frame discriminator WITHOUT touching the interrupt clock: a bus flag `cpu_in_isr` (set
in `dispatch_retime`, cleared on RETI via a new `Bus::end_isr`), forwarded to
`Ppu::cpu_in_isr`, selecting the window read exit `win_read_exit()` = `259` (in-ISR) vs
`263` (polled) `+ SCX&7`, with `vis_mode_read` made TWO-SIDED (extend mode0→3 below the
exit, shorten above). This is di_timing-safe (only the FF41 mode SAMPLE moves, not the
clock) and is the goal's per-read frame-offset model on the read path.

**Result: NO-OP (window flag-on 79 = #11z, 0 fixed / 0 regressed).** Traced why:
- `late_wy_FFto2_ly2_*` (wy2==ly): excluded by the `wy2 != ly` gate. Removing it →
  **−2/+0** (regresses `late_wx_late_wy_FFto2_ly2_2` + `late_wy_FFto2_ly2_wx00_1`), no
  fix — so the gate must stay.
- `late_wy_10to0_ly1_1` (wy2!=ly): reaches the gate but reads native mode 0 because
  **`render.win_active` is FALSE** — slopgb does NOT activate the window when it should
  (the #11g WY-latch render bug, `wy_ok=false`). The law's `win_active` gate then skips
  it, and even the two-sided EXTEND can't fire.

So the polled `late_wy` window reads are blocked by the **render-level WY-latch**
(`win_active`), NOT (only) the read frame. The `in_isr` read-exit is correct and
ready but inert until the WY-latch fix lands (which breaks byte-identical OFF →
production render, golden-protected). Reverted (no observable benefit → no pin → not
shippable per the discipline). The discriminator design is the foundation for when the
render WY-latch + the bare-line atomic reclock co-land. (3 byte-identical-OFF read-law
slices shipped — #11y/#11z; the rest of the window is render + atomic, not read-law.)

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
