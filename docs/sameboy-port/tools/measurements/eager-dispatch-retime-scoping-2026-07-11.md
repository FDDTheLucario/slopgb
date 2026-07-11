# EAGER dispatch/write retime SCOPING — the ~14 dispatch-frame bar rows are the PARTIAL-FLIP FLOOR: every scoped whole-dot retime (DS-domain, STAT-source, FF41/FF45 write-commit) is an inert no-op or a net-negative pair-shuffle that straddles sub-dot sibling boundaries; the only coherent retime is HALFDOT Part B (2026-07-11, #11dq)

Base: `finish-port-halfdot @ 4847b35`. Task (#11dq, plan-or-refute, ship NO
source): can a SCOPED coherent per-T dispatch retime move the CPU dispatch/write
clock cc+4→cc+0 for the ~14 dispatch-frame C3-flip bar rows (7 CGB DS
enable_display/lcd_offset/irq_precedence/m2int + 7 DMG lycEnable/m2enable) WITHOUT
re-breaking the DMG timers / `intr_2`? **Answer: NO — it is the FLOOR.** Every
whole-dot scoping measured is either inert (the eager clock has no deferred
machine to retime) or a net-negative A/B pair-shuffle whose recovered and dropped
rows are `_1`↔`_2` / register siblings straddling the SAME write/dispatch boundary
at sub-dot resolution the eager whole-M-cycle clock cannot represent. The lever is
not a dispatch SCOPING at all — it is the half-dot read/write frame (HALFDOT Part
B), the same redirect #11cl §7 / #11bw reached. All experiments env-gated +
REVERTED; tree byte-identical (`git diff 4847b35 -- crates/` empty,
`golden_fingerprint` ok 42.17s).

## Baselines reproduced (exact, at 4847b35)

`flagon_probe` two-bin, `scratchpad/{cgb,dmg}_rowlist.txt`:

| frame | CGB | DMG |
|---|---:|---:|
| OFF (`SLOPGB_PROBE_OFF`) | 486 | — |
| EV (`SLOPGB_PROBE_EV`) | **295** | **54** |
| tier2 (`SLOPGB_PROBE_RECLOCK`) | **291** | 116 |

golden_fingerprint byte-identical. The task's DMG timer wall (#11bv): the FULL
deferred DMG dispatch = tier2 DMG = **116 fails** vs eager 54 — the deferred clock
recovers the 7 engine rows but self-inflicts ~69 others (the tima/dispatch set).
That NO-GO is already on the table; this map asks whether a PARTIAL move escapes it.

## Method

Dual-trace 1 CGB DS + 1 DMG row under OFF/EV/tier2 (`run_gambatte --features
port_probe`, `SLOPGB_EAGER`/`SLOPGB_TIER2`, `SLOPGB_S5DBG=1`); the `m0rise`/
`dispatch`/`vec` traces already in-tree. Then A/B four candidate scopings behind
throwaway env flags (all reverted, Part-C):
- **`SLOPGB_COHERENT_DISP`** (pre-existing) — the `is_cgb`-scoped −2 dispatch
  reclock (incl. DS). `SLOPGB_DISP_ADVANCE` adds the deferred machine-advance.
- **`SLOPGB_DMG_STAT_DISP`** (throwaway, this session) — (b1) a DMG STAT-source
  dispatch reclock (`dispatch_reclock() |= !is_cgb && pending().trailing_zeros()==1`)
  AND (b2) the DMG FF41/FF45 WriteCpu write-commit borrow (the `borrow_addr` DMG
  branch extended to `0xFF0F|0xFF41|0xFF45`).

## §1 — The two traces: the divergence is NOT a CPU dispatch POSITION for either family

### CGB DS — `enable_display/ly0_m0irq_scx0_ds_1` (want E0; EV E2✗, tier2 E0✓, OFF E0✓)

The divergence is a **spurious PPU mode-0 STAT EMISSION at glitch-line (ly=0) dot 19**,
present ONLY under EV+DS — not a CPU dispatch dot. `m0rise`/`dispatch` on ly=0:

| config | first ly0 emission dot | note |
|---|---:|---|
| EV DS | **19** (+253) | `mnow=0` (deferred machine idle under eager) |
| tier2 DS | 253/255 | finer deferred advance never emits dot-19 |

The dot-19 STAT sets IF bit 1 early → the verdict FF0F read returns E2. This is the
eager DS glitch-line RENDER geometry (Part-A render / emission frame), reproducing
#11dk rows 1–4. Moving the CPU dispatch does not touch it (§2).

### DMG — `lycEnable/lyc153_late_m1disable_3` (want E0; EV E2✗, tier2 E0✓, OFF E0✓)

The STAT-IF emission is **IDENTICAL** under EV and tier2 — both `dispatch ly=153
dot=6 mfi=1 lycln=1`. The divergence is the **FF41 disable-WRITE commit position**:
under eager the write commits at the M-cycle boundary (cc+4), 4 dots late, so the
mode-1 STAT source is still armed when the LYC/mode-1 condition fires and sets IF
bit 1; tier2 commits it at cc+0, disarming in time. Confirms #11dp's "zero
read-value disagreement; the disable-write commits late" — a WRITE-commit clock, not
a read law, not a dispatch position.

## §2 — Candidate (a) CGB-DS-scoped dispatch retime — NET-NEGATIVE PAIR-SHUFFLE

`SLOPGB_COHERENT_DISP` (the `is_cgb`-scoped −2 dispatch reclock, includes DS):

| config | EV CGB | recovered | dropped |
|---|---:|---:|---:|
| EV base | 295 | — | — |
| + COHERENT_DISP | **304** (−9) | **6** | **15** (SameBoy-pass) |
| + COHERENT_DISP + DISP_ADVANCE | **304** | (same) | (same) |

- **Recovers ONLY 1 of the 7 CGB DS targets** (`irq_precedence/late_m0irq_retrigger_scx1_ds_2`
  — the #11dk-pre-refuted ack-squash row) at the cost of **15 SameBoy-pass drops**,
  incl. its own `_1`/`_ds_1` siblings (`late_m0irq_retrigger_1`/`_ds_1`) AND
  `tima/tc00_irq_late_retrigger_1`/`_ds_1` — the recovered `_2` and dropped `_1`
  straddle the moved dispatch boundary.
- The other 6 CGB DS targets are UNRECOVERED — incl. `ly0_m0irq_scx0_ds_1` (§1),
  which stays E2 (the dot-19 emission is PPU-side; the dispatch move never touches it).
- **`DISP_ADVANCE` is inert** (304, not #11cl's 611): at 4847b35 the deferred
  `clock` is FROZEN under eager (`mnow=0`, §1) so `dispatch_vector_retime()` +
  `advance_machine_t(before,now)` advance nothing. The eager reads peek the live PPU
  (driven by `tick_machine` inline), fully decoupled from the deferred clock the
  retime manipulates → the dispatch move re-frames nothing; the −9 is pure ack-reorder
  noise. **REFUTED**: DS-scoping a dispatch move is a pair-shuffle for 1 row and the
  wrong lever for the other 6 (render/emission frame).

## §3 — Candidate (b) DMG STAT/LYC source scoping

### (b1) dispatch-position source-scope — INERT (54→54)

`SLOPGB_DMG_STAT_DISP` on `dispatch_reclock`, gated `pending().trailing_zeros()==1`
(STAT lowest): **zero rows move, either direction.** Two independent reasons: (i) same
frozen-machine inertness as §2 (the reorder re-frames nothing on eager); (ii) the
relevant ly=153 dispatch is **VBlank-coincident** (bit 0 is the lowest pending bit),
so a STAT-source gate never even fires on it. The interrupt POSITION is not
source-separable — the CPU dispatches the lowest pending bit at one phase; VBlank and
STAT are co-instant on line 153. **REFUTED.**

### (b2) FF41/FF45 write-commit borrow — TIMER-SAFE but a PAIR-SHUFFLE that drops 8 SameBoy-pass

The DMG WriteCpu 1-dot borrow extended from FF0F to FF0F|FF41|FF45 (the write-commit
lever §1 named):

| config | EV DMG | recovered | dropped |
|---|---:|---:|---:|
| EV base | 54 | — | — |
| + FF41/FF45 borrow | **52** (net +2) | **10** | **8** (SameBoy-pass) |

- **Recovers 4 of the 7 DMG targets** (`lyc153_late_m1disable_3`,
  `lyc153_late_enable_m1disable_3`, `m2enable/late_enable_2`,
  `late_enable_after_lycint_disable_2`) **+ 6 bonus** (`m1/*`, `ly0/lycint152_*`,
  `irq_precedence/late_if_via_sp_if_1`).
- **Drops 8 SameBoy-pass siblings**: `m0enable/lycdisable_ff41_2`,
  `m0enable/lycdisable_ff45_3`, `m0enable/disable_scx3/7_2`,
  `{irq_precedence,lyc153int_m2irq,m2int_m2irq}/*_late_retrigger_1`.
- **THE ATOMICITY PROOF (direct sibling exhibition):** the recovered
  `lyc153_late_m1disable_3` (want E0 = FF41 disable commits EARLY) and the dropped
  `m0enable/lycdisable_ff41_2` (want out2 = the SAME FF41 disable commits LATE) are
  the SAME register write straddling the SAME 1-dot commit boundary with OPPOSITE
  required outcomes. No register/source/speed scoping separates them — the
  discriminator is the SUB-DOT phase of the write within the M-cycle, unrepresentable
  on the eager whole-M-cycle clock (`write_no_tick` lands at one whole dot; the ±1
  borrow is a whole-dot approximation of SameBoy's half-dot `GB_CONFLICT_WRITE_CPU`).
- **Candidate (b)'s premise is HALF-right and it does not save it:** the write-commit
  IS register-separable and **DMG-TIMER-SAFE** — under the borrow, `tima_reload`,
  `tima_write_reloading`, `rapid_toggle`, `div_write`, `ie_push`,
  `intr_2_mode0_timing`, `intr_2_oam_ok_timing`, `di_timing-GS` ALL PASS (the borrow
  touches FF41/FF45/FF0F, never FF04-07). So the floor here is NOT the timer wall —
  it is the sub-dot A/B swap WITHIN the STAT-register write-commit itself. **REFUTED
  as a shippable lever** (drops 8 SameBoy-pass; the golden law forbids it), and the
  reason is sub-dot, not timer.

## §4 — Why it is the floor (the unifying mechanism, code-anchored)

On the eager clock (`Bus::read`/`write`/`tick`, `eager_value` path) the CPU and PPU
advance together in whole M-cycles via `tick_machine` (inline): reads PEEK the live
PPU directly; writes commit `write_no_tick` at the M-cycle boundary; dispatch
services at the instruction boundary. Dispatch = read = write = ONE whole-dot eager
clock; none moves independently because there is no sub-M-cycle representation.

Tier2 reaches cc+0 by running a SEPARATE deferred `clock` that resolves reads/writes
at a different phase than the machine advance, coherently for ALL of dispatch+read+
write — that deferred clock IS the coherent retime, and it IS the DMG-timer wall
(tier2 DMG 116). The `dispatch_retime_impl` mechanism manipulates that deferred
clock, so on eager it is either **inert** (deferred clock frozen; §2/§3-b1) or, with
`advance_machine_t`, **double-advances** the already-`tick_machine`-ticked PPU
(#11cl: 611, intr_2 B=42). A SCOPED move (DS-domain, STAT-source, FF41 register) only
narrows WHICH dispatches use the broken mechanism; it never creates a coherent one.
Every scoping measured lands the fix on one whole-dot side of a sub-dot sibling
boundary and the drop on the other:

| scoping | net | recovered / dropped | why floor |
|---|---:|---|---|
| (a) CGB-DS dispatch (`COHERENT_DISP`) | −9 | +6 / −15 SB-pass | `_1`↔`_2` + tima siblings straddle the dispatch dot; 6/7 targets are PPU-emission-frame |
| (b1) DMG STAT-source dispatch | 0 | +0 / −0 inert | frozen machine + VBlank-coincident (position not source-separable) |
| (b2) DMG FF41/FF45 write-commit | +2 | +10 / −8 SB-pass | same-register siblings want opposite sub-dot commit sides; timer-SAFE |

## §5 — Bottom line: FLOOR, not a buildable scoped lever

The ~14 dispatch/write-frame bar rows are the **partial-flip floor** on the eager
whole-dot clock. A scoped coherent per-T dispatch retime is NOT buildable: the
dispatch POSITION is neither speed- nor source-separable (one CPU phase, VBlank-
coincident), and the WRITE-commit — the actual lever for the DMG engine rows, and
DMG-timer-safe — is a whole-dot A/B pair-shuffle that drops SameBoy-pass siblings
straddling the sub-dot commit boundary. This is the FOURTH independent refutation of
a dispatch move on the eager clock (after #11br incoherent-fold, #11cq stat_late
pair-shuffle, #11cl inert/corrupt) and it now has the direct atomicity exhibit:
`lyc153_late_m1disable_3` and `m0enable/lycdisable_ff41_2` are the same FF41 write
needing opposite sub-dot outcomes.

**The only lever that clears these rows is HALFDOT Part B** — resolve the eager
`Bus::read`/`write` to their true HALF-DOT (`tick_half`/`dhalf`, the
`GB_display_sync`/`GB_CONFLICT_WRITE_CPU` analogue) so the write-commit and the ISR
read land at SameBoy's sub-dot position WITHOUT routing wholesale through the
deferred machine (which double-advances / breaks the +16 ISR reads, #11cj). Part B
is a read/write-FRAME rewrite, NOT a dispatch scoping — it makes the commit position
emergent from the half-dot grid, which is the ONE representation that separates the
straddling siblings. This is the same redirect #11cl §7 and #11bw §"next lever"
reached; #11dq closes the dispatch-scoping question that stood between them and it.

### If Part B is built (predicted rows)

The DMG write-commit half-dot alone would flip the 4 DMG targets + 6 bonus WITHOUT
the 8 drops (each sibling resolves to its own half-dot side). The CGB DS 7 split: 1
(`late_m0irq_retrigger_scx1_ds_2`) is the ack-squash half-dot (#11de); the other 6
are the DS glitch-line emission frame / DS mid-dot floor (#11da/#11dk) — a Part-A
render half-dot, a DIFFERENT rewrite. So even Part B does not clear all 14 in one
lever: ~10 DMG-write + 1 CGB ack are the write/read half-dot; ~6 CGB DS are the
render/emission half-dot. Neither is a dispatch move.

## Gates (all hold; tree byte-identical)

- `git diff 4847b35 -- crates/` **empty** (all 3 throwaway edits reverted).
- `golden_fingerprint` ok (42.17s, default flags-off build).
- EV CGB 295 / EV DMG 54 / tier2 CGB 291 / tier2 DMG 116 reproduced.
- interconnect/engine reclock defaults NOT flipped; no push; parent branch untouched.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/agProbe
cargo test -p slopgb-core --test gbtr --release --features port_probe --no-run
BIN=$(ls -t target/agProbe/release/deps/gbtr-* | grep -v '\.d$' | head -1)
run(){ SLOPGB_ROWLIST=$PWD/scratchpad/$1 SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 env $2 \
       $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture; }
run cgb_rowlist.txt ""                    # 295
run cgb_rowlist.txt "SLOPGB_COHERENT_DISP=1"   # 304 (+6/−15 pair-shuffle)
# traces: run_gambatte --features port_probe <rom> [dmg|cgb], SLOPGB_EAGER|TIER2 + SLOPGB_S5DBG=1
#   CGB ly0_m0irq_scx0_ds_1: EV m0rise ly=0 dot=19 (spurious); tier2 dot=253/255
#   DMG lyc153_late_m1disable_3: EV & tier2 BOTH dispatch ly=153 dot=6 (write-commit differs)
# (b) DMG STAT/write-commit: re-add the SLOPGB_DMG_STAT_DISP throwaway (interconnect.rs
#   field + cycle.rs env read + bus.rs dispatch_reclock STAT branch + borrow_addr DMG
#   FF41/FF45), then run dmg_rowlist.txt "SLOPGB_DMG_STAT_DISP=1" → 52 (+10/−8), and the
#   tima/intr_2/di_timing tripwires under run_mooneye + the same env → all PASS.
```
