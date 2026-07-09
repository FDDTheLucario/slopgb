# EAGER HALFDOT Part A/B — the deferred-machine routing REFUTED as the eager lever; the read↔render clocks are SEPARABLE and CONFLICTING (2026-07-09, #11cj)

Task (the FINISH KEYSTONE): host the tier2 half-dot resolution on the EAGER path —
Part B (half-dot read on the eager `Bus::read`) + Part A-render (half-dot
render length) — so the EV two-bin drops below 400 (CGB) / 102 (DMG). The
`#11bw` doc's proposed lever: "resolve the PPU to the read's exact half-dot
before the cc+0 FF41 peek — the eager analogue of `read_deferred`'s
`advance_machine_t` half-dot drain."

**Result:**
1. **The literal reading of the #11bw lever — route the eager bus ops through
   the deferred half-dot machine (`read_deferred`/`write_deferred`/
   `advance_machine_t`) — is REFUTED.** It BREAKS the ISR-path reads
   (`m0int_m0stat`/`m2int_m2stat`/`scx_during_m3`) that the whole-dot eager clock
   gets RIGHT: 0→16 currently-PASSING rows broken on a 30-row ISR subset (tier2
   passes 17/18, EV-whole-dot passes 18/18, EV-deferred-routing passes 2/18).
   The eager READ frame and the eager RENDER frame want OPPOSITE clocks — the ISR
   reads need the whole-dot `leading_edge_sample` (cc+0 peek before `tick_machine`,
   which lands them correctly at cc+4 dispatch), while the mid-mode-3 write commit
   needs the half-dot deferred advance. They cannot both run the same machine.
2. **The real Part A is per-half-dot render/strobe advance _inside_ the eager
   `tick_machine`, keeping the whole-dot `leading_edge_sample` for reads.** The
   byte-identical Part A-INFRA for that shipped this session (commit on
   `halfdot-eager-A`): the eager `tick_machine` now advances the PPU per 8-MHz
   half-dot (`Ppu::tick_half`, the same grain `advance_machine_t` uses), gated on
   `eager_value`. EV two-bin UNCHANGED (CGB 400 / DMG 102 — byte-identical on the
   aligned grid, `dhalf==0`), golden byte-identical, tier2 291, mooneye EAGER
   91/91. This is the load-bearing seam the half-dot write-strobe (Part A-render,
   next session) sits on — no number movement yet (the strobe is still whole-dot).**

## Operational correction (READ FIRST if resuming)

The worktree handed to this session was checked out off **`main` (`ef5ece7`)**,
NOT off `finish-port-halfdot` (`2f1fffa`) as the task stated. `main` lacks the
whole `eager_value` infrastructure (no `SLOPGB_PROBE_EV` branch in
`gambatte_flagon_probe.rs`, no eager read arms), so `SLOPGB_PROBE_EV=1` silently
fell through to `boot_with_reclock` and every "EV" number equalled tier2
(291/116). Symptom: EV two-bin == tier2 two-bin exactly, even on timing ROMs.
Fix applied: `git reset --hard 2f1fffa` (finish-port-halfdot tip) on
`halfdot-eager-A`. After that the baselines matched the docs (below). Also:
the worktree has no `test-roms/` — symlink it:
`ln -s /home/soulcatcher/personal_repos/slopgb/test-roms/game-boy-test-roms-v7.0
test-roms/game-boy-test-roms-v7.0`.

## Baselines confirmed (branch `halfdot-eager-A` @ `2f1fffa`, `CARGO_TARGET_DIR=target/agH`)

| bin | rowlist | fail | note |
|---|---|---:|---|
| tier2 CGB (default probe) | cgb | **291** | the target; held |
| EV CGB (`SLOPGB_PROBE_EV=1`) | cgb | **400** | matches #11cg/#11ci |
| EV DMG | dmg | **102** | matches #11ci |
| DMG OFF (production) | dmg | 103 | floor reference |

## What IS separable (confirmed, and it helps)

- **The −2 dispatch move is already OFF under eager.** `Bus::dispatch_reclock()`
  returns `self.tier2_reclock` (`interconnect.rs:924`), which is FALSE under
  `eager_value`, so `dispatch_retime_impl` is never called — dispatch stays cc+4
  and no `read_carried` ISR peek arms. This is the count-safe property the whole
  eager approach exists for; it needs no work.

## The refuted lever — route the eager bus through the deferred half-dot machine

Change (reverted): all five `Bus` gates
`if self.tier2_reclock` → `if self.tier2_reclock || self.eager_value`
(`read`/`write`/`tick`/`tick_addr`/`read_inc`, `interconnect.rs`), so eager ops
run `read_deferred`/`write_deferred`/`tick_deferred` (the `advance_machine_t`
half-dot machine). Golden + tier2 stay byte-identical (all gates `|| eager_value`,
false in production/tier2).

Two sub-configs measured, both catastrophic on the ISR subset:

| config | PPU frame | tiny-30 ISR subset (18 eval) | vs 400-baseline |
|---|---|---:|---|
| EV whole-dot (400 baseline) | `ppu.eager_value` (frame 80 + arms + `read_pos_hd` +8 debt) | **18 pass / 0 fail** | — |
| tier2 (reference) | `ppu.tier2_reclock` + −2 dispatch | 17 pass / 1 fail | — |
| (a) deferred routing + `ppu.eager_value` (frame 80) | eager arms, deferred machine | **2 pass / 16 fail** | +16 broken |
| (b) deferred routing + `ppu.tier2_reclock` (frame 84) | full tier2 laws, cc+4 dispatch | 2 pass / 16 fail | +16 broken |
| (c) (a) + render-survive gates `|| eager_value` | +staged_pending/scx_write_dot/render_lcdc/wx_write_dot | 2 pass / 16 fail | +16 broken |

The 16 broken rows (ALL pass in the 400 baseline) are ISR-path FF41 reads:
`m0int_m0stat_{ds,scx2,scx3,scx5_ds}` (want 0/2, got 1/2), `m2int_m2stat_{,ds,
scx4_ds}` (want 2/3, got 1/blank), `scx_m3_extend_{,ds}` (want 0/3, got 2).

**Why:** the whole-dot eager `leading_edge_sample` peeks FF41 at cc+0 *before*
`tick_machine` (the previous M-cycle's end), which lands the STAT-ISR handler's
first FF41 read at exactly the position SameBoy reads it at cc+4 dispatch. The
deferred `read_deferred` instead pays the previous parked debt through
`advance_machine_t` and samples *there* — a different dot for a post-dispatch
handler read — and it has NO `read_carried` peek to compensate (that arms only
under `dispatch_reclock`, which is off). So the deferred machine mis-frames every
ISR read the whole-dot leading-edge peek nails. tier2 survives ONLY because its
−2 dispatch + `read_carried` peeks re-align them; eager cannot take the −2 move
(DMG counts). The literal "advance_machine_t on the eager read" therefore trades
the entire ISR-read class to (maybe) buy the render class — a massive net loss.

## The render-dot-recorder gate-flip — floor confirmed (consistent with #11cg)

Under eager the mid-mode-3 write-commit recorders are `ppu.tier2_reclock`-only, so
they never fire on the whole-dot eager clock:
`scx_write_dot` (`regs.rs:125`), `staged_pending` (`regs.rs:249`, the write-strobe
SURVIVE), `render_lcdc` split (`regs.rs:64`), `window_abort_render` split
(`regs.rs:94`), `wx_write_dot` (`regs.rs:660`). Flipping them to `|| eager_value`
records the eager clock's commit dot = the M-cycle END (`self.dot` at the
redundant `Ppu::write` `commit_eff`, D+4) whereas tier2 records the deferred
strobe dot (D+3) — off by ≥1 dot, so the render-length arms fire at the wrong
exit. This matches #11cg's already-shipped evidence: `win_reenable_dot`
(`regs.rs:113`) is ALREADY `|| eager_value` and is a KNOWN FLOOR (records 94 eager
vs 96 tier2 → `late_reenable_2` over-extends). Every render-dot recorder is the
same floor: the eager whole-dot clock commits at M-cycle boundaries, not the
deferred half-dot the render fetch-grid is calibrated to.

## The synthesis — the read and render clocks CONFLICT

| class | needs | why |
|---|---|---|
| ISR-path FF41 reads (`m0int`/`m2int`/`lyc*`/`enable`) | the WHOLE-DOT eager `leading_edge_sample` (cc+0 peek before `tick_machine`) | lands the handler read at cc+4 dispatch; the deferred machine mis-frames it (no `read_carried` at cc+4) |
| mid-mode-3 render length (`window`/`scx`/`cgbpal`/`m2int_m3stat`) | the HALF-DOT deferred write-commit (strobe survives, drains at the true half-dot) | the eager `tick_machine` drains the strobe within the M-cycle → commit 2–4 dots early → hunt closes → under-extends |

These pull opposite directions on the SAME `tick_machine`. Routing everything
through the deferred machine wins the render and loses the reads (measured +16).
Keeping the whole-dot machine wins the reads and loses the render (the 400
baseline). **This is the exact atomic coupling — but the seam is NOT the read
path (that must stay whole-dot leading-edge); it is the PPU/strobe advance
_inside_ `tick_machine`.**

## The exact next sub-part (precise, single)

**Part A-render — make the write STROBE half-dot precise (the infra it needs
SHIPPED this session).** The eager `tick_machine` already advances the PPU per
half-dot (`tick.rs`, `eager_value`-gated); the remaining work is the strobe:

1. ✅ DONE: `tick_machine`'s PPU advance runs `Ppu::tick_half` under `eager_value`
   (byte-identical, EV 400/102). The seam.
2. Move the write STROBE onto the half-dot grid: `strobe_tick` (`regs.rs:145`) is
   called from `Ppu::tick` (the whole-dot body, run on the dot-completing half),
   so today it still counts WHOLE dots. Call it from the half-dot path (or make
   `dots_left` count half-dots) so a mid-mode-3 `commit_eff` lands at the true
   half-dot. `stage_write_dots` (`cycle.rs:344`) must be re-derived to half-dots
   (×2 + the SameBoy per-register offset). THEN flip the render-dot recorders
   (`scx_write_dot` `regs.rs:125`, `staged_pending` `regs.rs:249`, `render_lcdc`
   split `regs.rs:64`, `window_abort_render` `regs.rs:94`, `wx_write_dot`
   `regs.rs:660`) to `|| eager_value` — now they record the half-dot commit,
   matching tier2's frame (today the whole-dot eager commit is the M-cycle END,
   D+4 vs tier2's strobe D+3 — the measured floor, see below). This is the
   convergence step: with the strobe at the true half-dot the `late_scx4`/window/
   scx render pairs separate on the eager clock WITHOUT touching the read path.
3. Leave `leading_edge_sample` (the FF41 read peek) exactly as-is — the ISR
   reads that broke above MUST keep the cc+0 whole-dot peek. Do NOT route reads
   through `read_deferred`.
4. Do NOT flip `early_lead`/`snap_ok` (`mode0.rs:184/216`) — they move the
   sprite-line dispatch and break `intr_2_*_sprites` (#11by).

Expected: render class recovers (window/scx/cgbpal ≈ 20–25 CGB blockers, ≈ 29
DMG), the ISR-read class stays green (untouched). The dispatch-loop blockers
(enable_display/m2int_m0irq/lycEnable/irq_precedence, ≈ 24 CGB) remain the
counter-pinned C3-flip floor — unrecoverable without moving dispatch.

## Gate state (Part A-infra SHIPPED; EV byte-identical)

- SHIPPED: the eager `tick_machine` half-dot PPU advance (`tick.rs`,
  `eager_value`-gated). The deferred-routing + render-dot-recorder experiments
  were REVERTED (net-negative, above).
- EV CGB two-bin **400** / EV DMG **102** (byte-identical — the half-dot infra
  changes nothing on the aligned grid). tier2 CGB **291**. golden_fingerprint
  PASS (production `eager_value`-false → the whole-dot `else if` branch is the
  original logic verbatim). mooneye EAGER `acceptance_ppu` 91/91 (intr_2 safe).
  clippy clean. All `.rs` < 1000 lines.

## Reproduction

```
git reset --hard 2f1fffa        # finish-port-halfdot tip (NOT main/ef5ece7)
ln -s .../test-roms/game-boy-test-roms-v7.0 test-roms/game-boy-test-roms-v7.0
CARGO_TARGET_DIR=target/agH cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agH/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# baselines:
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 400
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt                    $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 291 (tier2)
# the ISR subset that refutes deferred routing:
grep -E 'm0int_m0stat|m2int_m2stat|scx_during_m3' scratchpad/cgb_rowlist.txt | head -30 > scratchpad/tiny.txt
# (whole-dot EV → 18/0; deferred routing → 2/16)
```
