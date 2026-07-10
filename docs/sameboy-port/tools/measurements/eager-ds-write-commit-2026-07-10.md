# The DS extension of the eager write-conflict commit port — 2 CGB DS FF0F BUG rows SHIPPED (2026-07-10, #11df)

Extends #11dd's single-speed write-conflict commit port
(`eager-write-commit-port-2026-07-10.md`) to **double speed**: the two DS FF0F
`ifw` mode-0-IRQ rows the SS `!double_speed` scope left open. **Both recovered
(both SameBoy-PASS BUG rows), ZERO regressions on either model.** EV CGB
**325 → 323**.

## Baselines reproduced (exact, on the base `53791c3`)

`flagon_probe` two-bin: OFF CGB **486**, EV CGB **325**, tier2 CGB **291**,
EV DMG **74**. All exact.

## The mechanism — DS needs the squash ARM only, NOT a half-dot commit borrow

The task hypothesised a half-dot write borrow (SameBoy `GB_CONFLICT_WRITE_CPU`
commits 1 T = **0.5 dot** into the M-cycle at DS, vs 1 dot at SS). Tracing
REFUTED the need for a commit-dot move at DS:

- The eager whole-M-cycle `tick_machine` at DS ticks 2 dots and lands
  `write_no_tick` at the M-cycle boundary — **the SAME dot the tier2 deferred
  path commits** (measured below), unlike SS where the eager commit sits 1 dot
  early of the tier2 WriteCpu dot.
- The DS mode-0 STAT rise sits **1–2 dots PAST** the write commit, not
  co-instant. So there is no rise to fold at the commit; instead the existing
  `stat_update_tick` DS mode-0 squash window (`w = 2` when `self.ds && m0_rise`,
  `reclock.rs`) — armed to 2, consuming a rise at Δ 1–2 dots after the commit —
  already covers it.

The ONLY thing the eager DS path was missing is the `stat_if_squash` **arm**:
#11dd gated `arm_ff0f_if_squash` behind the SS-only `borrow` (`!double_speed`).

Fix (`interconnect/bus.rs`, `eager_value` + CGB + `double_speed` +
`!lcd_shift_active` + FF0F bit1-clear): arm `stat_if_squash = 2` at the eager DS
FF0F write. **No `tick_half` borrow, no `eager_wr_borrow` repay** — the commit
dot is already correct. The existing DS consumer (`w = 2`) does the rest.
Production (flag off), tier2 (`write_deferred`, early-returned) and single-speed
eager (`borrow` path unchanged) never reach it → golden byte-identical.

### DS half-dot trace tables (dot resolution, verified this session)

`m2int_m0irq_scx3_ifw_ds_2` (want 0), critical line ly=135:

| event | eager (before) | tier2 |
|---|---|---|
| FF0F=0x00 clear commit | dot **256** | dot **256** |
| mode-0 rise (`dispatch`) | dot **258** (Δ2) | dot 258 (`ifw squashed`) |
| digit | **2** ✗ (rise re-sets IF) | 0 ✓ |

`m2int_m0irq_scx4_ifw_ds_2` (want 0), ly=135:

| event | eager (before) | tier2 |
|---|---|---|
| FF0F=0x00 clear commit | dot **258** | dot **258** |
| mode-0 rise | dot **259** (Δ1) | dot 259 (`ifw squashed`) |
| digit | **2** ✗ | 0 ✓ |

The eager commit dot equals tier2's in BOTH rows — the calibration the DS squash
countdown (Δ 1–2 consume, Δ 3–4 survive) depends on. The `_ds_1` siblings sit
further from the rise and MUST survive:

| row | eager commit | eager rise | Δ | want | with arm |
|---|---|---|---|---|---|
| scx3_ifw_ds_2 | 256 | 258 | 2 | 0 | **0** ✓ (consumed) |
| scx4_ifw_ds_2 | 258 | 259 | 1 | 0 | **0** ✓ (consumed) |
| scx3_ifw_ds_1 | 254 | 258 | 4 | 2 | **2** ✓ (survives) |
| scx4_ifw_ds_1 | 256 | 259 | 3 | 2 | **2** ✓ (survives) |

## Results — 2 recovered, ZERO regressions

EV CGB **325 → 323** (−2). EV DMG **74 → 74** (DS+CGB-scoped; DMG has no DS).
tier2 CGB **291** unchanged. Golden byte-identical.

Zero-regression A/B (`comm`): recovered exactly `m2int_m0irq_scx3_ifw_ds_2` +
`m2int_m0irq_scx4_ifw_ds_2`; **new-fails EMPTY on both CGB and DMG**.
`classify_cgb_regr.py`: **2 BUG (SameBoy-pass, must-fix bar), 0 FLOOR**.

## Scope calibration

- **`!lcd_shift_active`**: carried from #11dd (the DS half-dot grid is even more
  shift-sensitive). Not A/B-separable here (no shifted-grid DS FF0F row in the
  recovered/regression sets), but kept as the conservative grid guard —
  arming on a shifted grid would mis-frame the squash countdown.
- The other probed candidates are OUT OF SCOPE (not write-commit):
  `irq_precedence/late_m0irq_retrigger_scx1_ds_2` is the ack-squash retrigger
  (#11de family, a dispatch-ack miss, not a write commit);
  `enable_display/*_ds_1`, `lcd_offset/*_ds_1`, `m2int_m0stat_ds_2` are
  dispatch/accessibility/STAT-read frame rows, not FF0F write-race. None fall
  out of the FF0F squash arm and none were touched.

## Gates (all green)

1. `golden_fingerprint` — byte-identical (9020 cases match HEAD, 42s).
2. EV CGB 325→323 ↓; tier2 CGB 291 unchanged; EV DMG 74 unchanged.
3. Zero-regression A/B — new-fails EMPTY on CGB AND DMG.
4. mooneye `ppu` green on all three clocks (off / `MOONEYE_RECLOCK` /
   `MOONEYE_EAGER`, 91/91 each).
5. eager intr_2 (mode0/mode3/mode0_sprites/oam_ok/intr_2_0) PASS both models.
6. clippy `-D warnings` clean; `bus.rs` 287 lines, `eager_web.rs` 180 (< 1000).
7. Red-before-green pin
   `gambatte::eager_web::eager_ds_write_conflict_commit_passes` (the 2 DS rows)
   — FAILS with the DS arm neutered (`ff0f_ds_squash = false && …`), PASSES with
   it.

## Files

- `interconnect/bus.rs` — the DS FF0F squash arm (`ff0f_ds_squash`, reuses the
  existing `arm_ff0f_if_squash` + `stat_update_tick` `w=2` DS consumer).
- `tests/gbtr/gambatte/eager_web.rs` — the pin.

## Endgame after #11df

The write-conflict commit port is now complete on both speeds (SS #11dd + DS
#11df). The DS FF41/FF45 commit-dot-move siblings (DS analogues of
`ff41_disable_2` etc.) are a SEPARATE lever (the commit-dot move, not the squash)
and were NOT in this slice's scope. Remaining C3-flip work (per #11db): the
FF41/FF45 DS commit-dot re-host, the eager ack-squash DS retriggers, the 5 CGB
halt rows, HDMA `defer_steal`.
