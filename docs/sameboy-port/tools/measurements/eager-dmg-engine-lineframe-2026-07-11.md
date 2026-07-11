# EAGER DMG engine / line-frame cluster (#11dp) — 2026-07-11

Base: `finish-port-halfdot @ 34c76ca`. Target: the 9 non-window DMG bar rows the
task named as the eager C3-flip DMG-engine cluster. Method: dual-trace each row
OFF/EV/tier2 (`--features port_probe` on `examples/run_gambatte` +
`examples/run_mooneye`, `SLOPGB_EAGER=1 SLOPGB_S5DBG=1`) with an FF41-read trace
(`ly/dot/vm/nat/cmp/glit/lrd/carr/roam/line_render_done`) and the existing
`dispatch`/`ff0f` traces. flagon_probe baselines reproduced EXACTLY: OFF DMG 103,
EV DMG 56, OFF CGB 486, EV CGB 295, tier2 CGB 291, tier2 DMG 116.

## Result: EV DMG 56 → 54. Two SameBoy-PASS (BUG) rows recovered, zero drops.

Ship: `read_laws.rs::vis_mode_read` — one guarded arm
(`line == 0 && m == 0 && dot < 4 && !line_render_done → 2`), `eager_value`+`!is_cgb`
scoped. Pin `tier2_eager_dmg_ly0_oam_entry_passes` (`eager_web.rs`).

## The decisive measurement: zero read-value lever for 7 of 9

For ALL 9 rows there is **zero read-VALUE disagreement** between EV and tier2 at
any same `(ly, dot)` FF41 read (`join`-diff of `ly_dot → vm`). The EV read-law web
is already position-correct everywhere it is asked. Where EV and tier2 diverge it
is in *which* positions the CPU reads (control flow) or *which cycle* an interrupt
is serviced / a write commits — the eager-vs-deferred CPU **dispatch clock**
(eager cc+4 vs deferred cc+0), the counter-pinned dispatch the task forbids moving.
A read-law changes the VALUE returned at a position; it cannot change which
positions the CPU visits, so **no discriminator can exist** for a control-flow /
dispatch divergence — a stronger refutation than #11dj's one-sided-drop (there the
values differed; here they never do).

## Per-row verdict

| row | want | EV | tier2 | divergence | verdict |
|---|---|---|---|---|---|
| `lycEnable/lyc153_late_enable_m1disable_3` | E0 | E2 | E0✓ | +1 STAT **dispatch** @ly153 dot6 (EV fires a spurious STAT int; disable-write commits late) | **REFUTE** (write/dispatch frame; 0 read-value lever) |
| `lycEnable/lyc153_late_m1disable_3` | E0 | E2 | E0✓ | same extra dispatch @ly153 dot6 | **REFUTE** (dispatch frame) |
| `lycEnable/lycwirq_trigger_ly00_stat50_2` | E0 | E2 | **E2✗** | tier2 ALSO fails; no EV/tier2 diff | **REFUTE** (un-ported deeper law, not a re-host) |
| `m2enable/late_enable_2` | 0 | 2 | 0✓ | identical FF41 values (all mode 2); EV does extra ISR reads = int serviced differently (dispatch clock) | **REFUTE** (dispatch/interrupt-service) |
| `m2enable/late_enable_after_lycint_disable_2` | 0 | 2 | 0✓ | same as above | **REFUTE** (dispatch) |
| `m2enable/late_enable_m0disable_2` | 0 | 2 | **2✗** | tier2 ALSO fails | **REFUTE** (deeper un-ported law) |
| `ly0/lycint152_ly0stat_3` | C2 | C0→**C2** | C2✓ | EV verdict read = eager LY=0 dot0 (mode 0); tier2 reads dot4 (mode 2) | **SHIP** (line-0 OAM-entry back-date) |
| `enable_display/frame1_m2stat_count_2` | 90 | 0→**90** | 90✓ | same line-0 dot<4 OAM-entry back-date | **SHIP** (bonus, same arm) |
| `m2int_m3stat/scx/late_scx4_2` | 0 | 3 | 0✓ | FF41 control-flow diff, 0 same-pos value mismatch | **REFUTE** (coupled render∧read; dispatch-driven) |

## The shipped arm — discriminator

`ly0stat_3` (want C2/mode 2) and mooneye `stat_lyc_onoff` (want mode 0) BOTH read
eager LY=0 dot 0 — the read_laws comment's cited "HALFDOT floor". Fresh EV trace
found the discriminator the floor lacked: **`line_render_done`**.

- `ly0stat_3` / `frame1_m2stat_count_2`: LY=0 dot 0, **`lrd=0`** — a genuine fresh
  line-0 with a PENDING OAM scan → cc+4 = OAM mode 2. RECOVER.
- `stat_lyc_onoff` (DMG/Mgb/Sgb/Sgb2): LY=0 dot 0, **`lrd=1`** — post-LCD-enable
  startup, OAM scan not pending → SameBoy reads mode 0. Arm does NOT fire.
- A/B sibling `ly0/lycint152_ly0stat_2` (want C0): verdict read is the *earlier*
  eager LY=153 dot 452 (mode 0), NOT the line-0 read — untouched (the sweep
  `_1`@153:448 / `_2`@153:452 / `_3`@0:0 separates whole-dot).

Naively porting the arm without the `!lrd` guard broke `stat_lyc_onoff` on the 4
non-CGB models (mooneye eager 91/92) — the guard is load-bearing.

## Gates (all pass)

- golden_fingerprint byte-identical (production flags-off untouched).
- EV DMG 56→54; EV CGB 295 unchanged; tier2 CGB 291 + tier2 DMG 116 unchanged
  (arm is `eager_value`+`!is_cgb` → tier2/CGB/production inert).
- Zero-regression A/B (EV DMG rowlist): recovered = {ly0stat_3, frame1_m2stat_count_2};
  new-fails = EMPTY. Both are gambatte `_out<hex>`-tagged = SameBoy-PASS = BUG.
- mooneye 3-clock: OFF 92/92, tier2 92/92, eager 92/92.
- eager intr_2 (0/mode0/mode3/oam_ok/mode0_sprites) PASS both models.
- clippy `-D warnings` clean; read_laws.rs 999 (<1000). (Pre-existing base debt:
  windows.rs/lib_tests.rs/cartridge.rs/oam_vram.rs over-cap — untouched by this
  change.)
- Red-before-green: disabling the arm makes the pin fail (`ly0stat_3` → C0).

## Refuted leads (do not re-chase)

- **FF0F eager-path gap**: `bus.rs` eager FF0F read applies `ff0f_stat_peek` +
  `ff0f_ly0_pulse_mask` but NOT `ff0f_dmg_service_clear` / `ff0f_dmg_m0_coincident_mask`
  (both internally `tier2_reclock`-gated). Real asymmetry, BUT the hblank_int /
  enable_display mode-0 rows those laws target already PASS under EV (none in the
  EV DMG fail set) — porting = regression risk, zero gain. Dropped.
- The 7 refuted rows are the counter-pinned dispatch/write clock (C3-flip / HALFDOT
  Part A). No read-frame lever; they land with the coherent per-T dispatch retime.
