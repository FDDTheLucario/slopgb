# SameBoy cycle-exact port — TDD task plan (remaining stages)

`/tdd-test-plan` output for the stages left after S0/S1/S2a–c/S5-foundation shipped
inert. Maps onto `PORT-PLAN.md` S2+S3 (atomic), S4, S6, S7. Each stage is
full-gbtr-baseline + golden-frame-hash + mooneye-439 + mealybug-55 gated,
revert-on-any-non-target regression.

**The S2+S3 convergence is atomic at the GATE** (intermediate flag-on states are
RED), but its constituent code changes (T2–T4) each build green *behind the
`leading_edge_reads` master flag*; only the flip + rebaseline (T5) is the
red-until-green step. So the plan is mostly green-buildable; the gamble is
concentrated in one measured task.

```xml
<plan goal="land SameBoy cycle-exact timing core; lift ~420 gambatte rows; hold every SameBoy-passing oracle">

  <task id="T12" model="sonnet" deps="none">
    <do>S5-refine: extend mode_for_interrupt to the line-0 OAM pulse + the VBlank-entry mode-2 source (display.c:2138, display.c:1778 line-0 exception) in ModeTimeline / the PPU field.</do>
    <test>mode_timeline_tests: mode2_irq_offset==0 on line 0; a new test pinning the VBlank-entry (line 144) mode-2 IRQ source dot vs display.c:2138.</test>
    <done>ModeTimeline + the PPU mode_for_interrupt field reproduce SameBoy's line-0 and VBlank-entry mode-2 sources; inert (flag-off byte-identical).</done>
  </task>

  <analysis id="T1" model="opus" deps="none">
    <do>Pin the exact reclock transform: how "move visible mode→0 boundary +7 dots earlier" maps onto m0_flip_events (ppu/render/mode0.rs) + vis_mode (ppu/stat_irq.rs). Decompose the measured +7 into the +4 line-start mode-0/1 window offset and the +3 longer-mode-3 component (S2c). Decide whether the reclock moves pixel pops (forbidden: breaks mealybug) or only the mode-boundary event dots (required: photo-safe).</do>
    <done>Written decision in docs/sameboy-port/: the precise dot-arithmetic edit list + a proof the pixel pops do NOT move (mealybug byte-identical by construction), OR an explicit finding that SameBoy's frame requires moving pixels (which forces full golden+mealybug re-validation against the hardware photos).</done>
    <why>Architecture crux: photo-safety of the reclock is the whole feasibility question; getting it wrong silently breaks non-recapturable hardware truth.</why>
  </analysis>

  <task id="T2" model="opus" deps="T1,T12">
    <do>PPU updates a live mode_for_interrupt each dot from the mode-2/3/0 spine: mode-2 IRQ 1 dot BEFORE visible mode→2 (lines 1-143), mode-0 IRQ 1 dot AFTER visible mode→0; consumed only when leading_edge_reads is on.</do>
    <test>ppu mod_tests: for a swept (line, scx) the per-dot mode_for_interrupt sequence equals ModeTimeline::mode_for_interrupt; flag-off path leaves the existing vis_mode untouched (net-zero assertion).</test>
    <done>Live mode_for_interrupt field tracks the spine each dot; flag-off byte-identical; full lib green.</done>
    <why>Sub-dot anchor-swing logic with opposite-sign offsets; the kernel-pair separation depends on exact dot placement.</why>
  </task>

  <task id="T3" model="opus" deps="T2">
    <do>Flag-gated StatUpdate engine: drive StatUpdate::update from (mode_for_interrupt, FF41, lyc_match) each dot, OR the 0→1 rising edge into IF bit 1, replacing stat_events_tick on the flag-on path. Add SameBoy's LCD-off guard + OAM-DMA-mode-2 guard.</do>
    <test>stat_update_tests / ppu stat tests: a synthetic dot timeline raises IF exactly on rising edges (STAT-blocking: a second source joining does NOT re-fire); LCD-off forces the line low; flag-off path unchanged.</test>
    <done>Flag-on STAT IRQ path is the rising-edge level engine; flag-off identical to stat_events_tick; lib green.</done>
    <why>Replaces the load-bearing IRQ engine; edge/blocking semantics + guard conditions are subtle and pinned by mooneye STAT tests.</why>
  </task>

  <task id="T4" model="opus" deps="T3">
    <do>On the flag-on path, enable leading_edge (cc+0) FF41/OAM/VRAM/palette reads + apply the +7 reclock to the visible mode→0 boundary together (per T1's edit list). Un-ignore the S0 kernel-pair acceptance test (run it flag-on).</do>
    <test>S0 acceptance (un-ignored, flag-on): m2int_m3stat_1 FF41 read == 3 AND m0int_m3stat_2 == 0 simultaneously, while mooneye intr_2_mode0_timing still passes (run flag-on).</test>
    <done>The kernel pair separates on the flag-on path with the canonical mooneye test held; this is the convergence point for the read/boundary machinery.</done>
    <why>The atomic read-phase + boundary change; every prior single-lever attempt was an A/B swap, so correctness here is all-or-nothing.</why>
  </task>

  <analysis id="T5" model="opus" deps="T4">
    <do>ATOMIC CONVERGENCE: make leading_edge_reads the default (flip the master flag on), rebaseline gambatte (~7000 cases), recapture the 146 golden frames, run the full gate. Hold EVERY SameBoy-passing oracle.</do>
    <done>Full gate green: 439 mooneye + 55 mealybug photos + gbmicrotest + wilbertpol + age all green (zero drop), golden recaptured, gambatte rebaselined NET-POSITIVE (the ~420 lift). If ANY SameBoy-passing oracle drops → REVERT to flag-off, pin the exact residual with numbers (this is the documented multi-session boundary).</done>
    <why>The all-or-nothing measured gamble; the project's never-drop-a-SameBoy-pass invariant is the kill criterion.</why>
  </analysis>

  <task id="T6" model="sonnet" deps="T5">
    <do>S4: OAM/VRAM accessibility unblock at the back-dated visible boundary (replace the m0_access_flip cc+2 half-split with the SameBoy visible-edge unblock).</do>
    <test>interconnect subdot tests: OAM (FE00-FE9F) + VRAM (8000-9FFF) read/write at the back-dated boundary dot return FF/are-dropped per SameBoy; lifts oam_access/vram_m3 _ds rows.</test>
    <done>OAM/VRAM accessibility matches SameBoy on the back-dated frame; gbtr net-positive; no oracle drop.</done>
  </task>

  <task id="T7" model="sonnet" deps="T6">
    <do>S4: CGB palette-RAM 2-dot HBlank pulse unblock (display.c:2090-2121) replacing pal_access_flip.</do>
    <test>interconnect/ppu cgb tests: FF69/FF6B readable window == the 2-dot pulse at HBlank entry; lifts cgbpal_m3end residue.</test>
    <done>CGB palette accessibility matches SameBoy; gbtr net-positive; no oracle drop.</done>
  </task>

  <task id="T8" model="sonnet" deps="T7">
    <do>S4: retire the now-dead m0_access_flip / pal_access_flip / stat_mode_edge stamps + obs/event_phase consumers superseded by the back-dated boundary.</do>
    <test>build + full gate green after removal; no test references the deleted stamps; subdot tests updated/removed.</test>
    <done>Dead stamp machinery removed; gate byte-identical to post-T7; clippy -D clean.</done>
  </task>

  <task id="T9" model="opus" deps="T5">
    <do>S6: port the per-model cycle_write conflict-staging table (sm83_cpu.c:131-318) into the Bus write path, replacing stage_write; use cycle_clock::Conflict (ReadOld/ReadNew/WriteCpu) per IO register per model.</do>
    <test>interconnect tests: each conflict class commits the write at the SameBoy dot (e.g. IF WriteCpu +1 T, the speedchange/hdma cases); mealybug 55 photos byte-identical; lifts speedchange/hdma rows.</test>
    <done>Write conflict timing matches SameBoy per model; mealybug byte-identical; gbtr net-positive; no oracle drop.</done>
    <why>Per-model register-by-register conflict map; off-by-one T-cycle errors silently corrupt mealybug hardware photos.</why>
  </task>

  <task id="T10" model="opus" deps="T5,T9">
    <do>S7: re-unify CGB double speed onto the back-dated model (DS = the same event-phase machinery at the 2-dots-per-cc rate, no separate DS half-split).</do>
    <test>gbtr DS suites: m3stat_ds / speedchange / hdma_cycles_ds rows match SameBoy; reproduce the INC-DS-1(+43)/task6(+84) trades on the new frame; no SS regression.</test>
    <done>Double speed unified onto the back-dated frame; DS gbtr net-positive; SS byte-identical; no oracle drop.</done>
    <why>DS sub-dot phase was the hardest historical cluster (class-A); unifying it without regressing the SS frame is delicate.</why>
  </task>

  <task id="T11" model="sonnet" deps="T10">
    <do>S7: delete the superseded scaffold — event_phase / lead_eighths / ACCESS_PHASE / EdgeKind + the cc-reclock dot_phase field — once the back-dated model subsumes them.</do>
    <test>build + full gate green after removal; no references remain; the half-dot-grid scaffold tests are removed or repurposed.</test>
    <done>Scaffold deleted; gate byte-identical to post-T10; line-count + clippy clean; CLAUDE.md/docs updated.</done>
  </task>

</plan>
```

**Summary:** 12 tasks (1 haiku-free: 5 sonnet, 6 opus, 1 mixed-analysis) — T1+T5 are the
opus analysis/gamble pair. Critical path: T1→T2→T3→T4→T5 (the atomic convergence),
then S4 (T6→T7→T8) and S6/S7 (T9→T10→T11) fan out from T5. T12 (S5-refine) is an
independent prerequisite feeding T2. The session's spike targets T1→T5; S4/S6/S7
land only if T5 converges green.
