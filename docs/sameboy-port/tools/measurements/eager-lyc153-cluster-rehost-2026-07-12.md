# eager line-153 LYC=153 IF-emission decouple + the LYC-153 sibling-cluster re-host — m1statwirq LANDED, 9/13 siblings re-hosted, 4 residual (2026-07-12)

Base: `finish-port-halfdot @ 339b4f9` (isolated worktree; no push, no default
flip — every change `eager_value && !is_cgb()`-gated, production + tier2
byte-identical). Builds on the `eager-pert-interleave-poc-2026-07-12.md` PoC.

## Verdict: PARTIAL — m1statwirq_3 `0→2` SHIPPED, the window sibling cluster (9/13) RE-HOSTED clean, 4 residual (1 WX=0 render edge + 3 PPU-identical dispatch-frame IF rows). The "CPU-T-atomic floor" premise stays REFUTED; the residual is a bounded, well-characterised floor, not a wall.

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
- **IF-delivery `_3` fail = NO PPU discriminator.** `lyc153int_m2irq_2`,
  `_late_retrigger_2`, `lycwirq_trigger_ly00_stat50_3`: the full dispatch trace
  (`dispatch`/FF0F/FF41 stream) is **byte-identical** between the passing `_2`/`_1`
  and the dropped sibling. The PPU IF-engine emits the same LYC=153 rise to the same
  handler entry; only the CPU's post-dispatch NOP count differs, and both siblings
  read the same identical engine state. This is the genuine counter-pinned
  dispatch/read-frame residual (confirming the PoC's read for THESE rows) — no
  representable latch separates fix from drop at the PPU level.

## Residual (4 SameBoy-pass rows, all classify BUG)

| row | want | family | why not re-hosted |
|---|---|---|---|
| `late_wy_FFto2_ly2_wx00_3` | 0 | window WX=0 | render ACTIVATES both `_2`/`_3` (`wx_match_dot` unrecorded for WX=0 → `win_extends_sb` deadline lever unavailable); the extend is pure `vis_hold_until` render-length, discriminator `wy_trig_sb_dot` vs the WX=0 prefill-match dot is not currently latched |
| `lyc153int_m2irq_2` | 2 | mode-2 IRQ | PPU-identical sibling pair (no discriminator) |
| `lyc153int_m2irq_late_retrigger_2` | 0 | retrigger | PPU-identical sibling pair |
| `lycwirq_trigger_ly00_stat50_3` | E2 | LYC-write retrigger | PPU-identical sibling pair |

The scx-fine-scroll interaction is non-uniform: `win_extends_sb`'s `−2` is correct
for scx0/2/3/wx0f (the mid-line `_3` bare split) but over-corrects
`late_wy_FFto2_ly2_scx5_2` (a base-fail the dot-4 emission had bonus-recovered at
`+2`; at wytrig 98 / wxm 97 scx0 wants bare while scx5 wants extend — same
latch/dot, opposite want, split only by scx7 and the render-activation flip the
fine-scroll induces). `−2` is the net-optimal simple choice (restores 6 base-pass
`_3` rows; scx5_2 reverts to its base-fail, NOT a new drop).

## Gates (all hold; DMG + CGB two-bins run TWICE, identical both runs)

- `m1statwirq_3` eager **0→2** ✓ (red-before-green: absent with the arm reverted).
- `golden_fingerprint` byte-identical defaults-OFF ✓ (`eager_value`-gated).
- mooneye **93/93 ×3** (OFF / `SLOPGB_MOONEYE_EAGER=1` / `SLOPGB_MOONEYE_RECLOCK=1`)
  ✓; every tripwire green under eager.
- flagon_probe EV two-bin: CGB **287 → 287** (DMG-scoped, zero drift) ✓ ×2;
  DMG **46 → 46** ×2 — 4 base-fail→pass recovered (incl m1statwirq_3), **4
  base-pass→fail residual** (the table above) → the "zero SameBoy-pass drop" bar is
  **NOT** met (4 residual). 9 of the 13 PoC drops re-hosted.
- tier2 unchanged (`eager_value` ≠ `tier2_reclock`); clippy `-D warnings` clean;
  no `.rs` > 1000 (reclock.rs 987).

## Next levers (for the parent's flip-with-4-floor vs continue call)

- **wx00** — record `wx_match_dot` for WX≤7 (or the prefill-match dot) and gate the
  DMG `vis_hold_until`/first-window-line extend on `wy_trig_sb_dot <
  wx_match_dot` under eager, so a co-incident-latch trigger line renders bare.
- **The 3 IF rows** — NOT PPU-re-hostable (sibling-identical dispatch). Either an
  FF0F read-frame peek keyed on `read_pos_hd` (the ONE thing that differs between
  the siblings — the CPU read dot) IF the STAT-bit-set instant can be reconstructed,
  or accept them into the flip floor (they are the counter-pinned dispatch residual
  the census has always parked). scx5_2 recovery needs an scx-aware `win_extends_sb`
  slack with a render-activation discriminator.
