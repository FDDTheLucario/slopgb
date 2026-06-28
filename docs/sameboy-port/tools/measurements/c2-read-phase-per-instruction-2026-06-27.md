# C2 — the cc-exact read phase is PER-INSTRUCTION (the read half's precise mechanism, #11y)

## RESOLUTION 3 (kernel read cfl MEASURED) — the read offset VARIES PER-TEST by ~1 dot; +3 was a wrong assumption.

Measured SameBoy's kernel read directly (not assumed): `m2int_m3stat_1` reads **ly1
cfl256** (slopgb dot252, mode 3) → offset **+4**. The window `m2int_wx03` reads **ly1
cfl265** (slopgb dot260, mode 0) → offset **+5**. The DISPATCH (mode-0 IRQ) is +3
(dot254≡cfl257). So the slopgb-dot ↔ SameBoy-cfl offset is NOT a single constant — it
is **+3 (dispatch), +4 (kernel read), +5 (window read)**, varying per-event and
per-test by ~1 dot. (My "+3 everywhere" was an unmeasured assumption — the 4th
correction this session.) Both reads PASS because each lands on the correct side of
its own boundary (kernel before its exit = mode 3; window after = mode 0); the FAILING
rows (late_wy/DMG) read where the ±1 per-test offset variation flips the result.

So the read half is the **per-test/per-event frame-offset model**: the CPU↔PPU frame
alignment differs by ~1 dot between tests (LCD-on timing, the read M-cycle's phase vs
the dispatch M-cycle), and the boundary-sensitive rows need it modelled. This is NOT a
uniform read shift, NOT a read phase (cc+0 both), NOT pending (4 both) — it is the
genuine per-test frame variation, the deepest characterization of the read residual.
The shipped `260+SCX&7` length law converges with it once the per-test offset is
modelled; the next step is to instrument the LCD-on cycle + the read M-cycle phase per
test and derive the offset, then apply it to the window/m3stat read frame.

## RESOLUTION 2 (pending traced) — NOT pending either; it's a PER-TEST frame/lcd-offset.

Added a `pend=` field to the FF41 read tracer (`read_deferred`). Measured: the window
read (`m2int_wx03` ly1 dot260) AND the kernel read (`m2int_m3stat_1` ly1 dot252) BOTH
have **`pend=4`** (the normal post-fetch debt). So slopgb's `pending` is identical for
both — the +2 is NOT a pending difference. Both at cc+0, both pend=4, yet the window
read offset is +5 (dot260/cfl265) and the kernel's is +3 (dot252/cfl≈255). The frame
offset (slopgb dot ↔ SameBoy cfl) therefore differs **PER-TEST**: the window ROM and
the kernel ROM turn the LCD on at different cycles → different CPU↔PPU frame alignment
= the **lcd-offset** (the #11q mechanism), NOT the read mechanics (read phase REFUTED,
pending REFUTED). The window-length law ships clean (+7/−0) because the m2int_wx reads
land mode 0 on BOTH sides of the +2; the excluded late_wy/DMG read across the boundary
where the per-test lcd-offset flips the result. So the cc-exact "read half" is really
the **lcd-offset / per-test frame model** (#11q lineage — line-start OAM windows
already model part of it) applied to the window/m3stat read frame, NOT a sub-M-cycle
read clock. That reframes the next step: extend the #11q lcd-offset frame model to the
window m3stat reads, not build a new read-phase clock.

## CORRECTION (source-checked) — SameBoy reads at cc+0, SAME as slopgb. The "cc+2 read phase" below is REFUTED.

`Core/sm83_cpu.c::cycle_read` (verified): `if (pending_cycles) GB_advance_cycles(pending_cycles); ret = GB_read_memory(addr); pending_cycles = 4;` — advance the previous debt, sample at the LEADING edge, park 4. This is BYTE-FOR-BYTE slopgb's `read_deferred` (`clock.read()` = `clock += pending; sample; pending = 4`). So SameBoy reads FF41 at **cc+0**, exactly like slopgb. **There is NO per-instruction read-phase difference (no cc+2).** The "≈cc+2 / per-instruction read phase" mechanism hypothesized below is REFUTED by the source. (Third over-interpretation this session the build-measure discipline caught — after the "read-frame de-mask" and the line-0-length-term, both also walked back. The lesson holds: infer nothing the source/measurement doesn't show.)

The SOLID residual: the window FF41 read offset is +5 (slopgb dot260 / SameBoy cfl265) vs the +3 IF-rise/dispatch offset — a real **+2 discrepancy** between the read frame and the dispatch frame, with BOTH emulators sampling at cc+0. Since the read sample is `before + pending` in both, the +2 is in the **`pending` (the M-cycle debt advanced before the read)** or a non-uniform frame offset — NOT the read phase. Resolving it needs tracing the exact `pending` at the window read in both emulators (does slopgb's pending differ from SameBoy's by 2 there?), which is the genuine cc-exact-read investigation — but it is a `pending`/frame question, not the read-phase model described below.

---


2026-06-27, after the #11y window-length law shipped (+7/−0). Pinning the read half of
the atomic reclock to its exact mechanism via direct SBREAD measurement.

## The finding

Direct `SBREAD ff41` (SameBoy) vs `SLOPGB ff41` (slopgb) for the window m3stat reads:
- `m2int_wx03_m3stat_2`: slopgb reads **ly1 dot260**; SameBoy reads **ly1 cfl265**
  (mode 0). Offset = **+5**.
- The DISPATCH offset is **+3** (slopgb `line_render_done` dot254 ≡ SameBoy mode-0
  IRQ cfl257, counter-pinned both models).

So the FF41 READ offset (+5) ≠ the DISPATCH offset (+3): the window read samples **2
dots later** than the dispatch-aligned frame would put it. Since slopgb's deferred
read samples at the M-cycle LEADING edge (cc+0), SameBoy is sampling this read at
≈**cc+2** (the data-access phase), 2 dots into the read M-cycle.

But the KERNEL `m2int_m3stat` FF41 read PASSES at slopgb's cc+0 (the pinned
`tier2_kernel_pair_matches_sameboy_target`). So **the FF41 read phase VARIES per
instruction**: the kernel's `ldh a,(FF41)` samples at cc+0, the window-line m3stat
read samples at cc+2. slopgb's uniform cc+0 deferred read collapses this — it reads
every FF41 at the leading edge, so reads SameBoy resolves at different sub-M-cycle
phases land at the same slopgb dot.

## Why the #11y window length law still ships clean (+7/−0)

The law fixes the window reads that land mode 0 BOTH ways: m2int_wx reads at/after the
exit (slopgb dot260 ≥ the law exit 260 → mode 0; SameBoy cfl265 > exit cfl263 → mode
0). The excluded rows (late_wy/DMG) read on the OTHER side of the exit, where the +2
read-phase difference flips the result — those need the per-instruction phase, not a
uniform offset (a +2 shift breaks the kernel cc+0 reads).

## The cc-exact read sample (the precise next step)

The read half is now pinned to its mechanism: the deferred read must sample FF41 (and
the accessibility/IF reads) at the **per-instruction sub-M-cycle phase** SameBoy uses,
not the uniform cc+0 leading edge. The phase source is the `cycle_clock.rs`
deferred-commit machine's conflict-class / `pending` model (the same per-instruction
sub-M-cycle phase the WRITE side uses via `write_conflict`). The build: route the FF41
read through a conflict-class-aware sample point (cc+0 vs cc+2 per the read's
instruction context) instead of `clock.now()`'s rounded leading edge, then the
excluded window/DMG rows converge with the validated `260+SCX&7` length law and the
kernel reads stay at cc+0.

This requires dual-emulator per-instruction read-cycle instrumentation to map which
reads sample at which phase (the kernel `ldh a,(FF41)` after a dispatched interrupt
vs the window m3stat read in a tight poll), then the conflict-class routing — a deep,
focused architectural change to `interconnect/cycle.rs::read_deferred`. It is NOT a
uniform read-frame shift (the +5 vs +3 + the kernel cc+0 prove that). It is the read
half of the atomic reclock, now characterized to the exact lever (the per-instruction
read phase) rather than "the read frame is off".
