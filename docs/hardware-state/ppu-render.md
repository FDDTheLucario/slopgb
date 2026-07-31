# PPU — window, fetch, OAM scan, mealybug, OAM bug

## Dot-serial OAM scan

`ppu/mod.rs` §Dot-serial OAM scan.

- Entry `i` is latched + evaluated at dot `2i+3` (gbctr; gambatte OamReader — `scan_latch_dot` anchoring pinned by gambatte oamdma/late_sp* + sprites/late_sizechange* per-slot races).
- Per-entry LCDC.2 sampling.
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

`ppu/render/mode0.rs` `fetcher_step`.

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
- Do: leave the Nintendo-licensee title-hash table deliberately unmodelled.

## PPU interrupt raising

- The PPU raises STAT/VBlank IRQs via `Ppu::write`'s return value (single drain).
- When adding a PPU register path, OR the returned IF bits into `intf` like the existing interconnect call sites.

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

So the carve-out is a CPU instruction-phase fingerprint of one ROM's control
flow, not a render law — which is also why no map lead reaches it (swept 0..7,
plain and with a pre-output exemption; everything >= 3 collapses 129 → 114 →
103 because other DS rows have writes landing closer still that must stay
visible). The gate is `ly >= 1 || SCX&7 != 0`, pinned from both sides in
`gambatte::misc::eager_scx_during_m3_map_column_passes`.

What would settle it properly: our line-0 dispatch phase depends on the
VBlank-region (ly 144 → 0) timing this ROM free-runs through. If that phase is
off by an M-cycle, the exception disappears on its own — that is the
`lyc153`/`ly0` family, not this cluster. The next section measures that phase
directly and finds it is a **general** property of the cluster, not one ROM's.

### The double-speed ladder, mostly closed (LANDED 2026-07-31, +27 rows)

Three whole-dot terms, all double-speed only, found by tracing the tile-number
read against the SCX write-commit dot and then classifying every leg against
what the reference actually accepts (`colreq`). Round matrix 103 → 130 of 141,
whole battery **27 baselined rows recovered, zero regressions**, golden drift
confined to 35 `scx_during_m3/*_ds_*` `[Cgb]` keys.

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

**3. Line 0 keeps lead 0** — a stated exemption, not a law; see below.

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

#### OPEN, and it is not a render term: the line-0 STAT dispatch is 4 dots late

These kernels are pure STAT handlers (`$0048` = `ei; jp $1000`, then a fixed
NOP sled), so every SCX write on a line is rigid relative to that line's
dispatch. Measured with an ack probe on `scx_0060c0/_ds_4 [Cgb]`: the ROM takes
**only** STAT (2169 acks, all bit 1, zero VBlank), and

```
RISE if=2 ly=1..143 dot=0   ->  ACK ly=N dot=6
RISE if=2 ly=0      dot=4   ->  ACK ly=0 dot=10
```

so every line-0 write lands 4 dots later than the same rung on lines 1-143 —
confirmed on all eight rungs of every directory, and at single speed too. The
`old/offset_3/_ds_1` disassembly above described this as one ROM's control-flow
fingerprint; it is not. It is the line-0 OAM interrupt source having no
prior-line carryover (`update_mode_for_interrupt`: lines 1-143 hold source 2
across dots 0-3, line 0 pulses at the visible mode-2 edge, dot 4).

The references want that dispatch **2 dots earlier**, uniformly: with the
line-0 writes 2 dots earlier a single lead of 2 satisfies line 0 and lines
1-143 alike, and no exemption is needed. Measured directly — pulsing the line-0
source at dot 2 in double speed (or holding it across dots 2-4) takes the round
matrix to 136/141 and the whole battery to **49 baselined rows recovered**,
including `bgtiledata_spx09_ds_*`, `bgtilemap_spx0{8,9}_ds_*`,
`scx_attrib_spx1_ds`, `scy_during_m3_ds_5/6` and `scy_spx08_ds_3/4`.

**It is not landable as-is: it costs 11 GREEN double-speed rows** —
`ly0/lycint152_m2irq_ds_1`, `lyc153int_m2irq_{ifw,late_retrigger}_ds_1`,
`lycEnable/lycwirq_trigger_ly00_stat50_ds_1`, `m2enable/{disable_ly0_ds_1,
lyc0_late_m2enable_lycdisable_ds_1, m2_late_m1disable_ly0_ds_1}`,
`miscmstatirq/lycstatwirq_trigger_ly00_10_50_ds_1`,
`window/late_enable_ly0_ds_2`, plus the pixel rows `scy_during_m3_ds_2` (8 px)
and `window/on_screen/late_wx_ds_1` (160 px).

The split inside that family is the lead worth chasing: the SAME ROM's
`lcdoffset1` rung goes the other way. `lycwirq_trigger_ly00_stat50_ds_1` wants
the old (later) edge while `..._ds_lcdoffset1_2` wants the new one, and
identically for `lycstatwirq_trigger_ly00_10_50`. `lcdoffset` shifts the LCD-on
phase by one dot, i.e. the CPU-vs-PPU dot phase — so the discriminator is that
phase, not the pulse dot. Until it exists the map lead carries an `ly == 0`
exemption instead, and both the exemption and the older holdback carve-out
dissolve the moment the line-0 edge is phase-correct.

#### The 11 rows still failing, and what each needs

| row(s) | shape | note |
|---|---|---|
| `old/offset_3/_ds_2` (10 px), `old/revoffset_3/_ds_1` (3 px), `scx_0063c0/_ds_1` (64 px), `scx_0360c0/_ds_1` (160 px), `_ds_2` (104 px), `scx_0367c0/_ds_2` (84 px), `scx_0761c0/_ds_1` (8 px), `_ds_4` (50 px), `_ds_5` (8 px), `scx_attrib_spx1_ds` (8 px) | line 0 only | the line-0 dispatch phase above; every one was 150-20500 px before this landing |
| `scx_0761c0/_ds_6` | first tile, lines 1-143 | its `$61` write commits exactly on the hunt lock dot 96 but is *pre*-lock by raw dot (92 < 96), so term 2 does not reach it; the reference wants it visible at the dot-98 read |
| `old/offset_3/_ds_3` | 2723 px | EXCEED — SameBoy misses it too |

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
* Ours is `$1CF2` because `interconnect/boot.rs` installs
  `CGB_COMPAT_OBJ_PALETTE` (`7FFF 421F 1CF2 0000`) on **any** CGB-model machine
  — including CGB-*flagged* carts. That contradicts the same function's
  `cgb_cart_cut` comment, which subtracts `$7D8` T-cycles from the hand-off
  precisely because "the DMG-compat path does its compatibility-palette work
  after the logo" and a CGB cart skips it. Booting these ROMs through the real
  `bootroms/cgb_boot.bin` / `cgbE_boot.bin` leaves OBJ palette RAM at its
  power-on fill instead.

So the row is a **power-on OBJ-palette-RAM residue** row, not a fetch, timing or
palette-write row. Making it pass means knowing what a cgb04c leaves there, which
neither the compat set nor our `0xFF` power-on fill supplies. The genuine bug the
investigation did surface — the unconditional compat install for CGB-flagged
carts — is worth fixing on its own merits, but it cannot recover this row (it
would render `$7FFF`, not `$4261`), and it moves post-boot state for every CGB
cart, so measure `misc/boot_hwio-C` (BCPS `$C8` / OCPS `$D0`) before touching it.

`scx_attrib_during_m3_spx2_ds` and `scx_during_m3_spx2_ds` dropped from 1096 px
to the same 8 px at x0 with the 2026-07-31 double-speed landing: all three spx2
rows are now this one divergence and nothing else.

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
