# EAGER dispatch retime — REFUTED: the dispatch is already at its true cc+4 position; the "dispatch-coupled" blockers are a READ-FRAME miss, and the eager leading-edge peek REGRESSES `intr_0_timing` on BOTH models (2026-07-09, #11cl)

Task (the last unknown gating the C3 flip): **can the CPU dispatch be moved to
its true SameBoy position on the eager clock while `mooneye intr_2` stays green —
if the READ frame co-moves coherently and the compensation tower is deleted?**
Build-measure, not argument. A clean refutation is as valuable as a success.

## Answer — NO, and the premise is wrong: the dispatch does not need to move

On the eager clock the dispatch is **already at cc+4 = production = SameBoy's
dispatch counter position**, and production (cc+4) PASSES both `intr_2` and
`intr_0_timing`. There is nothing to move that helps:

- **Every dispatch-move variant recovers ZERO gambatte rows and fixes ZERO of the
  dispatch tripwires.** The eager-native CGB −2 retime is inert (EV CGB 365→368,
  net −3); the deferred-machine variant is catastrophic (365→611, `intr_2`-CGB
  B=42) purely from a machine-driver mismatch.
- **The `intr_2`-CGB break is NOT a coherence failure — it is the deferred
  `advance_machine_t` DOUBLE-advancing the eager PPU** (which is driven by
  `tick_machine`, not `advance_machine_t`): +2 spurious PPU dots per STAT
  dispatch. Remove that advance and `intr_2` is green again (91/91) — because the
  dispatch no longer actually moves.
- **The tier2 COHERENT retime (dispatch −2 + reads co-moved through the deferred
  machine + `read_carried`) DOES keep `intr_2` green AND fixes `intr_0_timing`**
  (mooneye 91/91 flag-on; `intr_0` PASS both models). So a coherent dispatch move
  is NOT the `intr_2` breaker — but it IS the deferred clock, which breaks the
  ~86 DMG gambatte rows the eager clock exists to recover, and it cannot be
  grafted onto the eager machine piecemeal (below).
- **The real lever is the READ frame.** The "dispatch-coupled" CGB blockers read
  FF0F/STAT and fail because the eager clock's whole-dot leading-edge FF41 peek +
  the un-ported FF0F/STAT-IF two-latch delivery mis-frame the ISR read — NOT
  because the dispatch dot is wrong. Confirmed by build-measure (below).

## §1 — Compensation inventory (the eager `dispatch cc+4 / read cc+0` split)

Grepped at `8d244ef`. The split's compensations are ALL on the **read** side; the
dispatch is untouched (`dispatch_reclock()` → `tier2_reclock`, false under eager):

| compensation | site | what it papers over |
|---|---|---|
| FF41 cc+0 peek (only FF41 routed; FF0F/OAM/VRAM trail at cc+4) | `interconnect/cycle.rs:16` `leading_edge_sample` | the eager read samples before `tick_machine` |
| `mode3_entry_dot()` = **80** (leading_edge, !tier2, !ds) vs 84 | `ppu/stat_irq.rs:93` | frame-80 mode-2→3 entry back-date |
| eager read-debt **+8hd SS / +4hd DS** (`read_pos_hd`) | `ppu/engine.rs:288` | advance the cc+0 read to the deferred cc+4-equiv frame |
| ISR read carry (armed at STAT ack under eager) | `ppu/engine.rs:328` `isr_read_carry_hd`; `interconnect/speed.rs:201` | the OAM/HBlank-ISR first FF41 read offset |
| the whole `vis_mode_read` law web enabled under eager | `ppu/stat_irq/read_laws.rs:70` (`tier2_reclock ‖ eager_value`) | window length/shadow/abort/reenable/un-trigger exits |
| line-start mode-2 back-date (#11cb), line-boundary back-dates (#11cg/#11ci), write-commit read-debt (#11ck `stage_write_dots`) | `read_laws.rs`/`lyc.rs`/`interconnect/cycle.rs:stage_write_dots` | the cc+0 read's `[0,4)` mode-0 window + mid-m3 write commit |
| eager_value gates | `read_laws.rs`×9, `regs.rs`×12, `lyc.rs`×3, `mode0.rs`×2 | the ported CGB read/render laws |

**Dispatch is NOT in this list.** `dispatch_reclock()` returns `self.tier2_reclock`
(`interconnect.rs`), false under eager → no −2 move, no post-push ack, no
`dispatch_retime`. `read_carried` is armed on the eager path at the STAT ack
(`speed.rs:201`), independent of the dispatch move.

## §2 — What was built (3 env-gated probes; production/tier2/eager-baseline byte-identical)

All triple-gated (`eager_value` + env var), default off. Diff: `interconnect.rs`
(3 fields + gates), `interconnect/cycle.rs` (FF0F route + env reads),
`interconnect/speed.rs` (eager-native retime guard), `examples/run_mooneye.rs`
(`SLOPGB_EAGER` + `SLOPGB_WILBERT` measurement support).

- **`SLOPGB_COHERENT_DISP`** — `dispatch_reclock()` true under `eager_value &&
  is_cgb()` (CGB-scoped so DMG dispatch stays cc+4). `dispatch_retime_impl` SKIPS
  `advance_machine_t` under eager (the eager PPU is already advanced by
  `tick_machine`); only the post-push ack reorder + `read_carried` arm remain.
- **`SLOPGB_DISP_ADVANCE`** — re-adds `advance_machine_t` to the above (the
  literal tier2 mechanism; the corrupting variant, for the atomicity proof).
- **`SLOPGB_FF0F_LE`** — routes FF0F through the cc+0 leading edge
  (`0xE0 | intf` before `tick_machine`), dispatch untouched — the read-frame probe.

## §3 — Measurements (`CARGO_TARGET_DIR=target/agD2`, 3422-row CGB two-bin)

| config | EV CGB two-bin | `intr_2` (mooneye eager) | `intr_0_timing` [Cgb] / [Dmg] |
|---|---:|---|---|
| **eager baseline** | **365** | 91/91 GREEN | FAIL / FAIL (B=48) |
| eager + `COHERENT_DISP` (native, no advance) | 368 (−3) | 91/91 GREEN | FAIL (B=48) |
| eager + `COHERENT_DISP` + `DISP_ADVANCE` (corrupting) | **611** (+246) | **B=42** (Cgb+Agb) | FAIL (B=48) |
| eager + `FF0F_LE` (read-frame probe) | 434 (+69 net) | — | FAIL (B=48) |
| OFF (production, dispatch cc+4) | 486 | 91/91 | **PASS / PASS** (B=03) |
| LE (FF41 cc+0 peek only, cc+4 dispatch, no laws) | — | — | **FAIL** (B=48) |
| tier2 (deferred, coherent retime) | **291** | 91/91 flag-on | **PASS / PASS** (B=03) |

`intr_0_timing` = wilbertpol `acceptance/gpu/intr_0_timing.gb` (0xED exit + fib;
`run_mooneye SLOPGB_WILBERT=1 SLOPGB_EAGER=1 <rom> cgb`). B=03 ⇒ fib PASS; B=48 ⇒
the mode-0 IRQ mis-timed.

### The dispatch-move verdict (build-measured, not argued)

- **`intr_2` is GREEN whenever the PPU is not corrupted.** The eager-native move
  (no `advance_machine_t`) keeps `intr_2` 91/91. The ONLY break (B=42, Cgb+Agb) is
  `DISP_ADVANCE` = the deferred `advance_machine_t` double-advancing the eager PPU
  (+246 gambatte corruption too). That is a machine-driver mismatch
  (`tick_machine` vs `advance_machine_t`), not a coherent dispatch move.
- **Recovery = ZERO.** No dispatch variant recovers a single gambatte row or fixes
  either tripwire. The 53 must-fix CGB flip-bugs ALL pass under tier2; on eager the
  dispatch move recovers none of them.

### The read-frame is the lever (the redirect, confirmed)

- **`FF0F_LE` fixes 2 of 5 dispatch-cluster sample rows DIRECTLY, dispatch
  UNMOVED:** `enable_display/ly0_m0irq_scx0_1` (E2→E0) and
  `enable_display/frame0_m0irq_count_scx2_1` (00→90). Net −69 alone because a raw
  cc+0 FF0F over-clears for polls — it needs the two-latch DELIVER/SERVICE model
  (`ff0f.rs`, currently DMG-scoped), not a bare peek. `lycEnable/ff41_disable_2`
  reads FF41 (mode), untouched by FF0F-LE — a distinct read-frame leg.
- **`intr_0_timing` is a READ-frame regression, not a dispatch one.** LE (only FF41
  → cc+0, no dispatch move, no laws) ALREADY fails B=48 — the leading-edge FF41
  peek reads STAT one phase early, breaking `intr_0`'s mode-0 poll loop. No
  dispatch variant fixes it; tier2's deferred read frame (`advance_machine_t` +
  `read_carried`) does.

## §4 — The NEW finding (coordinator's tripwire): the eager peek regresses `intr_0` on BOTH models

`intr_0_timing` (wilbertpol): **OFF PASS, EAGER FAIL, tier2 PASS — on Dmg AND
Cgb.** This is caught by NEITHER gate the port relies on:
- mooneye (mts) has no `intr_0_timing`; its `intr_2`/`di_timing`/`int_hblank` stay
  green under eager.
- the gambatte OCR two-bins (EV DMG 102 / EV CGB 365) don't include it (wilbertpol
  uses 0xED-exit + fib, not an OCR row).

So the eager clock's "**DMG +0 / count-safe**" claim (CLAUDE.md #11bv) is measured
only on the gambatte DMG OCR set; `intr_0_timing` DMG is a real eager regression
**outside** that set. The eager leading-edge FF41 peek is DMG-unsafe for
`intr_0`. **Add `wilbertpol intr_0_timing` (both models) to the eager gate** — and
audit the wider wilbertpol/gbmicro suites for other peek-frame regressions mooneye
misses.

## §5 — Why the tier2 fix cannot be grafted onto the eager clock (the atomic weld)

tier2 passes `intr_0` + `intr_2` because it runs the WHOLE deferred machine
coherently (dispatch −2, all reads via `advance_machine_t` at the read's true
half-dot, `read_carried` at the retimed dispatch). Grafting any piece onto eager
fails:

1. **The retime's `advance_machine_t` double-advances the eager PPU** (this
   session: 365→611, `intr_2`-CGB B=42). The eager PPU is driven by `tick_machine`
   inline; `advance_machine_t` folds PPU dots a SECOND time.
2. **Routing eager reads through the deferred machine breaks the +16 ISR reads the
   eager leading-edge peek gets RIGHT** (#11cj: `m0int_m0stat`/`m2int_m2stat`/
   `scx_during_m3`, 18/18→2/18). The eager READ frame and the deferred READ frame
   want opposite clocks for different ISR reads — the exact conflict #11cj
   measured.

So `intr_0` needs the read at its true half-dot (the deferred `GB_display_sync`
analogue) WITHOUT the wholesale deferred routing — HALFDOT **Part B on the eager
`Bus::read`**, un-hosted. That is a read-frame lever, not a dispatch-position one.

## §6 — Recomputed flip bar (unchanged; probes ship nothing)

- EV CGB **365** / EV DMG **102** / tier2 **291** — byte-identical, env unset.
- **TRUE flip bar: 53 CGB (`OFF-pass ∩ EV-fail ∩ SameBoy-pass`, classified BUG) +
  42 FLOOR of 95 flip-bugs; DMG 55.** Unchanged — nothing shipped.
- **PLUS a newly-surfaced non-gambatte blocker: wilbertpol `intr_0_timing` on both
  models** (read-frame class). Was invisible to every gate; now a named tripwire.

## §7 — The next decomposition (if the read frame is chased)

1. **The coherent eager read frame (HALFDOT Part B on `Bus::read`).** Resolve the
   eager FF41/FF0F read to its true half-dot (the `advance_machine_t`/
   `GB_display_sync` analogue) for the ISR reads WITHOUT routing wholesale through
   the deferred machine (which breaks the +16, #11cj). `intr_0_timing` is the
   sharpest tripwire for the FF41 half of this.
2. **The FF0F/STAT-IF two-latch DELIVER/SERVICE-CLEAR model ported to CGB-eager**
   (`ff0f.rs`, currently `!is_cgb`). `FF0F_LE` proved direction (2/5 sample rows);
   the full two-latch web recovers the E0/E2 dispatch-cluster class net-positive.
3. The dispatch dot **stays at cc+4** (its true position). Do NOT re-attempt a
   dispatch-frame move on eager — this session is the third independent
   refutation (after #11br fold, #11bs eager-split): the move is inert without the
   deferred machine and corrupting with it.

## Reproduction

```
git checkout halfdot-dispatch     # this session's tip
CARGO_TARGET_DIR=target/agD2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agD2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# two-bins (env unset = byte-identical baseline):
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 365
SLOPGB_COHERENT_DISP=1 [... same ...]   # 368  (eager-native dispatch: inert)
SLOPGB_COHERENT_DISP=1 SLOPGB_DISP_ADVANCE=1 [...]   # 611 (corrupting)
SLOPGB_FF0F_LE=1 [...]   # 434 (read-frame probe)
# intr_0_timing tripwire:
cargo build -p slopgb-core --example run_mooneye --release
RM=target/agD2/release/examples/run_mooneye
ROM=test-roms/game-boy-test-roms-v7.0/mooneye-test-suite-wilbertpol/acceptance/gpu/intr_0_timing.gb
SLOPGB_WILBERT=1 SLOPGB_EAGER=1 $RM $ROM cgb    # FAIL B=48
SLOPGB_WILBERT=1 SLOPGB_TIER2=1 $RM $ROM cgb    # PASS B=03
SLOPGB_WILBERT=1 SLOPGB_LE=1 $RM $ROM cgb       # FAIL B=48 (read-frame, not dispatch)
```

## Gate state (all HARD invariants green; probes env-gated off)

golden_fingerprint PASS (9020 cases); tier2 CGB two-bin **291**; EV CGB **365** /
EV DMG **102** (byte-identical, env unset); mooneye `acceptance_ppu` 91/91 flag-off
AND `SLOPGB_MOONEYE_EAGER`; clippy `-D warnings` clean; all `.rs` < 1000
(`interconnect.rs` 999).
