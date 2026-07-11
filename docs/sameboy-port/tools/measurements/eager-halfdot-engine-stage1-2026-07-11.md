# HALFDOT STAT-engine Stage 1 — the half-dot STAT-rise infrastructure SCOPED + the cleanest #11dr sibling pair TRACED to its true event dots: the minimal half-dot lever (§5.1 exit-record + STAT-rise grain, and the #11dr write-commit) is REFUTED by direct measurement — both siblings' disable-write→STAT-rise gap is IDENTICAL (2 whole-dots), so no uniform half-dot commit/edge shift separates them; the separator is the sub-dot COINCIDENCE of the disabled-source DROP with the joining-source RISE in the LEVEL engine, which needs the LYC-interrupt-line edge (line 153) AND the mode-3→0 flip (line 1) resolved on the 8-MHz grid — the full mode/LYC half-dot FSM, unbuildable byte-identical + steady-state in a bounded Stage-1 edit → STOP-and-map per the task guard (2026-07-11, #11du)

Base: `finish-port-halfdot @ c2d87d5`. Task (#11du, Stage 1 of a multi-session
piece): build the half-dot STAT-engine INFRASTRUCTURE + PROVE it separates ONE
#11dr sibling pair, or REFUTE. **Outcome: (c)/(b) hybrid — the minimal half-dot
lever is REFUTED for the target pair by direct event-dot measurement; the pair
needs the FULL mode/LYC half-dot FSM (Stage 2+), which cannot be built
byte-identical-off AND steady-state-preserving-on in a bounded edit → STOP and
map (the task's own guard).** No code shipped; tree byte-identical
(`git diff c2d87d5 -- crates/` empty, `golden_fingerprint` ok 40.13s).

## Baselines reproduced (exact, at c2d87d5)

`flagon_probe` two-bin, `scratchpad/{cgb,dmg}_rowlist.txt`:

| frame | CGB | DMG |
|---|---:|---:|
| EV (`SLOPGB_PROBE_EV`) | **295** | **54** |

`golden_fingerprint` byte-identical. The target pair (`dmg_rowlist.txt` lines
1594 / 2044) both present. (Note: the task prompt's EV CGB 486/295/291 and DMG
54/116 are the OFF/EV/tier2 triple from #11dr @ 951d0b7; at c2d87d5 EV CGB is
**295**, EV DMG **54** — unchanged, the port state is stable across the two
commits.)

## The Stage-1 core edit, scoped (§5.1 of #11dh) — and why it is INSUFFICIENT here

`ppu/engine.rs::tick_half` (:251): the odd half (`dhalf 0→1`) runs only
`strobe_tick()` under eager; the even half (`dhalf 1→0`) runs the whole-dot
`tick()` body (mode/LY/LYC/STAT-rise via `stat_update_tick`, `m0_flip_events`).
`#11dm` proved `flip_hd = 2*dot + dhalf` degenerate (dhalf==0 in the tick body);
`#11dr` proved the whole-dot write-commit half-dot borrow INERT because "`dhalf`
is a RENDER grain, never a STAT grain."

The §5.1 edit ("record `flip_hd` on the `dhalf` where `proj <= lead` first holds;
move the exit-record + STAT-rise grain to half-dot") targets the **mode-3→0 flip**
(the HBLANK-source rise). **But the cleanest #11dr pair is decided by a
line-153 LYC edge and a line-1 mode-0 edge — and the target's decisive event is
NOT the mode-3 exit at all**; it is the line-153 LYC-interrupt-line re-edge (a
`ly_for_comparison` event, `reclock.rs` — nothing to do with `m0_flip_events`).
The §5.1 exit-record grain alone cannot move it. This is the first Stage-1
finding: the pair needs the LYC-line edge on the 8-MHz grid, a *different*
half-dot event than the render exit §5.1 scopes.

## The sibling-pair trace (measured, EV clock, DMG, `run_gambatte --features port_probe`)

Temporary `ff41commit` probe in `regs.rs::commit_eff` (ly/dot/dhalf/old→new/mfi)
+ the existing `dispatch` probe (the STAT 0→1 edge in `stat_update_tick`), both
`SLOPGB_S5DBG`; reverted after (tree byte-identical).

### Target `lycEnable/lyc153_late_m1disable_3` (want **E0**; EV got **E2** ✗)

```
ff41commit ly=152 dot=56 old=40 new=50   (enable VBLANK: 40=LYC → 50=VBLANK|LYC)
ff41commit ly=153 dot=4  old=50 new=40   (the m1-disable: 50 → 40=LYC-only)
dispatch   ly=153 dot=6  mfi=1 lycln=1    (STAT 0→1 edge — SPURIOUS, LYC source)
```

The VBLANK-disable commits at **ly=153 dot=4**; the STAT line DIPS (mode-1 source
gone) then RE-EDGES at **ly=153 dot=6** as a fresh LYC-source 0→1. **Δ = 2 dots.**
SameBoy holds the line continuously HIGH across dots 4→6 (VBLANK falls, LYC rises
co-instant → no edge → E0). slopgb's whole-dot engine separates the drop (dot 4)
from the rise (dot 6) → spurious edge → E2.

### Sibling `m0enable/lycdisable_ff41_2` (want **2**; EV got **2** ✓)

```
ff41commit ly=1 dot=252 old=48 new=08    (the lyc-disable: 48=OAM|LYC → 08=HBLANK)
dispatch   ly=1 dot=254 mfi=0 lycln=1     (STAT 0→1 edge — CORRECT, HBLANK source)
```

The LYC-disable commits at **ly=1 dot=252**; it drops the LYC source (line dips),
then the mode-3→0 flip's HBLANK source rises at **ly=1 dot=254** as a fresh 0→1.
**Δ = 2 dots.** Here the drop-then-rise separation is CORRECT (SameBoy: LYC falls,
HBLANK rises one tick later → edge → 2).

### The decisive number: IDENTICAL 2-dot gap

Both siblings: the disable commits exactly **2 whole-dots before** an
already-separated STAT rise. A half-dot write-commit lever shifts the write by
`δ` uniformly → both gaps become `4hd − δ` in lockstep. To fix the target (make
the drop coincide with the rise, gap→0) needs `δ = 4hd` (commit at dot 6) — which
ALSO collapses the sibling's gap to 0 (commit at dot 254), where the LYC-drop
coincides with the HBLANK-rise → line stays high → NO edge → **breaks the sibling
to 0**. **A uniform half-dot write-commit shift is a strict pair-shuffle** — the
#11dq/#11dr floor, now with the co-temporal gap directly measured identical
rather than inferred.

## Why the half-dot STAT engine CAN separate them (not a hard refutation) — and what it must do

The pair is separable, but ONLY by resolving the two *rises* to their true 8-MHz
half-dots, because they are DIFFERENT-TYPE edges whose SameBoy sub-dot phases
differ:

- **Target rise** = a LYC-source re-edge on line 153, at the SameBoy 8-MHz tick
  where `ly_for_comparison` re-latches LYC=153 — which co-incides with the
  VBLANK-disable commit tick → net level unchanged → no edge.
- **Sibling rise** = the mode-3→0 flip's HBLANK-source edge on line 1, at the
  8-MHz tick the fetcher/FIFO drains → one tick AFTER the LYC-disable commit tick
  → fresh edge.

slopgb's whole-dot engine puts BOTH the disabled-source drop and the joining-
source rise on adjacent whole-dots (Δ2) with IDENTICAL structure, erasing the
sub-dot phase difference. The half-dot STAT engine separates them iff it evaluates
the STAT **level** (`StatUpdate::level`, the OR of `mode_for_interrupt`'s source
and the LYC source) at half-dot resolution AND advances the two rise sources —
`lyc_interrupt_line` (line 153) and the mode-3→0 flip `m0_src` (line 1) — on the
8-MHz grid so a coincident drop+rise nets to no edge on the target while the
one-tick-late HBLANK rise stays an edge on the sibling.

**This is the full mode/LYC half-dot FSM, not the §5.1 exit-record edit.** It
requires: (1) `stat_update_tick`'s level eval on the half-dot grid; (2) the LYC
compare / `ly_for_comparison` re-latch at its 8-MHz tick (line 153 quirk); (3)
the mode-3→0 flip `m0_flip_events` recording a genuine odd `flip_hd` (§5.1); (4)
the FF41 write commit at its half-dot. All four coupled — none alone separates
the pair.

## The golden-safety analysis (the highest-risk build of the port)

- **The idempotent edge-eval seam IS byte-identical-safe.** `StatUpdate::update`
  is edge-triggered and idempotent: re-running it on the odd half with UNCHANGED
  `(mfi, eng_stat, lyc_line)` sets `line = level(...)` (unchanged) and returns
  `line && !previous = false` — no new edge, no state mutation. Gated on
  `eager_value`, `eager_value=false` never calls it → byte-identical OFF; on the
  aligned grid (no odd-half write) the inputs are unchanged → EV steady-state
  preserved. **This seam can be shipped byte-identical whenever Stage 2 needs it.**
  It was NOT shipped this session: with the DMG FF41 write still committing at
  `dhalf=0` (no borrow) the odd-half re-eval sees no changed input → fully inert
  (moves 0 EV rows) → shipping it now is dead scaffolding (ponytail: map, don't
  ship). It goes in WITH the mode/LYC half-dot advance that makes it live.

- **The mode/LYC FSM half-dot advance is NOT bounded-safe.** Moving mode
  transitions / the LYC re-latch / the flip onto the 8-MHz grid perturbs the
  inputs of every one of the 295 EV-passing CGB + 54 EV DMG rows that read
  `stat_update_tick` / `vis_mode_read`. Keeping it steady-state-preserving is
  itself the multi-session convergence (the same "one coherent per-T retime"
  #11bw/#11br/#11dq all reached). Per the task guard — "If you cannot keep it
  byte-identical off AND steady-state-preserving on, STOP and map" — this is the
  STOP.

## Staged plan for the residual 34–36 (the three walls, #11dh §4 / #11dq / #11dt)

The C3-flip bar residual is ~34–36 rows in three walls. The half-dot STAT engine
(this piece) is the lever for wall 1; walls 2 and 3 are separate half-dot FSMs.

| wall | rows | class | the half-dot piece it needs |
|---|---:|---|---|
| **1. dispatch / STAT-engine frame** | ~14 DMG-write + ~6 CGB line-153 (`ly_lyc_153_write` ×6, #11dt) + the #11dr/#11dq write-commit siblings | counter-pinned dispatch + STAT-level source-edge coincidence | **THIS piece — the mode/LYC half-dot FSM** (Stage 2: LYC-line edge + `flip_hd` + FF41 half-dot commit, coupled). The write-commit alone is REFUTED (this map). |
| **2. CGB mode-3-start / accessibility render frame** | ~5–6 (`vram_m3/postread_scx3_2`, `oam_access/postwrite_2_scx3`, DS sprite floor; #11dt A: `m3_bgp`/`m3_window_timing`/`m3_wx_4` render rows) | render half-dot (`vis_early` case-tower emergent-flip) | **Part-A render §5.1 exit-record** (the `early_lead` case-tower deletion) + the DS mid-dot floor (#11da). Distinct from wall 1. |
| **3. SCX length coupling** | 2 DMG (`m3_scx_high_5_bits`) | render-LENGTH A/B swap | REFUTED as a one-sided fix (#11cm); needs the coupled render∧read half-dot, NOT a discrete law. |

### Staging (each independently gateable, flag-off byte-identical)

1. **Stage 2 — the mode/LYC half-dot FSM (wall 1).** Build the four coupled
   pieces together (they do not separate — this map's core finding): (a)
   `stat_update_tick`'s `StatUpdate::level` eval on the half-dot grid via the
   idempotent odd-half seam (byte-identical, proven above); (b) the
   `lyc_interrupt_line` / `ly_for_comparison` re-latch at its 8-MHz tick (the
   line-153 LY-wrap quirk — `reclock.rs:169-296`); (c) `m0_flip_events` recording
   an odd `flip_hd` (§5.1); (d) the DMG FF41 write commit at `dhalf=1`. Success =
   the target pair separates (target E0, sibling stays 2) with EV steady-state
   preserved. This is the multi-session convergence; land it slice-by-slice like
   the #11by→#11cb read-frame ports (each slice: `flagon_probe` two-bin,
   golden byte-identical, EV monotone-down, mooneye ×3, intr_2 tripwires).
2. **Stage 3 — the Part-A render exit-record (wall 2).** §5.1 `flip_hd` odd-half
   + delete the `early_lead` case-tower (`mode0.rs:184-201`); recover the ~5 CGB
   accessibility/mode-3-start rows. Independent of Stage 2 (the render sources vs
   the LYC/mode sources).
3. **Wall 3** stays parked (documented A/B swap, #11cm) — clears only with the
   full coupled render∧read half-dot, not a discrete law.
4. **The flip (C3)** only when all three walls converge ∧ golden regen ∧ zero
   SameBoy-pass drop — `C3-FLIP-CHECKLIST.md`.

## What NOT to re-chase (adds to #11dq/#11dr/#11dt)

- **A uniform half-dot write-commit / odd-half edge-eval separating the #11dr
  pair** — REFUTED here: both siblings' disable→rise gap is IDENTICAL (2 dots);
  any uniform shift is a strict pair-shuffle. The separator is the two rises'
  distinct 8-MHz phases, needing the mode/LYC FSM on the half-dot grid.
- **The §5.1 exit-record edit ALONE clearing wall 1** — REFUTED: the target's
  decisive event is a line-153 LYC re-edge, not the mode-3 render exit; §5.1
  moves the flip (wall 2), not the LYC line.
- **Shipping the idempotent odd-half edge-eval seam standalone** — inert with the
  DMG FF41 write at `dhalf=0` (moves 0 EV rows); it lands WITH Stage 2's mode/LYC
  half-dot advance, never before.

## Gates (all hold; tree byte-identical, no code shipped)

1. `git diff c2d87d5 -- crates/` **empty** (the `ff41commit` trace reverted).
2. `golden_fingerprint` ok (40.13s, default flags-off build).
3. EV CGB 295 / EV DMG 54 reproduced exactly.
4. interconnect/engine reclock defaults NOT flipped; no push; parent branch
   untouched.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd1
# baselines
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt \
  cargo test -p slopgb-core --test gbtr --release -- --ignored --exact \
  gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['   # 54
# sibling trace (re-add the reverted regs.rs ff41commit probe, then):
cargo build -p slopgb-core --example run_gambatte --features port_probe --release
BIN=target/hd1/release/examples/run_gambatte
R=test-roms/game-boy-test-roms-v7.0/gambatte
SLOPGB_S5DBG=1 SLOPGB_EAGER=1 $BIN \
  $R/lycEnable/lyc153_late_m1disable_3_dmg08_cgb04c_outE0.gbc dmg 2>&1 \
  | grep -E 'ff41commit|dispatch' | grep 'ly=15[23]'
#   → disable commits ly153 dot4; spurious LYC re-edge ly153 dot6 (Δ2); OUT E2 (want E0)
SLOPGB_S5DBG=1 SLOPGB_EAGER=1 $BIN \
  $R/m0enable/lycdisable_ff41_2_dmg08_out2_cgb04c_out0.gbc dmg 2>&1 \
  | grep -E 'ff41commit|dispatch' | grep -E 'ly=1 '
#   → disable commits ly1 dot252; HBLANK edge ly1 dot254 (Δ2); OUT 2 (correct)
```
