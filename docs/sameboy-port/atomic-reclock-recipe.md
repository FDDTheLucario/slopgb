# The atomic read-frame reclock — implementation recipe (the S5 core)

> **2026-06-24 — REPRODUCED, then the literal "read frame +4" REFUTED. Read this
> box before executing step 1.** The /tmp tooling was rebuilt from scratch this
> session (SameBoy 1.0.2 instrumented tester + slopgb `SLOPGB_S5DBG` read-dot
> tracer, both verified to reproduce every dot below *exactly*). Two hard results
> change the plan:
>
> 1. **A uniform whole-dot +4 of read AND boundary is a NO-OP.** It preserves
>    every read-vs-boundary relative ordering, so it changes no observable. The
>    "+4 together" framing only *aligns frames* — it cannot fix a single regr row.
> 2. **"Read +4" cannot mean "sample 4 dots later" (= cc+4 = the trailing edge =
>    production's tick-then-access read).** Measured: production (flag-off, cc+4)
>    **FAILS the kernel `m2int_m3stat_1`** (reads mode 0 at the FF41 poll, want 3;
>    `m0int` passes). Only the flag-on cc+0 *leading-edge* read passes both. So
>    moving the read to cc+4 reverts to production and breaks the kernel. The cc+0
>    leading-edge sample is the flag-on path's whole value and must NOT move.
>    **EXECUTED 2026-06-24 (not just reasoned): wired step 1 literally** (FF41
>    `read_deferred` flush→sample at cc+4) → kernel m2int OCR **3→0 FAILS**, read
>    dot 252→256 past the boundary 254; m0int 256→260 still mode0. Recovering
>    m2int needs the mode-0 boundary >256, but the bare boundary is **structurally
>    capped at the pipe end** (`advance_lx` lx==160 = dot 256) — a local
>    "boundary +4" cannot exceed it. Only moving the whole line geometry +4 (the
>    pipe end, which cascades to the counter-pinned mooneye `intr_2_mode0`/DIV)
>    recovers it = the global rebaseline, NOT a local boundary tweak. So step 2's
>    "every boundary +4" is not a local lever for the bare kernel line.
>
> What the measurements actually show (slopgb dot ↔ SameBoy cfl, line-start
> aligned — both dispatch mode-2 at dot/cfl 0): the residual is a **~2-dot
> SUB-M-cycle read-vs-boundary phase**, not a whole-dot frame shift. slopgb's
> deferred read sits 2 dots *closer* to the mode-0 boundary than SameBoy's
> (slopgb m0int: dispatch 254, read 256 → gap 2; SameBoy: dispatch 257, read 261
> → gap 4). Closing that 2-dot gap is the real lever and it lives at the
> **eighth-grid (`event_phase`/`lead_eighths`, port S7)**, because the deferred
> read samples at the M-cycle *leading edge* (cc+0) and so cannot order a read
> against a boundary *within the same M-cycle* — the documented cc-collapse wall
> (`stat-irq-trace.md` halt family), now confirmed to also gate the bare/window
> `m2int_wx*_scx*_m3stat` FF41 reads (e.g. `m2int_wxA6_scx3_m3stat_2` [Dmg]:
> slopgb read dot 256 mode 3 / SameBoy cfl 260 mode 0 — reproduced exactly).
>
> **Real flip-regr distribution measured this session** (all 6844 gambatte rows,
> probe ON−OFF = 430 true regr; `tools/measurements/flip-regr-2026-06-24*.txt`):
> window 107, sprites 87, halt 32, m1 26, lycEnable 26, enable_display 14,
> speedchange 13 (DS), m0enable 12, vram_m3 11, oam_access 11, m2int_m3stat 11, …
> **Per-family diagnosis (dual-emulator traced — see
> `tools/measurements/flip-regr-2026-06-24-summary.txt`):** the window 107 is
> NOT the read-wall and NOT `early_lead`-tunable — it is per-config mode-3 LENGTH
> geometry with opposite-direction errors (`m2int_wx00` mode-3 too long: slopgb
> dot260 mode3 / SameBoy cfl265 mode0; `late_wy_10to0` mode-3 too short: slopgb
> dot260 mode0 / **SameBoy cfl260** mode3 — read dots MATCH → pure length bug).
> Fix = a tier2-gated window mode-3 length port (proj/lead per wx/scx/late-WY)
> vs SameBoy, NOT a uniform `early_lead`. m1+lycEnable 52 = mode-1/LYC/IF-delivery
> (want=E0↔E2 spurious/missed STAT IRQ + mode-bit 3↔1). sprites 87 = L2 geometry.
> halt 32 + scx-extended m3stat = the cc-collapse sub-M-cycle wall (S7).
> **Next session: per-config geometry ports (window length, sprite L2) + the
> IF-delivery re-arm for m1/lyc + the eighth-grid sub-dot read-observer phase for
> the m3stat family — NOT a whole-dot read+4.** Each needs SameBoy per-config
> ground truth + a tier2-gated parallel calc. Steps 1–2 below are kept for history
> but step 1 (move the read +4) is refuted.

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
