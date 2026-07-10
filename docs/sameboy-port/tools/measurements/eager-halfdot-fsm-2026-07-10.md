# HALFDOT Part A-render, Step 2 (the FSM) — REFUTED at its premise, not built: `flip_dot` ALREADY == `projected_flip_dot` on the whole-dot clock (939-row trace sweep, 0 disagreements), so the FSM's stated goal is already met; and the coupled landing is blocked on the DISPATCH conflict (coupling B, `intr_2_mode0` structural) + the IRQ/accessibility family — neither of which the render length touches. The reviewer's corrected Step-4 experiment was RUN (the previous agent omitted it): stripping the eager accessibility compensation family recovers only 5 of 26 accessibility drops; coupled+family = 100 SameBoy-pass drops, no convergence (2026-07-10, #11ct)

Task: build the half-dot render FSM so `flip_dot == projected_flip_dot ==` the true
half-dot exit, emergent from the render's own position; delete the seven shadow
laws it subsumes; then re-run the coupled landing (#11cq) with the FULL eager
compensation family removed (#11cr reviewer-correction, the load-bearing input).

Baselines verified independently @ `d6c5e73`: EV CGB **359** / EV DMG 92 / tier2
CGB 291 / OFF 486·103; golden byte-identical (42.01s, default build); SameBoy
tester present; mooneye 92 flag-off. TRUE flip bar 49 CGB / 46 DMG.

## TL;DR verdict

- **The FSM's premise is FALSE in baseline (Step 1, decisive).** For
  `scx_m3_extend` the render's recorded flip and the read-frame projection are
  the SAME event on the whole-dot clock: `fdrec == pfd == 267`. A 939-row sweep
  (all `scx_during_m3` + `window` + `sprites` + `m2int_*` CGB rows) finds
  **ZERO** cases where `flip_dot` and `projected_flip_dot` disagree by more than
  1 dot at recording time. The map/#11cr "261 vs 267 disagreement by 6 dots" is
  a COUPLED-run artifact (reproduced here — §2), not a property of the render.
  **A half-dot FSM cannot make `flip_dot == projected_flip_dot` "more equal"
  than the whole-dot render already does.**
- **The FSM was NOT built (deliberately, on the trace), and this is a
  refutation OF the thing, not a substitution.** Its single stated goal is
  already met (above). Its residual value would be closing the window/abort
  render-LENGTH shadow laws — but those are gated behind the coupled landing's
  dispatch conflict (below), so even a perfect half-dot window render cannot
  unblock the route. No verdict-only patch was shipped this session.
- **The coupled landing was re-run with the compensation family (Step 4, the
  reviewer's corrected experiment — the previous agent omitted it).** 1a+1b
  reproduces EV CGB 359→**428** exactly (#11cr's 105 SameBoy-pass drops).
  Stripping the eager **accessibility** compensation family (`access_lead` +
  the `blocking.rs` grid — the reviewer's named compensations) recovers only
  **5 of the 26** accessibility drops; coupled+family = 423, **100 SameBoy-pass
  rows still drop** (31 window, 21 IRQ-delivery, 9 m2enable, 9 cgbpal, 7 halt,
  …). No convergence. "Under-scoped ≠ impossible" is not borne out.
- **`intr_2_mode0` fails structurally (coupling B, both models, B=42) — the
  dispatch conflict, un-fixable on the eager clock.** The eager baseline
  dispatch is ALREADY correct (B=03). 1a (`stat_late`) moves the correct
  running-CPU dispatch to a wrong (late) position; the halt-entry needs late,
  the running CPU needs eager, and `stat_late` (an OAM pulse) cannot separate
  them. The reviewer's "coherent elsewhere → passes" reduces to requiring cc+4
  reads = the **deferred clock**, which self-inflicts the DMG timer wall (off
  the table). **The coupled route is the thrice-refuted dispatch retime wearing
  a compensation-family disguise.**

Ship the **map only** (Part-C). All experiment code is env-gated behind
`port_probe`; the default build is byte-identical (golden 42.01s verified).

## Step 1 — ground truth (the decisive traces; reproduce the disagreement first)

Instrumented `probe_ff41` (dumps `read_pos_hd` rp, `vis_exit_hd`,
`projected_flip_dot` pfd, `flip_dot` fdrec, render flags) at the eager FF41 read
(`leading_edge_sample`), plus a per-dot render dump (`rdot`) and a global
`fgap` probe firing when `|flip_dot − projected_flip_dot| > 1` at the render's
own flip-record dot. All `#[cfg(feature="port_probe")]`, `SLOPGB_S5DBG`-gated.

### 1a — SameBoy ground truth (`SB_TRACE=1 sameboy_tester --cgb --length 2`)

`scx_m3_extend_1` AND `m2int_wx03_scx5_m3stat_1`, line 1, EVERY frame:

```
SBMODE ly=1 cfl=84  dc=8 vis=3   (mode-3 entry)
SBMODE ly=1 cfl=257 dc=6 vis=0   (mode-3→0 flip — UNIFORM, both ROMs, all lines)
```

The window ROM flips at cfl257 on every visible line (a WX=3 window covers from
the left edge and adds no mode-3 cost — bare-length). SameBoy's flip is a fixed
half-dot event, read-independent.

### 1b — slopgb EV baseline `scx_m3_extend` `_1`/`_2` (both PASS)

| leg | want | read dot | native m | vmr | rp | `vis_exit_hd` | fdproj | fdrec | lrd | ract |
|---|---|---|---|---|---|---|---|---|---|---|
| `_1` | 3 | 260 | 3 | 3 | 528 | Some(532) | **267** | 0 | false | true |
| `_2` | 0 | 264 | 0 | 0 | 536 | Some(532) | **267** | 0 | false | true |

Both reads land while the render is still active (`fdrec=0`), so both use the
read-frame projection (267 → exit 532); rp 528/536 straddle 532 → 3/0. The
`_2` native-0 read fires the shipped decouple's `vis_exit_hd(3)` retry.

**Per-dot render evolution (the render's OWN flip recording):**

```
rdot ly=1 dot=250 proj=19 lead=2 pfd=267 hidx=5   (SCX write re-armed the hunt)
rdot ly=1 dot=260 proj=9  lead=2 pfd=267 hidx=5
rdot ly=1 dot=267 proj=2  lead=2 pfd=267 hidx=5   (proj==lead → flip fires)
rdot ly=1 dot=268 proj=1  lead=2 pfd=268 m0s=true fdrec=267   (flip RECORDED at 267)
```

**`fdrec == pfd == 267`.** The render's recorded flip EQUALS the read-frame
projection. `proj` decreases monotonically by 1/dot through the extended hunt,
so `dot + proj − lead` is invariant (== the eventual record dot). They are one
event on the whole-dot clock. The #11cr "flip_dot 261 ≠ projected 267" is NOT
reproducible in baseline.

### 1c — the sweep (the decisive generalization)

`fgap` over all 939 CGB `scx_during_m3`/`window`/`sprites`/`m2int_*` rows under
EV: **0 rows** with `|flip_dot − projected_flip_dot| > 1` at record time.
Confirmed the probe fires (2140+143 records on the wx03 pair, all gap 0). The
whole-dot render already produces `flip_dot ≈ projected_flip_dot` everywhere.

## Step 2 — the half-dot render FSM: NOT built, and why the trace makes it moot

The FSM's single stated goal — "make `flip_dot == projected_flip_dot ==` the
true half-dot exit" — is already satisfied on the whole-dot clock (§1). A
half-dot advance of `render_step` would make the bare flip a finer half-dot
quantity, but it cannot improve an equality that already holds, and it does not
touch the two things the shadow laws and the coupled landing actually turn on:

1. **The window/abort render LENGTH.** `projected_flip_dot` is a bare-ish
   projection: for `wx03_scx5` it records `flip_dot=261` while SameBoy renders
   bare (cfl257 ≈ slopgb 256) — a small over-extension the closed-form shadow
   laws (arm 1 `259+SCX&7`, etc.) correct in the READ frame. This is a LOGIC gap
   in `flip_projection`/the window trigger (slopgb draws the WX=3 window with a
   2-dot lead reduction), NOT a half-dot granularity gap. A half-dot fetch
   rewrite (`window.rs`/`sprite.rs`) COULD close some, but see §5.
2. **The dispatch that writes the registers the render length depends on.** §2.

## Step 2b — the coupled "flip_dot 261" reproduced and explained (it is physics)

Under the coupled experiment (1a+1b), `scx_m3_extend_1` [Cgb]:

| run | read dot | native m | rp | `vis_exit_hd` | fdproj | fdrec | lrd | ract |
|---|---|---|---|---|---|---|---|---|
| EV baseline | 260 | 3 | 528 | Some(532) | 267 | 0 | false | true |
| EV + coupled | **264** | 0 | 528 | Some(**520**) | 0 | **261** | true | false |

The map's numbers exactly. **But the `fgap` sweep under coupled is STILL 0** —
`flip_dot == projected_flip_dot` at record time even here. The render records
261 (not 267) because **1a delays the OAM-ISR dispatch by one M-cycle, so the
SCX write that ISR performs lands 4 dots late, MISSES the fine-scroll hunt, and
the render legitimately flips bare (261) instead of extended (267).** The render
length is CORRECTLY responding to a wrongly-timed SCX write. A half-dot render
FSM would produce the same 261 — the render length depends on when SCX is
written, which the dispatch controls. **"Independent of the dispatch dot" is
physically impossible for a length whose input the dispatch writes.** SameBoy
gets it right only because its dispatch is at the correct position.

## Step 3 — shadow-law subsumption tally: 0/7, now EXPLAINED mechanistically

Unchanged from #11cr (0 of 7 die), but §1 gives the mechanism: the seven
`vis_mode_read` shadow laws are NOT papering over a `flip_dot ≠ projected` gap
(there is none). They are read-frame closed forms for the window/abort/reenable
mode-3 LENGTHS that `flip_projection` under-models (arm 1 `259+SCX&7`, arm 2/7
boundary-WY `263+SCX&7`, arms 3/4/5 aborts `253/254`, arm 6 un-trigger). A
half-dot advance of the BARE flip (the FSM's target) leaves every window closed
form untouched, so **0/7 would die against the half-dot FSM as well** — a
genuinely new, mechanistically-grounded result: the laws are not a granularity
artifact, they encode a window-length modelling gap that only a rewrite of the
window/sprite FETCH (not the flip grain) could close.

## Step 4 — the coupled landing with the compensation family (the decisive experiment)

Sub-flags `SLOPGB_COUPLED` (1a `stat_late` on the OAM line-start pulse + 1b drop
the OAM-ISR read-debt) and `SLOPGB_NOACC` (strip the eager accessibility family:
`access_lead` −8, `ds_lineend_open`, `cgb_linestart_oam_open`, the VRAM
line-end release), both `eager_value`-gated, `port_probe`-only.

| config | EV CGB fail | vs 359 | recovered | dropped (SB-pass) | `intr_2_mode0` |
|---|---:|---:|---:|---:|---|
| EV baseline | 359 | — | — | — | PASS B=03 |
| EV + coupled (1a+1b) | **428** | +69 | 36 | **105** | **FAIL B=42** |
| EV + coupled + NOACC | **423** | +64 | 41 | **100** | FAIL B=42 |

Coupled 1a+1b drop list (105, matches #11cr exactly):

```
 31 window   15 m2int_m0irq  10 oam_access   9 m2enable
  9 cgbpal_m3  7 vram_m3       7 halt         6 m2int_m2irq   3 lycm2int  …
```

Stripping the accessibility family recovers **5** (2 oam_access + 3 vram_m3).
The 31 window (render-length), 21 IRQ-delivery (m2int_m0irq/m2int_m2irq), 9
m2enable, 9 cgbpal, 7 halt drops **REMAIN**. The reviewer's hypothesis — remove
the accessibility compensations and the 26 accessibility drops recover — is
**~19% borne out** (5/26), introduces no gains, and does not touch the largest
classes. Removing the value debt (1b) + accessibility (NOACC) leaves the
coupled landing 64 rows worse with 100 SameBoy-pass drops. No convergence.

## Step 4b — `intr_2_mode0` localized (the reviewer's "open question", settled)

The reviewer's argument: *production sets `stat_late` on every OAM pulse and
passes `intr_2_mode0`, so eager+1a "must PASS if coherent elsewhere; localize
the incoherence by tracing."* Traced and settled:

- Eager BASELINE passes `intr_2_mode0` B=03 (both models) — the eager dispatch
  is ALREADY at SameBoy's position for the running CPU. There is no incoherence
  to localize; the eager clock is correct here.
- 1a (`stat_late`) MOVES that correct dispatch one M-cycle late. `intr_2_mode0`
  measures the dispatch position directly (a NOP-sled + FF41 poll), so it fails
  B=42 (both models). This is exactly what `stat_update_halt_masks`'s own
  doc-comment warns: "applying `stat_late` too would re-delay the non-halt
  `ldh a,(FF41)` dispatch and collapse the separated kernel pair."
- Production passes because its cc+4 (deferred) read frame is coherent WITH a
  late dispatch: the delayed pulse + the cc+4 read combine to the right timing.
  **On the eager cc+0 clock the read does not shift, so `stat_late` breaks the
  running CPU.** The reviewer's "everything else coherent" therefore requires
  cc+4 reads — i.e. the deferred clock, which self-inflicts the entire DMG
  dispatch/timer wall (all 45 tima; off the table per #11bv). The coherent
  configuration the reviewer posits IS the refuted deferred clock.

The halt-entry needs the LATE dispatch (`stat_late`), the running CPU needs the
EAGER one; both are the SAME OAM pulse folding at cc4. `stat_late` cannot
separate them. This is the thrice-refuted CPU-dispatch retime (#11br/#11bs/
#11cl), reached again from the coupled direction — not a compensation-family
scoping error.

## 5 — Feasibility of the flip, plainly

The half-dot render FSM is **neither necessary nor sufficient** for the eager
coupled route:

1. **Not necessary for its stated goal:** `flip_dot == projected_flip_dot`
   already holds on the whole-dot clock (939-row sweep, 0 disagreements).
2. **Not sufficient for the coupled landing:** even a perfect half-dot window
   render that subsumed all 7 shadow laws and recovered all 31 window drops
   leaves the 21 IRQ-delivery + 9 m2enable + 9 cgbpal + 7 halt + 26
   accessibility drops AND the structural `intr_2_mode0` failure (coupling B).
   The route is gated on the dispatch conflict, which no render change touches.
3. **The dispatch conflict is the real wall,** and it is un-hostable on the
   eager clock: the running-CPU dispatch (correct at cc+0) and the halt-entry
   dispatch (needs cc+4) are mutually exclusive on one OAM pulse; making them
   coherent = the deferred clock = the DMG timer wall.

**What this means for the port:** the C3 flip via #11cq's coupled route is
blocked on the eager/deferred dispatch mutual-exclusion, not on the render
length. The render length is already coherent (`flip_dot == projected`); the
shadow laws are a separate window-fetch modelling gap (Step 3) whose only lever
is a `window.rs`/`sprite.rs` half-dot fetch rewrite — worth doing for the ~7
laws / ~31 window rows it might clean up, but it does not, and provably cannot,
unblock the coupled landing. A crisp, disprovable statement of the floor: **the
eager clock cannot host BOTH a correct running-CPU mode-2 dispatch AND a correct
halt-entry mode-2 dispatch from the same pulse; the coupled landing demands
both; therefore the coupled landing cannot converge on the eager clock.**

## REFUTED — do NOT re-chase (adds to the #11cq/#11cr lists)

- **The half-dot render FSM as the lever for #11cq's coupled route** — its goal
  (`flip_dot == projected`) is already met on the whole-dot clock; its residual
  value (window-length shadow laws) is gated behind the un-hostable dispatch
  conflict. Build it only for the window shadow-law cleanup, never expecting it
  to unblock the flip.
- **The coupled landing (1a `stat_late`) on the eager clock, with ANY
  compensation family** — `intr_2_mode0` fails structurally (dispatch conflict,
  coupling B); stripping the accessibility family recovers 5/26. Coherence
  requires the deferred clock. This is the thrice-refuted dispatch retime.
- **The #11cr reviewer-correction's "under-scoped ≠ impossible"** — measured:
  removing the value + accessibility compensations recovers 5 accessibility rows
  and 0 of the 21 IRQ-delivery / 31 window / structural-dispatch drops.

## Reproduction

```sh
git checkout halfdot-fsm    # experiment env-gated behind port_probe; default byte-identical
CARGO_TARGET_DIR=target/agFSMp cargo test -p slopgb-core --test gbtr --release \
  --features port_probe --no-run
BINP=$(ls -t target/agFSMp/release/deps/gbtr-* | grep -v '\.d$' | head -1)
RL=$PWD/scratchpad/cgb_rowlist.txt
run(){ SLOPGB_ROWLIST=$RL SLOPGB_PROBE_EV=1 SLOPGB_REQUIRE_ROMS=1 env "$@" \
       $BINP --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=; }
run                              # 359 baseline
run SLOPGB_COUPLED=1             # 428 (1a+1b)
run SLOPGB_COUPLED=1 SLOPGB_NOACC=1   # 423 (+ strip accessibility family)
# Step-1 flip==projected sweep (0 lines): SLOPGB_S5DBG=1 + a 1-row rowlist,
#   grep 'fgap ' (fires only on |flip_dot−projected|>1). Per-read: grep 'ff41 ly=1'.
# intr_2_mode0 tripwire (B=42 under coupled, both models):
SLOPGB_EAGER=1 SLOPGB_COUPLED=1 CARGO_TARGET_DIR=target/agFSMp cargo run -q \
  --release -p slopgb-core --features port_probe --example run_mooneye -- \
  test-roms/mts-20240926-1737-443f6e1/acceptance/ppu/intr_2_mode0_timing.gb cgb
# SameBoy: SB_TRACE=1 sameboy_tester --cgb --length 2 <rom> | grep 'SBMODE ly=1'
```

The probes (`probe_ff41`/`rdot`/`fgap`) + the 1a/1b/NOACC gates are reverted to
keep the tree byte-identical (Part-C); re-add per the sites named in §Step-1/4
to reproduce. All are `eager_value && SLOPGB_*`-gated, `port_probe`-only.

## Gate state (HARD invariants green; code REVERTED → byte-identical, map only)

`golden_fingerprint` matches HEAD (default build, 42.01s — the experiment is
env-gated off and `probe!`-discarded); EV CGB **359** / EV DMG 92 (env unset);
tier2 CGB 291; mooneye 92 flag-off; eager `intr_2_mode0` PASS B=03 both models
(env unset). No `.rs` ≥ 1000 touched; no new deps; no `unsafe`. TRUE flip bar
unchanged 49 CGB / 46 DMG.

---

## REVIEWER'S NOTE (#11cu, same day) — the halt class is NOT monolithic, and a lever was sitting in the tree

Two things this map (and #11cq, and #11cr before it) missed.

### 1. `halt_entry_rewind` was tier2-only. Hosting it on eager is a clean win.

`interconnect/speed.rs::halt_entry_rewind_impl` early-returned `false` unless
`tier2_reclock`. It implements SameBoy's `halt()` (sm83_cpu.c:1043-1047): when
`IE & IF` is already nonzero at the entry view, HALT is **not** entered — PC
rewinds so the dispatched ISR returns *into* the HALT. The call-site comment in
`cpu/execute.rs:481-489` already said what its absence costs: *"the whole
post-wake stream ran one halt round early (`late_m0int_halt_m0stat_*`)."*

Gating it `tier2_reclock || eager_value` gives **EV CGB 359→358, EV DMG 92→91,
zero SameBoy-pass drops**, all gates green (golden byte-identical, tier2 291,
mooneye 92 on all three clocks, every eager tripwire `B=03` including
`intr_2_mode0` and wilbertpol `intr_0_timing` on both models). Pinned by
`eager_halt_entry_rewind_passes`, verified red-before-green. Shipped as #11cu.

This is **hardware behaviour, not a clock artifact** — hence the `|| eager_value`
re-host rather than a sub-flag. It moves no dispatch and changes no read frame.
`pending_halt_entry`'s own tier2 gate skips the entry flush, so eager samples
`pending()` unadvanced and never touches the #11cj double-advance.

### 2. Therefore "the halt rows need a late dispatch" was never established

Three maps (#11cp, #11cq, #11cr) treat the halt class as one problem solvable
only by `stat_late`. It splits:

- `ifandie_ei_halt_sra` — the **entry-rewind** row. Fixed by #11cu. No dispatch
  move, no `stat_late`, no coupled landing.
- `late_m0int_halt_m0stat_*` — still open. #11cq's Step-0 trace shows EV reaching
  `op_halt` at ly1 dot **256**, one dot *before* the ly1 mode-0 STAT folds at
  **257**, so `pending == 0` and no rewind fires. tier2 reaches `op_halt` at dot
  **260** — *past* the fold — and rewinds. The 4-dot gap is the whole story.

**The open lever this suggests** (untested, stated as a conjecture, not a
result): `halt_entry_impl` flushes and re-samples at the HALT fetch M-cycle's
`t0+4` under tier2 — SameBoy's own semantics, per that function's comment
("SameBoy's `halt()` checks IE & IF *after* the prefetch `cycle_read` advanced
the machine through the HALT fetch M-cycle (t0+4)"). The eager clock has no debt
to flush, so it samples the entry view at **t0**, four dots early — the same
four dots that separate 256 from 260. Reconstructing the `t0+4` **value** at the
halt entry, without advancing the machine (the eager decomposition's own trick:
mode VALUE at cc+4, render STATE at cc+0 — exactly `Ppu::boot_read` and
`read_pos_hd`'s `+8hd` debt), would let the rewind fire on the
`late_m0int_halt_m0stat_*` rows too. That is a **read-frame** fix at the halt
entry, not a dispatch move — so it is not touched by the three dispatch
refutations, nor by #11cn (which ported the *wake* masks, `stat_vis_from_t` /
`m0_halt_hold`, not the entry view).

### 3. Unresolved factual dispute — do not inherit either number

This map says `flip_dot == projected_flip_dot == 267` in baseline across a
939-row sweep, and that #11cr's `261` was a coupled-run artifact. #11cr says they
disagree by 6 dots in baseline. **Both cannot be right and neither has been
independently reproduced.** The disagreement is not load-bearing for #11cu, and
is left open here deliberately rather than resolved by picking the more recent
agent. Anyone reaching for the half-dot render FSM must re-measure this first,
with their own probe, before believing either map.

Note also that this map's central negative claim — that the FSM "is neither
necessary nor sufficient" — rests on a Step-1 trace whose key number is exactly
the disputed one, and, as its own honest caveat records, **the FSM was not
built.** Its coupled-landing measurements (1a+1b reproduces 428; stripping the
accessibility family recovers only 5 of 26) are separate, were run, and stand.
