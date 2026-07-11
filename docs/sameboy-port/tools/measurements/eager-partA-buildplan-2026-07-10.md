# HALFDOT Part-A render, EAGER clock — the executable build plan, RE-SCOPED by fresh measurement: the render mode-3 LENGTH is already correct (tier2's whole-dot render passes 45 of the 49 TRUE-bar rows), so Part-A is NOT the ~35-row atomic render-length rewrite the prior maps assumed — it is the emergent-flip / case-tower-deletion lever for a SMALL residual (~2–5 rows), and the bar is dominated by SEPARABLE eager read-frame / wake / DS / DMG-window re-host slices. flip_dot == projected_flip_dot RECONCILED (both 267; #11cr's 261 was a coupled artifact, #11ct CONFIRMED). (2026-07-10, #11dh)

Base: `finish-port-halfdot @ b107806`. This map supersedes the tier2/deferred
framing of `docs/sameboy-port/HALFDOT-BUILD-PLAN.md` §2–§6 for the eager vehicle
(that plan's §1 SameBoy per-tick spec and §5 atomicity proof are re-used and
re-tested below). All measurement is fresh at b107806, own probe, reverted
(Part-C); tree byte-identical (`golden_fingerprint` ok, 42.83s).

---

## 0. The bottom line (read this first)

The task premise was: *"~35 of the 47 C3-flip bar rows are WELDED to the render
mode-3 LENGTH / flip-dot position; only the unbuilt Part-A half-dot render FSM
clears them."* **Measurement refutes the premise.** Three findings, each
independently reproduced at b107806:

1. **`flip_dot == projected_flip_dot`** in baseline EV on the sharpest disputed
   row (`scx_m3_extend`, scx&7=5): the read-frame projection is INVARIANT at
   **267** across the whole extended mode-3 region (dots 109–267), and the render
   records `flip_dot = 267` — they are ONE event on the whole-dot clock. #11cr's
   "flip_dot 261 ≠ projected 267" is a COUPLED-run artifact (the dispatch move
   makes the render legitimately flip bare at 261); #11ct is CONFIRMED. **There is
   no internal sub-dot flip granularity gap for a half-dot FSM to close.**

2. **The whole-dot render LENGTH is already correct.** The tier2 deferred clock
   (`tier2_reclock`, SAME whole-dot `render_step` / `m0_flip_events` as eager)
   PASSES **45 of the 49** TRUE-bar rows — CGB **17/17**, DMG **28/32**. Every one
   of those 45 is a mode-3-length row the whole-dot render handles; the eager
   clock fails them purely on the READ FRAME / gating / wake port, not the length.
   Only **4 DMG rows** fail even the whole-dot render (deeper engine/accessibility
   gaps, not a half-dot FSM need).

3. **Therefore Part-A is NOT one atomic render+read+dispatch landing** — the
   render length and the read position separate CLEANLY (the whole-dot render is
   the render half, already done; the eager read frame is the other half, ported
   slice-by-slice this session, 578→318). §5's `late_scx4` atomicity proof holds
   in principle but does not gate the flip: the render half is satisfied by the
   whole-dot render, so the coupled A/B swap is already resolved on one side.

**Part-A's actual, re-scoped role:** make the mode-3 flip / `vis_early` / DS
`vis_exit_hd` EMERGENT from the render's own half-dot position, so the
`mode0.rs::m0_flip_events` `early_lead` case-tower and the DS mid-dot floor — the
two whole-dot mechanisms that CANNOT be gate-ported to the eager clock without
breaking `intr_2_*_sprites` (#11by/#11dg) — are DELETED. That unblocks a SMALL
residual (~2–5 accessibility rows: `vram_m3/postread_scx3_2`,
`oam_access/postwrite_2_scx3`, `sprites/…_m3stat_ds_2`, maybe the 2 DMG wxA6
busyreads). Everything else on the bar is a plain re-host slice needing no FSM.

The honest recommendation: **stage the cheap separable re-hosts FIRST** (L1 DS, L2
DMG window, the wake-clock port — ~40 rows, proven-clean vein), and build the
half-dot render FSM ONLY for the case-tower-blocked accessibility residual — a
much smaller, better-targeted rewrite than the "atomic Part-A" the prior maps
scoped.

---

## 1. The TRUE flip bar at b107806 (reproduced from scratch)

Two-bins (`flagon_probe`, `scratchpad/{cgb,dmg}_rowlist.txt`, `SLOPGB_PROBE_OFF/EV`):

| | OFF fail | EV fail | flip-BUGs (OFF-pass ∩ EV-fail) | **TRUE bar** (∩ SameBoy-pass) | tier2 (whole-dot render) PASSES |
|---|---:|---:|---:|---:|---:|
| CGB | 486 | 318 | 56 | **17** | **17 / 17** |
| DMG | 103 | 74 | 41 | **32** | **28 / 32** |

TRUE bar **= 49** (CGB 17 + DMG 32). Classifiers (`classify_cgb_regr.py`,
`classify_dmg.py`, `SLOPGB_GBTR_ROOT` set, patched SBMODE tester) returned UNK 0.
(The task's "47 = CGB 15 + DMG 32" is the same bar ±2 CGB from a nearby commit;
b107806 measures 17 CGB.)

**tier2 whole-dot render pass rate is the load-bearing new number.** Running the
49 bar rows under the DEFAULT `ProbeMode::Reclock` (tier2 deferred clock, identical
`render_step`): CGB 17/17 pass, DMG 28/32 pass. The 4 DMG tier2-fails:
`lycEnable/lycwirq_trigger_ly00_stat50_2`, `m2enable/late_enable_m0disable_2`,
`window/m2int_wxA6_oambusyread_2`, `window/m2int_wxA6_vrambusyread_2`.

---

## 2. The flip_dot dispute — RECONCILED with raw dots (own probe, reverted)

Probe: per-mode-3-dot dump in `m0_flip_events` (`RDOT`: `dot proj lead
pfd=dot+proj−lead flip_dot`) + an FF41 read dump in `vis_mode_read` (`FF41`:
`dot read_pos_hd native_m vis_exit_hd(m) vis_exit_hd(3) pfd flip_dot lrd
win_active`). `port_probe`+`SLOPGB_S5DBG`; single-row rowlists; reverted after.

### `scx_m3_extend_1/_2` [Cgb] (scx&7=5) — the sharpest case, EV baseline (both PASS)

SameBoy ground truth (patched `sameboy_tester --cgb`, `SB_TRACE=1`): ly=1 mode-3
entry `cfl84 dc8`, mode-3→0 flip **`cfl257 dc6`** (slopgb-frame 2·257+6 = **520**),
identical `_1`/`_2`, read-independent.

slopgb EV, the extended-mode-3 region (SCX write re-armed the fine-scroll hunt):

```
FF41 dot=109 rphd=226 native_m=3 vexit=532 pfd=267 flip_dot=0    (pfd stabilised)
FF41 dot=110..250  ...        native_m=3 vexit=532 pfd=267 flip_dot=0
FF41 dot=251       rphd=510 native_m=0 vexit=510 pfd=267 flip_dot=0  (native flip)
FF41 dot=268       rphd=544 native_m=0            pfd=268 flip_dot=267 (RECORDED)
```

`pfd` is **INVARIANT at 267** from dot 109 through the flip; the render records
`flip_dot = 267`. **`flip_dot == projected_flip_dot == 267`.** The `pfd ≠ flip_dot`
lines the #11cr map cited are all POST-flip (once recorded, `pfd` degenerates to
`self.dot` because `proj` saturates to 0) — meaningless. Under the COUPLED
experiment the render records **261** because the moved dispatch lands the SCX
write 4 dots late, it MISSES the hunt, and the render legitimately flips bare
(cfl≈256) — a correct response to a wrongly-timed write, not a granularity gap.

**Verdict: #11ct is right, #11cr is wrong.** No half-dot FSM can make `flip_dot`
and `projected_flip_dot` "more equal" than the equality that already holds. The
real gap is a slopgb-frame ↔ SameBoy-frame offset (slopgb dot 267 vs SameBoy's
exit at slopgb-frame 520 ≈ dot 256), which the `vis_exit_hd` arm-8 constant (532)
and the +8hd read-debt already encode in the READ frame — a read-frame calibration,
not an internal flip disagreement.

---

## 3. Three welded-row traces (own probe, reverted)

All three PASS under tier2 (whole-dot render) and FAIL under EV → the weld is the
EAGER READ FRAME / gate, not the render length.

### Row 1 — `vram_m3/postread_scx3_2` [Cgb] (accessibility, the #11dg vis_early weld)

EV FAIL `want=0 got=3`; tier2 PASS. RDOT on the read line: `pfd=254` invariant,
render flips at `flip_dot=254` (bare, scx&7 varies by frame). The read is a VRAM
accessibility read at dot ≈256. #11dg's dual-trace:

```
EAGER dot=256 rphd=520 vram=BLOCKED vearly=true flipdot=0 vis_exit3=Some(512)
TIER2 dot=256 rphd=512 vram=OPEN    vearly=true flipdot=0 vis_exit3=Some(512)
```

`vram_read_blocked` releases on `vis_early` under tier2 only; eager stays blocked.
Extending the gate over-releases `postread_scx0/scx5_1` (#11dg: CGB +13/−9, DMG
+5). Root cause: the eager `vis_early` LATCH DOT uses `mode0.rs` `early_lead = 3`
(the bare LE residue) not the tier2 collapsed-parity residue (`early_lead`
case-tower, tier2-scoped; its residues break `intr_2_*_sprites` under eager,
#11by). **Weld class: eager flip-dot / case-tower — the Part-A residual.**
Same for `oam_access/postwrite_2_scx3` (`write_unblocked_early`, identical gate).

### Row 2 — `late_wy_1toFF_1` [Dmg] (window render-commit / un-trigger)

EV FAIL `want=0 got=3`; tier2 PASS. Trace: `win_active=true`, `flip_dot=261`,
`vexit_native(m=0)=None`, `vexit3=Some(514)`; the decisive read lands at dot
≈245 (`rphd 498 < 514`) → returns 3. SameBoy renders BARE (WY→FF un-triggered the
window via continuous `wy_check`), slopgb's `wy2`-lagged render keeps the window
active and over-holds mode 3. The correct arm is D6 (late-WY un-trigger, exit
253+scx&7) but it needs `!wy_trig_sb_raw`, and eager sets `wy_trig_sb_raw` on the
wrong dot. **Weld class: L2 DMG window re-host — the eager `wy_trig_sb_raw`
un-latch is un-ported (#11ck slice 2 did the CGB `late_wy` un-latch CGB-scoped; the
DMG class was never re-hosted). NOT Part-A** — tier2's whole-dot render + arm D6
already pass it.

### Row 3 — the mode-3 exit itself (`scx_m3_extend`, §2) — SEPARATES cleanly

Both `_1`/`_2` PASS EV; the render length (flip 267) and the read positions
(rphd 528/536 straddle vexit 532) are each correct and independent — the coupling
#11cq feared only bites when the DISPATCH moves (coupled), which the flip does not
do. On the plain eager clock the length half is DONE (whole-dot render), the read
half is DONE (+8hd debt). **Nothing to build here.**

---

## 4. The bar decomposed (per-row, tier2-pass annotated) — the executable target list

### CGB (17) — every row PASSES tier2 → 100% eager re-host debt

| family | rows | class | lever |
|---|---|---|---|
| halt | 5 (`late_m0int_halt_m0stat_scx2/3_3a`, `late_m0irq_halt_dec_scx2/3_2`, `late_m0irq_halt_m0stat_scx3_3b`) | WAKE-CLOCK | port tier2 wake masks (`stat_vis_from_t`/`m0_halt_hold`) to eager (#11cn was tier2-only; #11cu did the halt-ENTRY-rewind, wake masks remain) |
| enable_display | 4 (all DS: `frame0_m0irq_count_scx2/3_ds_1`, `ly0_m0irq_scx0/1_ds_1`) | L1 DS | un-scope `!self.ds` on the eager read arms + DS read-debt (+4hd, done for others) |
| window | 2 (`m2int_wxA5_m0irq_2`, `m2int_wxA6_scx5_m3stat_3`) | read-frame | eager off-screen-window exit arm |
| m2int_m0stat | 1 DS (`m2int_m0stat_ds_2`) | L1 DS | DS read-debt |
| lcd_offset | 1 DS (`offset1_lyc99int_m0stat_count_scx2_ds_1`) | L1 DS / lcd_shift | `lcd_shift_dots` DS half-dot align (parked #11by) |
| irq_precedence | 1 DS (`late_m0irq_retrigger_scx1_ds_2`) | L1 DS | DS read-debt |
| sprites | 1 DS (`10spritesPrLine_wx7_m3stat_ds_2`) | **Part-A (DS `vis_exit_hd` floor)** | emergent DS flip / mid-dot floor (#11da park) |
| vram_m3 | 1 (`postread_scx3_2`) | **Part-A (vis_early case-tower)** | emergent flip deletes `early_lead` tower |
| oam_access | 1 (`postwrite_2_scx3`) | **Part-A (vis_early case-tower)** | ditto |

CGB Part-A residual = **3** (2 SS vis_early + 1 DS floor). The other 14 are
wake-clock (5) + L1 DS (7) + read-frame (2).

### DMG (32) — 28 pass tier2 (re-host), 4 fail tier2 (deeper)

| family | rows | class |
|---|---|---|
| window `late_wy_*_1` | 7 | **L2 DMG window** (eager `wy_trig_sb_raw` un-latch) |
| window `late_disable/reenable/scx` | 5 | L2 DMG window (arm gate-port) |
| window `m2int_wxA5/A6 m0irq/m3stat` | 4 | L2 DMG window (off-screen exit arm) |
| window `m2int_wxA6_{oam,vram}busyread_2` | 2 | **tier2 FAILS — deeper accessibility** (candidate Part-A or un-ported tier2 law) |
| lycEnable | 3 (incl. `lycwirq_trigger_ly00_stat50_2` tier2-fail) | line-frame / engine |
| m2enable | 3 (incl. `late_enable_m0disable_2` tier2-fail) | line-frame / engine |
| ly0 | 2 | line-frame |
| m2int_m0irq | 2 | read-frame IF-delivery |
| m2int_m3stat, enable_display, oam_access, vram_m3, m2int_m3stat/late_scx4 | 5 | line-frame / read-frame |

DMG Part-A candidates = the **2 wxA6 busyread** (tier2 also fails → the whole-dot
render's off-screen-window accessibility grid is wrong; could be render-length OR
an un-ported tier2 DMG accessibility law — needs one more trace to disambiguate).
The other 26 are L2 window (16) + line-frame/engine (10) re-hosts.

### Roll-up — how many of 49 Part-A clears

| bucket | rows | needs half-dot render FSM? |
|---|---:|---|
| eager read-frame / L1 DS / L2 DMG-window / line-frame re-host | **~39** | NO — plain `\|\| eager_value` gate-ports + DS debt, proven vein (578→318 this session) |
| WAKE-CLOCK port (CGB halt) | **5** | NO — port the tier2 wake masks to eager |
| **Part-A emergent-flip (vis_early case-tower + DS floor)** | **3–5** | **YES** — delete `early_lead` tower / DS mid-dot floor via emergent half-dot flip |
| deeper DMG engine (lycEnable/m2enable tier2-fail) | 2 | maybe — un-ported tier2 DMG law, not render-length |

**Part-A (the half-dot render FSM) is the necessary lever for ~3–5 rows, not ~35.**
This is the major de-risking finding: the atomic render+read rewrite the prior
maps scoped is not on the critical path; the bar is separable re-host work.

---

## 5. The half-dot render FSM — executable spec (for the ~3–5 residual, and the shadow-law cleanup)

Part-A is still worth building — it makes the flip emergent and lets the seven
`vis_mode_read` shadow laws + the `early_lead` case-tower be DELETED — but its
JUSTIFICATION is the case-tower-blocked accessibility residual + code-debt
reduction, NOT the bulk of the bar. Build it AFTER the cheap re-hosts (§6).

### 5.1 What advances a whole dot today, and the edit

`ppu/render.rs::render_step` (:434) runs one whole dot per call; the fetcher steps
at 2 dots/read (`FetchPhase` waits), the SCX hunt (`render.rs:452`), the pixel pop
and `advance_lx` all whole-dot. `ppu/engine.rs::tick_half` (:250) ALREADY exists:
the write-strobe half (`dhalf 0→1`) runs `strobe_tick()` under eager (shipped
Part-A write side); the render half (`dhalf 1→0`) runs the whole-dot `tick()`.

**The edit:** split the render body so the mode-3 EXIT resolves at its own
half-dot. Concretely, `m0_flip_events` records `flip_dot = self.dot` (whole).
Make it record a half-dot `flip_hd = 2*self.dot + self.dhalf` from the render's own
position, fired on the `dhalf` where `proj <= lead` first holds — so the flip is a
genuine half-dot event (SameBoy `cfl257 dc6`, odd `dc`). The fetcher/hunt/pop stay
whole-dot (they need no sub-dot; §2 shows the LENGTH is already right); ONLY the
exit-record grain changes. This is a MUCH smaller edit than the §3-plan "convert
the whole FSM to half-dot" — the measurement says the sub-dot precision is needed
only at the exit boundary, nowhere else.

### 5.2 Which of the seven shadow laws collapse (verified against current code)

HALFDOT-BUILD-PLAN §2 claims all seven collapse. **Re-checked at b107806
(`read_laws.rs::vis_exit_hd` arms 1/2/3/4/5/6/7 + arm 8 + arms D1/D6): 0 collapse
on the whole-dot clock, and 0 would collapse against a half-dot BARE flip alone.**
The arms are closed-form WINDOW-LENGTH constants (`259+SCX&7`, `263+SCX&7`, abort
`253`, reenable, un-trigger), and §2's trace shows the whole-dot `flip_dot`==
projection already — so the arms are NOT papering over a flip granularity gap; they
encode window-length models `flip_projection` under-computes for window/abort
lines. Only a half-dot rewrite of the WINDOW/SPRITE FETCH (`window.rs`/`sprite.rs`),
not the bare flip grain, would let them die. **Recommendation: do NOT promise the
shadow-law collapse from the exit-record half-dot edit alone — it needs the fetch
rewrite, which is out of scope for the residual and delivers little (the arms are
correct, just un-collapsed).** The `early_lead` case-tower (`mode0.rs:184-201`) IS
deletable by the emergent flip — that is the residual's actual payoff.

### 5.3 The eager coupling (`read_pos_hd` vs `vis_exit_hd`) — already decomposed

- `read_pos_hd` (engine.rs:288) = `2*dot + dhalf + eager_debt` (SS +8hd / DS +4hd).
  Correct and independent (§3 row 3; #11cq's 1b proved the VALUE half sound).
- `vis_exit_hd` (read_laws.rs:299) is computed from the LIVE render FSM at the peek.
  On the plain eager clock (dispatch NOT moved) it holds fixed across the read
  (§2: pfd invariant 267). The #11cq weld ("vis_exit_hd drags +4 with the
  dispatch") ONLY appears when the dispatch moves (the coupled/halt route) — which
  is REFUTED (dispatch mutual-exclusion, `intr_2_mode0` B=42). **Part-A's job on
  the coupling is to hold `flip_hd` at the render's own half-dot so a FUTURE
  dispatch move would not drag it — but since the dispatch move is off the table
  (deferred-clock = DMG timer wall), this coupling is not on the flip's critical
  path.** The residual rows (§4) need the emergent flip for the case-tower, not for
  a dispatch-move robustness.

### 5.4 Constant table (half-dot frame) — only the exit-record constants move

| constant | file:line | today | half-dot |
|---|---|---|---|
| `flip_dot` record | `mode0.rs:52,244` | `= self.dot` (whole) | `flip_hd = 2*dot + dhalf` |
| `early_lead` tower | `mode0.rs:184-201` | case tower 0–4 | DELETE (emergent) |
| `m0_access_flip` / `m0_stat_flip` / `pal_access_flip` leads | `mode0.rs:280-295`, `advance_lx` | ±8/0 eighths | re-anchor to `flip_hd` |
| arm-8 bare exit | `read_laws.rs:298` | `2*flip + 2 − carry` | `flip_hd + 2 − carry` (identity once `flip_hd` exists) |
| `mode3_entry_dot` 84 (glitch 82) | `mod.rs` | dot 84 | unchanged (entry already back-dated 84→80 in the READ frame; the render entry stays 84) |

The `LINE_DOTS` 456 / fetcher waits / OAM-VRAM locks do NOT move — the length is
right; only the exit-record grain and its dependent access/stat leads move.

### 5.5 Atomicity, refreshed for the eager clock — SEPARATES (does not hold)

§5's `late_scx4` proof: render-length ALONE and read-position ALONE are each an
A/B swap, BOTH required. **On the eager clock at b107806 this is HALF-satisfied
already:** the render half is the whole-dot render, which §1 shows PASSES the
length rows under tier2 (same render code). So the "both required, converge
together" claim reduces to "port the read half onto the eager frame" — which is
exactly the separable slice work done 578→318. `late_scx4_2` itself is on the DMG
bar and PASSES tier2; its eager fail is the read frame. **The atomic coupling is
real but already resolved on the render side — Part-A does not need to co-land
render+read.** This directly overturns the "one atomic landing" framing.

---

## 6. Staging (each sub-step independently gateable + measurable, flag-off byte-identical)

Ordered by value/effort — cheap proven re-hosts first, the FSM last and narrow:

1. **L1 — DS re-host (≈7 CGB rows).** Un-scope `!self.ds` on the eager read arms
   (`blocking.rs`, `mode0.rs:256,280`, `read_laws.rs` DS arms) with the +4hd DS
   debt (already established #11bz). Gate `\|\| eager_value`. Measure EV CGB two-bin;
   expect −7, 0 drops.
2. **L2 — DMG window re-host (≈16 rows).** Port the eager `wy_trig_sb_raw`
   un-latch to DMG (the #11ck CGB slice, DMG-scoped) + gate arms D1/D6 under
   `eager_value` for DMG. `late_wy_*_1` first. Expect −16 DMG, 0 drops.
3. **Wake-clock port (5 CGB halt).** Host the tier2 wake masks
   (`stat_vis_from_t`/`m0_halt_hold`) on `\|\| eager_value` (the halt-entry rewind
   #11cu already went; these are the wake VIEW). Verify `intr_0_timing`/
   `int_hblank` eager tripwires.
4. **Line-frame / engine ports (≈10 DMG).** lycEnable/m2enable/ly0/m2int_m0irq
   read-frame back-dates (the #11cb line-start family, extended).
5. **Part-A emergent flip (the ≈3–5 residual).** Build §5.1 (half-dot exit
   record) + delete the `early_lead` case-tower; recover `vram_m3/postread_scx3_2`,
   `oam_access/postwrite_2_scx3`, the DS sprite floor. Re-measure the 2 DMG wxA6
   busyreads (they may fall to L2, or need this).
6. **The flip (C3):** `lib.rs` `new_inner(…, false)→true`; rebaseline
   `gambatte.txt`; only when all buckets converge ∧ golden regen ∧ zero SameBoy-pass
   drop. Execute `C3-FLIP-CHECKLIST.md`.

Method every slice (never assert — levers overturned ≥15×): `flagon_probe` EV vs
tier2 two-bin + `classify_*`; mooneye 92 flag-off AND `SLOPGB_MOONEYE_EAGER`; eager
tripwires (`intr_2_mode0/mode3/sprites`, `di_timing`, `intr_0_timing`) both models;
golden byte-identical. NEVER drop a SameBoy-pass; NEVER move the dispatch dot (the
coupled/`stat_late` route is thrice-refuted — #11br/#11cq/#11ct).

---

## 7. What NOT to re-chase (adds to the #11cq/#11cr/#11ct lists)

- **"Part-A is a ~35-row atomic render-length rewrite"** — REFUTED: tier2's
  whole-dot render passes 45/49 bar rows; the length is right. Part-A is the
  emergent-flip lever for ~3–5 case-tower-blocked accessibility rows only.
- **`flip_dot ≠ projected_flip_dot` (the 261/267 gap)** — REFUTED: both 267 in
  baseline; 261 is a coupled artifact. Do not inherit either as a live defect.
- **The coupled `stat_late` dispatch route** — thrice-refuted (dispatch
  mutual-exclusion, `intr_2_mode0` B=42). The halt rows are a WAKE re-host, not a
  dispatch move.
- **Promising the 7-shadow-law collapse from the bare-flip half-dot edit** —
  the arms are window-LENGTH closed forms; only a `window.rs`/`sprite.rs` fetch
  rewrite collapses them, and they are already correct, so it delivers little.

## 8. Reproduction

```sh
git checkout finish-port-halfdot   # @ b107806; probes reverted, tree byte-identical
CARGO_TARGET_DIR=target/agProbe cargo test -p slopgb-core --test gbtr --release \
  --features port_probe --no-run
BIN=$(ls -t target/agProbe/release/deps/gbtr-* | grep -v '\.d$' | head -1)
run(){ SLOPGB_ROWLIST=$PWD/scratchpad/$1 SLOPGB_REQUIRE_ROMS=1 env $2 \
       $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture; }
run cgb_rowlist.txt SLOPGB_PROBE_OFF=1   # 486
run cgb_rowlist.txt SLOPGB_PROBE_EV=1    # 318
run dmg_rowlist.txt SLOPGB_PROBE_EV=1    # 74
# TRUE bar: flip-BUGs = EV-fail \ OFF-fail; classify_{cgb_regr,dmg}.py → 17 / 32
# tier2 whole-dot pass rate: run the 49 bar rows with NO probe env (ProbeMode::Reclock)
#   → CGB 17/17, DMG 28/32
# flip_dot dispute: re-add the RDOT probe in m0_flip_events (dot proj lead
#   pfd=dot+proj−lead) + FF41 probe in vis_mode_read; SLOPGB_S5DBG=1 + 1-row rowlist
#   scx_m3_extend_1 → pfd invariant 267 == flip_dot 267.
# SameBoy: bash docs/sameboy-port/tools/build_sameboy_tracers.sh (patches SBMODE);
#   SB_TRACE=1 sameboy_tester --cgb --length 2 <rom> | grep 'SBMODE ly=1' → cfl257 dc6.
```

## 9. Gate state (map only; code REVERTED → byte-identical)

`golden_fingerprint` ok (42.83s, default build — probes env-gated + reverted); tree
`git diff b107806` empty; EV CGB **318** / EV DMG **74**; tier2 bar-rows CGB 17/17,
DMG 28/32; TRUE bar 17 CGB / 32 DMG. No `.rs` touched in the committed tree. The
SameBoy SBMODE tester was (re)built by `build_sameboy_tracers.sh` (patches its own
source cache, not this repo).
