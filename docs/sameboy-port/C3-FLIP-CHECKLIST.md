# C3 FLIP CHECKLIST — the tier2/leading-edge default flip

Written at census 21 (#11bh, 2026-07-03; the goal's <~25 trigger); census
**0** since #11bi (the speedchange quartet fell to the post-switch exit
table: `measurements/speedchange-postswitch-exit-2026-07-03.md`). The
step-3 dry run RAN 2026-07-03
(`measurements/c3-dryrun-flip-classify-2026-07-03.md`): the CGB-OCR bar
HOLDS (37/37 flip-BUGs classify SameBoy-FAIL), but the flip is still
blocked by the DMG side — see §3b. Update the numbers as levers land;
execute top-to-bottom in ONE session when §3b clears. Do NOT flip defaults
in any pushed commit until every step below is green.

Census bar (amended #11bi): **≤ 4, with any residue classified
SameBoy-pass known-BUG and listed by name** — at 4 the dry run still runs,
carrying the residue as known-BUG; at 0 (current) it ran clean.

## 0. State at writing (2026-07-03, #11bi)

- Worktree `.claude/worktrees/phase-b-s7`, branch `phase-b-s7` @ `9fe3ddf`
  (+ the phase-3 split commits after it).
- Flag-on two-bin: ON **291** / OFF 486 (`scratchpad/on_11bi{,_n}.txt` =
  the preserved ON list; diff name-level against
  `scratchpad/on_11bh_final3.txt` — 32 fixed, ZERO new).
- Must-fix (SameBoy-pass) blockers: **0** (CGB-OCR universe; the dry run
  confirms 37/37 CGB flip-BUGs rebaseline-OK).
- 51 tier2 pins; mooneye 91/91 flag-on AND flag-off (and 91/91 with the
  defaults FLIPPED — dry-run measured); gbtr OFF **236/0**; lib 660;
  clippy `-D warnings` clean.

## 1. Preconditions (all fresh, same tree)

- [ ] Fresh full-CGB two-bin (ON + OFF), name-level lists archived in
      `scratchpad/` AND copied into the session's measurements doc (the
      base-373 list was lost once — archive at session end, every session).
- [ ] Census of SameBoy-pass blockers == the flip bar (target 0; every
      remaining row has a fresh classify_cgb_regr.py verdict on record).
- [ ] Base commit hashes recorded (worktree + docs branch).
- [ ] 44+ tier2 pins green · mooneye 91/91 ON+OFF · gbtr OFF green · lib ·
      clippy.

## 2. Flip mechanics

- [ ] Defaults at `crates/slopgb-core/src/interconnect.rs:647-648`:
      `leading_edge_reads = true`, `tier2_reclock = true`.
- [ ] **`GameBoy::new` must take the `new_with_reclock` path** — the C0 DIV
      +4 is applied at construction; a post-boot `set_tier2` mis-frames DIV
      (measured, #11bd: the int_hblank pin mis-validated on the set-after
      path).
- [ ] Harness seams: `tests/common/mod.rs:274-279` — `SLOPGB_MOONEYE_RECLOCK`
      becomes a no-op (flag already default).
- [ ] `tests/gbtr/gambatte_flagon_probe.rs` retires (or flips meaning to
      "flag-off probe" for regression archaeology).
- [ ] The `boot_with_reclock` pin harness path collapses into the default
      boot; pins keep passing unchanged.

## 3. Rebaseline procedure (the flip-on fail set)

For EVERY row failing with defaults flipped:

- [ ] Run `docs/sameboy-port/tools/classify_cgb_regr.py` (input = bare
      `gambatte/...gbc` paths, one per line — NOT the FAIL lines; the script
      joins the line verbatim onto the ROM root).
- [ ] SameBoy-PASS ⇒ **STOP. Forbidden drop** — fix it or abort the flip.
- [ ] SameBoy-FAIL ⇒ add to `tests/gbtr/baselines/gambatte.txt` with a dated
      swap block, A/B evidence, and a floor-class letter (header rules,
      lines 1-15 of that file).
- [ ] Pre-seeded rebaseline joiners (already classified SameBoy-fail):
      the 8 #11bg floor-losses · the #11am 51-row rebaseline set · the 2
      #11bd bonus-losses (`speedchange2_nop_m2int_m3stat_scx1_1`,
      `ly0_m0irq_scx0_ds_2`) — SUPERSEDED by the #11bi dry-run fresh
      classification (`c3-dryrun-flip-classify-2026-07-03.md`): the 37
      CGB-OCR joiners are `scratchpad/flipregr_cgb_ocr.txt` (37/37
      SameBoy-fail), the 7 DMG-OCR joiners
      `scratchpad/flip_dmgocr_floorlist.txt`; the two #11bd bonus-losses
      fail ON **and** OFF (already-floored shared floor, no flip action).
- [x] At census 0, BEFORE the real flip: temp-flip locally, run the full
      battery, classify the entire flip-on fail set, revert — RAN
      2026-07-03 (#11bi): `c3-dryrun-flip-classify-2026-07-03.md`.

## 3b. Dry-run-found flip blockers (must clear before §2 executes)

From the #11bi dry run — all OUTSIDE the CGB-OCR census universe.
**#11bj UPDATE:** the DMG-OCR window count was UNDER-reported (the census
want-regex missed 33 shared-want `dmg08_cgb04c_out*` rows → the true DMG
window blocker set is **62**, not 29; the rebuilt `--dmg` classifier
`tools/classify_dmg.py` reclassifies them). Ported 56/62 (#11bj commit
`phase-b-s7 28eb69b`); see `measurements/dmg-window-port-2026-07-03.md`.

- [~] gambatte DMG-OCR window: **56 of 62 SameBoy-PASS legs FIXED** (#11bj
      `tier2_dmg_window_passes`; the CGB `vis_exit_hd` arms ported to DMG,
      every arm `!is_cgb()`-scoped so CGB two-bin stays 291/0). **6 residual
      parked** on the atomic classes CGB also parks: wxA6/wxA5 carried-read
      sub-dot wall (5), scx5 non-linear deadline (1), mid-frame SCX rewrite
      (1), render-trigger late_enable/reenable-scx5 (2). **3 rebaseline**
      (SameBoy-FAIL: `late_wy_1` ×2, `m2int_wxA6_spxA7_m0irq_2`).
- [~] gambatte DMG-OCR non-window singles: **CHARACTERIZED #11bm — true count
      61** (the #11bi "8" was an under-count, same want-regex miss as window
      29→62). **+1 read-frame leg SHIPPED** (`enable_display/ly0_m0irq_scx1_1`,
      the glitch-line mode-0 co-instant FF0F read mask
      `Ppu::ff0f_dmg_m0_coincident_mask`, `tier2_dmg_m0_coincident_passes`,
      `!is_cgb`+`glitch_line`-scoped; corrects the #11ad "DMG floor"). **60
      measured parks:** tima 45 + serial 1 = S6 timer/serial-completion (#11ai,
      C0-DIV refuted); `frame*_m0irq_count` 6 = dispatch-COUNT; sprites 2 =
      inverted IF lifecycle; miscmstatirq 1 = glitch `mfi=3` dispatch; m2enable
      1 + lycwirq_stat50 1 = line-start service co-temporal; ff40_disable 1 =
      LCD-disable timing; `ly0_late_scx7_m3stat` 2 = render-length atomic. All
      land with the flip's dispatch/render reclock, NOT a §3b lever. Map:
      `measurements/dmg-ocr-singles-2026-07-04.md`.
- [~] **DMG ENGINE SET — 36 SHIPPED (#11bk 16 + #11bl 20), 43 residual atomic**
      (#11bj `measurements/dmg-engine-set-classify-2026-07-03.md` + #11bk
      `dmg-hblank-if-2026-07-03.md` + #11bl `dmg-poweron-boot-read-2026-07-04.md`;
      probes `gbmicro_flagon_probe`/`wilbertpol_flagon_probe`): gbmicrotest **68**
      (hblank_int mode-0 IF-delivery read-frame straddle over the
      counter-pinned rise + poweron_* boot-read chain + timer), wilbertpol
      **10** (`ly_lyc_153_write`/`timer_if`, all B=48 dispatch shift), age
      **1** (halt-m0-interrupt). **#11bk: the `hblank_int` `if_c`/`if_d`
      READ-frame legs (16) SHIPPED** — the mode-0 STAT-IF two-latch (DELIVER
      `[R-4,R)` + SERVICE-CLEAR `[R,R+4)`, `tier2_dmg_hblank_if_passes`,
      `!is_cgb`-scoped). **#11bl: the 20 `poweron_*` boot-read rows SHIPPED** —
      the pristine-boot-frame FF41/OAM/VRAM/LY read at its true cc+4 position
      (`Ppu::boot_read`, +4-dot shift, `tier2_dmg_poweron_passes`, `!is_cgb` +
      `frame_count<=2` + `!lcd_regs_written`); the boot READ frame decouples
      from the `+4` boot DIV (`boot_div` HELD — MEASURED separable, the twin of
      #11bk one frame earlier). The remaining **43** (`if_b`/`nops`/`hblank_scx3`/
      `int_scx7` + wilbertpol + age) are the counter-pinned dispatch atomic core
      — NO flag-gated slice; they fix WITH the flip's global dispatch reclock
      (the C3 event itself), not a §3b lever.
- [~] **PIXEL-REFERENCE LEGS — CLASSIFIED** (#11bj `tools/classify_pixel.py`,
      `measurements/pixel-classify-2026-07-03.md`): 125 legs (gambatte 103 +
      mealybug 20 [the "mealybug 20" row] + age 2) →
      **100 SameBoy-PASS flip-blockers** (all mode-3 RENDER-RECLOCK atomic:
      scy/bgtiledata/bgtilemap/dmgpalette/mealybug m3_*/scx_during_m3 — NONE
      law-reachable; fix WITH the production render reclock at the flip) +
      **13 DMG rebaseline** (`scratchpad/pixel_rebaseline_dmg.txt`) +
      **12 golden-review** (2 DMG-uncertain mm<64 + 8 CGB colour-confound +
      2 age m3-bg — resolve at C4). Supersedes the "195 unclassified" row.
- Flip FIXES banked for the rebaseline: gambatte 332 now-pass legs +
  non-gambatte 59 (incl. wilbertpol 44) + mooneye/blargg/acid/same_suite/
  smallsuites flip-clean.

## 4. Harness re-anchor

- [ ] gbtr ratchet baselines re-anchored to the flipped defaults (the
      ON-list becomes the new floor; OFF scaffolding rows removed).
- [ ] All tier2 pins re-pointed at the default path (drop
      `boot_with_reclock`, use plain boot; pin only frame-stable rows —
      probe a candidate 3× single-row first).
- [ ] `SLOPGB_MOONEYE_RECLOCK`, `SLOPGB_PROBE_OFF`, `SLOPGB_PROBE_LE`
      cleaned out of CI invocations.

## 5. Line caps

- [x] Every touched `.rs` under 1000 lines — DONE #11bi phase 3:
      `ppu/stat_irq.rs` 755 + `ppu/stat_irq/read_laws.rs` 550 (the FF41
      read-law engine split out) and `interconnect.rs` 978 +
      `interconnect/speed.rs` 566 (the stop/halt-wake/ack/dispatch-retime
      trait-fn bodies as `pub(super)` `_impl` delegates — a trait impl
      cannot split across files). Suite-gated behavior-identical.

## 6. C4 gates (after the flip commit, before deleting scaffolding)

- [ ] 146 golden frames re-generated AND REVIEWED (not rubber-stamped).
- [ ] Every-oracle-zero-drop: mooneye 91/91 both models · blargg /
      wilbertpol / age / gbmicrotest / mealybug / acid no growth · full
      gbtr · lib · clippy.
- [ ] Then delete the OFF scaffolding (PORT-PLAN S7): `leading_edge_reads`
      forks, `stat_events_tick` vs `stat_update_tick` dispatch, the
      `tier2_*` gates, the flagon probe.
