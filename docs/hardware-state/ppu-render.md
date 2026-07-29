# PPU — window, fetch, OAM scan, mealybug, OAM bug

## Dot-serial OAM scan

`ppu/mod.rs` §Dot-serial OAM scan.

- Entry `i` is latched + evaluated at dot `2i+3` (gbctr; gambatte OamReader — `scan_latch_dot` anchoring pinned by gambatte oamdma/late_sp* + sprites/late_sizechange* per-slot races).
- Per-entry LCDC.2 sampling.
- While OAM DMA owns OAM (running, or halt/stop-frozen) the scan latches `$FF` — a disabled sprite (`Ppu::oam_dma_active`, edges = gambatte startOamDma/endOamDma: the first byte's cycle still latches real OAM; the disconnect outlives the last copy by one M-cycle).

Parked: chasing the residual late_sp `_ds` out3 rows (half-dot, cc-granular races compounded with the frozen-ds mode-0 flip lead) or strikethrough's 7-px residue (an undocumented glitch-sprite, see `smallsuites.rs`) with **whole-dot** timing — don't chase either; whole-dot granularity can't resolve them.

## Window machine

`ppu/render.rs`.

- WX comparator runs every dot, including the 8-dot prefill. Match position by WX value:

  | WX value | Match dot/column |
  |---|---|
  | WX 0-7 | pause-aware dot `WX+6` (sprite stalls shift it via `pos_dot`) |
  | WX >= 8 | `lx == WX-7` |
  | WX <= 166 | (upper bound; above this never matches) |

- Rising-edge only (`win_match_prev`); checked **before** the same-dot sprite trigger (window start wins).
- `win_line` = gambatte winYPos (`0xFF` at frame start, `++` per activation, so same-line retriggers draw the next row).
- LCDC.5 off mid-line aborts at the eff commit, with the BG resuming on the live column `(scx+x+1-cgb)/8` (`window_abort`).
- WX=166 on DMG never starts in-line — instead a 2-dot freeze + carryover into the next line's mode-3 start (`win_start_pending`, window drawn from col 1).
- A WX match while drawing injects one color-0 pixel when it lands on a window tile boundary (mealybug "reactivation").
- WX=0 start adds `SCX&7+1` discards.

### WY sampling

- Discrete weMaster sampling at dots 450/454 (+1 DMG) and line-0 dot 2.
- Plus a live compare against `wy2`, which lags the write per model:

  | Model | `wy2` write lag |
  |---|---|
  | DMG | 2 dots |
  | CGB | 6 dots |
  | ds (double-speed) | 5 dots |

- WX commits to the pipeline 1 dot later than the palette strobe (`stage_write` FF4B dots+1, pinned by m3_wx_4/5/6_change).

## Mode-3 fetch grid

`ppu/render.rs` `fetcher_step`.

- Every fetch VRAM access samples `eff` clean at its read dot on **both** families.
- LCDC.1 gates sprite pixels at the mix as well as the fetch (m3_lcdc_obj_en_change).
- Sprites with OAM X 0-7 fetch during the pause-aware prefill walk (`prefill_pos`), freezing the SCX hunt (gambatte spx0/spx1); penalty math unchanged (mooneye tables frozen).
- The BG fetcher free-runs through every sprite stall (prefill included), with the line's first push waiting for the pause-aware startup walk (`push_allowed`), keeping pixel 0 on its stall-shifted dot.

Parked: the rising-late CGB LCDC fetch view — tried and rejected. See the mealybug note below: it fits most `_cgb_c` photo columns but contradicts hardware-captured gambatte bgtiledata spx0B rows. Current law samples `eff` clean at the read dot on both families instead.

## Mealybug ppu state

Status of the `m3_*` ppu_state tests:

| Status | Tests |
|---|---|
| Pixel-perfect (both legs) | m3_bgp_change, m3_scx_low_3_bits, m3_window_timing, m3_window_timing_wx_0, m3_lcdc_win_en_change_multiple, m3_wx_4_change_sprites |
| Pixel-perfect, [Dmg]-only | m3_wx_4_change, m3_wx_5_change, m3_wx_6_change |
| Pixel-perfect [Dmg] legs | m3_lcdc_tile_sel_change, m3_lcdc_tile_sel_win_change, m3_lcdc_bg_map_change, m3_lcdc_win_map_change, m3_scx_high_5_bits, m3_bgp_change_sprites, m3_obp0_change |

Remaining (not yet pixel-perfect) legs are mostly:
- [Cgb] fetch-law residue — see the parked rising-late CGB LCDC fetch view above and the baseline comments (`_cgb_c` photo columns vs hardware-captured gambatte bgtiledata spx0B rows).
- Small [Dmg] scy / bg_en / obj_en single-pixel residue.
- The obj_size pair.
- Sub-dot LCDC-write races: win_en_change_multiple_wx, m2_win_en_toggle [Dmg].

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
- Do: leave the Nintendo-licensee title-hash table deliberately unmodelled.

## PPU interrupt raising

- The PPU raises STAT/VBlank IRQs via `Ppu::write`'s return value (single drain).
- When adding a PPU register path, OR the returned IF bits into `intf` like the existing interconnect call sites.

## Mid-mode-3 SCX and the BG map column (open, localized)

The gambatte `scx_during_m3/scx_*c0/` families write SCX during mode 3 at an
increasing NOP offset (`_1`.._8`); the dir name is the value sequence, so
`scx_0060c0` writes $00 -> $60 -> $c0. 116 of these rows are baselined and
SameBoy passes them, so they are bugs, not a floor.

The failures are far smaller than the raw pixel counts suggest — probed
2026-07-28 by running the 15+1-frame protocol and aligning each scanline
against the reference:

- `scx_0060c0/scx_during_m3_2` [Cgb]: **143 of 144 rows exact**; row 0 differs
  in exactly 8 pixels (x0-x7).
- `scx_0360c0/scx_during_m3_2` [Cgb]: 143 of 144 exact; row 0 is a clean
  **+8-pixel (one tile) shift**, residual 0.
- `scx_0060c0/scx_during_m3_3`: the inverse — row 0 exact, rows 1-143 off by 8.

The 8 wrong pixels keep the reference's *bit pattern* and change only the
shade, which is the test's tell: the map repeats one tile graphic with a
different CGB palette attribute per column, so a shade change means the wrong
map **column** was fetched. Attribute and tile number are read from the same
map index in the same dot (`render/mode0.rs`), so they cannot desync — the
column itself is wrong.

The column is `(scx / 8) + fetch_x & 31`, sampled **live at the tile-number
read** (`render/mode0.rs`). The pass/fail ladder is the discriminator:

| dir (SCX sequence) | `_1` | `_2` | `_3` | `_4` | `_5` | `_6` |
|---|---|---|---|---|---|---|
| scx_0060c0 | pass | FAIL | FAIL | pass | pass | pass |
| scx_0063c0 | FAIL | FAIL | FAIL | pass | pass | pass |
| scx_0360c0 | pass | FAIL | FAIL | FAIL | FAIL | FAIL |

so the coarse-SCX sample point is off by a bounded window, and the initial
fine scroll (`scx & 7` = 0 vs 3 vs 7) selects which offsets land wrong — a
nonzero initial fine scroll leaves the dot-5..12 comparator hunt
(`render.rs`, `hunt_idx` vs a live `eff.scx & 7`) still running when the write
arrives, whereas `scx & 7 == 0` matches on the first hunt dot and is immune.

### The coarse-SCX sample point is NOT the lever (swept 2026-07-28)

Do not re-chase this. Both arms were built and measured against the
scx_during_m3 + scy/window/bgtiledata/bgtilemap PNG legs:

- **Uniform delay** on the coarse SCX feeding the map column (sample it N dots
  before the tile-number read; N=0 is the shipped behavior). N=1 is a no-op,
  N=2 scores +3 net, N=3 is -14 and N=4 is -25, so N=2 looks like a unique
  optimum — but the per-row delta shows it is a **shuffle, not a fix**: 13 rows
  recover (`_2`, `_3`, `_ds_2/3/6/7`) while 10 rows that were passing break
  (`_ds_1/4/5/8`, `scx_during_m3_spx0/1/2`). Want-opposite siblings, exactly
  the uniform-lever artifact `rom-diff-weld` exists to catch.
- **Line-start coarse latch** (only the fine scroll live mid-line): 4/135 vs a
  31/135 baseline. Refuted — hardware does re-read coarse SCX mid-line.

The guard families (scy, window, bgtiledata, bgtilemap) were byte-stable across
every arm, so the effect is confined to this cluster.

What the ladder actually says: the double-speed row `scx_0060c0` is
`pass FAIL FAIL pass pass FAIL FAIL pass` over `_ds_1.._8` — a period of 4
M-cycles, and at double speed 4 M-cycles = 8 dots = exactly one steady-state
BG tile fetch cycle. So a 4-dot window inside each 8-dot fetch cycle is
mishandled, and a uniform shift only slides which offsets land in it. For
`scx_0060c0` the fine scroll never changes ($00/$60/$c0 all have `scx & 7 == 0`),
so `scx_write_dot` never latches and the comparator hunt matches on its first
dot: the coarse map column is the *only* live path, which is what makes this
family a clean probe.

### The in-flight fetch phase is not a discriminator either (swept 2026-07-28)

The follow-up hypothesis — that a mid-tile coarse write should retarget the
in-flight fetch only while it is early enough in the fetch — was built as a
real discriminated arm (`Render::coarse` latched per tile fetch, with the FF43
write applying to it only below a threshold) and swept two ways:

- threshold on `FetchPhase` rank (0..=6);
- threshold on dots-since-fetch-start (0..=8), which resolves finer than the
  phase because the fetcher parks in `Push` for the tail of the 8-dot cycle
  and the phase rank saturates there.

**Both collapse to a binary.** Threshold 0 (never retarget: coarse latched at
fetch start) scores 34/135; every threshold >= 1 reproduces the shipped live
read at 31/135, with nothing in between. The knob is degenerate — writes always
land at least one dot into a fetch, so "retarget if early" is never distinct
from "always retarget". Only two behaviors are reachable in this formulation,
and both were already measured above: latch-at-fetch-start is the +13/-10
shuffle, live is the baseline.

So the split between the two sibling groups is **not** about where in the tile
fetch the write lands.

### The FIFO pop/push coupling is correct (swept 2026-07-28)

The follow-up leads were measured too, and both are refuted:

- **No same-dot FIFO refill** (a FIFO that drains on a dot refills on the next
  dot instead of the same one): 29/312 against a 158/312 baseline, wrecking
  scy/bgtiledata/bgtilemap outright. The same-dot refill is load-bearing — it
  is what produces the 8-dot steady-state cadence. `render_step` pops first and
  then lets `fetcher_step` push into the emptied FIFO on that same dot, and
  that ordering is right.
- **A coarse SCX change restarts the in-flight tile fetch** (phase back to the
  tile-number read, the way a window start re-anchors): 2/135 in the cluster.
  The guard families are untouched, since the arm only fires on coarse changes,
  so this is a clean refutation rather than a trade.

Ruled out for this cluster so far, all measured: uniform coarse sample delay
(shuffle), line-start coarse latch, a fetch-phase-discriminated arm, a
dots-since-fetch-start threshold, deferred FIFO refill, and fetch restart on a
coarse change. The mod-4 split below is still unexplained by any of them, so
the next attempt should start from a *new* observable rather than another
variation on when the map column is addressed.

For the record, the groups that want opposite answers, from the double-speed
ladder: `_ds_1/4/5/8` (positions 0,1 mod 4) want the live read; `_ds_2/3/6/7`
(positions 2,3 mod 4) want the latched one.
