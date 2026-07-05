GOAL: **LAND THE C3 FLIP — flip the SameBoy cycle-exact timing on by default.**
The entire flag-gated approach is measured-COMPLETE (render 100/100; dispatch +
S6 build-measured atomic). The flip is gated on exactly ONE open architectural
problem: the **coherent per-T coroutine retime** (HALFDOT Part A). Build it, then
execute the C3-FLIP-CHECKLIST. This is a genuine multi-session atomic rewrite —
flag-on intermediate states are RED by construction; converge the WHOLE frame,
then measure. Do NOT re-attempt the refuted flag-gated slices (see DO-NOT-RETRY).

═══ STATE AT HANDOFF (2026-07-04, after #11bo–#11bu + the fork merge) ═══
- **`main` = the UNIFIED trunk** (`c7b2bd9`): the BGB-debugger UI fork + the
  SameBoy emulator-port fork merged (2-parent unrelated-histories merge, both
  ancestors). All gates green: workspace builds, lib 740/0, frontend bins 371/0,
  **mooneye 91/91 flag-off AND flag-on** (ROMs required, = 439/439 rom×model),
  gbtr golden fingerprint byte-identical, clippy `-D warnings` clean. `tier2`
  default NOT flipped (`interconnect.rs` `leading_edge_reads:false`,
  `tier2_reclock:false`). Branch from `main` for this work.
- **§3b RENDER half COMPLETE — pixel two-bin 100/100** (#11bo–#11bq). Every
  mode-3 pixel-reference flip-blocker lands flag-gated, production byte-identical
  OFF: 5 mechanisms (SCY/palette parity, LCDC BG-addr split, SCX-DS, BG-priority,
  OBJ-enable draw-side) + 6 residuals (SCY parity, WX defer/split, window-abort
  split). 63 tier2 pins; CGB two-bin 291/291 IDENTICAL SET (zero-drift, many ×).
  Map: `measurements/dmg-m3-render-reclock-2026-07-04.md`.
- **C3-flip census = NO-GO, 98 DMG SameBoy-pass blockers** (#11bt). The CGB-OCR
  bar HOLDS (0 blockers, 37/37 rebaseline). The 98 are 100% DMG: the interrupt-
  service / timer-completion / dispatch-count / read-frame families. They can
  neither be fixed by a flag-gated slice NOR rebaselined (SameBoy passes every
  one → forbidden drop). Map: `measurements/c3-flip-census-2026-07-04.md`
  (+ the 71-row rebaseline manifest, INERT until the 98 clear).

═══ THE ONE REMAINING LEVER — the coherent per-T coroutine retime (HALFDOT Part A) ═══
The 98 DMG blockers are all one thing: slopgb's deferred clock advances the WHOLE
machine (CPU+PPU+timer+serial) to the read/write's cc+0 leading edge, so the IRQ
DISPATCH, the timer/serial COMPLETION, and the ISR READS all sit on the cc+0
frame together (internally coherent → the count tests pass). SameBoy instead
advances the PPU/timer EAGERLY per-T (`GB_advance_cycles` runs the display
coroutine immediately) while the CPU keeps `pending_cycles`; reads sync to the
exact-T via `GB_display_sync`. The flip must move the WHOLE frame to SameBoy's
model COHERENTLY. Spec + refutations: `measurements/dispatch-retime-plan-2026-07-04.md`
(§4 build plan + §8 the eager-PPU refutation) + `HALFDOT-BUILD-PLAN.md` (§1 the
SameBoy per-tick order, §3 Parts A/B/C/D, §5 the atomicity proof, §6 staging).
The half-dot grain (`Ppu::tick_half`/`dhalf`, #11ba) + the read-sync
(`read_deferred` = half-dot `GB_display_sync`, #11be) are LANDED — Part A-render
is DONE (100/100). What remains is Part A-dispatch + Part C: move the dispatch/
halt-wake/timer/serial-completion to the coherent frame and DELETE the read-frame
approximations they subsume (the `early_lead` case-tower `render/mode0.rs`, the 7
`vis_mode_read` shadow laws, the #11bk hblank-IF two-latch, poweron/coincident
masks) as the coherent frame makes them correct — do NOT keep both.

═══ WHY IT IS ATOMIC (proven, not asserted — three refutations bound the space) ═══
- **#11ai** C2ADV +4 PPU advance at dispatch → mooneye `intr_2` HANGS (B=42): the
  dispatch dot is counter-pinned; it cannot move ALONE.
- **#11br** imminent-rise fold (dispatch@cc+4, reads@cc+0) → +22 presence rows but
  −9 count SameBoy-passes + `intr_2` hang: cc+4-dispatch ∧ cc+0-reads is
  INCOHERENT for the dispatch-COUNT tests, and there is NO bus-observable
  discriminator between a presence row (wants the move) and a count row (wants no
  move) using the same mode-0 rise.
- **#11bs** the genuine eager-PPU/deferred-CPU split (the §4 build plan) BUILT +
  REFUTED: +24 / −53 coherent-count rows, mooneye 88/91. Same weld.
- **#11bu** S6 timer/serial completion read-fold: even the minimal FF0F-OR fold
  drops a SameBoy-pass (`tc00_late_div_write_if_1a`). Co-temporal proof: identical
  timer read-state, opposite wants → no read-time discriminator. Welded.
The lesson: the dispatch dot is WELDED to the read frame by the count tests. Only
a SINGLE coherent frame (the WHOLE machine at SameBoy's per-T model) resolves it.
No partial/flag-gated slice exists — this is the make-or-break rewrite.

═══ THE PLAN ═══
1. **Build the coherent per-T coroutine retime** (flag-gated behind `tier2_reclock`,
   production OFF byte-identical): advance PPU + timer + serial eagerly per-T;
   the CPU dispatch/halt-wake reads the exact-T machine; reads sync to their T.
   Expect flag-ON RED mid-build (intermediate states are incoherent) — converge
   the whole frame, THEN measure. Gate flag-OFF byte-identical at EVERY commit
   (mooneye 91/91 OFF, gbtr golden byte-identical). Retire the read-frame
   approximations (§ above) as the coherent frame subsumes them.
2. **Re-run the census** (`gambatte_flagon_probe` + `gbmicro`/`wilbertpol` + DMG
   OCR + `classify_cgb_regr.py`): the 98 DMG blockers + the dispatch/timer/serial
   families must converge (fixed) with the retime. Target: census of SameBoy-pass
   blockers == 0 (the flip bar). mooneye flag-on 91/91 held THROUGHOUT.
3. **Execute `C3-FLIP-CHECKLIST.md`** top-to-bottom ONLY when the bar == 0:
   flip defaults (`interconnect.rs` `new_inner(...,false)`→`true`; `GameBoy::new`
   → the `new_with_reclock` path — carries the C0 +4 DIV); rebaseline the SameBoy-
   FAIL flip-on rows into `tests/gbtr/baselines/gambatte.txt` (+ mealybug/
   wilbertpol/gbmicro/age) with dated A/B-swap blocks + floor-class letters (the
   71-row manifest in the census doc §4 is pre-built, INERT until now); re-anchor
   the gbtr ratchet baselines + re-point all tier2 pins to the default path; split
   `ppu/mod.rs` (~1795) + `interconnect.rs` (~1104) under the 1000-line cap.
4. **C4** (after the flip commit): regen + REVIEW the 146 golden frames + the 12
   golden-review legs (2 DMG-uncertain mm<64 + 8 CGB colour-confound + 2 age);
   every-oracle-zero-drop (mooneye/blargg/mealybug/wilbertpol/age both models);
   delete the OFF scaffolding (`leading_edge_reads`, the flag machinery, PORT-PLAN
   S7). NEVER drop a test SameBoy passes.

═══ DO-NOT-RETRY (build-measured dead; re-attempting = budget arson) ═══
- Any DMG dispatch whole-dot slice / partial dispatch move (welded — #11ai/#11br/
  #11bs, three refutations). Only the coherent whole-frame retime lands them.
- The S6 timer/serial read-fold (welded — #11bu; even the minimal OR-fold drops a
  pass). The C0-DIV sweep {−4..12} has ZERO effect (#11ai). The FF0F-cc+4 read
  (`SLOPGB_C2IF`) refuted.
- The 22 gbmicro `if_b`/`nops` are SameBoy-emu FAILS → they REBASELINE at the flip
  (in the manifest), they are NOT convergence work (the "43-wall mirage", #11bs).
- The render pixel legs (DONE 100/100) — do not re-touch; production byte-identical.
- Flipping defaults before the census bar == 0 (ships 98 SameBoy-pass regressions,
  fails the C4 zero-drop gate).

═══ HARD CONSTRAINTS ═══
- Production byte-identical OFF at EVERY commit until the flip commit itself
  (gbtr golden fingerprint byte-identical; mooneye OFF 91/91).
- mooneye flag-on 91/91 held through the retime (the dispatch dot lands where the
  coherent frame puts it; the counter-pinned tests are the make-or-break gate).
- NEVER drop a SameBoy-pass; the flip rebaselines ONLY confirmed SameBoy-FAIL rows.
- SSH-sign every commit (`export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket`;
  `git commit -S`; committer `richard@richardmoch.xyz`; verify `%G?`=G). `git add`
  EXPLICIT paths (never `-A`).

═══ WORKSPACE + TOOLING ═══
- Branch from `main` (`c7b2bd9`, the unified trunk — has BOTH the emulator core
  and the BGB frontend). Fetch ROMs: `test-roms/download.sh` (mts + gbtr bundles).
- Pixel two-bin: `gambatte_pixel_probe` (`SLOPGB_ROWLIST`, `scratchpad/pixel100.txt`
  = the 100 legs; OFF 100/100, ON now 100/100). CGB two-bin: `gambatte_flagon_probe`
  (3422 rows, `scratchpad/cgb_rowlist.txt`; base fail set 291). SameBoy tester:
  `docs/sameboy-port/tools/build_sameboy_tracers.sh` → `--dmg`/`--cgb`, `SB_TRACE=1`
  (SBMODE/SBWSCX/SBPOP…); `fp = absolute_ticks − display_cycles` (NEVER `cfl*2+dc`).
  slopgb tracers: `SLOPGB_S5DBG=1` / `SLOPGB_ISRTRACE=1`. Separate `CARGO_TARGET_DIR`
  per parallel gate. `touch crates/slopgb-core/src/lib.rs` before measurement builds.
- READ FIRST: `docs/sameboy-port/HALFDOT-BUILD-PLAN.md` ·
  `docs/sameboy-port/C3-FLIP-CHECKLIST.md` ·
  `measurements/dispatch-retime-plan-2026-07-04.md` (§4 + §8) ·
  `measurements/c3-flip-census-2026-07-04.md` (the 71-row manifest) ·
  `measurements/s6-completion-weld-refuted-2026-07-04.md` ·
  `measurements/dmg-m3-render-reclock-2026-07-04.md` · `CLAUDE.md` State (#11bu).

═══ OPEN TASK ITEMS (carried from the session task list) ═══
- Collapse the `early_lead` tower + 7 `vis_mode_read` shadow laws — NOT an
  independent slice; folds into the coherent retime (Part C) as it subsumes them.
- C3 flip + rebaseline + C4 — the plan above, gated on the retime landing.

ONE LEVER: the coherent per-T coroutine retime. It is the whole remaining port.
Build it flag-gated (production OFF byte-identical), converge the 98 DMG blockers
+ hold mooneye 91/91, then execute the C3-FLIP-CHECKLIST + rebaseline + C4. The
render is done; the flip is the last mile; the retime is its only gate.
