# The EAGER STAT-ISR carried-read peek — m2int entry recovered (2026-07-08, #11cd)

Task: continue the eager-value (EV) re-host from #11cc (EV CGB 428, flip bar 97
SameBoy-PASS blockers). Recover the CLEAN read-verdict / render-length blocker
families toward the ~64 bar. **Result: one clean read-verdict lever shipped
(the eager STAT-ISR carried-read peek) — EV CGB 428 → 421, flip bar 97 → 89
SameBoy-PASS blockers (clean −8). The remaining target families (window
reenable/wx un-catch, LYC/enable STAT read-frame, halt wake-clock) MEASURED as
render-view / read-frame / wake floor under the eager whole-dot clock, NOT clean
gate-flips — REVERTED / left parked. All hard gates hold.**

## The lever: arm `read_carried` on the eager STAT ack

Root cause of the mode-2→3 entry (`m2int_m3stat`) blocker family: the tier2
dispatch retime (`dispatch_retime_impl`, speed.rs) arms `read_carried` for a
STAT OAM/HBlank ISR so the handler's first FF41 mode read takes the source's
read-position carry (`isr_read_carry_hd` — +4 hd SS OAM / +2 hd SS m0, exit-
folded in `vis_exit_hd` arm 8 + the DS `vis_mode_read` peeks). Under the eager
clock the dispatch stays cc+4 (no retime — `intr_2_*_sprites` must not move), so
`read_carried` was NEVER armed and the m2int carried reads landed at the polled
(uncarried) frame → the `_2` siblings over-held mode 3.

The `stat_rise_oam`/`stat_rise_m0` flags ARE already computed under EV (the
`StatUpdate` engine runs on `leading_edge_reads`, which EV sets — `stat_update_
halt_masks`), so the only missing piece was the ARM. The peek is a read VERDICT,
not a dispatch move, so it is armed at the STAT (bit 1) ack in `ack_impl` and
cleared one-shot after the FF41 read in `Bus::read` / `read_inc` (the tier2 twin
clears in `read_deferred`).

| site | change | scope |
|---|---|---|
| `interconnect/speed.rs` `ack_impl` (bit 0\|1 arm) | arm `read_carried` when `bit == 1 && (stat_rise_oam \|\| stat_rise_m0)` | `eager_value` only |
| `interconnect.rs` `Bus::read` + `read_inc` | clear `read_carried` one-shot on the FF41 read | `eager_value` + `addr == 0xFF41` |
| `ppu/regs.rs` FF40 win-reenable latch | `win_reenable_dot`/`win_enable_dot` set `\|\| eager_value` | `tier2 \|\| eager_value` |

Never fires flag-off (`eager_value` false) → production byte-identical.

## Two-bin (branch `finish-port-halfdot`)

| bin | fail | vs baseline |
|---|---:|---|
| EV CGB baseline (#11cc) | 428 | — |
| **EV CGB carried-read (this)** | **421** | **−7** |
| EV DMG baseline | 147 | — |
| **EV DMG carried-read** | **145** | **−2** (bonus; the arm is not `is_cgb`-scoped) |
| tier2 CGB (unchanged) | 291 | 0 |

**Flip-bar impact (the metric that matters):** OFF-pass ∩ EV-fail ∩ SameBoy-PASS
= **97 → 89** (clean −8). Per-row: 8 fixed / 1 broke on EV.

- **Fixed (8):** `m2int_m3stat/nobg/m2int_nobg_scx7_m3stat_2`,
  `m2int_m3stat/scx/m2int_scx2_m3stat_2`, `.../m2int_scx3_m3stat_2`,
  `m0int_m3stat/m0int_m3stat_ds_1`, `speedchange/speedchange2_m2int_m3stat_scx2_2`
  + `_frame1_` + `_lcdoff2_..._scx3_2`, `window/late_enable_ly0_ds_2` (the
  `win_enable_dot` DS late-enable arm, from the regs.rs flip).
- **Broke (1):** `speedchange/speedchange2_nop_m2int_m3stat_scx1_1` — the
  documented VBlank-anchored rebaseline joiner (read_laws.rs arm-8 comment), AND
  already-floored (OFF also fails it), so it adds **0** to the flip bar.

## Why the other target families are FLOOR (measured, not guessed)

The census #11cc listed ~28 clean candidates; empirically most are half-dot /
render-view floor on the eager whole-dot clock:

- **window reenable / wx un-catch (12+):** arms 5/D5 (`win_reenable_dot`) and
  the wx un-catch arms consume render-VIEW latch DOTS calibrated to the tier2
  DEFERRED render frame. Under EV the render is eager, so the recorded dots
  (`win_reenable_dot`, `wx_write_dot`) sit at a different position and the arms'
  `+3 > wx_match` / `write <= match` thresholds mis-fire. Flipping the latch
  gates (`win_reenable_dot`, `wx_write_dot`) fixed only the DS late-ENABLE row
  (via `win_enable_dot`); `wx_write_dot` was INERT (0/0), REVERTED. The
  reenable/wx recovery needs the reclocked (half-dot) render dot, not a gate flip
  — confirms the #11bz "render-view latch INERT" finding still holds.
- **`late_scx4_2` / `_ds_2` (m2int_m3stat, 2):** arm-8 emergent exit anchors to
  the render's `flip_dot`, which on the eager frame extends later than tier2's →
  over-holds. Render-view floor.
- **LYC/enable STAT (8):** the mode reads (`ff41_disable`/`late_ff41_enable`/
  `lycdisable_ff41`/`m2enable`/`lyc153int_m2irq`) are LYC-sourced STAT-ISR reads
  — `stat_rise_oam/m0` are false, so `read_carried`/`isr_read_carry_hd` apply no
  carry; the residual read-frame mismatch needs an un-ported LYC-ISR carry, not a
  gate flip. `lycint_lycflag` (3) reads the coincidence BIT (`self.cmp`, a stored
  frame value sampled cc+0) — needs a `cmp`-read-position law that doesn't exist.
  The `E0/E2` legs are the counter-pinned dispatch signature (dispatch floor).
- **m2int_m0irq FF0F (scx3_ifw, 2-3):** CGB IF-register dispatch-frame reads; the
  FF0F peek laws (`ff0f_dmg_service_clear`/`dmg_m0_if_rise`/coincident) are all
  `!is_cgb` — no CGB peek exists. Dispatch/IF-frame floor.
- **halt (5):** the halt-wake reads (`late_m0int_halt_m0stat`,
  `late_m0irq_halt_dec`, `late_m0irq_halt_m0stat`) need the deferred `wake_skew`
  sub-M-cycle machine (`m0_halt_hold`/`wake_skew`/`halt_ly_phase`, all repaid in
  `read_deferred`, the tier2 path). Per the task criterion (revert if it needs
  the deferred wake_skew machine) → FLOOR, not attempted.

## Gate state (ALL hold, verified this run)

- golden_fingerprint (`--release`) PASS — production byte-identical.
- mooneye OFF 91/91; mooneye tier2 (`SLOPGB_MOONEYE_RECLOCK=1`) 91/91; **tier2 CGB
  two-bin 291 (unchanged — `eager_value` off under tier2)**.
- mooneye EAGER (`SLOPGB_MOONEYE_EAGER=1` acceptance_ppu): only `lcdon_timing-GS`
  ×4 (pre-existing exemption); **`intr_2_mode0/mode3/sprites` PASS** (dispatch
  stays cc+4 — the arm is a read verdict, not a dispatch move).
- EV DMG (dmg_rowlist): **145 ≤ 147**.
- clippy `-D warnings` clean (default + `port_probe`); every `.rs` < 1000
  (interconnect.rs 987).

## The bar now (updated)

Flip bar = **89 SameBoy-PASS blockers** (was 97). The clean read-verdict lever is
now EXHAUSTED (`read_carried` was the last un-armed cc+4 read-frame). The residual
89 split: DS mid-dot ∪ counter-pinned dispatch (≈56) + render-view window latch
(≈12) + LYC/enable-ISR + cmp read-frame (≈11) + halt wake-clock (5) + FF0F-CGB
dispatch. All need HALFDOT Part A (the per-T half-dot clock + coherent render/
dispatch/wake retime), NOT a gate flip. The census "~28 clean → 64" was
optimistic: only the m2int read-verdict slice (+8) was gate-flippable; the window
family it counted as clean is render-view floor.

## Reproduction

`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head
-1)`; `SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN
--ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=` (→ 421).
tier2 two-bin: same WITHOUT `SLOPGB_PROBE_EV` (→ 291). EV DMG: `SLOPGB_PROBE_EV=1`
with `scratchpad/dmg_rowlist.txt` (→ 145). Flip bar: `comm -13 fail_off.txt
ev_fail | classify_cgb_regr.py` (→ BUG=89). Commit: `f540cc6` (+ the read_inc
clear follow-up).
