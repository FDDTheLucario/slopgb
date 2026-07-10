# L1 — the CGB double-speed re-host of the shipped eager slices (2026-07-10, #11da)

Task: extend the shipped `!self.ds`-scoped eager slices to double speed with the
DS read-debt, recovering the ~19 CGB DS bar rows #11cs flagged as un-ported (an
SS sibling already passes under eager). **Result: EV CGB 358 → 348 (−10), two
clean flag-gated families shipped, 7 TRUE-bar DS rows + 3 gains recovered, all
hard gates hold. Two families measured as half-dot-blocked (pair-shuffle) and
mapped, not forced; the 9-row dispatch/IF web is left to the concurrent #11db
write-commit effort.**

## Baseline (reproduced end-to-end at `e307e7a`)

EV CGB **358**, tier2 CGB **291**, EV DMG **116**, OFF CGB 486 — all reproduced
exactly. DS flip-bugs (OFF-pass ∩ EV-fail, `_ds` rows) = 31, of which the
classifier (`classify_cgb_regr.py`, SameBoy tester) puts **21 SameBoy-pass (TRUE
bar)** / 10 floor.

The 21 TRUE-bar DS rows: cgbpal_m3 2, oam_access 2, vram_m3 2 (accessibility);
enable_display m0irq 4, irq_precedence 2, m2int_m0irq 3, m2int_m0stat 1,
m2int_m2stat 2 (IRQ/STAT); lcd_offset 2 (STOP-shift); sprites 1.

## The mechanism split (why the task's "un-scope `!self.ds`" premise was half-right)

The shipped SS eager slices carry `!self.ds` in `blocking.rs`/`mode0.rs`, but the
DS bar rows do NOT all fall out of un-scoping them. The eager clock routes
accessibility and STAT reads through TWO different mechanisms, and the DS bar
splits by which one a row uses:

- **Accessibility (OAM/VRAM/palette) reads** resolve at the eager `Bus::read`
  *trailing* `read_no_tick` (post-`tick_machine`, cc+4), gated by the PRODUCTION
  `m0_access_edge`/`pal_access_edge` whole-M-cycle straddle stamp — which is
  mis-framed at double speed (the eager mode-0 flip lands at the reclocked render
  dot). The stamp short-circuits BEFORE `ppu.read`, so the ported
  `Ppu::{oam,vram,pal}_*_blocked` DS laws (which already carry `|| eager_value`)
  never ran. Both eager and tier2 land at the SAME dot; the divergence is
  stamp-vs-ported, not a dot offset.
- **FF41 STAT reads** resolve at the eager *leading* `leading_edge_sample`
  (`vis_mode_read`, cc+0 pre-tick value peek). The leading sample sits a DS
  M-cycle (2 dots) before the trailing view, so line-boundary mode reads fed by
  `vis_mode()`'s raw FSM saw the un-shifted boundary.

## Family 1 — DS accessibility stamp bypass (SHIPPED, `7916536`)

**EV CGB 358 → 353, clean +5/−0.** Route eager DS accessibility through the
ported `Ppu::*_blocked` laws by taking the same stamp bypass `tier2_reclock`
already takes: a new `Interconnect::ev_ds_access()` (`eager_value &&
double_speed`) added to the 5 stamp sites in `interconnect/memory.rs` (OAM read,
VRAM read, palette read, OAM write, CGB FEA0 extra-OAM read). The VRAM WRITE
stamp stays on both paths (co-temporal `vramw_m3end`, unchanged). Single speed
keeps the stamp (it passes under eager); production + tier2-off byte-identical.

| recovered (SameBoy-pass, TRUE bar) |
|---|
| oam_access/postread_scx5_ds_2, postwrite_scx1_ds_2 |
| vram_m3/postread_scx5_ds_2 |
| cgbpal_m3/cgbpal_m3end_ds_2, cgbpal_m3end_scx5_ds_2 |

The 6th accessibility bar row `vram_m3/preread_ds_lcdoffset1_1` stays parked (the
STOP-shift `lcd_shift_dots` frame is unported on the eager clock, #11bz). Pin:
`eager_ds_access_passes` (5 recovered + 5 `_1` regression guards).

## Family 2 — DS mode-2→3 entry back-date (SHIPPED, `28397b7`)

**EV CGB 353 → 348, clean +5/−0.** The eager cc+0 FF41 value peek samples the
PPU pre-tick, a DS M-cycle before the trailing cc+4 view, so a line-start FF41
read straddling the mode-2→3 boundary saw the un-shifted dot-84 entry as mode 2
where SameBoy's cc+4 view reads mode 3. `Ppu::mode3_entry_dot` now back-dates DS
to **80** (as single speed — the leading peek is a full pre-tick read, so it
takes the SS back-date, not the +2 DS read-debt) under `eager_value && self.ds`.
Tier2's deferred DS frame keeps 84.

| recovered | class |
|---|---|
| m2int_m2stat/m2int_m2stat_ds_2, m2int_scx4_m2stat_ds_2 | TRUE bar |
| enable_display/frame0_m3stat_count_ds_2, frame1_m3stat_count_ds_2 | gain (OFF-fail) |
| lcd_offset/offset1_lyc99int_m3stat_count_ds_2 | gain (SameBoy=00, gambatte-want=90) |

Pin: `eager_ds_mode3_entry_passes` (4 recovered + 2 `_1` mode-2 guards).

## Families measured half-dot-blocked (NOT shipped, parked)

- **m2int_m0stat DS line-start pair (1 bar).** Same line-start region as family 2
  but the mode-0→2 boundary at dot 0-3, where the `_1`/`_2` pair straddles dot 2.
  The line-start arm's `+2` DS debt (`read_laws.rs:164`) was load-bearing: forcing
  it to fire at dot 0 (the eager leading dot for `_2`) recovered 5 halt/m0int
  m0stat rows but **broke 6** (`lycint_m0stat_ds_1`, `m0int_m0stat_ds_1`,
  `lycint_lycflag_ds_1/3`, `lyc0flag_ds_3`, `lyc_ff45_trigger_delay_ds_1`) — the
  `_1` siblings' leading peeks collapse onto the same whole-dot as `_2`. This is
  the DS mid-dot floor #11cb/#11by named; needs the half-dot read. Reverted.
- **sprites/10spritesPrLine_wx7_m3stat_ds_2 (1 bar).** The DS sprite mode-3-exit
  read. tier2 recovers it via the PRODUCTION `stat_mode_edge` whole-M-cycle stamp
  (`tier2_sprite_m3stat_ds_passes`), which the eager FF41 leading peek bypasses.
  The eager `vis_exit_hd` DS sprite arm is the parked mid-dot floor. Not chased.

## Left to other levers (out of L1 read-frame scope)

- **The 9-row dispatch/IF web** — enable_display m0irq 4, irq_precedence 2,
  m2int_m0irq 3. These are FF0F IF-rise reads whose eager value differs by the
  IRQ dispatch / IF-commit timing, not a read-frame miss (traced: `m2int_m0irq_ds_2`
  reads intf at cc+4 where production's rise lands elsewhere). This is exactly the
  concurrent **#11db** "CGB SS dispatch/IRQ web — 2 read-frame + 14
  write-commit-frame" domain; the DS versions want the same write-commit
  machinery. Left to that effort.
- **STOP-shift lcd-offset (3)** — lcd_offset m0irq/m0stat_count_ds_1 (2) +
  vram preread_ds_lcdoffset1_1 (1). The machine STOPADV `lcd_shift_dots` frame is
  unported on the eager clock (#11bz parked lever).

## Result

| | before (#11cs base was 361/49; this run `e307e7a`) | after L1 |
|---|---:|---:|
| EV CGB fails | 358 | **348** |
| DS TRUE-bar (of 49 CGB bar) | 21 | **14** |

**7 TRUE-bar DS rows recovered** (5 accessibility + 2 m2int_m2stat) + **3 flip
gains**. Remaining 14 DS bar = 9 dispatch/IF web (#11db) + 3 STOP-shift + 1
m0stat pair-shuffle + 1 sprites — all parked or another lever's domain.

## Gates (both shipped commits)

- golden_fingerprint byte-identical (production = flags false; both changes
  `eager_value`-gated).
- tier2 CGB two-bin **291** (unchanged — `|| eager_value` no-op under tier2; the
  entry back-date's `eager_value && ds` branch never fires under tier2).
- EV DMG **116** unchanged (DMG has no double speed; all arms cgb+ds gated).
- mooneye **92** flag-off AND `SLOPGB_MOONEYE_EAGER=1` AND
  `SLOPGB_MOONEYE_RECLOCK=1`.
- eager tripwires (both models): `intr_0_timing`, `intr_2_mode0/mode3/oam_ok/
  0_timing`, `di_timing-GS` — all `B=03 C=05 D=08 E=0D H=15 L=22`. The entry
  back-date is `intr_2_mode3`-adjacent; it passes both models.
- clippy `-D warnings` clean (default + `port_probe`); every `.rs` < 1000.
- interconnect/engine reclock defaults NOT flipped.

## Reproduction

```sh
CARGO_TARGET_DIR=target/agDS cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agDS/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$PWD/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 SLOPGB_REQUIRE_ROMS=1 \
  $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 348
```

Classify DS flip-bugs: `comm -23 ev.keys off.keys | grep _ds > ds.rels;
python3 docs/sameboy-port/tools/classify_cgb_regr.py ds.rels`.
