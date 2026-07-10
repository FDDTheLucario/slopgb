# HALFDOT Part A-render — the independent half-dot mode-3 exit: one clean read-frame slice SHIPPED (EV CGB 361→359, 0 drops); the coupled landing REFUTED — the OAM dispatch move is a broad m2int-ISR re-timing, not a length shuffle, and the whole-dot render's flip DISAGREES with the read-frame projection (2026-07-10, #11cr)

Task: make the mode-3 exit resolve to its OWN half-dot from the render FSM's own
position — independent of BOTH the CPU dispatch dot AND the read dot — so
`vis_exit_hd` holds fixed while the OAM dispatch moves (#11cq's coupled landing),
letting the `_1`/`_2` length pairs survive. Then delete the seven `vis_mode_read`
shadow laws it subsumes. Behind a sub-flag active only under `eager_value`.

## TL;DR verdict

- **SHIPPED (clean, converges):** a verdict-only decouple of the FF41 mode-3→0
  read from the peek-time native mode — when a window's `m == 3` length arm is the
  true exit but the read peek crossed the native flip (`vis_exit_hd(native_m) ==
  None`), retry with a forced mode-3 view so the length arm fires. **EV CGB
  361→359 (+2, ZERO SameBoy-pass drops), EV DMG 92 neutral.** `eager_value`-gated;
  golden byte-identical, tier2 291, mooneye 92 OFF+eager, all eager tripwires
  green. This subsumes the native-mode FALLBACK (the implicit 8th "law"); the seven
  enumerated shadow laws survive (still load-bearing).
- **REFUTED (map only, code NOT merged — Part-C):** #11cq's coupled landing
  (1a OAM `stat_late` + 1b OAM-ISR read-debt drop). Even with the exit-decouple
  holding the length, EV+RHD+COUPLED = **428/436** (vs baseline 361), **105 CGB
  SameBoy-pass drops**, and `intr_2_mode0_timing` still **FAILS B=42**. The task's
  Step-4 hypothesis — "blocker (B) intr_2_mode0 is a SYMPTOM of blocker (A), the
  length shuffle" — is **REFUTED**. See §4.
- **Root cause of the residual (traced, decisive):** the whole-dot render's
  recorded flip `flip_dot` and the read-frame projection `projected_flip_dot()`
  **DISAGREE by 6 dots** for extend/window lines (`scx_m3_extend`: recorded 261 →
  exit 520, projection 267 → exit 532; the reads need 532). Baseline passes only
  because `_1`/`_2` read the SAME side of the flip; a dispatch move that straddles
  that 12-hd gap is NOT held by any verdict-side lever. **A frame-independent exit
  requires the half-dot render FSM (Step 2), which was not built this session** —
  the focused Step-1+Step-4 refutation made the large rewrite unwarranted here. §5.

Baselines verified independently @ `42c54f6`: EV CGB **361** / EV DMG **92** /
tier2 CGB **291** / OFF 486·103; golden 9020 match; mooneye 92 flag-off.

## Step 1 — ground truth (the decisive traces, NOT re-derived)

Single-row `flagon_probe` + `port_probe` trace at the eager FF41 read
(`Bus::read` → `leading_edge_sample`), dumping `read_pos_hd` (rp), `vis_exit_hd`,
the render's recorded (`fdrec`=`flip_dot`) and projected (`fdproj`=
`projected_flip_dot`) flip, and the live render flags the arms gate on. SameBoy
side: `sameboy_tester --cgb SB_TRACE=1`, SBMODE `vis 3→0` on the read's line.

### 1a — `scx_m3_extend` [Cgb] (bare/SCX, scx&7=5) — the sharpest case

SameBoy mode-3→0 exit (identical `_1`/`_2`, read-independent): **cfl257 dc6**
(= slopgb-frame rp 520). slopgb, EV baseline (both PASS):

| leg | want | read dot | native m | vmr | rp | `vis_exit_hd` | fdproj | fdrec | lrd |
|---|---|---|---|---|---|---|---|---|---|
| `_1` | 3 | 260 | 3 | 3 | 528 | Some(532) | 267 | 0 | false |
| `_2` | 0 | 264 | 0 | 0 | 536 | Some(532) | 267 | 0 | false |

Both legs read PRE-flip (lrd=false, fdproj=267), both exit=**532** (arm 8 bare
`2·fdproj+2−carry`), rp 528/536 straddle 532 → 3/0 correct. **The exit IS
peek-independent here (fdproj constant 260↔264).**

### 1b — `m2int_wxA6_m3stat` [Cgb] triple (off-screen window, wx=A6)

| leg | want | read dot | native m | vmr | rp | `vis_exit_hd` | note |
|---|---|---|---|---|---|---|---|
| `_1` | 3 | 248 | 3 | 3 | 504 | **None** | native-m fallback; no arm (win_active, pre-abort) |
| `_2` | 3 | 252 | 3 | 3 | 512 | **None** | native-m fallback |
| `_3` | 0 | 256 | 3 | 0 | 520 | Some(518) | win_aborted → arm fires |

`_1`/`_2` get 3 from the **native mode** (no arm fires on the off-screen
pre-abort window). This is the fragile class: a peek move past the native flip →
native 0 → wrong.

### 1c — `m2int_wx03_scx5_m3stat_1` [Cgb] (on-screen window) — baseline vs coupled

| run | read dot | native m | vmr | rp | `vis_exit_hd` | fdrec | lrd | wa |
|---|---|---|---|---|---|---|---|---|
| EV baseline (PASS 3) | 256 | 3 | 3 | 520 | Some(528) | 0 | false | true |
| EV+RHD+COUPLED (FAIL, want 3 got 0) | **260** | **0** | 0 | 520 | **None** | 0 | false | true |

The coupled dispatch move drags the read peek 256→260; rp HELD at 520 (1b works);
but at dot 260 native m=0 (`win_active` line drops the visible mode WITHOUT setting
`line_render_done`), arm 1's `m == 3` guard fails → exit=None → native 0. **This is
the shipped fix's target: the retry re-fires arm 1.**

## Step 2 — half-dot render FSM: NOT built (deliberately, see §5/§6)

`render_step` still advances one whole dot. Building the half-dot pixel FSM is the
large rewrite the plan reserves; the Step-1 ground truth + the focused Step-4
coupled measure refuted the central hypothesis before the rewrite was warranted.
§5 pins exactly what a future half-dot FSM must achieve (make `flip_dot` ==
`projected_flip_dot` == SameBoy's true half-dot exit).

## Step 3 — shadow-law subsumption tally

The shipped decouple SUBSUMES the **native-mode fallback** (the implicit 8th
"law" — reads that land no arm and rely on `vis_mode()`): the `wxA6`-class
off-arm window ISR reads (`m2int_wx03/07_scx5_m3stat_ds_1`, +2). Of the **seven**
enumerated shadow laws (`HALFDOT-BUILD-PLAN.md` §2): **0 die**. They are the
closed-form exit constants (`259+SCX&7`, `263+SCX&7`, abort `253`, reenable, …),
and §5 shows why they cannot be deleted on the whole-dot clock — the render's own
`flip_dot` gives the WRONG read-frame exit (520 vs the needed 532 on
`scx_m3_extend`), so the closed forms are still doing real work the emergent
whole-dot flip cannot. A law you can delete is the proof the length is right; none
were deletable ⇒ the whole-dot length is still wrong. That is the finding.

## Step 4 — the coupled landing re-run (the central experiment) — REFUTED

Re-added #11cq's 1a (`stat_update_halt_masks`: OAM line-start pulse also sets
`stat_late`) + 1b (`read_pos_hd`: drop the eager debt for `read_carried &&
stat_rise_oam`) under a `coupled_landing` sub-flag, on top of the exit-decouple.

| config | EV CGB | vs baseline 361 | intr_2_mode0 |
|---|---:|---:|---|
| EV baseline | 361 | — | PASS B=03 |
| EV + decouple (shipped) | **359** | +2 clean | PASS B=03 |
| EV + COUPLED (no decouple, #11cq control) | 438 | −77 | FAIL B=42 |
| EV + decouple + COUPLED | **428** | −67 (105 regr / 38 recov) | **FAIL B=42** |

The decouple recovers ~10 window `_1` legs under coupled (438→428) but **105
SameBoy-pass rows still drop**, spanning the WHOLE mode-2-interrupt (OAM) ISR
family — NOT just length:

```
 31 window        15 m2int_m0irq   10 oam_access    9 m2enable
  9 cgbpal_m3      7 vram_m3         7 halt          6 m2int_m2irq
  3 lycm2int       2 scx_during_m3   2 m2int_m3stat  2 dma  …
```

`oam_access`/`vram_m3`/`cgbpal_m3` are ACCESSIBILITY reads; `m2int_m0irq`/
`m2int_m2irq` are IRQ-DELIVERY timing; the rest are FF41-mode. **1a re-times the
actual mode-2 interrupt DISPATCH**, which shifts every ISR read through FOUR
independent channels (FF41-mode, OAM/VRAM/palette accessibility, IRQ delivery, the
mooneye dispatch kernel). The exit-decouple addresses ONLY the FF41-mode channel.

**intr_2_mode0 B=42 is a REAL, independent blocker — not a symptom of the length
shuffle.** `intr_2_mode0_timing` measures the dispatch POSITION directly (cycle
count), which a verdict-only length hold cannot move; 1a genuinely moves the
dispatch, so it fails regardless of the length. This is #11cp's mutually-exclusive
demand confirmed once more: the halt ENTRY needs the late (production) dispatch,
the running CPU needs the eager (cc+0) one, and `stat_late` (an OAM pulse) cannot
separate them (both fold at cc4).

## 5 — Root cause of the residual: projection ≠ recorded flip (the feasibility finding)

`scx_m3_extend_1` under coupled (want 3, got 0):

| run | read dot | native m | rp | `vis_exit_hd` | fdproj | fdrec | lrd |
|---|---|---|---|---|---|---|---|
| EV baseline | 260 | 3 | 528 | Some(**532**) | 267 | 0 | false |
| EV+decouple+COUPLED | **264** | 0 | 528 | Some(**520**) | 0 | **261** | true |

The coupled move pushes the read from dot 260 (PRE-flip) to 264 (POST-flip). And
the two flip estimates DISAGREE: pre-flip arm-8 uses `fdproj=267` → exit 532;
post-flip it uses the recorded `fdrec=261` → exit **520**. `528 < 532` (3) but
`528 ≥ 520` (0) — a **12-hd verdict discontinuity at the flip**. SameBoy's true
exit is cfl257·2+6 = slopgb-frame 520, but the eager read-frame the reads live in
needs 532, so **neither the recorded whole-dot flip (physically 261) nor a naive
closed form gives a single peek-independent exit** — the whole-dot render simply
does not carry the sub-dot the reads straddle. Baseline PASSES only because both
`_1`/`_2` sit on the same side of the flip; the dispatch move separates them across
the discontinuity.

**This is the port's remaining floor, now pinned precisely:** holding
`vis_exit_hd` fixed under a dispatch move requires `flip_dot ==
projected_flip_dot == SameBoy's true half-dot exit` — i.e. the render FSM must
resolve its mode-3 exit at its OWN half-dot so recording and projection coincide.
A whole-dot render cannot (the two differ by up to 6 dots on extend/window lines).
Verdict-side levers (freeze the projection, force m=3, record the exit) each
band-aid one class but cannot reconcile a 6-dot physical/read-frame gap, and NONE
of them touch the accessibility/IRQ/dispatch channels 1a also breaks.

## 6 — Feasibility of the flip, plainly

The single remaining lever (independent half-dot render length) is **necessary but
provably insufficient on its own** to unblock the flip via #11cq's route:

1. Even a perfect frame-independent length does NOT make 1a's dispatch move
   harmless — 1a breaks 105 rows through accessibility/IRQ/dispatch channels a
   length fix cannot reach, and fails intr_2_mode0 (a direct dispatch-timing
   measurement) by construction.
2. So the halt rows that motivate the coupled landing are blocked on the
   **halt-entry/dispatch reclock** (#11br/#11bs/#11cl/#11cn — all refuted), NOT on
   the render length. #11cq's "the single remaining lever is the render length"
   read is **narrowed**: the render length is a real prerequisite for the length
   pairs, but the halt/kernel rows sit behind the (thrice-refuted) dispatch retime,
   which the eager clock cannot host without the coherent per-T retime that hangs
   mooneye.
3. A future half-dot render FSM (Step 2) should still be built — it makes
   `flip_dot == projected == true half-dot` so the length pairs stop being
   accidental (fixing `scx_m3_extend`-class discontinuities and letting the seven
   shadow laws collapse) — but it will NOT, alone, deliver the halt/kernel rows.
   The flip is gated on BOTH the half-dot render AND a coherent dispatch retime,
   and the latter remains the un-cracked wall.

## What SHIPPED (converges — code merged)

`ppu/stat_irq/read_laws.rs::vis_mode_read` (+37/−1, verdict-only, `eager_value`-
gated): when `vis_exit_hd(native_m)` is `None` on a visible non-glitch line in the
mode-3 regime (native 0, dot ≥ 84, render active-or-flipped), retry
`vis_exit_hd(3)` so a window `m == 3` length arm fires — decoupling the verdict
from the peek native mode where no arm otherwise fires. The `m == 0` HOLD arms
(arm 2/7 boundary-WY, arm D6) return `Some` on the native call and are untouched.

Gates (all green): golden 9020 byte-identical; tier2 CGB **291**; EV CGB **359**
(+2, 0 drops), EV DMG **92** (neutral); mooneye **92** flag-off AND
`SLOPGB_MOONEYE_EAGER`; eager tripwires both models — intr_2_mode0/mode3/sprites,
intr_2_0_timing, di_timing, int_hblank_incs (PASS), wilbertpol intr_0_timing
(B=03 C=05 D=08 E=0D H=15 L=22); lib 758; clippy (default+port_probe) clean;
read_laws.rs 984 < 1000.

## REFUTED — do NOT re-chase (adds to #11cq's list)

- **The coupled landing (1a `stat_late` + 1b debt-drop) as a flip route** — a broad
  m2int-ISR dispatch re-timing (105 drops via 4 channels), fails intr_2_mode0 by
  direct dispatch measurement. Verdict-side length holds recover only ~10 window
  legs. Code reverted (Part-C).
- **Any verdict-only exit fix (freeze projection / force m=3 / record exit) as a
  substitute for the half-dot render FSM** — cannot reconcile the 6-dot
  physical(`flip_dot`)↔read-frame(`projected`) gap, and cannot touch the
  accessibility/IRQ/dispatch channels.

## Reproduction

```sh
git checkout halfdot-render-A   # the shipped decouple @ read_laws.rs
CARGO_TARGET_DIR=target/agR2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agR2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# shipped baseline (env unset; decouple is always-on under eager):
SLOPGB_ROWLIST=$PWD/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 SLOPGB_REQUIRE_ROMS=1 \
  $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 359
# The refuted coupled landing (re-add to reproduce 428): sub-flag on
#   eager_value && SLOPGB_COUPLED; 1a `self.stat_late=true` in the OAM line!=0
#   branch of stat_update_halt_masks; 1b `return base` in read_pos_hd when
#   read_carried && stat_rise_oam. intr_2_mode0 → B=42.
# Ground-truth FF41 trace (port_probe): eprintln self.ppu.probe_ff41() after
#   leading_edge_sample in Bus::read; probe_ff41 dumps rp / vis_exit_hd /
#   projected_flip_dot / flip_dot / render flags. Run one-row rowlist + SLOPGB_S5DBG=1.
# SameBoy: SB_TRACE=1 sameboy_tester --cgb --length 2 <rom> | grep 'SBMODE ly=1'
```
