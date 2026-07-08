# The EAGER DS read-debt + DS accessibility/palette slice — the DS lever LANDED CLEAN (2026-07-07, #11bz)

Task: continue the eager-value re-host from #11by (EV CGB 553). Drive it toward
tier2's 291 by porting the named **DS sub-M-cycle alignment** lever + accumulate
clean families. **Result: EV CGB 553 → 516 (−37), three clean families shipped
flag-gated (`|| eager_value`), all hard gates hold (golden byte-identical, tier2
CGB two-bin 291 unchanged, mooneye 91/91 OFF+tier2, eager intr_2 PASS, EV DMG
147 unchanged).**

## The DS read-debt is speed-halved — the lever that #11by's uniform +8 missed

#11by's warning: un-`!ds`-scoping the read web with the SS +8 hd read-debt only
SHUFFLES the DS `_1`/`_2` pairs (34-fix/34-break) — a uniform shift can't
separate them. The ROOT CAUSE it missed: **the eager→deferred read-debt is
speed-dependent.** A CPU M-cycle is 4 dots at single speed but **2 dots at
double speed** (the CPU runs 2×), so the deferred DS read lands only +2 dots
(+4 hd) ahead of the eager cc+0, not +4 dots. The tier2 DS exit constants
(`vis_exit_hd`'s `ds1` term + the DS arms) are calibrated to that +2-dot
deferred position. Splitting the debt lands the eager DS read on the tier2 DS
frame and the pairs separate on their real exit:

```
read_pos_hd:  base + eager_value ? (ds ? +4 hd : +8 hd) : 0
vis_mode_read gate:  tier2_reclock || eager_value    (was: || (eager_value && !ds))
```

Measured `553 → 525` — **CLEAN 34 fixed / 6 broke** (5.7:1), NOT the 34/34
shuffle +8 gave. Fixed: window +12, m2int_m3stat +9, dma +8, speedchange +2,
lcd_offset +2, m0int_m3stat +1. This is the principled DS frame, not a swept
constant: +8 was 34/34 (539, shuffle), +4 is 34/6 (525, clean).

## Three clean families (each measured, gated `tier2 || eager_value`)

| # | family | file | EV CGB | fixed / broke |
|---|---|---|---|---|
| 1 | **DS read-debt** (`read_pos_hd` SS +8 / DS +4; un-`!ds`-scope `vis_mode_read`) | `engine.rs`, `read_laws.rs` | 553 → 525 | 34 / 6 |
| 2 | **DS OAM/VRAM accessibility release** (`ds_lineend_open`, DS VRAM read/write lock + line-end release, `cgb_linestart_oam_open`) | `blocking.rs` | 525 → 517 | 10 / 2 |
| 3 | **CGB palette-RAM pipe-end release** (`pal_ram_blocked` render-finished unblock + SS m3-entry lock) | `blocking.rs` | 517 → 516 | 1 / 0 |

Families 2+3 port for FREE once family 1 aligns the DS read: the accessibility
predicates evaluate the blocked verdict at the eager cc+4 dot, which equals the
tier2 deferred read dot (both cc+4-equivalent), and `pal_open_dot` is recorded
unconditionally in the dot-clock render → available on the eager clock.

## The residuals (the 8 principled breaks + the un-moved DS legs)

- **DS pre-draw-abort `_2` legs (4):** `late_disable_early_scx00_wx0f/10/11/12_ds_2`
  — arm 4 (`vis_exit_hd`, DS abort boundary `(89+WX)&!1`, exit 254). Their `_1`
  siblings were FIXED; the `_2` pair-partner (differing by abort_dot) needs the
  DS mid-dot the eager whole-dot clock can't represent.
- **DS ISR-carry legs:** `read_carried` / `isr_read_carry_hd` (the mode-2 OAM +4 /
  mode-0 HBlank +2 sub-M-cycle carry) is armed ONLY on the tier2 deferred
  dispatch path (`dispatch_retime_impl`, `speed.rs:473`). The eager dispatch is
  cc+4 (production) and never calls it → DS ISR reads carry 0. `m0int_m3stat_ds_1`
  and similar stay parked.
- **STOP-shift (`lcd_offset`/`lcdoffset`) legs:** the machine STOPADV advance
  (`lcd_shift_dots`, `law_pos`) is not tracked on the eager clock (0 under eager),
  so shifted-poll reads land at the unshifted dot. 2 accessibility breaks
  (`vram_m3/{preread,prewrite}_ds_lcdoffset1_1`) + the whole `lcd_offset`/
  `speedchange` families.

## What was INERT / reverted (measured, not guessed)

- **The `regs.rs` render-view commit_eff latches** (`render_lcdc_pending`,
  `window_abort_render` defer, `win_reenable_dot`/`win_enable_dot`,
  `scx_write_dot`, `staged_pending`) gated `|| eager_value`: **FULLY INERT** —
  byte-identical fail set (0 fixed / 0 broke). The remaining SS window fails are
  NOT blocked by the render-view latch frame; the write stage already expires
  during `tick_machine` under eager (strobe_tick at dot D+3), so the latches are
  not the lever. Reverted (no dead gates).

## The exact next lever (in priority order)

1. **The DS sub-dot (`sb_dsa8` mid-dot + `read_carried` ISR carry).** The eager
   whole-dot clock (`tick_machine` ticks the PPU whole-dot, even-cc at DS,
   `dhalf` always 0) cannot represent the deferred DS read's mid-dot (`dhalf==1`,
   the odd-CPU-T read) or the ISR sub-M-cycle carry. To recover the ~6 DS
   pre-draw-abort + DS ISR + remaining DS window legs: (a) reconstruct the DS
   half-dot phase from `sb_dsa8` (SameBoy `double_speed_alignment`, maintained
   under eager via `engine.rs:206`) so `read_pos_hd` adds +5 hd (not +4) for an
   odd-aligned DS read; (b) arm `read_carried` on the EAGER dispatch path (hook
   the cc+4 dispatch to `set_read_carried(true)` for OAM/HBlank STAT ISRs) so
   `isr_read_carry_hd` applies. Both are the DS analogue of the SS coupling.
2. **The STOP-shift frame (`lcd_shift_dots`/`law_pos`) port.** The
   `speedchange`/`lcd_offset` families (73+26 EV CGB fails, the largest blocks)
   need the machine STOPADV advance tracked on the eager clock. This is the
   biggest remaining lever by row count but a multi-mechanism port (the STOP
   leave/anchor/dsa-correction shadows in `engine.rs`).
3. **The SS render/read families still parked** (`dma` 73, `halt` 43,
   `enable_display` 35, `lycEnable` 28) — the dispatch/wake/HBlank-DMA families;
   these are the eager clock's native-recovery targets (foundation doc) but need
   their tier2 laws re-hosted per-family.

## Gate state (ALL hold, verified per commit)

- golden_fingerprint (`--release`) PASS — production byte-identical (`eager_value` off).
- mooneye OFF 91/91; mooneye tier2 (`SLOPGB_MOONEYE_RECLOCK=1`) 91/91; **tier2 CGB
  two-bin 291 (unchanged — `|| eager_value` is a no-op under tier2, and the
  eager-debt branch in `read_pos_hd` never fires under tier2).**
- mooneye EAGER (`SLOPGB_MOONEYE_EAGER=1` acceptance_ppu): `intr_2_mode0/mode3/
  sprites` PASS; only `lcdon_timing-GS` ×4 fail (pre-existing exemption).
- EV DMG 147 (unchanged — DMG has no double speed, and the DS/palette arms are
  cgb+ds gated).
- clippy `-D warnings` clean (default + `port_probe`); every `.rs` < 1000.

## Reproduction

`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head
-1)`; `SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN
--ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=` (exact
test path). tier2 two-bin: same WITHOUT `SLOPGB_PROBE_EV`. Commits: `ed8116e`
(DS debt), `9a95514` (DS accessibility), `3fc1456` (palette).
