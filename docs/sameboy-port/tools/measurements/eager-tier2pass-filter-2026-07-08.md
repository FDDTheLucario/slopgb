# The "does tier2 pass?" filter — 3 read-frame back-dates SHIPPED, EV 421→400, flip bar 89→70 (2026-07-08, #11cg)

Task: apply the "does tier2 pass?" discriminator to the EV flip-BUG set —
split the flip-would-break rows into **tier2-PASS** (the law exists in the
codebase, a clean `|| eager_value` / read-frame port candidate) vs
**tier2-FAIL** (the HALFDOT Part A floor). Port every clean candidate; prove
the rest floors-in-disguise.

**Result: THREE flag-gated read-frame back-dates SHIPPED (all `eager_value`-scoped,
production + tier2 byte-identical), EV CGB two-bin 421→400, the C3 flip bar
89→70 SameBoy-PASS blockers (19 cleared). The tier2-PASS filter's key LIMIT is
now proven: `tier2 passes` is necessary but NOT sufficient for a clean port — it
does not distinguish "law not gated for EV" (portable) from "tier2's deferred
clock produces different render/dispatch/write-commit state" (floor). The
decisive secondary filter is a per-row trace: a divergence in a read-VERDICT
that samples ARCH state (line/dot/LY/LYC) is portable; a divergence in
render-length / write-commit-dot / dispatch-frame / accessibility-half-dot is a
floor.**

## Baselines (branch `finish-port-halfdot`, `CARGO_TARGET_DIR=target/ag1`, HEAD `70dbb79` at start)

Same BIN for all probes, `SLOPGB_ROWLIST=scratchpad/cgb_rowlist.txt` (3422 rows, 402 skip):

| bin | fail | note |
|---|---:|---|
| tier2 (ON, default) | **291** | the target; UNCHANGED all session |
| CGB OFF (production) | 486 | the golden ratchet reference |
| EV (`SLOPGB_PROBE_EV=1`) start | **421** | |
| EV end (after 3 slices) | **400** | −21 |

## The flip-BUG split (start, EV=421)

| set | count |
|---|---:|
| flip-BUGs (OFF-pass ∩ EV-fail) | 133 |
| — tier2-PASS candidates (clean-port candidates) | **99** |
| — tier2-FAIL (HALFDOT Part A floor) | 34 |
| SameBoy-PASS blockers (the flip bar) | **89** |

## The 3 SHIPPED slices — the clean read-frame vein

All three are the same mechanism: an FF41 STAT byte read under the eager clock
samples cc+0, but the CPU-visible value is the cc+4 one (the +4 SS / +2 DS
read-debt the *mode* bits already take via `read_pos_hd`). A sub-field or a
line-boundary hold that has NO eager adjustment reads the pre-boundary value.
These sample ARCH state (line/dot/LY/LYC), NOT render state — so a whole-dot
back-date on the frame grid reproduces SameBoy's real cc+4 read exactly. All
`eager_value`-scoped → golden + tier2 291 byte-identical by construction.

| # | slice | mechanism | ΔEV | recovered (0 regressions) |
|---|---|---|---:|---|
| 1 | coincidence-bit back-date (`Ppu::read_cmp` / `compare_ly_shift`, `lyc.rs`) | the CGB readable compare switches `L-1`→`L` at line-start dot 4; the cc+0 line-start read saw `L-1` where cc+4 sees `L`. Back-date the FF41 bit-2 coincidence to the debt-shifted dot. | 421→411 (−10) | `lycint_lycflag` ×3, `ly0/lycint152_lyc0flag`/`lyc153flag` ×6 (census-mislabeled "dispatch floor"), `enable_display/frame0_m2stat_count` ×1 |
| 2 | VBlank-entry mode-1 back-date (`vis_mode_read` arm, `read_laws.rs`) | line-144 dots-0-3 mode-0 hold in `vis_mode` is the raw FSM state NO production read observes (all sample cc+4 = VBlank mode 1). Back-date m=0→1. | 411→404 (−7) | `enable_display/frameN_m1stat` ×2, `lcd_offset/offsetN_lyc8fint_m1stat` ×4 (incl DS), `m1/lycint_m1stat` ×1 |
| 3 | line-0 entry mode-2 back-date (`vis_mode_read` arm) | the VBlank→OAM mirror of #2: CGB line-0 dots-0-3 keeps VBlank mode 1 (no mode-0 gap); cc+4 = mode-2 OAM scan. Back-date m=1→2. | 404→400 (−4) | `ly0/lycint152_ly0stat_3` (+ds) ×2, `enable_display/frame1_m2stat_count`, `lcd_offset/offset1_lyc99int_m2stat_count` |

Commits: `72a4977`, `53e9578`, `fad492e` (each signed, golden PASS + tier2 291
+ mooneye 91/91 OFF+ON verified).

## Traces that DECIDED each classification (the secondary filter)

The task's warm-up (`scx_m3_extend`) proved the filter's blind spot immediately:

- **`scx_m3_extend_ds_1` — FLOOR (render-length).** Trace: EV render flipped to
  mode 0 at dot 259 (`m=0 lrd=1 flip=259`) while tier2 held mode 3 at dot 330
  (`m=3 lrd=0`) — the deferred SCX writes keep the fine-scroll hunt open past
  330; the eager writes commit within the M-cycle (`tick_machine` drains the
  staged write's `dots_left` before it can defer past the M-cycle), the hunt
  closes, the render under-extends. Structural — not gate-flippable. The
  parent's "tier2 passes ⇒ port" headline is a FALSE POSITIVE here.
- **`lycint_lycflag_2` — PORT (coincidence read-frame).** Trace: EV's critical
  read at line 6 dot 0 saw `cmp=0` (pre-switch); tier2's deferred read at dot 4
  saw `cmp=1`. Pure cc+0-vs-cc+4 on an ARCH-sampled sub-field → slice 1.
- **`window/late_reenable_2` — FLOOR (write-commit-dot).** Trace: the reenable
  arm's `win_reenable_dot` = 94 under EV vs 96 under tier2 (the LCDC.5 write
  commits at a different render dot per the eager-vs-deferred clock); arm 5
  fires at 96 (bare exit 506) but not at 94 (over-extends, exit 518). Same
  M-cycle-drain root cause as scx.
- **`m0enable/lycdisable_ff41_2` — FLOOR (dispatch loop-timing).** Trace: BOTH
  clocks read mode 2 at line 0 dot 16, yet land different OCR values — the OCR
  captures a different loop read (dispatch cc+4 vs deferred frame). The reads
  are correct; the frame that reaches the store differs.

## The end state — EV=400, flip bar 70 (all 70 tier2-PASS)

Classified the 112 flip-BUGs (OFF-pass ∩ EV-fail at EV=400) against SameBoy
1.0.2 (`classify_cgb_regr.py`, `--cgb --length 4`): **BUG(SameBoy-PASS)=70,
FLOOR(SameBoy-FAIL rebaseline)=42, UNK=0.** ALL 70 blockers are tier2-PASS
(tier2-FAIL blockers = 0) — i.e. every remaining blocker HAS the law in the
codebase but fails EV on a non-gate difference. The 70 by family + verdict:

| family | blk | verdict (floor mechanism) |
|---|---:|---|
| window | 16 | **render / write-commit-dot floor** (reen/wx/scx-write dot differs eager↔deferred; traced) |
| enable_display | 8 | dispatch loop-timing floor (mode-0/2 count E0/E2, counter-pinned) |
| m2int_m0irq | 5 | IF-write dispatch floor (`_ifw`) |
| lycEnable | 5 | mode-read dispatch/loop-timing floor |
| halt | 5 | halt-wake clock (dispatch-adjacent multi-mechanism port; line-start back-date OVER-fires post-wake) |
| vram_m3 | 4 | `vis_early` accessibility half-dot floor (#11cb REFUTED the gate-flip) |
| irq_precedence | 4 | `late_m0irq_retrigger` dispatch (E0/E2) floor |
| cgbpal_m3 | 4 | palette-RAM accessibility / render read-frame (scx `_2` sub-dot) floor |
| oam_access | 3 | `vis_early` accessibility half-dot floor |
| m2int_m3stat | 2 | `late_scx4` render floor (write-strobe) |
| m2int_m2stat | 2 | DS mid-dot floor |
| m0enable | 2 | mode-read dispatch floor (traced) |
| ly0 | 2 | dispatch straddle (`lyc153irq` E0/E2, opposite wants) |
| lcd_offset | 2 | STOP-shift dispatch floor |
| sprites / scx_during_m3 / miscmstatirq / m2int_m0stat / m2enable / lyc153int_m2irq | 1 ea | DS mid-dot / render / dispatch floor |

## VERDICT — the clean read-frame vein is EXHAUSTED; the residual 70 is the half-dot/deferred-clock floor

The three ARCH-sampled line-boundary back-dates (coincidence, VBlank entry,
line-0 entry) were the whole clean-port vein: they adjust a read's cc+0→cc+4
frame on a field the eager clock left un-shifted, reproducible whole-dot. Every
remaining SameBoy-PASS blocker fails EV on a difference the eager whole-dot
clock **structurally cannot** match to the deferred clock:

- **render-length / write-commit-dot** (window, m2int_m3stat, scx, cgbpal): the
  eager `tick_machine` drains a staged write within its M-cycle, so a mid-mode-3
  register commit lands 2-4 dots earlier than the deferred frame → the
  fine-scroll hunt / window arms resolve to a different `flip_dot`/`reen`/`wxm`.
  A uniform render-dot offset is the #11bw-refuted lever (needs the half-dot).
- **dispatch loop-timing** (enable_display, m2int_m0irq `_ifw`, lycEnable,
  m0enable, irq_precedence, ly0, lcd_offset): the reads are correct but the OCR
  captures a different loop iteration — moving the dispatch is forbidden (breaks
  `intr_2`, #11br).
- **`vis_early` accessibility half-dot** (vram_m3, oam_access): #11cb REFUTED the
  gate-flip.
- **halt-wake clock** (halt): dispatch-adjacent multi-mechanism port; the
  line-start back-date over-fires post-wake (a different read frame).

**K ≈ 70, all half-dot/deferred-clock/dispatch floor.** The C3 flip stays gated
SOLELY on the coherent per-T half-dot retime (HALFDOT Part A) + the deferred
write clock — confirming #11bw/#11cf. This session peeled the last whole-dot
read-frame recoveries (89→70); no further whole-dot lever moves EV.

## Reproduction

```
CARGO_TARGET_DIR=target/ag1 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/ag1/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # pass=2620 fail=400
# flip bar:
grep '^FAIL' ev.txt|awk '{print $2}'|sort -u > ev_f; comm -13 off_f ev_f | sed 's# \[Cgb\]##' > flipbugs
python3 docs/sameboy-port/tools/classify_cgb_regr.py flipbugs   # BUG=70 FLOOR=42 UNK=0
```

Golden-safe verified after every slice: `golden_fingerprint --release` PASS;
tier2 two-bin 291; mooneye `acceptance_ppu` OFF + `SLOPGB_MOONEYE_EAGER=1` ON
both 91/91 (only `lcdon_timing-GS`, pre-existing exemption); clippy
`-D warnings` clean; all `.rs` < 1000 lines.
