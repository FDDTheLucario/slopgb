# EAGER HALFDOT Part B — the intr_0 tripwire was NOT a read-frame miss; the eager glitch-line mode-0 IRQ fired 4 dots early (ENGINE dispatch dot). Fixed CGB+DMG, EV CGB 361 / DMG 98 (2026-07-09, #11cm)

Task (the last read-frame gating the C3 flip): resolve the eager FF41/FF0F read
to its true half-dot (`GB_display_sync` analogue) + port the FF0F two-latch
DELIVER/SERVICE to CGB-eager, so `intr_0_timing` passes under eager with the
dispatch unmoved.

## Result — intr_0_timing PASSES under eager on BOTH models; the root cause was the ENGINE, not the read frame

**The redirect (the load-bearing finding):** the eager `intr_0_timing` failure
(B=48) was NOT the FF0F read frame the #11cl / #11bw docs hypothesized. It was
the eager **glitch-line mode-0 STAT IRQ firing 4 dots early** — an ENGINE
(`mode_for_interrupt`) miss, not a read miss. The FF0F read at the co-instant
dot was CORRECT to see the bit; the bit was raised at the wrong dot.

- Fix: one gate widening in `ppu/stat_irq/reclock.rs` — the SS glitch-line
  mode-0 IRQ arm (which keys the IRQ on `line_render_done`, the true dispatch
  dot, NOT on `vis_early`) now fires under `eager_value` on BOTH models
  (`(tier2 && is_cgb) || eager_value`). Was `tier2 && is_cgb`-only.
- `intr_0_timing` eager: **B=48 FAIL → B=03 PASS on Dmg AND Cgb.**
- EV CGB **365 → 361** (clean +4/−0). EV DMG **102 → 98** (clean +4/−0).
- TRUE flip bar (classified vs SameBoy `--cgb --length 4`): **CGB 55 → 51,
  DMG 55 → 51** (−4 each). All 8 fixed rows are BUG (SameBoy-PASS): the SS
  `enable_display/ly0_m0irq_scx0/1` (want E0) + `frame0_m0irq_count_scx2/3`
  (want 90). Zero rows dropped (both floor and SameBoy-pass sets unchanged
  except the −4).

## The measurement (single-ROM dual-trace on `intr_0_timing.gb`, CGB, EAGER vs OFF)

The wilbertpol GPU timing tests toggle the LCD each iteration → line 0 is a
**glitch line**. On the mode-0-STAT-armed glitch line, tracing the STAT IF rise
dot + the FF0F ISR read:

| config | line-0 mode-0 STAT rise dot | FF0F ISR read (dot 248) |
|---|---|---|
| OFF (production, `stat_events_tick`) | **252** (= the render `flip_dot`) | E0 (read 4 dots BEFORE the rise) |
| EAGER (`stat_update_tick`, before fix) | **248** (4 dots early) | **E2** (co-instant with the early rise) → **B=48** |
| EAGER (after fix) | **252** (= `line_render_done`) | E0 → **B=03** |

The render flip (`m0_flip_events` → `line_render_done`/`flip_dot`) is IDENTICAL
across configs (dot 252). Production's `stat_events_tick` emits the mode-0 IF on
`m0_rise_dot` (the flip dot, 252). The eager `stat_update_tick` emits when
`mode_for_interrupt` becomes 0 — and on a glitch line the eager path fell
through to the `vis_mode` branch, which yields mode 0 at **`vis_early`** (dot
248, `lead+3` ahead of the flip), 4 dots early. Keying the eager glitch-line
IRQ on `line_render_done` (like the existing tier2 CGB arm) puts the rise back
at 252.

## Why item #1 (read half-dot sync) and item #2 (FF0F two-latch) did NOT apply

- **The eager FF0F/FF41 reads are already coherent with the cc+4 dispatch.**
  Under eager the dispatch stays at cc+4 (production frame) and the FF0F read
  TRAILS at cc+4 (`leading_edge_sample` routes only FF41; FF0F is the trailing
  `read_no_tick`). So the read frame already matches the dispatch frame — no
  `GB_display_sync` resolve and no DELIVER/SERVICE two-latch is needed. The
  FF0F two-latch (`dmg_m0_if_rise`/`ff0f_dmg_service_clear`, #11bk) exists ONLY
  to compensate the tier2 DEFERRED cc+0 read against the cc+0 dispatch; the
  eager cc+4 frame has no such offset.
- **`SLOPGB_FF0F_LE` "fixed" `ly0_m0irq`/`frame0_m0irq_count` by reading FF0F at
  cc+0** — sampling `intf` BEFORE the early rise folded — but that was net −69
  (#11cl): it papered over the early rise for those rows while mis-framing every
  other FF0F read. The root-cause engine fix here fixes the SAME rows cleanly
  (+4/−0) by raising the IF at the correct dot, so the trailing cc+4 read is
  right with no read-frame surgery.
- **Item #3 (shrink the compensation tower) is moot for this slice:** the read
  frame (`read_pos_hd` +8hd debt, `mode3_entry_dot()==80`, line-boundary
  back-dates) was NOT touched — the bug was the IRQ-source engine, orthogonal to
  the FF41 read laws. Those compensations still serve the FF41 read side.

## Gate state (all HARD invariants green; the change is `eager_value`-gated)

- `golden_fingerprint` PASS (production byte-identical — the arm is
  `(tier2 && is_cgb) || eager_value`; tier2 keeps its exact old `tier2 && is_cgb
  && !ds` predicate, eager is a new leaf).
- tier2 CGB two-bin **291** (unchanged). mooneye `--test mooneye` **92 passed**
  flag-off; `SLOPGB_MOONEYE_EAGER` acceptance_ppu 91; `SLOPGB_MOONEYE_RECLOCK`
  91. clippy `-D warnings` clean. `reclock.rs` 855 lines (< 1000).
- Eager tripwires PASS: `intr_0_timing` (both), `intr_2_mode0/mode3/oam_ok/0`
  (both), `di_timing-GS` (both), `int_hblank_halt_scx0/3/7` +
  `hblank_int_l0/scx0/if_a/scx0_if_a` (DMG FF82=01, = OFF — the tier2 CGB-only
  DMG conflict does not exist on the eager cc+4 frame).

## The residual flip bar (after 51 CGB / 51 DMG) — the precise next levers

CGB (51): m2int_m0irq **FF0F write-race** (`ifw`, 5 — `arm_ff0f_if_squash` is
NOT armed on the eager `Bus::write`; welded per #11bu), halt-wake (5, unported
eager wake clock), enable_display **DS** glitch (5, all `_ds` — the `!ds` floor,
the DS read grid BRACKETS the rise), accessibility exits vram_m3/cgbpal_m3/
oam_access (11, Part-B write-frame, #11ck measured wrong-direction), lycEnable
FF41/FF45 write-frame (5), irq_precedence (4). 22 of 51 are `_ds`.

DMG (51): **window render-length (27)** — the #11ck-refuted DMG write-commit
A/B trade — halt-wake (6), ly0 (3), the rest mixed.

**Next lever (single, precise):** the eager halt-wake clock port (the eager
analogue of the tier2 `stat_vis_from_t`/`m0_halt_hold` grid) — 5 CGB + 6 DMG
`halt/*_m0stat` rows, a self-contained clock separate from the read frame. The
FF0F write-race (`ifw`) and DMG window families are the documented atomic/welded
floors; do not re-chase without a new mechanism.

## Reproduction

```
git checkout halfdot-partb    # this session's tip
CARGO_TARGET_DIR=target/agB2 cargo build -p slopgb-core --example run_mooneye --release
RM=target/agB2/release/examples/run_mooneye
ROM=test-roms/game-boy-test-roms-v7.0/mooneye-test-suite-wilbertpol/acceptance/gpu/intr_0_timing.gb
SLOPGB_WILBERT=1 SLOPGB_EAGER=1 $RM $ROM cgb   # PASS B=03
SLOPGB_WILBERT=1 SLOPGB_EAGER=1 $RM $ROM dmg   # PASS B=03
# EV two-bins:
CARGO_TARGET_DIR=target/agB2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agB2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 361
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=  # 98
```
