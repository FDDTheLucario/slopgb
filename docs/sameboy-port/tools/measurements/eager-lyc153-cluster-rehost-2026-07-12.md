# eager line-153 LYC=153 IF-emission decouple + the FULL LYC-153 sibling-cluster re-host — m1statwirq LANDED, ALL 13 siblings re-hosted, ZERO net drops (2026-07-12)

Base: `finish-port-halfdot @ 339b4f9` (isolated worktree; no push, no default
flip — every change `eager_value`-gated (DMG-family for all but the ff0f CGB-keep
scope), production + tier2 byte-identical). Builds on the
`eager-pert-interleave-poc-2026-07-12.md` PoC.

## Verdict (final): LANDED — m1statwirq_3 `0→2` + ALL 13 PoC drops re-hosted, DMG EV 46→41 with ZERO SameBoy-pass drops, CGB EV 287/287. Six mechanisms, six representable discriminators. The "CPU-T-atomic floor" premise is REFUTED end-to-end: every one of the 13 drops — window, ack-squash retrigger, FF0F co-instant, LYC-write compare-wrap — was a STALE dot-8-frame downstream compensation that the dot-4 emission left 4 dots mis-framed, NOT a counter-pinned CPU dispatch phase. Even the two rows first (wrongly) read as "PPU-identical, no discriminator" fell to a re-host once the actual downstream latch/window/mask was traced.

## The two-stage history

The first commit (87d6ff2) shipped mechanisms 1-3 (dot-4 emission + the two
window re-hosts) → 9/13, 4 residual. The coordinator's BATTERY-CLEAN re-attack
then cracked the last 4 (mechanisms 4-6), each the same shape as the window
re-host: a downstream constant the dot-4 emission left stale. **Lesson (again):
never concede "PPU-identical / counter-pinned" from a dispatch trace alone —
`lyc153int_m2irq_late_retrigger_2` and `lyc153int_m2irq_2` both showed
byte-identical dispatch streams yet BOTH had a representable stale downstream
window/mask.**

## What shipped (three flag-gated mechanisms, all `eager_value && !is_cgb`)

### 1. The dot-4 LYC=153 IF-emission decouple (`stat_irq/reclock.rs`)
The DMG `ly_for_comparison` line-153 table (`GB_SLEEP(14,4)`, pinned by wilbertpol
`ly_lyc_153-C`) sets 153 only at slopgb **dot 6**, so the eager `stat_update`
engine's natural LYC 0→1 rise fires at dot 6 (the READ frame, cc+4 = +2 read-debt).
SameBoy sets `IF |= 2` at `display_cycles == 4` (traced `SBIF su ly=153 dc=4`), the
DISPATCH frame; the dot-6 fold lands mid-M-cycle → the eager CPU recognises it one
M-cycle late → `m1statwirq_3`'s ISR fixed-cycle wait carries the offset to the FF41
glitch write (`0`, want `2`). Emit the IF at dot 4 via `pending_if |= IF_STAT` +
`force_level(true)` (the C015 disable-direction template), leaving the
`ly_for_comparison`/`refresh_cmp` register-read latch at dot 6 — a two-latch split,
NOT a dispatch move (mooneye `intr_2_*` incl `_sprites`, `di_timing`, `int_hblank`,
`ie_push`, `rapid_di_ei` all green under eager). **`m1statwirq_3` `0→2`.**

Isolated, this drops 13 SameBoy-pass siblings (the PoC's −5/+13): the shared LYC=153
ISR — and every ISR-timed WY write / mode-2 event it schedules — now fires 4 dots
(1 M-cycle SS) EARLIER, tipping the downstream compensations calibrated for the old
dot-6/dot-8 recognition. Mechanisms 2+3 re-host the WINDOW subset.

### 2. `win_extends_sb` deadline re-derivation (`stat_irq/read_laws_exit.rs`)
The mid-line late-WY shadow-extend (Arm 2) fires when `wy_trig_sb_dot <=
wx_match_dot + 2`. The dot-4 emission moves each ISR-timed WY write — and its
`wy_trig_sb_dot` — 4 dots earlier (`FFto2_ly2_3` latch 102→98). The stale `+2`
deadline (wxm 97 → 99) then extends BOTH `_2` (94) and `_3` (98) where SameBoy
renders `_3` bare. Re-derive to `−2` (wxm → 95): `_2` (94 ≤ 95, extend) / `_3`
(98 > 95, bare) re-split — the SS twin of the DS lyfc wake re-derivation already
documented in `win_extends_sb` (+4→+2). Recovers the mid-line `_3` family
(`FFto2_ly2_3`, `scx2/scx3_3`, `wx0f_3`, `10to1_ly1_3`).

### 3. `wy_xline_trig` classification shift (`ppu/regs.rs`)
The boundary/head-WY cross-line latch (`wy_xline_trig` → Arm 7) classifies a WY
write by its commit dot vs the tail/head boundaries (`dot >= 452 || dot < 4`).
The dot-4 emission moves a boundary write from `ly N dot 4` (base: past the head →
bare) to `ly N dot 0` (inside the head → spurious cross-line extend). Re-map the
classify dot by the +4 read-debt (`xdot = dot + 4`): `FFto0_ly2_3` ly1-dot0 → xdot 4
(NOT head → bare); its `_2` ly0-dot452 → xdot 456 (still tail → extend). Recovers
the cross-line/head family (`FFto0_ly2_3`, `FFto1_ly2_3`, `10to0_ly1_3`).

## The decisive traces (rom-diff-weld step 1b/2)

- **Window `_3` fail = the ISR-carried WY write moved 4 dots earlier**, tipping a
  render-state discriminator. `FFto2_ly2_3`: WY write ly2 dot 100→96,
  `wy_trig_sb_dot` 102→98, `visexit` 251→259 — the RENDER exit is bit-identical
  base vs part1; only `wy_trig_sb_dot` (Arm 2 / `win_extends_sb`) or the head/tail
  class (`FFto0` xline) moved. REPRESENTABLE → re-hostable. (The earlier "read moved
  under the exit" read was corrected: at the OCR frame `fc=3`, `vis_exit_hd` returns
  526 = the shadow-extend arm, the discriminator is the WY-write dot, not the read.)
## The last four re-hosts (mechanisms 4-6 + the WX=0 render edge)

### 4. WX=0 co-incident-trigger BARE exit — Arm D-wx0 (`read_laws_exit.rs`)
`late_wy_FFto2_ly2_wx00_3` (want 0). Unlike the wx>0 `_3` rows (render goes bare,
rides `win_extends_sb`), a WX=0 window's WX comparator matches during the 8-dot
PREFILL, so slopgb's whole-dot render ACTIVATES the instant `wy2 == ly` is caught
— even AT the match dot (`wytrig 90 == wxmatch 90`). SameBoy's mode-2 `wy_check`
samples ~2 dots before the match, so a co-incident wy2 does NOT trigger → BARE. Arm
D-wx0 forces the bare exit when `win_active && wy2==ly && !win_extends_sb() && wx<7
&& scx&7==0`. The `scx&7==0` scope is load-bearing: `late_scx_late_wy_*_wx00_2`
(scx7=4, mid-line SCX rewrite) has the IDENTICAL render state (wytrig 90 == wxmatch
90) but its fine scroll legitimately extends → wants out3, so a nonzero SCX&7 must
NOT bare. (Traced: base wx00_3 read dot 256/rph 520 sees native mode 0 but
`vis_hold_until`=263 overrides → exit 526; the arm folds bare 502 → read 520 ≥ 502
→ mode 0.)

### 5. Line-153 retrigger ack-squash widen 6→10 (`interconnect/speed.rs`)
`lyc153int_m2irq_late_retrigger_2` (want 0). NOT PPU-identical — the dispatch trace
LOOKS identical but the ack DOT differs: `_1` ack ly153 dot 448, `_2` dot 452 (the
sibling NOP count). The eager SS STAT ack-squash window `6` counts from the ack; the
dot-4 emission fired this line-153 ISR (and its ack) 4 dots earlier, growing the
ack→retrigger gap (to the ly0 mode-2 pulse at ack+8 for `_2`) OUTSIDE window 6 →
wrongly DELIVERED. Widen the LINE-153 SS window by the read-debt (6→10): `_2` (gap 8
≤ 10) re-squashes to E0 while `_1` (gap 12 > 10) still delivers E2. `!is_cgb &&
line_dot().0 == 153`-scoped (the `late_m0irq_retrigger` es=08 family keeps 6). Bonus:
also recovers `lycint152_lyc0irq_late_retrigger_2`.

### 6a. DMG ly0 dot-4 OAM co-instant mask disable (`stat_irq/ff0f.rs`)
`lyc153int_m2irq_2` (want 2). Again NOT PPU-identical in the way it first read: the
`ff0f_ly0_pulse_mask` (`line0 dot4 lyc153`) MASKS the OAM pulse for the LYC-153 ISR
read. #11ee tuned it for the pre-#11cu eager frame (`_1` read at dot 4, want mask);
the dot-4 emission moved the LYC-153 reads a full M-cycle earlier — `_1` to dot 0
(BEFORE the pulse → naturally clear, no mask) and `_2` to dot 4 (co-instant with
the pulse → must SEE it). Disable the mask under eager DMG so `_2` reads E2; `_1`
(dot 0) is unaffected. CGB KEEPS the mask (its read frame is unmoved — a blanket
eager disable regressed CGB EV 287→288; the `is_cgb ||` re-scope restored it).

### 6b. (0,4) LYC-write compare-wrap un-block re-enable (`ppu/lyc.rs`)
`lycwirq_trigger_ly00_stat50_3` (want E2). The DMG LYC-write retrigger's `their_line`
boundary (`dot < 8` ⇒ prev/VBLANK branch) and the (0,4) un-block exception. #11ee
DISABLED the (0,4) exception under eager because there `_3` fired at dot 8 (VISIBLE
branch) and `_2` (want block) sat at dot 4. The dot-4 emission moves the ly0 LYC
write another M-cycle earlier: `_3` dot 8→4 (back onto the (0,4) compare-wrap cell,
VBLANK branch → wrongly blocked) and `_2` dot 4→0 (still blocked). Re-enable the
(0,4) exception for eager: `_3` fires at dot 4 (E2) while `_1`/`_2` (dot 0) stay
blocked. The seamless-handoff `force_level(true)` already covers `_2`'s dot-0 block.

## Gates (all hold; DMG + CGB two-bins run TWICE, identical both runs)

- `m1statwirq_3` eager **0→2** ✓; all 13 PoC-drop siblings PASS under eager
  (pin `eager_dmg_lyc153_cluster_passes`, 13 rows red-before-green).
- `golden_fingerprint` byte-identical defaults-OFF ✓ (`eager_value`-gated).
- mooneye **93/93 ×3** (OFF / `SLOPGB_MOONEYE_EAGER=1` / `SLOPGB_MOONEYE_RECLOCK=1`)
  ✓; every tripwire green under eager (`intr_2_*` incl `_sprites`, `di_timing`,
  `int_hblank`, `ie_push`, `rapid_di_ei`).
- flagon_probe EV two-bin: CGB **287 → 287** ✓ ×2; DMG **46 → 41** ✓ ×2 —
  **ZERO base-pass→fail drops**; 5 base-fail→pass recovered (m1statwirq_3,
  lyc153int_m2irq_ifw_1, late_wy_1 ×2, lycint152_lyc0irq_late_retrigger_2). All 4
  recovered residual classify **BUG** (SameBoy-pass, `classify_dmg.py`).
- tier2 unchanged (`eager_value` ≠ `tier2_reclock`); clippy `-D warnings` clean;
  no `.rs` > 1000.

## The recurring lesson

Every one of the 13 was a **stale downstream compensation** left 4 dots mis-framed
by the dot-4 emission — window exit deadline, xline classify dot, ack-squash window,
FF0F co-instant mask, LYC-write compare-wrap cell — each a whole-M-cycle shift of a
REPRESENTABLE latch/window, not a sub-M-cycle CPU weld. The two rows first read as
"PPU-identical / counter-pinned" from a dispatch trace were the trap: the dispatch
stream WAS identical, but the ack DOT / the read DOT / the write DOT carried the
discriminator. rom-diff-weld holds: trace the WRITE/ack/read dot, never trust
"identical dispatch ⇒ no discriminator." scx5_2 (a pre-existing base-fail the +2→−2
`win_extends_sb` reverts) stays base-fail — NOT a drop; its recovery needs an
scx-aware slack, parked.
