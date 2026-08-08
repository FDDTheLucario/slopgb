# PPU — window, fetch, OAM scan, mealybug, OAM bug

## Dot-serial OAM scan

`ppu/mod.rs` §Dot-serial OAM scan.

- Entry `i` is latched + evaluated at dot `2i+3` (gbctr; gambatte OamReader — `scan_latch_dot` anchoring pinned by gambatte oamdma/late_sp* + sprites/late_sizechange* per-slot races).
- Per-entry LCDC.2 sampling, off a one-dot-old snapshot (`Ppu::scan_obj_size`,
  taken at the end of each dot in `Ppu::tick`) rather than the live `eff` bit.
  **DMG reads the snapshot alone; CGB reads it OR'd with the live bit.** Our FF40
  commit lands two dots earlier on DMG than on CGB, so the snapshot puts the DMG
  scan sample on the same phase relative to the write that CGB's live read
  already sits on — a write on an entry's own latch dot loses the race on DMG.
  On CGB the OR only differs from the live bit on the dot the commit lands, and
  there it resolves TALL whichever way the bit moved. Pinned by the gambatte
  `late_sizechange*` / `late_sizechange2*` per-slot ladders (sp00/sp01/sp02/sp39
  = latch dots 3/5/7/81, write dot stepped 4 per rung, both write directions);
  the four boundary rungs `late_sizechange_{sp01,sp39}_2` (shrink) and
  `late_sizechange2_{sp01,sp39}_1` (grow) sit at the same write and latch dots —
  their ROMs differ only in the two swapped LCDC constants — and all four select
  the sprite, which is what forces the OR rather than a second sample dot. Whole
  family green: 20/20 DMG, 20/20 CGB. Unit test:
  `oam_scan_obj_size_sample_lags_the_commit_on_dmg`.
- The *fetch*-time LCDC.2 re-read (`fetch_sprite`) stays live on both models.
- Parked, measured: the seven `late_sizechange*_ds_1` rungs. In double speed the
  `_1` rung writes FF40 one dot after the entry's latch (latch 3, write 4; latch
  81, write 82) and the reference still latches the NEW height — in BOTH write
  directions, so it is not the CGB OR. The `_2` rungs (write at latch + 3) want
  the OLD height and already pass. So the reference has the write visible to the
  scan from the START of its double-speed M-cycle, where our FF40 `eff` commit
  lands at the M-cycle END. No lag or OR of past views can reach it: the value
  the latch needs is in our future. Fixing it means moving the DS FF40 commit a
  dot earlier, which is a global render/read-law change, not a scan-local one.
  All seven are bucketed EXCEED — SameBoy misses them too.
- While OAM DMA owns OAM (running, or halt/stop-frozen) the scan latches `$FF` — a disabled sprite (`Ppu::oam_dma_active`, edges = gambatte startOamDma/endOamDma: the first byte's cycle still latches real OAM; the disconnect outlives the last copy by one M-cycle).

Parked: chasing the residual late_sp `_ds` out3 rows (half-dot, cc-granular races compounded with the frozen-ds mode-0 flip lead) or strikethrough's 7-px residue (an undocumented glitch-sprite, see `smallsuites.rs`) with **whole-dot** timing — don't chase either; whole-dot granularity can't resolve them.

## Window machine

`ppu/render/window.rs`.

- WX comparator runs every dot, including the 8-dot prefill. Match position by WX value:

  | WX value | Match dot/column |
  |---|---|
  | WX 0-7 | pause-aware dot `WX+6` (sprite stalls shift it via `pos_dot`) |
  | WX >= 8 | `lx == WX-7` |
  | WX <= 166 | (upper bound; above this never matches) |

- Rising-edge only (`win_match_prev`); checked **before** the same-dot sprite trigger (window start wins).
- `win_line` = gambatte winYPos (`0xFF` at frame start, `++` per activation, so same-line retriggers draw the next row).
- LCDC.5 off mid-line aborts at the eff commit, with the BG resuming on the live column `(scx+x+1-cgb)/8` (`window_abort_render`; `window_abort_flags` carries the pre-draw / DMG-abort classification).
- WX=166 on DMG never starts in-line — instead a 2-dot freeze + carryover into the next line's mode-3 start (`win_start_pending`, window drawn from col 1).
- A WX match while drawing injects one color-0 pixel when it lands on a window tile boundary (mealybug "reactivation").
- WX=0 start adds `SCX&7+1` discards.

#### Measured, NOT landed: the double-speed WX render-commit class (2026-08-03)

At single speed WX (FF4B) carries its own render-commit debt (12, against the
uniform 8) because "its render stage is the smallest, so it needs the largest
debt to reach the WX activation comparator" (`regs/stage.rs`). Double speed has
no WX arm — everything takes the uniform 4. Giving it one, `0xFF4B => 8`, scores
**+2/−0** on the full collection: `window/late_wx_ds_2` and its pixel-exact
`window/on_screen/late_wx_ds_2` sibling.

The scenario, probed from the ROM: WY=0 and WX=7 are set in VBlank with the
window enabled, so the window matches at screen x 0; then on line 1 WX is moved
to 255 mid-mode-3, at dot 92 (`_1`) or 94 (`_2`). At dot 92 the write still
suppresses the window (the FF41 read at dot 258 wants mode 0); at dot 94 it does
not, and the extended mode 3 is what the read sees. Our uniform-4 commit lands
early enough to suppress in both.

**Not landed for want of a pin.** Five reproductions were built; none
discriminates. Against a bare `Ppu`: replaying the ROM's write order leaves
`render.win_active` false at both dots, and probing the `eff.wx` commit dot
directly shows no difference between debt 4 and 8 — writes straight to `Ppu`
never reach the staged-commit path. Through the **interconnect** (which does
reach it, `interconnect_tests`): arming the window right after the enable makes
line 1 the glitch line's successor and both dots read mode 0 (the fix changes
nothing); arming from VBlank so line 1 is an ordinary line of a running frame
overshoots the other way and both dots read mode 3, losing the `_1` arm the ROM
pins. The deadline evidently depends on further line state the replay does not
carry — the window having already run on earlier lines, and whatever the
suite's `_1`/`_2` share beyond WY/WX/LCDC.

So the constant is corpus-fitted only. Landing it needs the ROM's own state
reproduced (drive the actual `late_wx_ds` kernel, or diff its full render trace
against the replay to find the missing term) — not a sixth guess at the setup.

### WY sampling

- Discrete weMaster sampling at dots 450/454 (+1 DMG) and line-0 dot 2.
- Plus a live compare against `wy2`, which lags the write per model:

  | Model | `wy2` write lag |
  |---|---|
  | DMG | 2 dots |
  | CGB | 6 dots |
  | ds (double-speed) | 5 dots |

- WX commits to the pipeline 1 dot later than the palette strobe (`stage_write` FF4B dots+1, pinned by m3_wx_4/5/6_change).
- The shadow WY trigger's extend deadline (`Ppu::win_extend_deadline`, consumed
  by `win_extends_sb`) is `wx_match_dot + discard + phase`. The discard is the
  fine scroll the window's own fetch waits out — `hunt_fine`, the value the
  comparator locked in, so a late SCX rewrite that missed the hunt does not
  count — and it applies only to a **WX >= 7** window; a low-WX window is already
  fetching through the prefill. Phase: +1 CGB single speed, +2 CGB double speed,
  −3 on the DMG family (whose shared LYC=153 ISR fires one M-cycle earlier off
  the dot-4 emission decouple). Pinned by the gambatte
  `arg/late_wy_FFto2_ly2{,_scx2,_scx3,_scx5}{,_ds}` sweep — the WX match sits at
  dot 97 on every rung, the rungs step the WY write 4 dots apart, and the split
  moves one whole rung later at SCX 5 while SCX 0/2/3 share theirs — plus
  `arg/late_scx_late_wy_FFto4_ly4_wx00`, which writes SCX 4 under a WX 0 window
  and keeps the SCX-0 split. Unit test:
  `window_extend_deadline_tracks_the_fine_scroll_only_above_wx7`.

## Mode-3 fetch grid

`ppu/render/mode0.rs` `fetcher_step`.

- Every fetch VRAM access samples `eff` clean at its read dot on **both** families, except the two scroll registers, which are read from per-dot rings at a lead (`map_scx_formed` for the column, `map_scy_formed` for the row — see "Mid-mode-3 SCY" below).
- LCDC.1 gates sprite pixels at the mix as well as the fetch (m3_lcdc_obj_en_change).
- The fetch-side LCDC.1 gate is a **window straddling each sprite's fetch
  trigger dot `F`, not a sample at `F`** (`OBJ_ENABLE_LAG` = 4 dots, `ppu/mod.rs`):
  the bit must have been set for all of `F-4..=F` for the fetch to start
  (`Ppu::obj_fetch_enabled`, off the per-dot `obj_en_lag` history), and a clear
  landing anywhere in `F..=F+4` still cancels a fetch already charged
  (`Ppu::stall_tick` → `abort_obj_fetch`). A cancelled fetch drops its stall to
  1 remaining dot, un-marks its own `fetched` slot, and takes its pixels back
  out of the sprite FIFO (a fetch that never completes never pushed them —
  m3_lcdc_obj_en_change [Cgb] is the pin for the pixel half).
  Both edges and the 1-dot restart are pinned two-sided by three gambatte
  ladders over SPX $18-$1B, whose kernels move one write by one NOP per rung:
  `sprite_late_enable_spx*` (enable 4-7 dots before `F` pays the stall, 0-3
  dots before pays none), and `sprite_late_disable_spx*` +
  `sprite_late_late_disable_spx*`, which between them sweep the clear from
  `F-3` to `F+8` and keep the penalty only from `F+5` on. The two disable
  ladders read STAT one M-cycle apart, so the same clear offset appears in both
  with opposite demands — that pair is what fixes the restart cost at 1 dot and
  rules out a full refund. Unit test: `obj_enable_window_straddles_the_sprite_fetch_trigger`.
- Sprites with OAM X 0-7 fetch during the pause-aware prefill walk (`prefill_pos`), freezing the SCX hunt (gambatte spx0/spx1); penalty math unchanged (mooneye tables frozen).
- The OBJ **alignment** penalty is measured off whichever tile grid the fetcher
  is on (`Ppu::penalty_pos`). Normally that is the BG's `x + SCX`; a **WX 0-7**
  window owns the fetcher from before the first pixel pops, so its own column
  (origin `WX - 7`) sets the tile phase for the whole line. Pinned by the
  gambatte `space/10spritesPrLine_wx{0..7}_m3stat_ds` ladder: ten sprites one
  tile apart shed exactly 10 dots per step of WX (5 dots each at WX 7 down to 0
  at WX 2, floored), which fixes the origin two-sided. A window starting
  mid-line keeps the BG phase — `m0enable/enable_wxA6_2x_spxA7` puts a WX 166
  start at lx 159 with a sprite at OAM X 167 in the window's own first tile and
  charges no alignment there. Whether a *later* tile of a mid-line window
  switches phase is unpinned by the corpus, so it does not. Unit test:
  `obj_alignment_follows_a_left_edge_window_grid`.
- The BG fetcher free-runs through every sprite stall (prefill included), with the line's first push waiting for the pause-aware startup walk (`push_allowed`), keeping pixel 0 on its stall-shifted dot.

### CLOSED as class F: `scx_during_m3_spx2` [Cgb] is an unwritten palette entry

`scx_during_m3/scx_during_m3_spx2` [Cgb] misses by exactly **one pixel** — x=0
on lines 0-7 — while its `spx0`/`spx1` siblings and its own [Dmg] leg pass. The
reference wants `(33,146,108)` there, a *coloured* CGB palette entry; we emit
white. Traced through the mixer, the pipeline is right and the palette is not:

- the X=2 sprite IS fetched (line 0 dot 91, `lx` 0, data `lo=00 hi=7E`), so its
  px 6 lands in `sp_fifo[0]` with colour 2 and px 7 (colour 0) in slot 1 — only
  x=0 can show it;
- at the pop (line 0 dot 105) the mixer has `sp0=2`, `sp_bgprio=false`,
  `bg_attr=00`, LCDC `93`, so the sprite legitimately wins;
- but BOTH lookups return white: `objcol` (OBJ palette 0, colour 2) and `bgcol`
  (BG palette 0, colour 3) are `FFFFFF` in our palette RAM at that dot.

The palette RAM is what differs. The reference pixel is the 15-bit word
`$4261`, which appears NOWHERE in our palette RAM: the ROM's only palette writes
are at line 144 and line 0 dot 0 (traced, none blocked), and they set BG palette
0 to `0000 5294 2108 FFFF` and one OBJ byte — exactly the `cgb_boot.bin`
hand-off this port already reproduces (see "Frame skip and CGB boot palettes").
OBJ palette 0 colour 2 is never written by the boot ROM or the ROM, so it holds
**power-on** contents: `$FFFF` here, `$4261` on the CGB that was captured.

SameBoy does not reproduce it either — its tester disables `GB_random`, so its
power-on palette fill is all zero and its pixel there is `(0,0,0)` against the
reference's `(33,146,108)`. The row is therefore class F (the reference asset
bakes in one console's uninitialised palette RAM), not a chaseable timing bug,
and the fine-scroll discard / mixer / attribute paths are all ruled out
(`discard` is 0 on those lines; the mixer has the sprite winning with
`sp0=2`, `sp_bgprio=false`, `bg_attr=00`, LCDC `93`).

### The mid-mode-3 LCDC fetch view splits map from data (CGB single speed)

A mid-mode-3 LCDC write reaches the BG fetcher's **map**-select bits (BG bit3 /
window bit6) one dot after its **data**-select bit on CGB single speed —
`RENDER_LCDC_MAP_DELAY` 4 against `RENDER_LCDC_DELAY` 3 — because the fetch grid
reads the map byte a step ahead of the tile data, so one write lands on the two
views at different dots. DMG and double speed keep the common dot.

Derived by pixel-differencing, not swept: `bgtilemap_spx09_{1..4}` [Cgb] put
their whole error in ONE tile column (x 16-23 on lines 1-15, x 144-151 on line
0 — 8 px per line), and it is a one-tile-early switch to the new map, i.e. our
fetch picked the write up one tile sooner than the reference. The three
candidate scopes score:

| lever | score |
|---|---|
| `RENDER_LCDC_DELAY` 3 → 4 (uniform) | +8 / −34 (kills the `bgtiledata_spx*` data-bit rows) |
| map bits +1 dot, all models/speeds | +8 / −8 (kills the DMG + `_ds` map rows) |
| **map bits +1 dot, CGB single speed** | **+8 / −0** |

The +8 is `bgtilemap_spx09_{1..4}` [Cgb] plus the four hardware-captured
mealybug photos `m3_lcdc_{bg,win}_map_change{,2}` [Cgb] — the photo agreement is
what makes this a law rather than a fit. Unit test
`cgb_single_speed_map_select_lands_one_dot_after_the_data_bit`.

Parked: the rising-late CGB LCDC fetch view — tried and rejected. See the mealybug note below: it fits most `_cgb_c` photo columns but contradicts hardware-captured gambatte bgtiledata spx0B rows. Current law samples `eff` clean at the read dot on both families instead.

### Measured, NOT landed: the fine-scroll hunt latches one M-cycle early on the LCD-enable line (2026-08-06)

`enable_display/ly0_late_scx7_m3stat_scx1_2` (both models, SameBoy passes both)
is the only failing member of its family, and it is a **render-length** row, not
a read-frame one. The kernel enables the LCD, writes SCX = 7 late into line 0's
mode 3, and reads STAT; the `_1`/`_2` rungs move that write by one M-cycle and
read at the **same absolute instant** (SameBoy `SBREAD ff41 … fp=26311304` in
both).

| ROM | SCX write | SameBoy at the read | slopgb `hunt_fine` → flip |
|---|---|---|---|
| `…_scx1_1` (want 3) | our dot 84 / SB cfl 92 | mode **3** | 1 (old) → 253 |
| `…_scx1_2` (want 0) | our dot 88 / SB cfl 96 | mode **0** | 1 (old) → 253 |

So SameBoy's hunt still takes the NEW SCX for a write at cfl 92 (mode 3 extends
by `7`) and keeps the old one at cfl 96. slopgb takes the old value in both, and
the `_1` rung passes only because its read lands short of our flip anyway. Our
threshold sits one M-cycle early: `scx0_1` (write dot 80) latches 7 while
`scx1_1` (write dot 84) latches 1. The latch dot is base-SCX dependent (the hunt
is a live position comparator — `scx3_1`, also written at dot 84, does take 7),
so this is a comparator-position fix in `render.rs`, not a constant.

**Refuted, measured — do not retry:** delaying the glitch line's hunt start by
one dot (`mode3_dot >= 6` when `glitch_line`). It leaves `scx1_2` at `87` on
both models and costs `scx3_2` [Dmg] (84 → 87), because the row needs *two*
coupled changes, not one: the write at dot 84 must take the new SCX (lengthening
`scx1_1`'s mode 3, which our model shares with `scx1_2`) **and** the old-value
enable-line flip must land at or before 252 so the read at dot 248 sees mode 0
under the glitch line's 4-dot `early_lead`. Both need SameBoy's enable-line
mode-3 entry and flip measured per (base SCX, write dot) first — that
measurement, not another knob, is the next step here.

## Mealybug ppu state

Status of the `m3_*` ppu_state tests:

| Status | Tests |
|---|---|
| Pixel-perfect (both legs) | m3_bgp_change, m3_scx_low_3_bits, m3_window_timing, m3_window_timing_wx_0, m3_lcdc_win_en_change_multiple, m3_wx_4_change_sprites |
| Pixel-perfect, [Dmg]-only | m3_wx_4_change, m3_wx_5_change, m3_wx_6_change |
| Pixel-perfect [Dmg] legs | m3_lcdc_tile_sel_change, m3_lcdc_tile_sel_win_change, m3_lcdc_bg_map_change, m3_lcdc_win_map_change, m3_scx_high_5_bits, m3_bgp_change_sprites, m3_obp0_change |

Remaining (not yet pixel-perfect) legs are mostly:
- [Cgb] fetch-law residue — see the parked rising-late CGB LCDC fetch view above and the baseline comments (`_cgb_c` photo columns vs hardware-captured gambatte bgtiledata spx0B rows). `m3_scy_change` [Cgb] is down to 493 px on the SCY read frame below (`m3_scy_change` [Dmg] and `m3_scy_change2` [Cgb] now pass).
- Small [Dmg] bg_en / obj_en single-pixel residue.
- The obj_size pair.
- Sub-dot LCDC-write races: m3_lcdc_win_en_change_multiple_wx [Dmg] (m2_win_en_toggle now passes on both legs).

## DMG OAM corruption bug

- Implemented via `Ppu::oam_bug` + `Bus::tick_addr` / `read_inc`.
- DMG-family only; suppressed while halted / during OAM DMA.
- Window + patterns are CRC-calibrated against blargg `oam_bug/` — all green **except** 7-timing_effect, a defective single build that self-destructs on real hardware too (see the baseline note in `tests/gbtr/blargg.rs`).

## Post-boot VRAM (boot logo)

- Post-boot VRAM holds the boot logo *tile data* (incl. the (R) tile `$19`; `install_boot_logo_vram`).
- Do: leave the DMG logo tile-**map** rows uninstalled — the pinned gambatte reference PNGs predate initial-VRAM modelling (see the doc comment), and gambatte's `_blank` halt ROMs are judged on the top tile row only.

## Frame skip and CGB boot palettes

- The first frame after an LCD enable is presented blank (`Ppu::frame_skip`, Pan Docs LCDC.7 / SameBoy frame-skip) — frame-compare harnesses must sample >=2 vblanks after the ROM's re-enable.
- CGB DMG-compat boot palettes are the real boot-ROM *defaults* (BG table != OBJ table, `interconnect.rs`).
- Hand-off palette RAM splits on the **cart's** CGB flag, not the hardware (`apply_post_boot_state`). A DMG-flagged cart takes the compatibility palettes and leaves BCPS `$C8` / OCPS `$D0`; a CGB-flagged cart never reaches that code and inherits the boot logo's own state — BG palette 0 = `0000 5294 2108 FFFF`, BG 1-7 white, one OBJ byte cleared (`FF00` in palette 0 colour 0), BCPS `$C8` / OCPS `$C1`. Both arms are byte-for-byte what `bootroms/cgb_boot.bin` (also `cgbE`/`cgb0`) hands off, and `misc/boot_hwio-C` is itself DMG-flagged so it pins only the compat arm. Pinned by `cgb_flagged_cart_keeps_the_boot_logo_palettes`.
- Do: leave the Nintendo-licensee title-hash table deliberately unmodelled.

## PPU interrupt raising

- The PPU raises STAT/VBlank IRQs via `Ppu::write`'s return value (single drain).
- When adding a PPU register path, OR the returned IF bits into `intf` like the existing interconnect call sites.

## Mid-mode-3 SCY and the BG map row (single speed CLOSED 2026-07-31)

`map_scy_formed` (`ppu/render/mode0.rs`). The row half of the same law
`map_scx_formed` carries for the column: the fetch reads SCY from a per-dot ring
(`Render::scy_ring`, sharing `scx_ring_i`) at a lead, not live at the access.

| leg | lead | plus the line's first visible tile |
|---|---|---|
| CGB single speed, no sprite on the line | 2 | +2 |
| CGB single speed, line selected a sprite | 1 | +2 |
| DMG single speed | 0 | +2 |
| double speed (either model) | 0 | 0 |

**+22 gambatte rows and +2 mealybug rows, zero regressions** over the whole
5268-row gambatte corpus: every single-speed row of `gambatte/scy/` now passes
(both `scy/` and `scy/scx3/`, `spx08`/`spx09`/`spx0A`/`spx0B` included), plus
`m3_scy_change` [Dmg] and `m3_scy_change2` [Cgb]. Golden drift is confined to
SCY-named ROMs. Pinned per arm by
`gambatte::misc::eager_scy_during_m3_read_frame_passes`.

### What the kernel measures, and how the references were read

`scy/scy_during_m3_*` is a STAT handler (`$0048` = `ei; jp $1000`) that writes
`SCY = 144 - LY` early in mode 3 and `SCY = 0` late, stepping the two writes in
opposite directions per rung. The map holds tile 1 on row 18 and tile 0
everywhere else, and tile 1's data is `FF FF` on row 0 and zero on rows 1-7. So

* SCY = `144 - LY` at a fetch → map row 18, fine row 0 → **black**;
* SCY = 0 → map row `LY >> 3` → tile 0 → **white**;
* the tile number read under one value and the data reads under the other are
  the mealybug `m3_scy_change` mixed fetch, and on CGB they render a *distinct*
  colour — LO under the old scroll (`FF`) with HI under the new (`00`) is BG
  palette 0 **colour 1**, raw `$2AAA`, which is what the `_2`/`_4`/`_6`
  references demand at the line's last tiles. That colour is what pins the CGB
  lead exactly; the DMG reference at the same pixels is colour 0, which rules
  the mixed fetch *out* there.

Our tile-number reads sit at dots 86 (the discarded fetch), 92 (`fetch_x` 0)
then `90 + 8k`; LO is +2 and HI +4 from each. The FF42 commit lands at the raw
write dot + 2, and a read at dot `d` sees a commit at `d` as the old value.
Solving the six rungs of the plain directory against those dots gives a lead of
2 for the CGB body and 4 for its first tile, 0 and 2 for the DMG — which is
exactly the swept optimum.

### The first tile's extra two steps

Hardware fetches the line's first visible tile once; we throw the first fetch
away and re-run it six dots later (`first_discard`, the same artefact that makes
`bg_map_col`'s anchor 6/7 instead of 8), so that tile's reads sit late on our
grid. `_3` is the discriminating rung on both models: its line-start write
commits between the discarded fetch and the re-fetch, and without the extra lead
the whole first tile flips to the new scroll on all 143 lines (1144 px [Dmg] and
[Cgb] alike, 715 px in `scx3/`). Two steps is the measured value; 3 ties it, 0/1
and 4 lose rows.

### Swept, on the full 5268-row gambatte corpus

| knob | 0 | 1 | 2 | 3 | 4 |
|---|---|---|---|---|---|
| CGB lead | 4646 | 4646 | **4660** | 4660 | 4632 |
| CGB sprite-line lead | 4654 | **4660** | 4654 | 4648 | — |
| first-tile lead | 4656 | 4656 | **4660** | 4660 | 4656 |
| DMG lead | **4660** | 4656 | 4643 | — | — |
| double-speed lead | **4660** | 4655 | 4655 | — | — |

The sprite-line step is a *unique* optimum, which is why it is not simply the
`n_sprites > 0` exemption `map_scx_formed` uses for the column (that would be
lead 0 and costs `spx08_1`/`_3`).

### Measured dead ends (do not re-chase)

* A double-speed lead: pure loss — it drops `scy_during_m3_ds_{1,4,7}` and
  `spx08_ds_{1,2}` and recovers nothing.
* Giving the first-tile lead to double speed: it recovers `scy_during_m3_ds_3`
  and loses `scy_during_m3_ds_1`, a green row, at the same corpus total.
* Moving the FF42 commit instead of the read frame: `eff.scy` has no consumer
  outside the fetch, so the two are arithmetically identical — the sweep above
  was run in the commit form first and gives the same optima.

## Mid-mode-3 SCX and the BG map column (single speed CLOSED 2026-07-30)

Single-speed status: every row in this cluster passes except
`scx_during_m3_spx2 [Cgb]`, which is a CGB OBJ-palette value divergence, not a
fetch or timing row (see the last section). The `_ds_` ladder is **not** class A
— 27 of its rows fell to whole-dot terms on 2026-07-31, see
"The double-speed ladder, mostly closed" below.

The BG tile-map column is derived from the pixel output position, not from a
tile counter. SameBoy `display.c` `GB_FETCHER_GET_TILE_T1` forms it as
`(SCX + position_in_line + 8 - (is_cgb && !during_object_fetch)) / 8`: SCX is
summed with the position and divided **once**, so SCX's low three bits carry
into the tile index. Dividing the coarse part out first and counting tiles
separately (`scx / 8 + fetch_x`) cannot express that carry — which is exactly
what the gambatte `scx_during_m3/scx_*c0` ladders measure, and why the whole
family failed on any *timing* adjustment.

`Render::lx` is our position. Four details are ours rather than SameBoy's, all
measured with an equivalence check (see below):

- the anchor is **6 on CGB and 7 on DMG**, not `8 - is_cgb`, because our `lx`
  sits at `8 * fetch_x - 6` at the tile-number read instead of a multiple of 8 —
  we re-fetch the discarded first tile where SameBoy pushes it and pops its
  pixels. The DMG runs a dot further back (6-dot OBJ fetches against the CGB's
  5, mode-0 flip leading by 3 not 2). Sweeping the DMG anchor over 0..=7 against
  the `scx1_scx0`/`scx2_scx1`/`scx2_scx0`/`scx_0761c0` legs gives a unique
  optimum: 0..=6 fail all four identically, 7 passes all four;
- the position form is gated to fetches after the line's first pixel ships, with
  a running sprite stall excluded: our BG fetcher free-runs through a stall
  while the output position is frozen, where SameBoy parks its fetcher in
  `PUSH`;
- it applies only once `SCX & 7` has moved away from the value latched at the
  fine-scroll comparator match (`Render::hunt_fine`) — the value that fixed this
  line's discard, and therefore `lx`'s alignment to the tile grid. While it
  holds, the two forms are provably identical, so the gate costs nothing.
- the SCX the fetch reads is taken from a per-dot ring at a lead: 0 in double
  speed or on a line that selected sprites, **3 on DMG before the line's first
  pixel ships**, 2 otherwise (`map_scx_formed`).

Landed in three passes, each zero-regression with golden drift confined to
SCX-named ROMs: **48 rows** (the position form + the CGB/DMG anchors), **+20**
(the ring lead and its sprite gate), **+20** (the DMG pre-output lead and the
never-matched-hunt column holdback, 2026-07-30). The single-speed half of the
cluster is now closed; the `_ds_` half is class A.

### The equivalence check (use this for any future work here)

With SCX held constant the position-derived column must reproduce
`(scx / 8) + fetch_x` fetch-for-fetch, because every BG row outside
`scx_during_m3` already passes. Instrument both at the tile-number read and
count mismatches: seconds per ROM, no battery run. It found the anchor and
rejected four wrong constants that would each have cost a six-minute matrix run,
and it is what proves any future variant safe before measuring it.

Cover the fine-scroll values: ROMs with `scx & 7` of 0-3 are plentiful
(`spx08`..`spx0B`) but a constant that is right there can still be wrong at 6/7
— that mistake cost two mealybug rows once.

### Exact pixel shape of a failing single-speed row (DMG shades)

The evidence that localised the bug, kept because it is the fastest way to
re-orient here. `scx_0363c0/scx_during_m3_4 [Dmg]`, row 0, x0..23 as shade
indices (0 = white .. 3 = black), against the passing `_1`:

```
_4  ours 1111 0 2333333 2 32222223233
_4  want 1111 0 1000000 1 32222223233     <- want == 3 - ours, x5..12 only
_1  ours 2222 3 2333333 2 32222223233     (identical to want)
```

* the differing run is **x5-12** and x4/x13 match, so the grid is aligned at
  5/13/21 — the line renders with fine scroll 3 and the discard is correct;
* `want == 3 - ours` is a clean shade inversion, which on DMG (global BGP)
  means different tile *data* — so exactly **one tile has the wrong column**,
  the first full tile;
* everything after x13 matches, so the error does not propagate.

That tile is `fetch_x = 1`, and it resolves through the **counter** branch of
`bg_map_col`, not the position branch: with SCX constant across a fetch the two
forms are identical, so the branch was never the variable. The variable is
which SCX the fetch sees, which is why the fix landed in `map_scx_formed`.

### The FF43 write already carries a commit stage — the lead stacks on it

`Ppu::stage_write_dots` (`regs/stage.rs`) defers every FF43 write before it
reaches `eff.scx`: **2 dots in double speed, 3 in single**, both flat (the
`2 + (scan_pos().1 & 1)` parity term in that function belongs to the DMG
palette and SCY arms, not to FF43). So `map_scx_formed`'s lead applies on top
of an existing staged commit, not to the raw CPU write.

**Swept, and the staging is already optimal** (120 legs, landed values scoring
76):

| (DS, SS) | (2,3) | (1,3) | (3,3) | (0,3) | (2,2) | (2,4) |
|---|---|---|---|---|---|---|
| total | **76** | 71 | 73 | 57 | 68 | 53 |

Both axes are at their optimum: the SS score is flat at 56/72 for any DS value
and collapses when SS moves off 3, and the DS score peaks at 2. Every *scalar*
offset in the SCX write path is measured and at its best value.

Trace hygiene, learned the hard way: these ROMs write FF43 on essentially every
line, so a trace must filter on `old != new` rather than on write events.

### Measured dead ends (do not re-chase)

All of these were built and measured against this cluster; every one either
shuffles want-opposite siblings or retimes rows outside the intended window:
a uniform coarse-SCX sample delay, a line-start coarse latch, a fetch-phase
threshold, a dots-since-fetch-start threshold, a deferred FIFO refill (the
same-dot refill is load-bearing — it is what produces the 8-dot cadence), a
fetch restart on a coarse change, a `read - 4` dot rule, a per-line measured
anchor, and latching the column at `TileNoWait` to mirror SameBoy's T1/T2 split
(13 regressions — computing it on the T2 side is correct for our pipeline).

Added 2026-07-30, same verdict: a uniform pre-match or post-match FF43 commit
debt (plateau 4-7 hd, 6 already optimal), that debt discriminated on `mode3_dot`
at stage time or on whether the write moves `SCX & 7`, a first-fetch
(`fetch_x == 0`) lead, and a separate earlier-committing *coarse* SCX view
feeding the map address — that last one is arithmetically identical to widening
the ring lead, which the sweep above already kills.

### The read frame: which dot the address is formed on

The `_2`/`_3` rounds of every `scx_*c0` dir are not a column-*value* problem.
`scx_0060c0` writes `$00 -> $60 -> $c0`, all with `SCX & 7 == 0`, so only the
coarse scroll moves — and for a coarse-only change the position form and the
tile counter shift by the same amount. Widening the gate from "fine moved" to
"any SCX moved" changes nothing (40/80 either way), which is the proof.

They fail on **when** SCX is sampled. Three whole-latch arms, all refuted:

| arm | result |
|---|---|
| latch SCX at `TileNoWait` (SameBoy's T1) | shuffle — round 19 regresses in all four dirs |
| latch SCX **and** `lx` at `TileNoWait` | `scx_0761c0` round 1 regresses |
| latch SCX at the previous fetch's `Hi` (read − 4) | 8/72 — `Hi == read - 4` only holds in steady state |

No single latch point expresses it; the shipped model is a per-dot ring read at
a lead, below.

#### MEASURED: the address forms 2 dots early, single speed only

A per-dot `eff.scx` ring with the delay swept over 0..=7, scored on the round
matrix (six `scx_*c0` dirs x 14 rounds x 2 models = 120 legs, baseline 60 pass):

| delay | 0 | 1 | **2** | 3 | 4 | 5-7 |
|---|---|---|---|---|---|---|
| pass | 60 | 66 | **71** | 36 | 4 | 2 |

Splitting by speed sharpens it — the lead is single-speed only:

| (ds, single) | (2,0) | (0,2) | (2,2) | (1,2) | (2,1) | (0,1) | (0,3) | (0,4) |
|---|---|---|---|---|---|---|---|---|
| pass | 55 | **76** | 71 | 74 | 63 | 68 | 53 | 24 |

`(ds 0, single 2)` is a unique optimum at 76/120, and delaying the double-speed
side at all costs rows. On the full battery that is **+20 gambatte rows**.

**Initially blocked: all 7 regressions were SameBoy-PASS** (`classify_pixel`,
`mm=0`) — mealybug `m3_scx_high_5_bits` [Dmg]+[Cgb] and `_change2` [Cgb], plus
the four `scx_*_spx0/1/2` sprite-position ROMs. Gating the delay away from
sprite stalls changes nothing; delaying only the coarse bits scores worse (16).

#### The discriminator: sprites (LANDED, +20)

The `spx*` names are the tell — they place an OBJ at x = 0/1/2. An OBJ fetch
stalls the BG fetcher and carries the address formation with it, so the fixed
lead does not hold on a line that selected any sprite. Gating it off there
(`r.n_sprites > 0`) keeps the round matrix at 76/120 — the `scx_*c0` dirs have
no sprites, so they are untouched — and returns all four `spx` rows to passing.

Final law, in `render/mode0.rs`:

```rust
let d = if ds || r.n_sprites > 0 { 0 } else { 2 };
r.scx_ring[(usize::from(r.scx_ring_i) + 8 - d) & 7]
```

with `scx_ring` sampled once per BG fetcher advance.

#### The double-speed residual does NOT respond to these parameters

After the landing the residual is **57 rows, 39 of them CGB double-speed** (9
CGB single-speed, 9 DMG single-speed). Three sweeps, each scored on the DS half
of the round matrix (48 legs, baseline 20 pass) with the single-speed side
pinned at its landed values:

| sweep | result |
|---|---|
| DS lead 0..=4 | 20, 18, 15, 3, 0 — **0 is best**, any lead is worse |
| CGB DS anchor 2..=9 | 14, 17, 17, 20, 20, 20, 18, 10 — flat plateau at 5-7 |
| DS lead keyed on `sb_dsa8 & 4` (hi, lo) | best (0, 2) = 21 vs 20 baseline — noise |

So the double-speed rows are not a read-frame, anchor, or DS-alignment-phase
problem in the shape that solved single speed. Something structurally different
governs them; do not spend a fourth sweep on this parameter family.

#### The DS ladder ROTATES under the lead — the groups are pinned

Per-directory tracing shows why every DS sweep looks flat: the lead does not
fix the ladder, it **rotates** it. `scx_0060c0` double speed, CGB:

```
lead 0:  . X X . . X X .      (ds_1..ds_8)
lead 2:  X . . X X . . X      exactly complementary
```

So `ds_1/4/5/8` want no lead and `ds_2/3/6/7` want it — want-opposite siblings,
and any single lead trades one group for the other. The two groups are pinned by
their write dots (reads at 234 and 242, writes stepping 2 dots from 243 down to
229):

| group | write dots | `(read - write) mod 8` |
|---|---|---|
| wants lead 0 | 243, 237, 235, 229 | {5, 7} |
| wants lead 2 | 241, 239, 233, 231 | {1, 3} |

Confirmed by trace that the column value is *not* the difference: `ds_2` (write
241) and `ds_4` (write 237) both produce column 11 for `fetch_x=19`, yet `ds_4`
passes and `ds_2` fails, so hardware honours the write at 237 and ignores the
one at 241.

Keying the lead on that phase was built and swept anyway — every 8-bit mask over
`(dot - scx_any_dot) & 7` scores at best 20/48, i.e. never better than no lead.
(Note `scx_write_dot` is useless as the key: it only latches when `SCX & 7`
changes, and `scx_0060c0` never moves the fine bits. A separate `scx_any_dot`
was added for the sweep and reverted.)

So a discriminator provably exists and its two groups are known, but it is not
the write's dot phase, the fetch phase, the anchor, or `sb_dsa8`. Five arms
measured. The next attempt needs a *new observable* — most likely a trace of
what column the reference demands per line on a `ds_2`/`ds_4` pair, the way the
single-speed law was found.

#### The new observable, run: the divergence is at the line's FIRST fetch

Logging the actual map column per fetch on the `ds_2`/`ds_4` pair (line 1,
`scx_0060c0`, CGB) — `ds_4` passes, `ds_2` fails:

```
ds_4  fx: 0 0 1  2  3  ...      cols: 0  0 13 14 15 ...
ds_2  fx: 0 0 1  2  3  ...      cols: 0 12 13 14 15 ...
                 ^ identical from here on
```

They differ at **`fetch_x = 0`, the line's first real fetch**, where `ds_2`
already sees the line's `$00 -> $60` write (column 12) and `ds_4` does not
(column 0). Everything from `fetch_x = 1` onward is identical. So the residual
is decided at the *start* of the line, not at the last tile as the write-dot
ladder analysis assumed — that earlier reading was wrong.

Gating the DS lead to exactly that window (`lx == 0`) and sweeping it 1..=4
still does not move the ladder: 20, 18, 18, 16 of 48, against a 20 baseline.
Making `ds_2` agree with `ds_4` at `fetch_x = 0` is therefore *not* what its
reference wants — the two ROMs have different write timing and legitimately
expect different first columns.

Calibrating signature to column from the passing sibling was also tried and is a
dead end: the map reuses palettes, so only 4 distinct cell signatures exist
across the frame and the mapping is not injective.

Six arms measured on the double-speed half, all capped at 20/48: lead, anchor,
`sb_dsa8`, write-phase mask, phase-keyed lead, pre-output-gated lead. The
rotation result stands (the two groups are real and complementary) but no state
term yet separates them.

#### RE-OPENED 2026-07-30: the class-A *reason* above is wrong

The section below concludes the DS rows are separated only by "which half of
that dot the write took effect in". Measured, that is false. The
`scx_0060c0` rotation group's eight legs commit their `$60 -> $c0` write at
eight **distinct whole dots**, every one on the **same** half:

```
ds_1 dot=243 dh=1   ds_3 dot=239 dh=1   ds_5 dot=235 dh=1   ds_7 dot=231 dh=1
ds_2 dot=241 dh=1   ds_4 dot=237 dh=1   ds_6 dot=233 dh=1   ds_8 dot=229 dh=1
```

There is no half-dot distinction to represent — the legs are already separated
on the whole-dot grid, which is what the section's own `(read - write) mod 8`
= {5,7} vs {1,3} table says. The first writes (86, 88, 90, 92, **93**, 95, 97,
99) are not even uniformly spaced, so parity arguments fail too. And a
whole-dot render-FSM law *does* move DS rows: extending the never-matched-hunt
column holdback (below) to double speed recovers `scx_0360c0/_ds_3` and
`scx_0761c0/_ds_3` — the section claims no whole-dot term separates the groups.

Every one of the six arms that produced the class-A verdict is a
lead/anchor/phase **scalar**. None was a render-FSM state term. That is the
same uniform-lever error that made the single-speed half look welded until it
fell to two non-scalar terms.

**So "these rows need a half-dot PPU clock" is not established.** What IS
established is narrower, and proven below.

#### The DS holdback: required on lines >= 1, off on line 0 (LANDED)

The earlier "welded on the render FSM" reading in this file was **wrong**, and
the error is instructive: it compared `old/offset_3/_ds_1` and
`scx_0360c0/_ds_3` at the *start* of the line, where they are indeed identical
(same hunt trace, same commit dot 90 / same half, same first-output state: dot
105, 8 dropped, `fetch_x` 2, `hunt_fine` 0). But `_ds_1`'s failure is at the
line **end**, where their late `$c0` commits differ — dot 247 against 239.
Comparing the wrong end manufactured a weld that is not there.

Ground truth, taken by matching each reference's 8-pixel segments against the
map's own column signatures (`colreq` method: dump MAP/ATTR/TILE/BGPAL, render
each column, match) rather than by sweeping:

| ROM (ly 1) | first tile | last tile |
|---|---|---|
| `scx_0060c0/_ds_1`, `_ds_2` | even 12..30 | odd 13..31 |
| `scx_0060c0/_ds_3`..`_ds_8` | even 0..10 | odd 1..11 |
| `scx_0360c0/_ds_1`, `_ds_2` | even 12..30 | odd 13..31 |
| **`scx_0360c0/_ds_3`** | **even 12..30** | **odd 1..11** |
| `old/offset_3/_ds_1` | ANY | ANY |

`scx_0360c0/_ds_3` is decisive: held, its indices are 0 and 19, giving columns
12 (even) and 11 (odd, in 1..11) — both satisfied. Unheld, index 1 gives column
13, odd where an even column is required. **The holdback is required at double
speed.** It recovers that row with zero regressions.

The line-0 carve-out is narrower than first landed: `scx_0761c0/_ds_3`
exhausts its hunt on line 0 too and **needs** the hold there, so line 0 cannot
be excluded wholesale. The single case that must not hold is
`old/offset_3/_ds_1` — line 0 with the fine scroll back at 0 — and
disassembling it gives the direct cause rather than a fitted predicate.

That ROM's kernel is driven purely from STAT: `$0048` is `ei; jp $1000`, the
kernel writes `$60` at `$101E` and `$c0` at `$106A` (the ladder steps those two
writes in opposite directions), and it takes **no VBlank interrupt at all** —
zero dispatches to `$0040` across a whole run. So between the ly-144 dispatch
and the ly-0 one the CPU runs off the end of the kernel's NOP sled instead of
waiting on it. Measured, every line dispatches STAT at **dot 6 except line 0,
which dispatches at dot 10**: the interrupt arrives mid-instruction there and
completes 2 M-cycles (4 dots) later. That is the whole of the 247-vs-243
difference in the line's `$c0` commit, and with the read at dot 250 it is what
puts the held last tile at column 11 where the reference wants 12..31.

No map lead reaches it either (swept 0..7, plain and with a pre-output
exemption; everything >= 3 collapses 129 → 114 → 103 because other DS rows have
writes landing closer still that must stay visible). The gate is
`ly >= 1 || SCX&7 != 0`, pinned from both sides in
`gambatte::misc::eager_scx_during_m3_map_column_passes`.

The dot-10 line-0 dispatch is **not** this ROM's control-flow fingerprint: the
next section measures it on every rung of every directory and at both speeds,
and settles what the references actually want done about it.

### The double-speed ladder, mostly closed (LANDED 2026-07-31, +27 then +7 rows)

Three whole-dot terms, all double-speed only, found by tracing the tile-number
read against the SCX write-commit dot and then classifying every leg against
what the reference actually accepts (`colreq`). Round matrix 103 → 130 of 141,
whole battery **27 baselined rows recovered, zero regressions**, golden drift
confined to 35 `scx_during_m3/*_ds_*` `[Cgb]` keys. A fourth term (below)
replaced term 3 and took another **7 rows, also zero regressions**.

**1. The double-speed map lead is 2, exactly like CGB single speed**
(`map_scx_formed`). The `ds` special case that forced it to 0 is gone. The
earlier sweep that scored DS lead 0 best was taken before the DS post-match
staging debt landed, so its commit dots were stale.

**2. A write landing ON the fine-scroll comparator lock takes the post-lock
commit debt** (`stage_write_dots`, DS FF43: `dot >= hunt_match_dot`, was `>`).
The comparator resolves against the OLD SCX in that dot's render tick and the
write commits behind it — the same ordering the same-dot hunt re-open in
`regs.rs` FF43 already keys on. This is what separates `scx_0360c0/_ds_6` from
`_ds_7`: raw writes 92 and 94, hunt lock at 92, and the reference wants 92
visible to the dot-98 fetch and 94 not.

**3. Line 0's double-speed FF43 write defers nothing** (`stage_write_dots`, DS
FF43: `u8::from(self.line != 0) * 2`). Line 0's STAT source has no prior-line
OAM carryover, so its handler — and every write in it — starts one double-speed
M-cycle later than the same rung on lines 1-143; the references place the rungs
2 dots ahead of that, and the deferral is exactly what absorbs it. The map lead
is 2 on **every** line at both speeds (`map_scx_formed` has no line-0 arm). This
replaced the earlier "line 0 keeps lead 0" exemption, which was observationally
equivalent for the map-column ring but left the fine-scroll comparator path —
the hunt lock and `eff.scx` — 2 dots late. Both halves are required together:
the exemption alone scores 1019 of the 1259-ROM double-speed corpus, the
deferral alone 997, the pair 1026.

#### The measurement that drove it (keep this, it is the whole derivation)

The raw CPU write dots are identical in every `scx_*c0` directory: the `$60`
write steps 82, 84, …, 96 across rungs 1-8 and the `$c0` write steps 240, 238,
…, 226. Only the staging debt differs. The tile-number reads are at dots 92
(`fetch_x` 0), 98 (1), 234 (18) and 242 (19).

`colreq` on the `[Cgb]` references gives, per rung, whether the write must be
visible to a given read — all DISCRIMINATING, none degenerate:

| read | must see | must not see | implied lead |
|---|---|---|---|
| 92 (fine-0 dirs) | commit 88 | commit 90 | 2-3 |
| 234 | commit 231 | commit 233 | 1-2 |
| 242 | commit 239 | commit 241 | 1-2 |
| 98 (fine-3 dirs) | commit 95 | commit 97 | 1-2 (needs term 2) |

Lead 2 satisfies all four. Term 2 is what pulls the fine-3 dirs' `_ds_6` commit
from 96 to 95 so its column comes out even.

#### The line-0 STAT dispatch: 4 dots late, and the pulse dot may NOT move

These kernels are pure STAT handlers (`$0048` = `ei; jp $1000`, then a fixed
NOP sled) with `STAT = $20` — the OAM source alone (disassembled at `$01D2` of
every `scx_*c0` ROM) — so every SCX write on a line is rigid relative to that
line's dispatch. Measured with an ack probe on `scx_0060c0/_ds_4 [Cgb]`: the ROM
takes **only** STAT (2169 acks, all bit 1, zero VBlank), and

```
RISE if=2 ly=1..143 dot=0   ->  ACK ly=N dot=6
RISE if=2 ly=0      dot=4   ->  ACK ly=0 dot=10
```

so every line-0 write lands 4 dots later than the same rung on lines 1-143 —
confirmed on all eight rungs of every directory, and at single speed too. It is
the line-0 OAM interrupt source having no prior-line carryover
(`update_mode_for_interrupt`: lines 1-143 hold source 2 across dots 0-3, line 0
pulses at the visible mode-2 edge, dot 4).

**Moving that pulse is refuted.** Pulsing the line-0 source at dot 2 in double
speed scores **1000** of the 1259-ROM double-speed corpus against 1019 for the
shipped tree — +21 rows, −40. (An earlier note claimed +49/−11; that was
measured on a hand-picked row list before the map-lead landing, and the full
corpus does not support it.) Nor is the pulse dot free: it is bracketed within
one double-speed M-cycle by ROMs whose own writes come from the LINE-152
dispatch and so do not move with it —

| ROM | kernel (disassembled) | pins |
|---|---|---|
| `m2enable/disable_ly0_ds_{1,2}` | ISR sets `STAT=$20`, then `STAT=$00` at `$11BA`; reads `IF & 3` | want 1 / 3 — the disable straddles the pulse, so the pulse is at dot 3 or 4 |
| `m2enable/lyc0_late_m2enable_lycdisable_ds_1` | late OAM *enable* | the enable must still catch a high OAM source |
| `lycEnable/lycwirq_trigger_ly00_stat50_ds_1`, `miscmstatirq/lycstatwirq_trigger_ly00_10_50_ds_1` | ISR sets `STAT=$50` (LYC + mode-1), then `LYC=0` at `$10D5`; reads `IF` | want E0 — the line is still held HIGH by the line-0 mode-1 carry when `LYC=0` lands, so the carry must survive to dot 4 |
| `m2enable/m2_late_m1disable_ly0_ds_1` | `STAT=$30` → `$20` at `$11B8` | want 2 — dropping mode-1 must make the line FALL, i.e. OAM is not yet selected |

The last two rows pin the mode-1 carry's END, the first two pin the OAM
source's rise; both land on dot 4 and a single `mode_for_interrupt` scalar
cannot separate them. Sweeping the pulse over 0..4 gives only two distinct
outcomes (0/1/2 vs 3/4) — the double-speed M-cycle is 2 dots, so only its
parity is observable.

`lcdoffset` is **not** the discriminator an earlier note proposed. Disassembly
(`$014E`-`$0163`) shows `lcdoffset1` runs the KEY1 `stop` speed switch three
times instead of once, shifting the CPU-vs-PPU phase; but both
`lycwirq_trigger_ly00_stat50_ds_1` and its `..._ds_lcdoffset1_2` sibling are
satisfied by the shipped term, so there is no split to chase.

The shift the references want is therefore in the **write frame**, not the
interrupt: term 3 above.

#### The rows still failing, and what each needs

| row(s) | shape | note |
|---|---|---|
| `old/offset_3/_ds_2` (8 px), `scx_0360c0/_ds_2` (160 px), `scx_0761c0/_ds_5` (8 px) | line 0 only | rungs that still straddle a fetch boundary after term 3 |
| `scx_0761c0/_ds_6` | first tile, lines 1-143 | its `$61` write commits exactly on the hunt lock dot; see the ground truth below |
| `old/offset_3/_ds_3` | 2723 px | EXCEED — SameBoy misses it too |

##### `scx_0761c0/_ds_6`: ground truth and four refuted levers (2026-07-31)

`colreq` on the `[Cgb]` reference at ly 8 makes this row DISCRIMINATING at the
failing tile and settles what it wants — the earlier "wants it visible at the
dot-98 read" note was the right instinct with the wrong dots:

| row | x1-8 accepts | x9-16 accepts | we produce at x1-8 |
|---|---|---|---|
| `_ds_5` | UNRESOLVED | UNRESOLVED | (fails on line 0 only) |
| **`_ds_6`** | **even 12..30** | odd 13..31 | **1** |
| `_ds_7` | odd 1..11 | odd 13..31 | 1 ✓ |

Column 12 is `$61 >> 3`, so `_ds_6`'s first tile has to be formed from the NEW
SCX and `_ds_7`'s from the old — with both fetching at dot 98. The whole
difference is the commit: `_ds_6` writes at raw dot 94 and commits at 96, which
is exactly the dot the fine-scroll comparator resolves on (`HUNT` fires at 96
against the OLD fine 7, then the write commits behind it); `_ds_7` writes at 96,
after that dot's `hunt_done`, and commits at 98. The map ring is sampled at the
top of each fetcher step, so a commit landing on dot 96 misses `scx_ring[96]`
and the lead-2 read at dot 98 still sees `$07` — hence our column 1 through
`bg_map_col`'s counter branch (`fine_moved` is false because the ring value's
fine still equals `hunt_fine`).

Four arms built and measured against the 178-row `scx_during_m3` corpus
(baseline 170 pass), all refuted:

| arm | result |
|---|---|
| DS FF43 pre-lock debt swept 0..4 (blanket `!hunt_done`) | 151, 152, 152, 170, 170 — 4 (the shipped value) is optimal; anything shorter costs 18 rows |
| flush a pending FF43 stage at the hunt lock (swept over `dots_left` 0..6) | 170, 167, 159, 159, 156, 156, 152 — monotonically worse |
| DS pre-output (`lx == 0`) map lead swept 0..3 | 151, 160, **170**, 162 — the shipped 2 is optimal, so the lead cannot be the lever either |
| debt 2 gated on "the commit lands on the comparator's resolve dot" (`hunt_idx == (SCX&7) - 1` at stage time) | 168 — fires on the wrong rungs (`scx_0367c0/_ds_3`, `scx_0360c0/_ds_3` break) and `_ds_6` does not move: the `hunt_idx` visible at `stage_write_dots` is not the render's value for that dot |

So the mechanism is identified — a commit landing ON the resolve dot must reach
the fetch grid from that dot, the fetch-grid twin of the `dot >= hunt_match_dot`
ordering term 2 already encodes on the comparator side — but no predicate
available at *stage* time expresses it. The next attempt should move the
ordering instead of the debt: sample `scx_ring` after the strobe rather than
before it, on the lock dot only, and re-score the whole corpus.

#### OPEN: the SCY line-0 rungs are a sub-dot separation (derived, 2026-07-31)

Double speed only — the single-speed half of this cluster is closed by the SCY
read frame above, which leaves the double-speed lead at 0 and so does not touch
the derivation below.

`scy/scy_during_m3_ds_*` writes FF42 twice per line 0, at dots 86, 88, … 96
(rungs 1-6) and 248, 246, … 238. Sweeping the line-0 FF42 commit debt 0..3 and
cross-matching every rung's frame against *every* rung's reference (all seven
references distinct — none degenerate) puts the fetch's SCY sample dots at 90
(tile 0), 96 (tile 1) and 240 (tile 19) — spacing 6 then 8×18, exactly the
`first_discard` geometry, so there is no tile-0 anomaly. What the references
then demand is:

* `_ds_2` (write 88, tile 0, 8 px at x0-7): commit ≥ 90 → debt ≥ 2;
* `_ds_5` (write 94, tile 1, 8 px at x8-15): commit ≤ 95 → debt ≤ 1;
* `_ds_6` (write 238, tile 19, 8 px at x152-159): commit ≤ 239 → debt ≤ 1.

Both constraints are the same "raw + 2 vs the sample dot" relation resolved in
opposite directions, so **no whole-dot debt and no render-FSM state term
separates them** — the write dot, the sample geometry and the tile index are
all identical in form. Debt 1 scores +5/−1 (`_ds_5`, `_ds_6`, `spx08_ds_3/4`,
`spx09_ds_3` recovered, `_ds_2` lost); it is not landed, because a `+N/−M` with
a classified SameBoy-PASS on the `−M` side is a missing discriminator. The
missing discriminator is sub-dot.

#### Measured dead end (2026-07-31)

The double-speed **sprite-line** map lead stays 0: swept 0..3 against the spx
rows with the two landed terms in place — 46, 46, 45, 45 of 55. The
single-speed sprite exemption carries over unchanged.

#### SUPERSEDED: the DS residual as "the whole-dot contract" (kept for its measurements)

Reading SameBoy's clock settles it. Its PPU is driven at **8 MHz — half-dots**,
not dots:

* `display.c` `dma_sync`: `unsigned offset = *cycles - gb->display_cycles;`
  commented *"Time passed in 8MHz ticks"*;
* `GB_display_run`: `gb->cycles_since_vblank_callback += cycles / 2;` — the
  display's `cycles` unit is half of a 4 MHz dot;
* the state machine itself runs with a divisor of two:
  `GB_BATCHABLE_STATE_MACHINE(gb, display, cycles, 2, !force)`, so one PPU step
  consumes two 8 MHz ticks but the entry point can be interrupted **between**
  them.

So a CPU write can land on either half of a dot, and SameBoy resolves which.
`Ppu::render_step` advances one whole dot at a time and cannot represent that.

At single speed one M-cycle is 4 dots, so consecutive ladder rounds are 4 dots
apart and the half-dot they land on never separates them — which is why the
single-speed side fell to a plain 2-dot lead. At double speed an M-cycle is
2 dots: the rounds step 2 dots, all land on odd dots, and the surviving
distinction between the two rotation groups is *which half of that dot the write
took effect in*. That is unrepresentable on a whole-dot clock, which is exactly
what `baselines/gambatte.txt` class A calls the "CGB double-speed sub-cycle
phase" floor with the stated lift condition of re-clocking to a half-dot grid.

~~**Therefore the 39 CGB double-speed rows are class A.**~~ **The conclusion of
this section is withdrawn — see "RE-OPENED" above.** The six measured arms are
real and still worth not re-running, but they are all lead/anchor/phase
scalars, so they do not prove what the paragraph claimed. The half-dot reading
of the write commit is contradicted by the measured commit dots.

The single-speed rows are *not* covered by this argument; they were closed on
their own merits (below).

#### The single-speed residual: what survived the false starts

Three arms rejected on the per-dir ladder (baseline 56/72), all still dead:

| arm | result |
|---|---|
| anchor scaled by `hunt_fine` (`+/-1 * fine`) | 23 and 45 — far worse |
| DMG anchor swept 5..=9 | 55, 55, **56**, 54, 46 — 7 already optimal |
| single-speed lead swept 1..=3 | 48, **56**, 33 — 2 already optimal |

Two measurements from that pass are durable and were reused to close the
cluster:

* `8 * fetch_x - lx == 6` **uniformly** at the tile-number read, on fine-0 and
  fine-3 directories alike (364 of 364 sampled fetches). There is no
  discard-dependent alignment term to derive; the anchor is universally
  correct. The offset only grows when a whole tile is dropped — which is
  exactly the never-matched-hunt case the section below turns into a law.
* The frame-diff probe was validated against **known-passing** rows before any
  reading was trusted: `scx_0060c0/_4` on both models and `scx_0363c0/_1`/`_2`
  on DMG all report 0 px, and the hand-rolled diff agrees with
  `harness::expect_frame_png` on every row. Do this before trusting any frame
  diff here.

Two hypotheses from that pass were **disproven** and must not be revived: that
a non-zero *initial* fine scroll selects the failures (the directory name is
the value sequence, not the line-start value), and that these ROMs write SCX
only on lines 139-143 (they write on every line, so row 0 is ordinary). One
more: removing the per-line `scx_ring` reset changes nothing — the ring is
refilled every fetcher step, so at a line boundary it already holds the
previous line's last dots.

Signature map of the failures, which is what split the cluster into its two
laws — rounds 2/3 corrupt the line's **last** tile and rounds 4/5 its **first**,
`_2`/`_4` on row 0 only and `_3`/`_5` on every row:

| ROM | signature |
|---|---|
| `scx_0363c0/_4` [Dmg] | row 0, 8 px at x5-12 — the first full tile |
| `scx_0360c0/_2` [Cgb] | row 0, 160 px — the whole row |
| `old/offset_3/_2` [Dmg] | row 0, 8 px at x152-159 — the last tile |
| `old/offset_3/_3` [Dmg] | 143 rows, same last tile |

### CLOSED: the single-speed residual (LANDED 2026-07-30)

Two independent laws, both found by tracing the tile-number read against the
SCX write-commit dot on a fail/pass round pair.

**1. The DMG pre-output map lead is 3, not 2** (`map_scx_formed`). On
`scx_0363c0/_4 [Dmg]` the first full tile (x5-12, `fetch_x = 1`) reads its tile
number at dot 98; the `$03 -> $63` write commits at dot 95 and its passing
sibling `_3` commits at 94. The reference separates them, so the read frame has
to cut between 94 and 95 — one dot earlier than the landed lead 2 gives. Gating
the extra step to the pre-output band (`lx == 0`) and to DMG is what makes it
safe: a uniform lead 3 scores 33/72 on the round matrix, `lx == 0` alone 57/72
(it breaks five CGB rows), `lx == 0 && !cgb` **62/72**. CGB keeps 2 — the same
model split its `bg_map_col` anchor already carries.

**2. A prefill hunt that never matches must not advance the map column**
(`render.rs`, at `prefill_pos == 8`). When an SCX write moves `SCX & 7` *behind*
the comparator counter (`$03 -> $60`, `$07 -> $61`), the prefill runs out with
no match, the counter wraps into the pop phase and the whole first tile shifts
out. Our fetcher counted that thrown-away tile, so every column on the line was
one too high — a whole-row tile swap (160 px) wherever the count crossed the
map's `02/03` → `00/01` block boundary. Holding `fetch_x` back one tile at the
exhaustion point fixes both ends of the line at once. The wrap itself is
correct: it already yields the right discard (`8 + SCX&7` dropped pixels —
`scx_0761c0` needs 9, `scx_0360c0` 8), only the column counter was wrong.

Both laws are **single speed only**. Letting the holdback run in double speed
scores +3/-1 on the DS ladder — it breaks `old/offset_3/_ds_1 [Cgb]`, a
green SameBoy-PASS row — so it is gated off there; the DS ladder stays class A.

Together: round matrix **72/72** (was 56/72), whole `scx_during_m3` tree
108 → 128 of 172, **20 baseline rows recovered**, which is every targeted
single-speed row except the two below. Zero regressions across the battery.

Method note: the round matrix runs in **3.6 s**, not 90 s — a release build of a
15+1-frame dumper plus a numpy compare against the reference PNGs. Score
mealybug/age guards separately (`LD B,B` + 1 frame); the two
`m3_lcdc_obj_size_change_scx` legs fail at baseline, so the guard bar is 6/8.

#### CLOSED: `scx_m3_extend_1 [Dmg]` — the bare-exit back-out was over-correcting

Not a mode-3-length row (mode 3 ends on the same dot on both models) and not a
dispatch floor. `cmp -l` against its `_2` sibling shows a single inserted `00`
before the shared `ldh a,(FF41)`, so `_2` reads exactly one M-cycle later;
`_1` wants mode 3, `_2` mode 0. Traced at the scoring read (ly 1):

| | read | arm-8 exit | verdict |
|---|---|---|---|
| `_1` CGB | rp 528 | 532 (flip 267) | 3 ✓ |
| `_1` DMG | rp 528 | 522 (flip 267 − 5) | 0 ✗ |
| `_2` both | rp 536 | 534/536 | 0 ✓ |

The whole difference was the DMG `scx_write_dot` back-out in arm 8 subtracting
the live `SCX & 7`. That back-out exists to undo the render's *spurious*
mid-mode-3 SCX extension, and the spurious part is only what the render added
beyond the fine scroll its comparator actually resolved:

| ROM | `hunt_fine` | `eff.scx & 7` | back out |
|---|---|---|---|
| `late_scx4_2` | 0 | 4 | 4 — the render added a discard the hunt never latched |
| `scx_m3_extend_1` | 5 | 5 | 0 — the render's length is legitimate |

So the term is `SCX&7 − hunt_fine`, not `SCX&7`. Byte-identical on
`late_scx4_2` (the case it was written for), and `scx_m3_extend_1 [Dmg]`
recovers. **+1 row, zero regressions**, golden drift confined to that one ROM.

Note the earlier reading in this file — "mode 3 ends at dot 256 on both models
and every line" — was sampled on lines 0 and 100+, never on line 1, the kernel
line. It does not.

#### OPEN: `scx_during_m3_spx2 [Cgb]` is a palette-VALUE row, not a fetch row

8 px, x0 of rows 0-7. `cmp -l` across the ladder: spx0/spx1/spx2 differ in
**two bytes** — the OBJ X (0/1/2) and the header checksum. So X=2 is simply the
first leg where any sprite pixel reaches the screen (X=0 is fully clipped, X=1
shows only the transparent px7), which is why the passing siblings cannot
discriminate anything.

We and the reference agree on the colour *index*: the DMG reference is black at
x0, and the DMG leg passes. They disagree on the CGB OBJ palette *content* —
reference `(r5,g5,b5) = (1,19,16)` = raw `$4261`, ours OBP0 c2 = `$1CF2`.
Measured, so do not re-chase:

* the palette is written **once**, 16 bytes at frame 2 ly 144 (VBlank, not
  blocked) — there is no mid-mode-3 palette write to time, and no write is
  dropped;
* the ROM's init routine (`$01C4-$01E1`) writes OAM, BGP and the BG palette but
  **never** touches FF6A/FF6B, and the ROM contains no `ld c,6A`/`ld c,6B` and
  no literal `61 42` / `F2 1C` byte pair — the entries are computed at runtime;
* no index shift of the 16 written bytes produces `$4261`: the byte `$61` is
  never written at all.

**RESOLVED to post-boot residue 2026-07-31** — the earlier "the ROM computes the
entry at runtime" reading was wrong; the ROM never produces it at all.

* Which entry the pixel uses was settled by marking every palette slot with a
  unique value before the scoring frame: x0 of rows 0-7 is **OBJ palette 0,
  colour 2**, and everything from x1 on is BG palette 0.
* A byte scan of the whole 32 KiB finds exactly two palette-port writes,
  `ldh (FF68),a` at `$01AD` and `ld c,69` at `$01AF` — the **BG** ports. There
  is no `ldh (FF6A/6B)`, no `ld c,6A/6B` and no `ld (FF6A/6B),a` anywhere. The
  ROM never writes an OBJ palette, so the reference's `$4261` is whatever the
  console had in OBJ palette RAM at hand-off.
* Ours was `$1CF2` because `interconnect/boot.rs` installed
  `CGB_COMPAT_OBJ_PALETTE` (`7FFF 421F 1CF2 0000`) on **any** CGB-model machine
  — including CGB-*flagged* carts. That contradicted the same function's
  `cgb_cart_cut` comment, which subtracts `$7D8` T-cycles from the hand-off
  precisely because "the DMG-compat path does its compatibility-palette work
  after the logo" and a CGB cart skips it.

That install bug was **fixed on its own merits 2026-07-31** (see "Frame skip and
CGB boot palettes" above): the CGB-cart arm now reproduces `cgb_boot.bin`
byte-for-byte, and the constraint that made the pass safe is that
`misc/boot_hwio-C` is itself a DMG-flagged cart (`$143 == $00`), so its BCPS
`$C8` / OCPS `$D0` expectation measures the compat arm alone. Golden drift: 141
of 9020 keys, every one a CGB-flagged cart that renders before setting its own
palettes; zero suite verdicts moved.

It does not recover these three rows, exactly as predicted. Colour 2 of OBJ
palette 0 is now `$7FFF` (our `0xFF` power-on fill, which the boot ROM leaves
untouched — it clears only colour 0's low byte) where it was `$1CF2`; the
reference still wants `$4261`. So the row is a **power-on OBJ-palette-RAM
residue** row, not a fetch, timing or palette-write row, and making it pass means
knowing what one particular cgb04c unit left in undefined RAM. Keep it
baselined.

`scx_attrib_during_m3_spx2_ds` and `scx_during_m3_spx2_ds` dropped from 1096 px
to 16 px each (8 px on the single-speed row) with the 2026-07-31 double-speed
landing: all three spx2 rows are now this one divergence and nothing else.

#### Measured dead end: the CGB LCDC render-view delay (2026-07-31)

`bgtilemap_spx09_{1,2,3,4} [Cgb]` and `bgtiledata_spx08_ds_{3,4} [Cgb]` corrupt
the tile at BOTH ends of the mid-mode-3 LCDC toggle (128 px over rows 0-15, the
pair of tiles stepping with the rung), while the DMG legs pass with an
*identical* map-select sequence — traced fetch for fetch, the two models differ
only by the CGB pipeline's 1-dot lead. `RENDER_LCDC_DELAY` swept CGB-only over
1..=12 against the whole bgtilemap+bgtiledata set: 37, 37, **61**, 61, 49, 45,
then 37 flat. The shipped 3 is already optimal and no value recovers the spx
rows, so the delay is not the lever — both failing families are sprite lines, so
the term to look for is the CGB OBJ-stall geometry's effect on when the deferred
LCDC view reaches the fetch grid, not the deferral length.

The arms measured and rejected in this pass are folded into the canonical dead-end
list above.

#### The CGB OBJ-stall LCDC rows: what the references actually demand (2026-07-31)

Traced rather than swept, on `bgtilemap_spx0{8,9,A,B}_1 [Cgb]` line 8. The kernel
is a STAT handler writing `LCDC = $9F` (BG map `9C00`, all-black) then `$97`
(`9800`, all-white); the OBJ is transparent on lines 0-7 and black on 8-15, so
it contributes nothing but its fetch stall. Every rung writes at the same dot in
every directory (112 on the left, 236 on the right) and the render view commits
at 114/238. Only the fetch grid moves, by the sprite's alignment penalty:

| OAM X | screen x | `fetch_x = 2` tile-number read | reference wants tile 2 |
|---|---|---|---|
| `$08` | 0 | 116 | NEW map (`$9F`) |
| `$09` | 1 | 115 | OLD map (`$97`) |
| `$0A` | 2 | 114 | NEW map |
| `$0B` | 3 | 113 | OLD map |

The demanded answer is **not monotone in the read dot** — 113 old, 114 new, 115
old, 116 new — so no read-frame lever can express it. That rules out the whole
family of levers at once: the deferred-view length (`RENDER_LCDC_DELAY`, swept
1..=12 CGB-only, shipped 3 already optimal), an extra deferral on
sprite-selecting lines (swept 1..=6, monotonically worse: 58, 44, 40, 36, 32,
32 of 74), holding the deferral countdown while the pipeline is stalled (bit-for
-bit identical to not holding it), and a per-dot ring lead on the fetch's
`render_lcdc` view (lead 2 recovers all four `bgtilemap_spx09` rows exactly as
the dot arithmetic predicts, but breaks `spx08`/`spx0A`/`spx0B` and
`bgtiledata_spx09`, 61 → 49 of 74).

So the error is in the **stall geometry**, not the read frame: our
`fetch_x = 2` dot must be wrong for OAM X `$09` specifically. It is
`98 + 8 + stall` with `stall = obj_fetch_base(cgb, 0) + max(0, 5 - (x + SCX) % 8)`
= `10 - x` on CGB, and the four references want that fetch at `>= 114` for even
x and `<= 113` for odd x — a parity the current stall (strictly decreasing in x)
cannot produce. The mode-3 *length* those same stalls feed is frozen by the
mooneye `intr_2_mode0_timing_sprites` tables, so the next attempt has to
separate the pixel grid from the length, not retune the penalty.

The lead-2 ring probe also showed the two LCDC address bits want different
frames: bit 3 (map select, read at the tile-number phase) takes the lead, bit 4
(tile-data select, read at the Lo/Hi phases two and four dots later) does not.
Any future arm here must treat them separately or latch both at the
tile-number dot.
