# C3 FLIP CHECKLIST — the tier2/leading-edge default flip

Written at census 21 (#11bh, 2026-07-03; the goal's <~25 trigger); census
now **4** at session end (#11bh final — the `speedchange2*_m2int_m3stat_
scx2_2` quartet, parked on the measured `E(scx)` A/B:
`measurements/speedchange-postswitch-law-2026-07-03.md`). Update the
numbers as levers land; execute top-to-bottom in ONE session when census
reaches the flip bar. Do NOT flip defaults in any pushed commit until every
step below is green.

## 0. State at writing (2026-07-03, #11bh)

- Worktree `.claude/worktrees/phase-b-s7`, branch `phase-b-s7` @ `873b2e9`
  (the 9 #11bh commits on top of #11bg `c3d1991`).
- Flag-on two-bin: ON **323** / OFF 486 (`scratchpad/on_11bh_final3.txt` =
  the preserved ON list; diff name-level against `scratchpad/base354_n.txt`
  — 31 fixed [23 must-fix + 8 bonus], ZERO new vs the #11bg base).
- Must-fix (SameBoy-pass) blockers: **4** = the `speedchange2*_m2int_
  m3stat_scx2_2` quartet (the S6 co-land, parked with the measured A/B).
- 50 tier2 pins; mooneye 91/91 flag-on AND flag-off; gbtr OFF 234/0; lib
  660; clippy `-D warnings` clean.

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
      `ly0_m0irq_scx0_ds_2`).
- [ ] At census 0, BEFORE the real flip: temp-flip locally, run the full
      battery, classify the entire flip-on fail set, revert — the dry run
      that lets the flip session start with a complete rebaseline list.

## 4. Harness re-anchor

- [ ] gbtr ratchet baselines re-anchored to the flipped defaults (the
      ON-list becomes the new floor; OFF scaffolding rows removed).
- [ ] All tier2 pins re-pointed at the default path (drop
      `boot_with_reclock`, use plain boot; pin only frame-stable rows —
      probe a candidate 3× single-row first).
- [ ] `SLOPGB_MOONEYE_RECLOCK`, `SLOPGB_PROBE_OFF`, `SLOPGB_PROBE_LE`
      cleaned out of CI invocations.

## 5. Line caps

- [ ] Every touched `.rs` under 1000 lines. Risks at writing:
      `ppu/stat_irq.rs` (~1160) and `interconnect.rs` (~1460) are ALREADY
      over — the flip session must split them (seam map:
      `docs/tdd-split-plan.md`) before or with the flip commit.

## 6. C4 gates (after the flip commit, before deleting scaffolding)

- [ ] 146 golden frames re-generated AND REVIEWED (not rubber-stamped).
- [ ] Every-oracle-zero-drop: mooneye 91/91 both models · blargg /
      wilbertpol / age / gbmicrotest / mealybug / acid no growth · full
      gbtr · lib · clippy.
- [ ] Then delete the OFF scaffolding (PORT-PLAN S7): `leading_edge_reads`
      forks, `stat_events_tick` vs `stat_update_tick` dispatch, the
      `tier2_*` gates, the flagon probe.
