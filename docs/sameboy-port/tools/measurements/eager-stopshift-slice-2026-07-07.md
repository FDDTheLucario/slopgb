# The EAGER STOP-shift frame port — the largest block LANDED CLEAN (2026-07-07, #11ca)

Task: continue the eager-value re-host from #11bz (EV CGB 516). Port the named
**STOP-shift / lcd-offset frame** (`lcd_shift_dots`/`law_pos` + the post-switch
exit-table anchors) to the eager clock — the single largest remaining block
(speedchange + lcd_offset, ~99 EV CGB fails). **Result: EV CGB 516 → 462 (−54),
one clean family shipped flag-gated (`|| eager_value`); all hard gates hold
(golden byte-identical, tier2 CGB two-bin 291 unchanged, mooneye 91/91 OFF+tier2,
eager intr_2 PASS, EV DMG 147 unchanged).**

## The lever: the switching-STOP handler installs the phase shadows under eager too

Under tier2 the switching-STOP handler (`interconnect/speed.rs` `stop_impl`)
installs the PPU↔CPU phase offset the gambatte-modeled pause leaves:
`lcd_shift_dots` (via `add_lcd_shift(k/2)`), the `sb_dsa8` correction
(`dsa_pause_correction`), and the post-switch exit-table anchors
(`note_switch_stop` → `stop_anchor_midframe`; `note_switch_leave(k)` →
`stop_leave_lcd_on`/`stop_leave_k`). The whole install block gated on
`if self.tier2_reclock`. Under eager it never fired, so `lcd_shift_dots` stayed 0
and the `speedchange`/`lcd_offset` reads classified on the WRONG (un-shifted)
frame.

**The fix is two gate flips (`speed.rs`), nothing else:**

```
speed.rs:224  note_switch_stop:   tier2_reclock          →  tier2_reclock || eager_value
speed.rs:356  the K-realign block: tier2_reclock          →  tier2_reclock || eager_value
```

Everything downstream **auto-activates** — no consumer change was needed, because
the `law_pos` consumers already key on `lcd_shift_dots`/`leading_edge_reads` (both
correct under eager: `leading_edge_reads` is on, `lcd_shift_dots` is now installed)
and the `vis_exit_hd` post-switch exit-table arms already key on
`stop_anchor_midframe`/`stop_leave_*`:

| consumer | keys on | site |
|---|---|---|
| `hdma_period_law` | `lcd_shift_dots == 0` else `law_pos` | `access.rs:167` |
| STAT write-trigger (eng_lyc / lyc_carryover / lyc_wrap_153 / vis0) | `lcd_shift_dots == 0`, `leading_edge_reads` | `stat_irq.rs:498/552/615` |
| FF0F m0-coincident poll suppress | `lcd_shift_dots == 0` | `ff0f.rs:67` |
| FF41 two-phase engine view | `lcd_shift_dots == 0`, `leading_edge_reads` | `regs.rs:396` |
| shifted line-start re-latch | `lcd_shift_dots > 0`, `leading_edge_reads` | `lyc.rs:266` |
| VRAM/pal-RAM/OAM lock law-pos | already `tier2 || eager`, reads `lcd_shift_dots` | `blocking.rs:267/293/347` |
| post-switch bare-exit table | `stop_anchor_midframe`/`stop_leave_k`/`lcd_enable_in_ds` | `read_laws.rs:642/682` |

`sb_dsa8` (the `double_speed_alignment` shadow) is advanced +2/tick in `Ppu::tick`
UNCONDITIONALLY (flag-independent), so the leave `k = if sb_dsa()&7==4 {6} else {2}`
computes identically under eager — no extra plumbing.

## Measured: CLEAN 61 fixed / 7 broke (8.7:1), NOT a shuffle

Two-bin EV CGB **516 → 462 (−54)**. Diff of the fail lists (branch
`finish-port-halfdot`, `SLOPGB_PROBE_EV=1`):

| fixed (61) | broke (7) |
|---|---|
| speedchange 45, lcd_offset 4, ly 2, late_ff 2, prewrite_ds_lcdoffset 1, oamdma_late_speedchange 1, lyc 1, late_enable(_lcdoffset) 2, ff 1, cgbpal_read/write_m 2 | offset1_lyc99int_m0{irq,stat}_count_scx2_ds_{1,2} (2), offset3_lyc98int_ly_count_1 / offset3_lyc99int_m0stat_count_scx1_2 / offset3_lyc99int_m3stat_count_2 (3), lycstatwirq_trigger_..._lcdoffset3_2 (1), preread_lcdoffset1_1 (1) |

The 45-row speedchange bulk are the mid-frame-anchored dances that need
`stop_anchor_midframe`/`stop_leave_k` — the post-switch exit-table arms
(`read_laws.rs:642/682`) that only the STOP install sets. That is the real
mechanism, not an A/B shuffle.

## The 7 breaks are the sub-dot poll-phase floor (NOT read_carried — that is inert here)

All 7 breaks are **polled** `count`/accessibility/`lycstatwirq` rows — none is an
ISR-first-read — so item 3a (arm `read_carried` on the eager dispatch) does
**NOTHING** for them (the count-loop polls are `!read_carried`; the shifted arm at
`read_laws.rs:129` is `!read_carried`-scoped). They are the `_1`/`_2` **sub-dot
poll-phase splits** `read_laws.rs:110-128` already documents as *"the whole-dot
frame carries NO other observable — the true split is the sub-dot poll phase, not
resolvable in this frame."* The eager `tick_machine` clock is whole-dot
(`dhalf` always 0), so it cannot represent the DS/offset mid-dot (SameBoy
`cfl D+3` DS), and a uniform `sb_dsa8`-based +5hd term can't split a pair
differing ONLY by sub-dot — it would shuffle. This is the genuine half-dot-clock
floor, not a 4-line arm.

## What was considered and NOT done (measured reasoning, no code)

- **read_carried arming on the eager dispatch (item 3a):** INERT for these breaks
  (all polled, above) AND high shuffle risk — the eager FF41 read uses
  `leading_edge_sample` (cc+0, `&self`), which never clears `read_carried` (only
  `read_deferred` does, `cycle.rs:233`), so a blanket arm would LEAK into every
  later DS FF41 poll; and the SS m2int/m0int ISR reads are calibrated for
  `read_carried == false` under eager (the #11by coupling fixed them uncarried),
  so arming flips them to the carried base and breaks the green set. Not attempted.
- **sb_dsa8 +5hd mid-dot (item 3b):** the pair-splitting sub-dot term — a uniform
  rule shuffles the `_1`/`_2` pair (one wants +4, the other +5). Needs the
  per-T half-dot clock (the HALFDOT rewrite), not a constant.

## Gate state (ALL hold, verified this run)

- golden_fingerprint (`--release`) PASS — production byte-identical (`eager_value`
  off → the STOP block does not fire; `note_switch_stop`/`add_lcd_shift` never run).
- mooneye OFF 91/91; mooneye tier2 (`SLOPGB_MOONEYE_RECLOCK=1`) 91/91; **tier2 CGB
  two-bin 291 (unchanged — `|| eager_value` is a no-op under tier2, which already
  gated on `tier2_reclock`).**
- mooneye EAGER (`SLOPGB_MOONEYE_EAGER=1` acceptance_ppu): only `lcdon_timing-GS`
  ×4 fail (pre-existing exemption); `intr_2_mode0/mode3/sprites` PASS (dispatch
  stays cc+4 — no dispatch-moving law enabled).
- EV DMG 462-run rowlist: fail 147 (unchanged — STOP speed-switch is CGB-only:
  `switching = cgb_mode && key1_armed`, false on DMG, so the block never installs).
- clippy `-D warnings` clean (default + `port_probe`); every `.rs` < 1000.

## The exact next lever (priority order)

1. **The DS/offset sub-dot poll phase (the 7 residuals + the DS `_2` legs).** The
   whole-dot floor above — needs the per-T half-dot clock so `read_pos_hd` can
   carry the DS mid-dot (`dhalf==1`, SameBoy `cfl D+3`). This is the HALFDOT Part A
   rewrite the maps flag; a constant cannot split the `_1`/`_2` pairs.
2. **The parked SS render/read families (biggest remaining blocks at 462):**
   `late_m*` ~30, `hdma_late_m*` ~25, `frame*` ~19, `lycint*` ~18, `hdma_*`/
   `late_sizechange`/`enable`. These are the eager clock's native-recovery targets
   (the dispatch/wake/HBlank-DMA families) — each needs its tier2 law re-hosted
   `|| eager_value` per-family (the #11bz pattern), measured for clean net gain.

## Reproduction

`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head -1)`;
`SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored
gambatte::flagon_probe::flagon_probe --nocapture | grep pass=` (exact test path).
tier2 two-bin: same WITHOUT `SLOPGB_PROBE_EV` (→ 291). Commit: `537c9dc`.
