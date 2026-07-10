# EAGER coupled landing — REFUTED on the whole-dot clock: the coupled change (OAM `stat_late` + OAM-ISR read-debt drop) IS the right direction for the halt rows (TRUE bar CGB 49→47, DMG 46→38, the eager CPU halts at production's dot) but is blocked on the RENDER frame, not the read frame — moving the OAM-ISR dispatch drags `vis_exit_hd` +4 dots (a `_1`/`_2` length pair-shuffle, −104 CGB / −72 DMG SameBoy-pass) AND collapses the running-CPU kernel pair (`intr_2_mode0_timing` B=42). Neither is a read-side lever; both need the independent half-dot render LENGTH (Part A-render). Step-0 ground truth settles #11co vs #11cp (2026-07-10, #11cq)

Task (#11cp redirect): remove BOTH compensating eager errors together — (1a) give
the OAM line-start pulse the `stat_late` dispatch mask so the setup ISR dispatches one
M-cycle late; (1b) resolve the read to its true position so the read-anchored ISR
VALUE frame stays put. Behind `coupled_landing` (`SLOPGB_COUPLED`, active only under
`eager_value`). Build-measure, RED-tolerant; a crisp refutation is first-class.

## Step-0 ground truth — the #11co/#11cp disagreement SETTLED (trust the trace)

Single-row `late_m0int_halt_m0stat_scx3_3a` [Cgb], instrumented `op_halt` /
`fold_ppu_events` (STAT-IF fold) / the running-CPU dispatch / the halt-idle wake,
dual-traced under OFF / EV / tier2 (probes all reverted, `SLOPGB_HALTDBG`):

| event | OFF (pass) | EV (fail) | tier2 (pass) |
|---|---|---|---|
| setup OAM STAT folds `intf` | ly1 dot **0** cc4 cyc4852 | ly1 dot **0** cc4 cyc4852 | ly1 dot **0** cc4 cyc4851 |
| setup ISR dispatch (running CPU) | ly1 dot **4** cyc4856 pc**01b2** | ly1 dot **0** cyc4852 pc**01b1** | ly1 dot 0 cyc4852 pc01b2 |
| `op_halt` HALTED | IME0 dot **332** | IME1 dot **256** | IME0 dot 336 (after IME1 REWIND dot260) |
| ly1 mode-0 STAT folds | dot 257 | dot 257 | dot 257 |
| ly1 mode-0 STAT serviced | inline dispatch dot260 (BEFORE halt) | **never** (halted at 256) | via rewind dispatch dot260 |
| first idle wake | dot336 wake=**00** → stays, wakes ly2 → **mode 0 ✓** | dot260 wake=**02** → wakes ly1 → **mode 2 ✗** | dot336 wake=00 → mode 0 ✓ |

- **#11co ("the eager CPU NEVER enters HALT — dispatches inline") is WRONG.** The
  eager CPU DOES `op_halt` → `cpu.halted=true` (IME1 path) at ly1 dot **256**. The
  "0 set_cpu_halted" #11co counted was the first-idle-check waking immediately.
- **#11cp is CONFIRMED.** The STAT-IF fold is byte-identical (dot 0 cc4, all three
  clocks). The divergence is the setup ISR dispatching **4 dots + 1 instruction**
  early under EV (dot 0 pc01b1 vs OFF dot 4 pc01b2) — the eager OAM pulse lacks the
  `stat_late` running-CPU mask. That head start cascades to `op_halt` landing 76 dots
  early (256 vs 332), **one dot before** the ly1 mode-0 STAT fold (257) — so EV halts
  before it can dispatch ly1's STAT inline, then its first idle wake (dot 260) catches
  ly1's STAT and wakes one line early → reads mode 2 where OFF/tier2 read mode 0.
- tier2 reaches the same correct end via `halt_entry_rewind` (IME1 REWIND at dot 260
  dispatches ly1's STAT, re-enters halt, lands `op_halt` at dot 336). EV has no rewind
  demand met because it dispatched the setup ISR from the wrong (early) pc/dot.

## The coupled change as built (byte-identical off; REVERTED, Part-C)

Sub-flag `coupled_landing` (`on && SLOPGB_COUPLED`, forwarded to the PPU):
- **1a** (`ppu/stat_irq/reclock.rs::stat_update_halt_masks`): the lines-1-143 OAM
  line-start pulse also sets `self.stat_late = true` under `coupled_landing` — the
  running-CPU dispatch mask production's `stat_events_tick` already sets.
- **1b** (`ppu/engine.rs::read_pos_hd`): for the carried OAM-ISR read
  (`read_carried && stat_rise_oam`) drop the eager read-debt (SS +8hd / DS +4hd) → the
  late dispatch's cc+0 peek already lands at the read's true position, so the VALUE
  frame `read_pos_hd = 2·dot` is UNCHANGED across the change (this is why 1b is the
  correct read half — NOT #11co design (a)'s cc+4 sample, which also drags the value).

## Results (baselines EV CGB 361 / EV DMG 92 / tier2 291 / OFF 486·103; golden 9020 match)

| bin | baseline | coupled | Δ |
|---|---:|---:|---:|
| halt row `late_m0int_halt_m0stat_scx3_3a` | got 2 (FAIL) | got **0 (PASS)** — `op_halt` HALTED at **dot 332** = OFF | fixed |
| EV CGB two-bin fail | 361 | **438** | +77 (recover 36 / regress 113) |
| EV DMG two-bin fail | 92 | **136** | +44 (recover 28 / regress 72) |
| recovered SameBoy-pass | — | 29 CGB / 28 DMG | halt + line-start `_2` legs |
| **regressed SameBoy-pass** | — | **104 CGB / 72 DMG** | window/sprite length `_1` legs |
| TRUE flip bar (OFF-pass ∩ EV-fail ∩ SB-pass) | 49 CGB / 46 DMG | **47 CGB / 38 DMG** | −2 / −8 (halt rows leave the bar) |
| `intr_2_mode0_timing` (eager) | B=03 PASS both | **B=42 FAIL both** | broken |
| `intr_2_mode3/sprites/0`, `di_timing`, `intr_0_timing` (both models) | B=03 | B=03 | held |

The TRUE flip bar DROPS (the coupled change is the right direction — it recovers the
halt/line-start rows) but the RAW two-bin WORSENS badly: the 104/72 dropped
SameBoy-pass rows are mostly OFF-fail flip-gains (not in the TRUE bar), so they don't
show in the bar count yet are real EV passes lost. **A shipped slice may not drop a
SameBoy-pass row → non-shippable.**

## Why refuted — TWO un-decouplable couplings, BOTH render-side, neither a read lever

**(A) Render-frame length shuffle (the −104/−72).** The regressions are the `_1`
legs of window/sprite length pairs (`m2int_wx*_m3stat_1`, `m2int_m2stat_1`,
`postread/preread/prewrite_1`, `scx_m3_extend_1`); the recoveries are the matching
`_2` legs — a pure `_1`↔`_2` pair-shuffle across the mode-3 flip. Mechanism: 1a moves
the OAM-ISR **dispatch** +4 dots, dragging the WHOLE handler (including its near-flip
m3stat FF41 read) +4 dots. 1b holds the read VALUE (`read_pos_hd`) fixed, but the
exit comparison is `read_pos_hd < vis_exit_hd(m)` and `vis_exit_hd` is computed from
the LIVE render FSM at the (now +4) peek — `flip_dot` / `render.win_active` /
`render.n_sprites` / `eff.wx` genuinely advanced 4 dots. So the flip position moves +4
while the value stays → both legs cross the flip → verdict flips. **No read-side lever
can hold `vis_exit_hd` fixed while the dispatch moves**: design (b) (advance the PPU to
the read's true half-dot, sample there) samples the render state at the ADVANCED
position — strictly worse, collapsing value onto the render frame (= #11co design (a),
361→425). The render LENGTH must move to its own half-dot INDEPENDENTLY of both the
dispatch and the read — Part A-render.

**(B) Running-CPU kernel-pair collapse (`intr_2_mode0` B=42).** The eager cc+0
dispatch is ALREADY SameBoy-aligned for the running CPU (baseline `intr_2_mode0`
passes B=03). `stat_late` (1a) re-delays that non-halt `ldh a,(FF41)` dispatch too,
collapsing the separated m2int kernel pair — the exact failure the
`stat_update_halt_masks` doc-comment already warned of ("applying `stat_late` too
would re-delay the non-halt dispatch and collapse the separated kernel pair"). The
halt ENTRY needs the late dispatch; the running CPU needs the eager one — #11cp's
mutually-exclusive demands, now confirmed by the tripwire. `stat_late` cannot separate
them (both are OAM pulses folding at cc4).

## Verdict — the eager decomposition is NOT wrong; the block is the render length

The coupled landing does NOT converge on the whole-dot eager clock, but this does NOT
indict the eager clock or its read frame. 1b proves the read VALUE half is sound
(`read_pos_hd` unchanged across the coupled change; the tower's value reconstruction is
correct). The obstruction is entirely the **render frame**: `vis_exit_hd` (the emergent
mode-3 length) is welded to the live render FSM position and moves with any dispatch
shift. The port should NOT return to a coherent deferred clock — the DMG dispatch/timer
recovery that motivates the eager clock is intact, and the halt rows are not a read
problem. The single remaining lever is the **independent half-dot render LENGTH**
(HALFDOT Part A-render): move the mode-3 exit to its own half-dot so `vis_exit_hd`
holds while the OAM dispatch moves late (fixing halt) and the value stays put (1b).
Only then do the length `_1`/`_2` pairs survive the dispatch move. That is the same
un-hosted multi-session render-FSM reclock #11bw/#11co/#11cp all point to — now pinned
to the render side, with the read side (1b) and the dispatch side (1a) proven correct
in isolation.

Also parked, do NOT re-chase: the OAM-dispatch `stat_late` mask alone (#11cp, +127/+56);
the true-T / design-(a) read (#11co, 361→425); design (b) read-resolve (collapses value
onto the advanced render frame = worse); the CPU IRQ dispatch dot move
(#11br/#11bs/#11cl, thrice-refuted); the tier2 halt-wake mask port (#11cn).

## Gate state (HARD invariants green; code REVERTED → byte-identical, map only)

`golden_fingerprint` 9020 cases match HEAD (production byte-identical — the sub-flag +
Step-0 probes are env-gated off); mooneye **92 passed** flag-off; tier2 CGB two-bin
**291**; EV CGB **361** / EV DMG **92** (env unset); clippy `-D warnings` clean; no
`.rs` ≥ 1000 touched. TRUE flip bar unchanged at **49 CGB / 46 DMG** on the shipped
tree; the 5 CGB + 6 DMG halt rows sit inside it, blocked on Part A-render.

## Reproduction

```
git checkout halfdot-coupled     # code byte-identical @ 03ebd14; this map only
CARGO_TARGET_DIR=target/agC2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agC2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# baselines (env unset):
SLOPGB_ROWLIST=$PWD/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 361
# the refuted coupled slice (re-add): coupled_landing sub-flag = eager_value &&
#   SLOPGB_COUPLED; 1a `self.stat_late=true` in stat_update_halt_masks (OAM pulse,
#   line!=0); 1b return `base` (no debt) in read_pos_hd when read_carried &&
#   stat_rise_oam → EV CGB 438, DMG 136; intr_2_mode0 B=42.
# Step-0 halt trace: re-add SLOPGB_HALTDBG eprintln at op_halt / fold_ppu_events /
#   the running dispatch / the halt-idle wake (all reverted).
```
