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

## Mid-mode-3 SCX and the BG map column (LANDED 2026-07-29, both models)

The BG tile-map column is derived from the pixel output position, not from a
tile counter. SameBoy `display.c` `GB_FETCHER_GET_TILE_T1` forms it as
`(SCX + position_in_line + 8 - (is_cgb && !during_object_fetch)) / 8`: SCX is
summed with the position and divided **once**, so SCX's low three bits carry
into the tile index. Dividing the coarse part out first and counting tiles
separately (`scx / 8 + fetch_x`) cannot express that carry — which is exactly
what the gambatte `scx_during_m3/scx_*c0` ladders measure, and why the whole
family failed on any *timing* adjustment.

`Render::lx` is our position. Three details are ours rather than SameBoy's, all
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

Result: **48 baseline rows recovered, zero regressions** — 37 on the CGB pass
(34 gambatte + 3 age `m3-bg-scx`) and 11 more when the DMG anchor enabled the
form there (10 gambatte + `m3-bg-scx.gb [Dmg]`). Golden drift is confined to
SCX-named ROMs in both passes (79, then 34).

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

### Measured dead ends (do not re-chase)

All of these were built and measured against this cluster; every one either
shuffles want-opposite siblings or retimes rows outside the intended window:
a uniform coarse-SCX sample delay, a line-start coarse latch, a fetch-phase
threshold, a dots-since-fetch-start threshold, a deferred FIFO refill (the
same-dot refill is load-bearing — it is what produces the 8-dot cadence), a
fetch restart on a coarse change, a `read - 4` dot rule, a per-line measured
anchor, and latching the column at `TileNoWait` to mirror SameBoy's T1/T2 split
(13 regressions — computing it on the T2 side is correct for our pipeline).

### Residual: 73 rows, and it is the read frame, not the column value

What is left in this family is the `_2`/`_3` rounds of every `scx_*c0` dir plus
their `_ds_2/3/6/7` counterparts, 39 double-speed and 34 single-speed.

These are **not** a column-value problem. `scx_0060c0` writes `$00 -> $60 -> $c0`,
all with `SCX & 7 == 0`, so only the coarse scroll moves — and for a coarse-only
change the position form and the tile counter shift by the same amount. Widening
the gate from "fine moved" to "any SCX moved" was measured and changes nothing
(40/80 either way on the round matrix), which is the proof: whatever the column
formula, these rounds land the same value.

They fail on **when** SCX is sampled. On line 1 of the double-speed ladder the
tile-number reads sit at dots 234 and 242 and the writes step 2 dots per round;
`_2` writes at 241 and `_3` at 239, both inside `(238, 242)`, so we apply a write
that hardware's already-formed address does not see.

Three read-frame arms measured against the round matrix (baseline 40 pass / 40
fail), all refuted:

| arm | result |
|---|---|
| latch SCX at `TileNoWait` (SameBoy's T1) | 43/37 — a shuffle, round 19 regresses in all four dirs |
| latch SCX **and** `lx` at `TileNoWait` | 40/40, `scx_0761c0` round 1 regresses |
| latch SCX at the previous fetch's `Hi` (read − 4) | 8/72 — `Hi == read - 4` only holds in steady state |

So the address-formation dot is somewhere between our `TileNoWait` and the read,
and no single latch point expresses it — the same shape as the original bug, one
level down. A fast round-matrix harness (four dirs x 14 rounds x 2 models,
seconds per run) is the tool to iterate on it; rebuild it before trying a fourth
arm.
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
governs them; do not spend a fourth sweep on this parameter family. **+20 rows, zero
regressions**; mealybug and age both clean. Note the earlier "mealybug
m3_scx_high_5_bits regresses" reading was an artifact of scoring mealybug ROMs
with the gambatte 15+1-frame protocol — they exit on `LD B,B`.

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

Ruled out for this cluster, all measured: uniform coarse sample delay
(shuffle), line-start coarse latch, a fetch-phase-discriminated arm, a
dots-since-fetch-start threshold, deferred FIFO refill, and fetch restart on a
coarse change.

### The map-column latch: what one unified trace actually shows

Tracing fetch phases and FF43 writes **in the same run on `ds_4`**, on the
evaluated (16th) frame, settles the earlier inconsistency. The previous two
traces had been taken from different ROMs and different frames, which is what
made the numbers disagree.

Line 1 of `scx_0060c0/scx_during_m3_ds_4`, steady state:

```
* dot= 92 SCXWR 00->60
  dot=233 TileNoWait fx=18   dot=234 TileNo fx=18 (scx=60)
  dot=237 HiWait     fx=18
* dot=237 SCXWR 60->C0
  dot=238 Hi         fx=18  (scx already C0)
  dot=241 TileNoWait fx=19   dot=242 TileNo fx=19 (scx=C0)
```

So the fetch cadence is 8 dots with `Hi` at `read - 4`, exactly as inferred,
and the ladder's writes step 2 dots per index (ds_1 at 243 down to ds_8 at 229).

**Correction to the earlier entry in this file:** the claim that a
`latch = read - 4` rule makes all eight `_ds` rows pass was wrong. The rule only
changes rows whose write falls in the open window `(read-4, read)` — dots 239
and 241 for the `fx=19` read at 242, i.e. **`ds_3` and `ds_2` only**. Rows
whose write lands outside that window (ds_1 at 243, ds_4 at 237, ds_5 at 235,
ds_8 at 229) are unaffected by the rule and keep whatever the shipped live read
already gives them. The honest prediction is +2 with no regressions, not +8.

That also explains why implementing it as "latch at the previous `Hi`" measured
31/135 -> 9/135. `Hi == read - 4` holds only in steady state; around the
line-start fetches (`first_discard`, the push gating in `push_allowed`, the
12-dot startup walk) the previous `Hi` sits much further back than 4 dots, so
the arm silently retimed the *early* tiles too. The cell-symbol dump is the
check that catches this: on `ds_2` the shipped build already matches the
reference on cells 0-18 and differs only at cell 19, so any arm that perturbs
an early cell is wrong by construction.

### The `read - 4` rule is REFUTED as a dot offset (built 2026-07-28)

Built exactly as specified: a per-dot `eff.scx` ring, the tile-number read
taking the value from 4 dots earlier, and the line's first real fetch
(`fetch_x == 0`) exempted — that fetch's read coincides with the line-start SCX
write, measured at dot 92 on both counts, and the startup walk has no 4-dot
history.

The prediction was +2 (`ds_2`, `ds_3`) with nothing else moving. Measured: the
cluster goes 31/135 -> **8/135**. Exactly one row recovers (`ds_2`) and 24
regress. `ds_3` does not recover at all. The guard families are byte-stable, so
the arm is scoped correctly and this is a genuine refutation of the rule, not a
cross-family trade.

The regressions name the reason: `scx_0060c0` and `scx_0063c0` lose their
single-speed `_4/_5/_6` legs on **both models**, plus `_ds_4/5/8`, all three
`scx_during_m3_spx*` ROMs, and `scx_0761c0/_1 [Dmg]`. A 4-dot offset is two
M-cycles at double speed but a **single** M-cycle at single speed, so one
constant cannot mean the same thing on both — the single-speed legs shift by a
whole instruction's worth of write timing and fall out of the window they were
already inside.

So the window is not a fixed dot offset. Anything replacing it has to be
expressed in a unit that survives the speed switch (fetch-relative, or
M-cycle-relative with a speed term), and it has to explain why `ds_3` stays red
under a rule tuned to admit exactly its write dot. Both remain open.

### ROOT CAUSE: our BG map column formula is structurally wrong

Reading SameBoy 1.0.2 `Core/display.c` ends the guessing. In
`advance_fetcher_state_machine`, case `GB_FETCHER_GET_TILE_T1` (display.c:958-962):

```c
else if ((uint8_t)(gb->position_in_line + 16) < 8) {
    x = gb->io_registers[GB_IO_SCX] >> 3;          // line-start window
}
else {
    x = ((gb->io_registers[GB_IO_SCX] + gb->position_in_line + 8
          - (GB_is_cgb(gb) && !gb->during_object_fetch)) / 8) & 0x1F;
}
gb->last_tile_index_address = map + x + y / 8 * 32;
```

Ours (`render/mode0.rs`, `FetchPhase::TileNo`) is
`(scx / 8).wrapping_add(fetch_x) & 31`.

Three divergences, in order of importance:

1. **Sum then divide, not divide then count.** SameBoy adds SCX to
   `position_in_line` (the *pixel* output position, running from -16) and
   divides once, so SCX's low three bits carry into the tile index. We divide
   the coarse part out first and track tiles with an independent `fetch_x`
   counter, so a fine-scroll change can never move our column. For a stable SCX
   the two agree exactly — which is why the rest of the BG corpus passes — and
   they diverge precisely when SCX changes mid-line, i.e. this cluster.
2. **A CGB-only -1 term**, `8 - (is_cgb && !during_object_fetch)`: the CGB forms
   the address one pixel earlier than the DMG except while an object fetch is in
   flight. We have no such term, which is why the single-speed and
   double-speed legs of the same dir disagree under every uniform arm.
3. **The address is formed one T-cycle before the read.** SameBoy computes it in
   `GET_TILE_T1` and does the VRAM read in `GET_TILE_T2`; we compute and read in
   the same dot.

This explains all seven failed arms at once: every one of them tuned *when* SCX
is sampled, but the *formula* is wrong, so no sampling time can be right for
both a fine-scroll-0 dir (`scx_0060c0`) and a fine-scroll-3/7 one
(`scx_0360c0`, `scx_0761c0`). It also explains why the pass/fail ladder keyed on
the initial fine scroll from the very first measurement.

Note the SCX fine comparator itself already matches SameBoy: display.c:710
resolves the discard with `(position_in_line & 7) == (SCX & 7)` against a live
SCX, which is what `render.rs`'s `hunt_idx` does.

**Fixing this is a fetcher-structure change, not a timing tweak.** Replacing
`scx / 8 + fetch_x` with a position-derived column touches every BG fetch on
every line, so it re-derives the ~6000 green dot-level cases (mealybug photos,
the mode-3 fetch grid, the window machine) and must be gated on the full battery
plus `golden_fingerprint`, not on this cluster. `fetch_x` is also the window's
tile counter (`win_mode` uses it directly), so the window path has to keep its
own counter when the BG path stops using one.

### Porting the formula in isolation does NOT work (attempted 2026-07-28)

The formula was ported behind a gate, with a `pos_in_line` field added to
`Render` to stand in for SameBoy's `position_in_line`. Two variants were
measured against the cluster plus the scy / window / bgtiledata / bgtilemap /
dmgpalette guard families:

| variant | cluster | guards |
|---|---|---|
| shipped | 31/135 | unchanged |
| ported formula, position advanced on every pop | 21/135 | unchanged |
| + SameBoy's `-9 -> -16` hunt wrap (display.c:716) | **0/135** | unchanged |

The guards are byte-stable in both variants, which proves the formula is
*equivalent to ours for a stable SCX* — the port is arithmetically right. What
is wrong is the position semantics, and that is not a detail that can be bolted
on:

- our pipeline has no pixel-position counter. It carries `prefill_pos`,
  `hunt_idx`, `discard` and `fetch_x` as four separate pieces of state, and the
  discarded first tile's pixels are never actually popped (the comparator runs
  "as a bare counter", see `render.rs`), so there is nothing that corresponds
  to `position_in_line` running -16 -> 160;
- SameBoy hunts in a single counter that wraps `-9 -> -16` until the comparator
  matches, so its position is never in `[-8, 0)` while hunting. We hunt in *two*
  phases (a dot-rate prefill phase and a pop-rate phase after the FIFO starts
  draining). Feeding the wrap into the pop-rate phase pushes mid-line fetches
  into SameBoy's line-start branch (`position_in_line < -8` -> bare `SCX >> 3`),
  which is what takes the cluster to zero.

**So the column formula cannot be ported without first porting
`position_in_line` itself as the pipeline's primary position state**, replacing
the prefill/hunt/discard/fetch_x quartet. That is the fetcher-structure change,
and it is a multi-session refactor: `fetch_x` is also the window's tile counter,
the discarded-tile phase would have to start popping real pixels, and every one
of the ~6000 green dot-level cases re-derives.

### Why the refactor is a pipeline rewrite: the fetch/output coupling differs

The cheap way to test any port of the column formula is an **equivalence check
on a stable-SCX ROM**: with SCX constant the ported formula must reproduce
`(scx / 8) + fetch_x` exactly, fetch for fetch, because every non-`scx_during_m3`
BG row already passes. Instrument both columns at the tile-number read and
count mismatches — seconds per run, no battery needed. Use it before any
full-matrix measurement.

Running that check while porting `position_in_line` gives the blocker directly.
On `bgtilemap_spx08_ds_1` (SCX constant at 0), with the position maintained as
a real output counter:

```
COLDIFF ly=0 dot=84 fx=0 pos=0
COLDIFF ly=0 dot=90 fx=0 pos=0
COLDIFF ly=0 dot=96 fx=1 pos=2      <-- fetch 1 happens at output position 2
```

SameBoy's formula assumes a **fixed** fetcher-ahead-of-output distance: the tile
fetched at position `p` supplies the pixels at `p + 8`, which is exactly what the
`+8` in `(SCX + position_in_line + 8 - cgb) / 8` encodes. Our pipeline does not
hold that invariant — at `fetch_x == 1` the output position is 2, not 8, because
our fetcher runs ahead during the 12-dot startup while the FIFO fills and,
unlike SameBoy, we never pop the discarded first tile's pixels. A single
additive constant cannot reconcile the two: sweeping it moved the bulk error
from a uniform -1 tile (constant `+8`) to 23.6M mismatches (constant `0`).

So the column formula is not portable on top of our fetch/output relationship.
Landing it requires giving the pipeline SameBoy's lockstep first — the
discarded tile actually popping its 8 pixels, the position counter as the
primary state, and `prefill_pos`/`hunt_idx`/`discard`/`fetch_x` collapsing into
it — with the window keeping its own tile counter. That re-derives every green
dot-level case (mealybug photos, the mode-3 fetch grid, the window machine) and
is a scoped rewrite of mode 3, not an increment.

### The working formula (measured, +40, not yet landed)

The port DOES work once the position mapping is derived from data instead of
guessed. `Render::lx` is our `position_in_line`; the mapping was found with an
**equivalence check** — with SCX held constant the ported column must reproduce
`(scx / 8) + fetch_x` fetch-for-fetch, which runs in seconds on one ROM and
needs no battery. Use it for every future iteration; it caught three wrong
constants that a battery run would have taken 6 minutes each to reject.

Current best (equivalence-clean on both models, `bgtilemap` / `bgtiledata` /
`window`, CGB and DMG all zero mismatches):

```rust
fn bg_map_col(scx: u8, lx: u8, fetch_x: u8, cgb: bool, lead: bool) -> u8 {
    // lx == 0: pre-output, still in the leading-discard band.
    // !lead (sprite stall running): our BG fetcher free-runs while the output
    // position is frozen, so the position cannot track the fetch there —
    // SameBoy instead parks its fetcher in PUSH.
    if lx == 0 || !lead {
        return (scx >> 3).wrapping_add(fetch_x) & 31;
    }
    let v = i32::from(scx) + i32::from(lx) + 6 + i32::from(cgb);
    (v.div_euclid(8) & 31) as u8
}
// call site: bg_map_col(scx, r.lx, r.fetch_x, model.is_cgb(), r.stall == 0)
```

The `6 + cgb` anchor is measured, not SameBoy's literal `8 - is_cgb`: our `lx`
sits at `8 * fetch_x - 6` at fetch time rather than SameBoy's multiple of 8,
because we re-fetch the discarded first tile instead of pushing and popping it.
DMG anchors 2 lower than CGB, consistent with the blob's pipeline already
sitting two dots behind (6-dot OBJ fetches vs 5, mode-0 flip leading by 3 vs 2).

**Ledger: `scx_during_m3` 31/135 -> 64/135, 40 baseline rows now passing, 5
regressions** — 4 gambatte (`scx_during_m3/old/offset_3/scx_during_m3_ds_1
[Cgb]` and three siblings, 8 px on row 0 each) and 1 mealybug
(`m3_scx_high_5_bits_change2 [Cgb]`, 160 px). Every guard family
(scy/window/bgtiledata/bgtilemap/dmgpalette) is byte-stable. An earlier variant
(`+8 - cgb_lead`, no stall fallback) scored +39 with 3 regressions.

**Not landed — blocked by the SameBoy-pass rule, not by effort.** Two variants
were taken all the way through the battery:

| variant | recovered | regressed |
|---|---|---|
| anchor 6, both models | **43** | 4 |
| anchor 6, CGB only (`\|\| !cgb` in the fallback) | 16 | 1 |

Every regression in both variants was classified with `classify_pixel.py`:
**all of them are SameBoy-PASS at `mm=0`** (exact match against the reference).
`baselines/gambatte.txt` class F is explicit that dropping a row SameBoy passes
is forbidden, so neither variant can be landed by absorbing its residual, however
favourable the net. The C3-flip precedent added 44 rows, but those were all
SameBoy-FAIL.

The residual rows, all `[Cgb]` unless noted, and all SameBoy-PASS:
`scx_during_m3/old/offset_3/scx_during_m3_ds_1`, plus (both-models variant)
`scx1_scx0_during_m3_1 [Dmg]`, `scx2_scx1_during_m3_1 [Dmg]`,
`scx_0761c0/scx_during_m3_1 [Dmg]`.

Variants measured and rejected while narrowing this: anchor `8 - cgb_lead`
(2 mealybug regressions, spurious carry at `scx & 7 >= 6`), anchor `6 + cgb`
(breaks CGB-during-stall), dropping the stall fallback (mealybug regresses),
a per-line measured anchor (mealybug regresses), and extending the position form
into the pre-output window once the hunt resolved (no effect).

**Best variant so far: 39 recovered, 1 regression, mealybug clean.** Add a
`fine_moved` gate to the fallback — the position form is only reached once
`scx & 7` differs from the value the line started with:

```rust
// Render: line_fine = eff.scx & 7, captured at the render reset.
// call: bg_map_col(scx, r.lx, r.fetch_x, is_cgb, r.stall == 0, r.hunt_done,
//                  scx & 7 != r.line_fine)
if (lx == 0 && !hunted) || !lead || !cgb || !fine_moved {
    return (scx >> 3).wrapping_add(fetch_x) & 31;
}
let v = i32::from(scx) + i32::from(lx) + 6;
(v.div_euclid(8) & 31) as u8
```

The gate is free by construction: while the fine scroll holds, the two forms are
provably identical (that is what the equivalence check proves), so it can only
remove behavior on lines that actually move `SCX & 7`.

Also rejected: latching the column at `TileNoWait` to mirror SameBoy forming the
address in `GET_TILE_T1` and reading in T2 — 13 regressions plus mealybug, so
our T2-side computation is right for our pipeline.

What remains is the `old/offset_3` DS + lcd-offset row: 8 px on row 0, the last
tile. Our per-line dot alignment there puts `lx` off the `≡ 2 (mod 8)` grid the
fixed anchor assumes. Solve that one row — without disturbing mealybug's
`m3_scx_high_5_bits` or the DMG fine-transition ROMs — and the CGB-only variant
lands 16 rows immediately; extending the same treatment to DMG lands 43.

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

Ruled out for this cluster, all measured: uniform coarse sample delay
(shuffle), line-start coarse latch, a fetch-phase-discriminated arm, a
dots-since-fetch-start threshold, deferred FIFO refill, and fetch restart on a
coarse change.

### The map-column latch: what one unified trace actually shows

Tracing fetch phases and FF43 writes **in the same run on `ds_4`**, on the
evaluated (16th) frame, settles the earlier inconsistency. The previous two
traces had been taken from different ROMs and different frames, which is what
made the numbers disagree.

Line 1 of `scx_0060c0/scx_during_m3_ds_4`, steady state:

```
* dot= 92 SCXWR 00->60
  dot=233 TileNoWait fx=18   dot=234 TileNo fx=18 (scx=60)
  dot=237 HiWait     fx=18
* dot=237 SCXWR 60->C0
  dot=238 Hi         fx=18  (scx already C0)
  dot=241 TileNoWait fx=19   dot=242 TileNo fx=19 (scx=C0)
```

So the fetch cadence is 8 dots with `Hi` at `read - 4`, exactly as inferred,
and the ladder's writes step 2 dots per index (ds_1 at 243 down to ds_8 at 229).

**Correction to the earlier entry in this file:** the claim that a
`latch = read - 4` rule makes all eight `_ds` rows pass was wrong. The rule only
changes rows whose write falls in the open window `(read-4, read)` — dots 239
and 241 for the `fx=19` read at 242, i.e. **`ds_3` and `ds_2` only**. Rows
whose write lands outside that window (ds_1 at 243, ds_4 at 237, ds_5 at 235,
ds_8 at 229) are unaffected by the rule and keep whatever the shipped live read
already gives them. The honest prediction is +2 with no regressions, not +8.

That also explains why implementing it as "latch at the previous `Hi`" measured
31/135 -> 9/135. `Hi == read - 4` holds only in steady state; around the
line-start fetches (`first_discard`, the push gating in `push_allowed`, the
12-dot startup walk) the previous `Hi` sits much further back than 4 dots, so
the arm silently retimed the *early* tiles too. The cell-symbol dump is the
check that catches this: on `ds_2` the shipped build already matches the
reference on cells 0-18 and differs only at cell 19, so any arm that perturbs
an early cell is wrong by construction.

### The `read - 4` rule is REFUTED as a dot offset (built 2026-07-28)

Built exactly as specified: a per-dot `eff.scx` ring, the tile-number read
taking the value from 4 dots earlier, and the line's first real fetch
(`fetch_x == 0`) exempted — that fetch's read coincides with the line-start SCX
write, measured at dot 92 on both counts, and the startup walk has no 4-dot
history.

The prediction was +2 (`ds_2`, `ds_3`) with nothing else moving. Measured: the
cluster goes 31/135 -> **8/135**. Exactly one row recovers (`ds_2`) and 24
regress. `ds_3` does not recover at all. The guard families are byte-stable, so
the arm is scoped correctly and this is a genuine refutation of the rule, not a
cross-family trade.

The regressions name the reason: `scx_0060c0` and `scx_0063c0` lose their
single-speed `_4/_5/_6` legs on **both models**, plus `_ds_4/5/8`, all three
`scx_during_m3_spx*` ROMs, and `scx_0761c0/_1 [Dmg]`. A 4-dot offset is two
M-cycles at double speed but a **single** M-cycle at single speed, so one
constant cannot mean the same thing on both — the single-speed legs shift by a
whole instruction's worth of write timing and fall out of the window they were
already inside.

So the window is not a fixed dot offset. Anything replacing it has to be
expressed in a unit that survives the speed switch (fetch-relative, or
M-cycle-relative with a speed term), and it has to explain why `ds_3` stays red
under a rule tuned to admit exactly its write dot. Both remain open.

### ROOT CAUSE: our BG map column formula is structurally wrong

Reading SameBoy 1.0.2 `Core/display.c` ends the guessing. In
`advance_fetcher_state_machine`, case `GB_FETCHER_GET_TILE_T1` (display.c:958-962):

```c
else if ((uint8_t)(gb->position_in_line + 16) < 8) {
    x = gb->io_registers[GB_IO_SCX] >> 3;          // line-start window
}
else {
    x = ((gb->io_registers[GB_IO_SCX] + gb->position_in_line + 8
          - (GB_is_cgb(gb) && !gb->during_object_fetch)) / 8) & 0x1F;
}
gb->last_tile_index_address = map + x + y / 8 * 32;
```

Ours (`render/mode0.rs`, `FetchPhase::TileNo`) is
`(scx / 8).wrapping_add(fetch_x) & 31`.

Three divergences, in order of importance:

1. **Sum then divide, not divide then count.** SameBoy adds SCX to
   `position_in_line` (the *pixel* output position, running from -16) and
   divides once, so SCX's low three bits carry into the tile index. We divide
   the coarse part out first and track tiles with an independent `fetch_x`
   counter, so a fine-scroll change can never move our column. For a stable SCX
   the two agree exactly — which is why the rest of the BG corpus passes — and
   they diverge precisely when SCX changes mid-line, i.e. this cluster.
2. **A CGB-only -1 term**, `8 - (is_cgb && !during_object_fetch)`: the CGB forms
   the address one pixel earlier than the DMG except while an object fetch is in
   flight. We have no such term, which is why the single-speed and
   double-speed legs of the same dir disagree under every uniform arm.
3. **The address is formed one T-cycle before the read.** SameBoy computes it in
   `GET_TILE_T1` and does the VRAM read in `GET_TILE_T2`; we compute and read in
   the same dot.

This explains all seven failed arms at once: every one of them tuned *when* SCX
is sampled, but the *formula* is wrong, so no sampling time can be right for
both a fine-scroll-0 dir (`scx_0060c0`) and a fine-scroll-3/7 one
(`scx_0360c0`, `scx_0761c0`). It also explains why the pass/fail ladder keyed on
the initial fine scroll from the very first measurement.

Note the SCX fine comparator itself already matches SameBoy: display.c:710
resolves the discard with `(position_in_line & 7) == (SCX & 7)` against a live
SCX, which is what `render.rs`'s `hunt_idx` does.

**Fixing this is a fetcher-structure change, not a timing tweak.** Replacing
`scx / 8 + fetch_x` with a position-derived column touches every BG fetch on
every line, so it re-derives the ~6000 green dot-level cases (mealybug photos,
the mode-3 fetch grid, the window machine) and must be gated on the full battery
plus `golden_fingerprint`, not on this cluster. `fetch_x` is also the window's
tile counter (`win_mode` uses it directly), so the window path has to keep its
own counter when the BG path stops using one.

### Porting the formula in isolation does NOT work (attempted 2026-07-28)

The formula was ported behind a gate, with a `pos_in_line` field added to
`Render` to stand in for SameBoy's `position_in_line`. Two variants were
measured against the cluster plus the scy / window / bgtiledata / bgtilemap /
dmgpalette guard families:

| variant | cluster | guards |
|---|---|---|
| shipped | 31/135 | unchanged |
| ported formula, position advanced on every pop | 21/135 | unchanged |
| + SameBoy's `-9 -> -16` hunt wrap (display.c:716) | **0/135** | unchanged |

The guards are byte-stable in both variants, which proves the formula is
*equivalent to ours for a stable SCX* — the port is arithmetically right. What
is wrong is the position semantics, and that is not a detail that can be bolted
on:

- our pipeline has no pixel-position counter. It carries `prefill_pos`,
  `hunt_idx`, `discard` and `fetch_x` as four separate pieces of state, and the
  discarded first tile's pixels are never actually popped (the comparator runs
  "as a bare counter", see `render.rs`), so there is nothing that corresponds
  to `position_in_line` running -16 -> 160;
- SameBoy hunts in a single counter that wraps `-9 -> -16` until the comparator
  matches, so its position is never in `[-8, 0)` while hunting. We hunt in *two*
  phases (a dot-rate prefill phase and a pop-rate phase after the FIFO starts
  draining). Feeding the wrap into the pop-rate phase pushes mid-line fetches
  into SameBoy's line-start branch (`position_in_line < -8` -> bare `SCX >> 3`),
  which is what takes the cluster to zero.

**So the column formula cannot be ported without first porting
`position_in_line` itself as the pipeline's primary position state**, replacing
the prefill/hunt/discard/fetch_x quartet. That is the fetcher-structure change,
and it is a multi-session refactor: `fetch_x` is also the window's tile counter,
the discarded-tile phase would have to start popping real pixels, and every one
of the ~6000 green dot-level cases re-derives.

### Why the refactor is a pipeline rewrite: the fetch/output coupling differs

The cheap way to test any port of the column formula is an **equivalence check
on a stable-SCX ROM**: with SCX constant the ported formula must reproduce
`(scx / 8) + fetch_x` exactly, fetch for fetch, because every non-`scx_during_m3`
BG row already passes. Instrument both columns at the tile-number read and
count mismatches — seconds per run, no battery needed. Use it before any
full-matrix measurement.

Running that check while porting `position_in_line` gives the blocker directly.
On `bgtilemap_spx08_ds_1` (SCX constant at 0), with the position maintained as
a real output counter:

```
COLDIFF ly=0 dot=84 fx=0 pos=0
COLDIFF ly=0 dot=90 fx=0 pos=0
COLDIFF ly=0 dot=96 fx=1 pos=2      <-- fetch 1 happens at output position 2
```

SameBoy's formula assumes a **fixed** fetcher-ahead-of-output distance: the tile
fetched at position `p` supplies the pixels at `p + 8`, which is exactly what the
`+8` in `(SCX + position_in_line + 8 - cgb) / 8` encodes. Our pipeline does not
hold that invariant — at `fetch_x == 1` the output position is 2, not 8, because
our fetcher runs ahead during the 12-dot startup while the FIFO fills and,
unlike SameBoy, we never pop the discarded first tile's pixels. A single
additive constant cannot reconcile the two: sweeping it moved the bulk error
from a uniform -1 tile (constant `+8`) to 23.6M mismatches (constant `0`).

So the column formula is not portable on top of our fetch/output relationship.
Landing it requires giving the pipeline SameBoy's lockstep first — the
discarded tile actually popping its 8 pixels, the position counter as the
primary state, and `prefill_pos`/`hunt_idx`/`discard`/`fetch_x` collapsing into
it — with the window keeping its own tile counter. That re-derives every green
dot-level case (mealybug photos, the mode-3 fetch grid, the window machine) and
is a scoped rewrite of mode 3, not an increment.

### The working formula (measured, +40, not yet landed)

The port DOES work once the position mapping is derived from data instead of
guessed. `Render::lx` is our `position_in_line`; the mapping was found with an
**equivalence check** — with SCX held constant the ported column must reproduce
`(scx / 8) + fetch_x` fetch-for-fetch, which runs in seconds on one ROM and
needs no battery. Use it for every future iteration; it caught three wrong
constants that a battery run would have taken 6 minutes each to reject.

Current best (equivalence-clean on both models, `bgtilemap` / `bgtiledata` /
`window`, CGB and DMG all zero mismatches):

```rust
fn bg_map_col(scx: u8, lx: u8, fetch_x: u8, cgb: bool, lead: bool) -> u8 {
    // lx == 0: pre-output, still in the leading-discard band.
    // !lead (sprite stall running): our BG fetcher free-runs while the output
    // position is frozen, so the position cannot track the fetch there —
    // SameBoy instead parks its fetcher in PUSH.
    if lx == 0 || !lead {
        return (scx >> 3).wrapping_add(fetch_x) & 31;
    }
    let v = i32::from(scx) + i32::from(lx) + 6 + i32::from(cgb);
    (v.div_euclid(8) & 31) as u8
}
// call site: bg_map_col(scx, r.lx, r.fetch_x, model.is_cgb(), r.stall == 0)
```

The `6 + cgb` anchor is measured, not SameBoy's literal `8 - is_cgb`: our `lx`
sits at `8 * fetch_x - 6` at fetch time rather than SameBoy's multiple of 8,
because we re-fetch the discarded first tile instead of pushing and popping it.
DMG anchors 2 lower than CGB, consistent with the blob's pipeline already
sitting two dots behind (6-dot OBJ fetches vs 5, mode-0 flip leading by 3 vs 2).

**Ledger: `scx_during_m3` 31/135 -> 64/135, 40 baseline rows now passing, 5
regressions** — 4 gambatte (`scx_during_m3/old/offset_3/scx_during_m3_ds_1
[Cgb]` and three siblings, 8 px on row 0 each) and 1 mealybug
(`m3_scx_high_5_bits_change2 [Cgb]`, 160 px). Every guard family
(scy/window/bgtiledata/bgtilemap/dmgpalette) is byte-stable. An earlier variant
(`+8 - cgb_lead`, no stall fallback) scored +39 with 3 regressions.

**Not landed.** A net -35 floor is the kind of trade this project does land (the
C3 flip removed 327 and added 44), but landing it needs the full sequence:
whole battery, baseline rewrite (drop 40, add the residual), golden fingerprint
regeneration, mooneye, and `classify_*.py` confirmation that each newly
baselined row is SameBoy-FAIL — a row SameBoy passes may never be added. Do that
in one pass; the formula above is the starting point, and the equivalence check
is how to iterate on the residual cheaply.

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

Ruled out for this cluster, all measured: uniform coarse sample delay
(shuffle), line-start coarse latch, a fetch-phase-discriminated arm, a
dots-since-fetch-start threshold, deferred FIFO refill, and fetch restart on a
coarse change.

### The map-column latch: what one unified trace actually shows

Tracing fetch phases and FF43 writes **in the same run on `ds_4`**, on the
evaluated (16th) frame, settles the earlier inconsistency. The previous two
traces had been taken from different ROMs and different frames, which is what
made the numbers disagree.

Line 1 of `scx_0060c0/scx_during_m3_ds_4`, steady state:

```
* dot= 92 SCXWR 00->60
  dot=233 TileNoWait fx=18   dot=234 TileNo fx=18 (scx=60)
  dot=237 HiWait     fx=18
* dot=237 SCXWR 60->C0
  dot=238 Hi         fx=18  (scx already C0)
  dot=241 TileNoWait fx=19   dot=242 TileNo fx=19 (scx=C0)
```

So the fetch cadence is 8 dots with `Hi` at `read - 4`, exactly as inferred,
and the ladder's writes step 2 dots per index (ds_1 at 243 down to ds_8 at 229).

**Correction to the earlier entry in this file:** the claim that a
`latch = read - 4` rule makes all eight `_ds` rows pass was wrong. The rule only
changes rows whose write falls in the open window `(read-4, read)` — dots 239
and 241 for the `fx=19` read at 242, i.e. **`ds_3` and `ds_2` only**. Rows
whose write lands outside that window (ds_1 at 243, ds_4 at 237, ds_5 at 235,
ds_8 at 229) are unaffected by the rule and keep whatever the shipped live read
already gives them. The honest prediction is +2 with no regressions, not +8.

That also explains why implementing it as "latch at the previous `Hi`" measured
31/135 -> 9/135. `Hi == read - 4` holds only in steady state; around the
line-start fetches (`first_discard`, the push gating in `push_allowed`, the
12-dot startup walk) the previous `Hi` sits much further back than 4 dots, so
the arm silently retimed the *early* tiles too. The cell-symbol dump is the
check that catches this: on `ds_2` the shipped build already matches the
reference on cells 0-18 and differs only at cell 19, so any arm that perturbs
an early cell is wrong by construction.

### The `read - 4` rule is REFUTED as a dot offset (built 2026-07-28)

Built exactly as specified: a per-dot `eff.scx` ring, the tile-number read
taking the value from 4 dots earlier, and the line's first real fetch
(`fetch_x == 0`) exempted — that fetch's read coincides with the line-start SCX
write, measured at dot 92 on both counts, and the startup walk has no 4-dot
history.

The prediction was +2 (`ds_2`, `ds_3`) with nothing else moving. Measured: the
cluster goes 31/135 -> **8/135**. Exactly one row recovers (`ds_2`) and 24
regress. `ds_3` does not recover at all. The guard families are byte-stable, so
the arm is scoped correctly and this is a genuine refutation of the rule, not a
cross-family trade.

The regressions name the reason: `scx_0060c0` and `scx_0063c0` lose their
single-speed `_4/_5/_6` legs on **both models**, plus `_ds_4/5/8`, all three
`scx_during_m3_spx*` ROMs, and `scx_0761c0/_1 [Dmg]`. A 4-dot offset is two
M-cycles at double speed but a **single** M-cycle at single speed, so one
constant cannot mean the same thing on both — the single-speed legs shift by a
whole instruction's worth of write timing and fall out of the window they were
already inside.

So the window is not a fixed dot offset. Anything replacing it has to be
expressed in a unit that survives the speed switch (fetch-relative, or
M-cycle-relative with a speed term), and it has to explain why `ds_3` stays red
under a rule tuned to admit exactly its write dot. Both remain open.

### ROOT CAUSE: our BG map column formula is structurally wrong

Reading SameBoy 1.0.2 `Core/display.c` ends the guessing. In
`advance_fetcher_state_machine`, case `GB_FETCHER_GET_TILE_T1` (display.c:958-962):

```c
else if ((uint8_t)(gb->position_in_line + 16) < 8) {
    x = gb->io_registers[GB_IO_SCX] >> 3;          // line-start window
}
else {
    x = ((gb->io_registers[GB_IO_SCX] + gb->position_in_line + 8
          - (GB_is_cgb(gb) && !gb->during_object_fetch)) / 8) & 0x1F;
}
gb->last_tile_index_address = map + x + y / 8 * 32;
```

Ours (`render/mode0.rs`, `FetchPhase::TileNo`) is
`(scx / 8).wrapping_add(fetch_x) & 31`.

Three divergences, in order of importance:

1. **Sum then divide, not divide then count.** SameBoy adds SCX to
   `position_in_line` (the *pixel* output position, running from -16) and
   divides once, so SCX's low three bits carry into the tile index. We divide
   the coarse part out first and track tiles with an independent `fetch_x`
   counter, so a fine-scroll change can never move our column. For a stable SCX
   the two agree exactly — which is why the rest of the BG corpus passes — and
   they diverge precisely when SCX changes mid-line, i.e. this cluster.
2. **A CGB-only -1 term**, `8 - (is_cgb && !during_object_fetch)`: the CGB forms
   the address one pixel earlier than the DMG except while an object fetch is in
   flight. We have no such term, which is why the single-speed and
   double-speed legs of the same dir disagree under every uniform arm.
3. **The address is formed one T-cycle before the read.** SameBoy computes it in
   `GET_TILE_T1` and does the VRAM read in `GET_TILE_T2`; we compute and read in
   the same dot.

This explains all seven failed arms at once: every one of them tuned *when* SCX
is sampled, but the *formula* is wrong, so no sampling time can be right for
both a fine-scroll-0 dir (`scx_0060c0`) and a fine-scroll-3/7 one
(`scx_0360c0`, `scx_0761c0`). It also explains why the pass/fail ladder keyed on
the initial fine scroll from the very first measurement.

Note the SCX fine comparator itself already matches SameBoy: display.c:710
resolves the discard with `(position_in_line & 7) == (SCX & 7)` against a live
SCX, which is what `render.rs`'s `hunt_idx` does.

**Fixing this is a fetcher-structure change, not a timing tweak.** Replacing
`scx / 8 + fetch_x` with a position-derived column touches every BG fetch on
every line, so it re-derives the ~6000 green dot-level cases (mealybug photos,
the mode-3 fetch grid, the window machine) and must be gated on the full battery
plus `golden_fingerprint`, not on this cluster. `fetch_x` is also the window's
tile counter (`win_mode` uses it directly), so the window path has to keep its
own counter when the BG path stops using one.

### Porting the formula in isolation does NOT work (attempted 2026-07-28)

The formula was ported behind a gate, with a `pos_in_line` field added to
`Render` to stand in for SameBoy's `position_in_line`. Two variants were
measured against the cluster plus the scy / window / bgtiledata / bgtilemap /
dmgpalette guard families:

| variant | cluster | guards |
|---|---|---|
| shipped | 31/135 | unchanged |
| ported formula, position advanced on every pop | 21/135 | unchanged |
| + SameBoy's `-9 -> -16` hunt wrap (display.c:716) | **0/135** | unchanged |

The guards are byte-stable in both variants, which proves the formula is
*equivalent to ours for a stable SCX* — the port is arithmetically right. What
is wrong is the position semantics, and that is not a detail that can be bolted
on:

- our pipeline has no pixel-position counter. It carries `prefill_pos`,
  `hunt_idx`, `discard` and `fetch_x` as four separate pieces of state, and the
  discarded first tile's pixels are never actually popped (the comparator runs
  "as a bare counter", see `render.rs`), so there is nothing that corresponds
  to `position_in_line` running -16 -> 160;
- SameBoy hunts in a single counter that wraps `-9 -> -16` until the comparator
  matches, so its position is never in `[-8, 0)` while hunting. We hunt in *two*
  phases (a dot-rate prefill phase and a pop-rate phase after the FIFO starts
  draining). Feeding the wrap into the pop-rate phase pushes mid-line fetches
  into SameBoy's line-start branch (`position_in_line < -8` -> bare `SCX >> 3`),
  which is what takes the cluster to zero.

**So the column formula cannot be ported without first porting
`position_in_line` itself as the pipeline's primary position state**, replacing
the prefill/hunt/discard/fetch_x quartet. That is the fetcher-structure change,
and it is a multi-session refactor: `fetch_x` is also the window's tile counter,
the discarded-tile phase would have to start popping real pixels, and every one
of the ~6000 green dot-level cases re-derives.

### Why the refactor is a pipeline rewrite: the fetch/output coupling differs

The cheap way to test any port of the column formula is an **equivalence check
on a stable-SCX ROM**: with SCX constant the ported formula must reproduce
`(scx / 8) + fetch_x` exactly, fetch for fetch, because every non-`scx_during_m3`
BG row already passes. Instrument both columns at the tile-number read and
count mismatches — seconds per run, no battery needed. Use it before any
full-matrix measurement.

Running that check while porting `position_in_line` gives the blocker directly.
On `bgtilemap_spx08_ds_1` (SCX constant at 0), with the position maintained as
a real output counter:

```
COLDIFF ly=0 dot=84 fx=0 pos=0
COLDIFF ly=0 dot=90 fx=0 pos=0
COLDIFF ly=0 dot=96 fx=1 pos=2      <-- fetch 1 happens at output position 2
```

SameBoy's formula assumes a **fixed** fetcher-ahead-of-output distance: the tile
fetched at position `p` supplies the pixels at `p + 8`, which is exactly what the
`+8` in `(SCX + position_in_line + 8 - cgb) / 8` encodes. Our pipeline does not
hold that invariant — at `fetch_x == 1` the output position is 2, not 8, because
our fetcher runs ahead during the 12-dot startup while the FIFO fills and,
unlike SameBoy, we never pop the discarded first tile's pixels. A single
additive constant cannot reconcile the two: sweeping it moved the bulk error
from a uniform -1 tile (constant `+8`) to 23.6M mismatches (constant `0`).

So the column formula is not portable on top of our fetch/output relationship.
Landing it requires giving the pipeline SameBoy's lockstep first — the
discarded tile actually popping its 8 pixels, the position counter as the
primary state, and `prefill_pos`/`hunt_idx`/`discard`/`fetch_x` collapsing into
it — with the window keeping its own tile counter. That re-derives every green
dot-level case (mealybug photos, the mode-3 fetch grid, the window machine) and
is a scoped rewrite of mode 3, not an increment.

**Status: unfixed. Root cause identified, incremental porting exhausted.** Nine
arms measured and refuted (uniform delay, line-start latch, fetch-phase
threshold, dots-since-fetch-start, deferred FIFO refill, fetch restart,
`read - 4` dot rule, ported column formula, ported formula + position counter).
The first seven were variations on sampling time and were doomed by the formula;
the last two show neither the formula nor the position counter ports without the
pipeline underneath them.

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

Ruled out for this cluster, all measured: uniform coarse sample delay
(shuffle), line-start coarse latch, a fetch-phase-discriminated arm, a
dots-since-fetch-start threshold, deferred FIFO refill, and fetch restart on a
coarse change.

### DERIVED: the map column is latched 4 dots before our tile-number read

Instrumenting what the reference actually demands (decode each 8-pixel cell to
a column identity via its palette, then trace the FF43 write dot against every
tile-number read dot on line 1) resolves the whole `scx_0060c0` double-speed
ladder with one constant.

On line 1 the tile-number reads land on an 8-dot cadence — `fetch_x=18` at dot
234, `fetch_x=19` at dot 242 — and the `_ds_1.._8` ROMs step their write 2 dots
earlier per index (243, 241, 239, 237, 235, 233, 231, 229). Writing
`latch = read_dot - 4`, a write retargets a fetch only if it lands strictly
before that fetch's latch:

| ROM | write dot | fx18 (latch 230) | fx19 (latch 238) | ours == HW | verdict |
|---|---|---|---|---|---|
| ds_1 | 243 | no | no | yes | pass |
| ds_2 | 241 | no | no (we apply) | **no** | FAIL |
| ds_3 | 239 | no | no (we apply) | **no** | FAIL |
| ds_4 | 237 | no | yes | yes | pass |
| ds_5 | 235 | no | yes | yes | pass |
| ds_6 | 233 | no (we apply) | yes | **no** | FAIL |
| ds_7 | 231 | no (we apply) | yes | **no** | FAIL |
| ds_8 | 229 | yes | yes | yes | pass |

All eight agree. Our model applies a write whenever it precedes the *read* dot,
so it wrongly retargets exactly the writes landing in the 4-dot window
`[read-4, read)` — which is the mod-4 split, and why every uniform arm traded
one group for the other: shifting the sample point moves the window instead of
narrowing it.

Direct evidence for the pixel claim: on `_ds_2` the reference's last cell
(x152-159) is a pure column with the dark palette (`404040`/`000000`, symbol
`b`, continuing the `c,b,c,b…` alternation), while we emit the light palette
(`A0A0A0`/`F8F8F8`) — a third symbol that appears nowhere in the reference. The
trace shows why: our `fetch_x=19` read at dot 242 picked up the `$C0` written
at 241 and addressed column 11 instead of 31.

### Implementing the latch does NOT reproduce the law (attempted 2026-07-28)

The law was built as an explicit one-fetch-ahead latch: `Render::map_scx`
captured at the previous tile's `Hi` phase and consumed at the next
tile-number read, with a `map_scx_valid` fallback so the first fetch of a line
uses the live value. The phase layout was then *measured* rather than inferred
and confirms the intent exactly — on line 1 the fetcher runs
`… Hi@238, Push@239, Push@240, TileNoWait@241, TileNo@242 …`, so the latch sits
at `read - 4` and is consumed by the next fetch, which is what the derivation
asks for.

It still does not reproduce the predicted result. The cluster drops 31/135 ->
9/135 and the `scx_0060c0` double-speed ladder moves by one index in the wrong
direction:

| arm | ds_1 | ds_2 | ds_3 | ds_4 | ds_5 | ds_6 | ds_7 | ds_8 |
|---|---|---|---|---|---|---|---|---|
| shipped (live read) | pass | FAIL | FAIL | pass | pass | FAIL | FAIL | pass |
| latch at `Hi` | FAIL | pass | FAIL | FAIL | FAIL | pass | FAIL | FAIL |
| law predicts | pass | pass | pass | pass | pass | pass | pass | pass |

The guard families (scy, window, bgtiledata, bgtilemap) are byte-stable under
the latch, so the arm is correctly scoped — the error is inside the cluster's
own accounting, not a cross-family trade. `ds_4` is the sharpest contradiction:
its write lands at dot 237, one dot *before* the measured latch at 238, so the
latch should admit it and leave the row exactly as the shipped arm has it
(passing), yet it fails.

So one of the two measurements is lying and they have not been taken in the
same run: the write dots came from a per-ROM trace of the `_ds` ladder, while
the phase dots were captured from whichever ROM the sweep visited first (a
single-speed one). **The next step is to trace phases and FF43 writes together,
in one run, on `ds_4` specifically**, and check what `map_scx` actually holds at
dot 242 — rather than trusting a phase layout measured on a different ROM.
Until that is done neither the law nor its refutation is safe to build on.

Nothing was shipped: every probe was reverted and the battery is green.

For the record, the groups that want opposite answers, from the double-speed
ladder: `_ds_1/4/5/8` (positions 0,1 mod 4) want the live read; `_ds_2/3/6/7`
(positions 2,3 mod 4) want the latched one.
