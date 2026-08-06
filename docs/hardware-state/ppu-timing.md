# PPU — STAT / LY / mode-3 / mode-0 timing

## Mode-3 register write strobe

- `Bus::write` stages rendering-register writes (FF40, FF42/43, FF47-4B) with the PPU **before** ticking.
- The pipeline's register view (`Ppu::eff`) sees them **2 dots** before the architectural commit (**1 dot in ds**).
- DMG palettes read `old|new` on the transition dot (ARCHITECTURE.md §Timing).

| Concern | Routed through `eff`? | Reads from |
|---|---|---|
| Rendering registers (FF40, FF42/43, FF47-4B) | Yes — staged before tick | `Ppu::eff` (2 dots / 1 ds early) |
| STAT / LYC / IRQ / blocking | **No — do NOT route through `eff`** | the architectural registers |

## SCX fine scroll & SCY sampling

- SCX fine scroll is a **live position-comparator hunt** (`Render::hunt_idx`, SameBoy `render_pixel_if_possible` semantics incl. the −9→−16 wrap).
- SCY is **re-sampled at each fetcher VRAM access** (`bg_tile_addr`).
- The discard is **not** latched at mode-3 start — mid-hunt SCX writes change mode-3 length.

## STAT IRQs — per-source events with predicates

STAT IRQs are **per-source events with predicates** (`Ppu::stat_update_tick` in `ppu/stat_irq/reclock.rs`, a function-by-function port of gambatte `mstat_irq.h` `MStatIrqEvent` + `lyc_irq.cpp` `LycIrq` — the truth table is the doc comment there).

- There is **NO wired-OR STAT line on the IRQ side.**

### Event dots (unchanged)

| Source | Dot(s) |
|---|---|
| m2 pulses | line-start dot 0, lines 1-144; line-0 dot 4; DMG dot 12 lines 145-153 |
| m1 | 144:4 |
| LYC | (N,4) / (153,12) |
| m0 | the flip dot |

### Source gating via delayed FF41/FF45 copies

- Each event is gated by the **OTHER** sources' enables through delayed FF41/FF45 copies.
- Staging delays: `stat_ev` / `stat_lyc_ev` staged **6 dots**, `lyc_ev_m` **8 dots**, **ds 2 dots**.
- The m0 event and the CGB pulse write-reach read fresher views — `stat_ev_fresh` / `lyc_ev_m_fresh`.

### Key predicates

- m0 blocked **ONLY** by a matching delayed LYC (never by m2en).
- m1 blocked by delayed `m2en|m0en`.
- Per-line m2 pulses don't exist while m0en is live, and are lyc-blocked against line−1.
- LYC events blocked by m2en for values 1-144, by m1en otherwise.

### Emission masks (unchanged)

- dot-0 pulses second-half commits.
- CGB 144:0 exempt.
- line-0 dispatch-late.
- m0 half-cycle halt law.

### The FF41 read frame — the polled bare-line mode-0 edge

A polled CGB read on a bare (no sprite, no window, no glitch) line reads mode 0
from **five dots before** the render's flip (`254 + SCX&7`), i.e. the bare arm's
half-dot exit is `2*flip - 2`, not `2*flip + 2` (`read_laws_exit.rs`, `over`).
The arm covers every visible line at both speeds, LCD on.

Measured, not swept — SameBoy's own trace of the same ROMs
(`SB_TRACE=1 sameboy_tester`, `SBMODE` visible mode-0 edge vs the `SBREAD ff41`
instant, differenced on the absolute 8 MHz `fp` clock, since `cfl`/`dc` reset
per line):

| | SameBoy, dots from the line's mode-3 entry |
|---|---|
| visible mode-0 edge | `173 + SCX&7` |
| `*dma_cycles_*_1` read (wants mode 3) | 172 |
| `*dma_cycles_*_2` read (wants mode 0) | 176 |

so SameBoy's verdict is `read >= edge`, and the slopgb read (cc+0, +8 hd debt)
lands on that edge exactly at `flip - 5`. The old `+2` put the boundary two dots
late, which only shows where a ROM reads inside those two dots: the
`{g,h}dma_cycles_*_scx{2,3}` pairs read dot 248 (`_1`, want 3) and dot 252
(`_2`, want 0) with the edge at 251/252.
Scores **+10/−0** (6 `dma/*_cycles_*`, 2 `display_startstate/stat_scx{2,3}_2`, 2
wilbertpol `hblank_ly_scx_timing_variant_nops` [Cgb]/[Agb]); golden drift is 13
cases confined to the CGB STAT-read cluster, no verdict changes. Unit test
`polled_bare_cgb_mode0_edge_is_five_dots_before_the_flip`.

Two scopes are load-bearing: **DMG** polled reads keep `2*flip`
(`gbmicrotest/ppu_sprite0_scx{2,6}_b` read exactly there and want mode 0), and a
**post-STOP shifted frame** (`lcd_shift_dots != 0`) keeps `+2` — its flip sits a
half-dot past the whole-dot sample and the shifted-frame arms in `read_laws.rs`
already compensate; dropping that scope costs
`lcd_offset/offset{1,2}_lyc99int_m0stat_count_scx{1,2}_1` (+10/−2 instead of
+10/−0). Carried (ISR) reads are untouched — `isr_read_carry_hd` owns them.

One caveat on the derivation. The `{g,h}dma_cycles_*` reads sit **9** dots
before SameBoy's equivalent instant, not the +4 read debt every other family
shows (the `lcd_offset` polls measure exactly +4): our GDMA of 128 blocks
retires in 4100 dots (1024 stolen M-cycles + the gambatte teardown M-cycle)
against SameBoy's 4104 between its `SBWHDMA run` and `end` markers. That
M-cycle is a gambatte-vs-SameBoy model disagreement, not a row this port drops
— every `*_cycles_*` row passes with the gambatte length — but it means the
constant above rests on the non-DMA members of the +10 (display_startstate,
wilbertpol), and a future DMA-length change must re-check the six DMA rows.

### The shifted (post-STOP) frame's mode-0 edge — MEASURED FLOOR

`lcd_offset/offset{1,2,3}_lyc99int_m0stat_count_*` poll FF41 once per line and
must read mode 3 (`_1`) / mode 0 (`_2`) on every visible line to VBlank, so each
(offset, SCX) pair brackets the exit to one M-cycle. In the shifted frame those
brackets **contradict each other** at whole-dot resolution — with the exit
written `2*flip + K` (K folding `over` and `lcd_phase_hd`):

| row | want | read dot | flip | needs |
|---|---|---|---|---|
| `offset1_…_scx3_1` | 3 | 259 | 257 | `K > 12` |
| `offset2_…_scx2_1` | 3 | 258 | 256 | `K > 12` |
| `offset3_…_scx0_2` | 0 | 255 | 254 | `K <= 10` |

Swept end to end: `over` 4 → +0/−1, 6 → **+2/−1**, 8 → +2/−3. Every setting is a
trade, so the family stays baselined (`over = 2`). Separating it needs the
sub-dot poll phase the whole-dot flip cannot carry — the same precondition as
floor class A.

### Line 153's LY hold in a shifted frame — MEASURED FLOOR

`lcd_offset/offset{1,2}_lyc98int_ly_count_1` fail one step earlier, on LY, not
STAT: their kernel reads FF44 at line-153 dot 6/7 and requires `$99`. slopgb
holds LY = 153 from 152:454 through 153:3 (`engine.rs`), so those reads get 0;
SameBoy still returns `$99` 8 dots into line 153 (`SBREAD ff44 … val=99`, 16 hd
after the line-153 boundary). Widening the hold is a two-sided trade, swept:
wrap 8 → **+6/−18**, wrap 12 → +1/−19. The casualties include the
hardware-captured `age/ly-dmgC-cgbBC` + `ly-ncmBC` and the whole DMG
`wilbertpol ly_lyc_0-GS` / `ly_new_frame-GS` matrix, which the cross-oracle rule
forbids dropping. Unchanged; lift needs the sub-M-cycle vblank-LY-load skew
model the wilbertpol baseline header already names.

### FF41 reads — the line-144 VBlank-entry hold

The line-144 dots-0..3 mode-0 hold in `vis_mode` is raw FSM state no read ever
observes: reads sample cc+4, which is already VBlank. The read law back-dates a
cc+0 read of the hold to mode 1 (CGB), and that arm is **speed-independent**
(2026-08-02) — the hold is 4 dots at single speed and 2 in double (the DS
mode-bits lag), while the read debt is the matching +4 / +2, so every cc+0 read
inside the hold lands past the boundary either way. The former `dot + debt >= 4`
guard left double-speed dots 0..1 on the raw mode 0:
`enable_display/frame{0,1}_m1stat{,_ds}_2` all read line 144 dot 0 and all want
the VBlank bit, their `_1` siblings read the previous line's dot 452/454 and
want mode 0 (excluded by `line == 144`). Scores **+2/−0**; unit test
`cgb_line144_hold_reads_vblank_at_both_speeds`, which also asserts the raw
`vis_mode` stays 0 so the back-date remains read-scoped.

**Refuted, measured — do not retry:** giving the *line-0* OAM-entry back-date the
same treatment. Its `dot + debt >= 4` guard looks identically vacuous, and the
DMG analogue carries no debt term at all, but dropping it scores **0/−1**:
`ly0/lycint152_ly0stat_ds_2` (want `C1`) reads a double-speed line-0 dot 0/1 and
pins it to the raw VBlank mode **1**, so that hold is genuinely still 4 dots
wide on the read grid at both speeds. The line-144 and line-0 arms are not
mirror images. The visible-line arm (lines 1..143) was left alone for the same
reason — its `_1`-rung siblings poll dot 0 of every line and currently pass.

The `enable_display/frame{0,1}_m2stat_count_ds_2` rows are the **third** arm, and
they are blocked. Disassembled: the kernel enables the LCD, delays, then spins on
`ldh a,(41); cp $86; jp nz` and on the first mismatch prints **LY** — so the
reference holds mode 2 + coincidence until VBlank (`$90`) and ours bails at the
first poll. `frame1` polls (ly 0, dot 0) and needs mode 2 from the LINE-0 arm;
`frame0` polls (ly 1, dot 0) and needs it from the VISIBLE-LINE arm. Both are
refuted:

| arm | drop the `dot + debt >= 4` guard | why |
|---|---|---|
| line 0 | **0/−1** | `ly0/lycint152_ly0stat_ds_2` (want `C1`) reads the IDENTICAL state — ly 0, dot 0, DS, `line_render_done` false, `read_carried` false, not the glitch line — and demands mode **1** where the count row demands **2**. Co-temporal at whole-dot resolution; `line_render_done` (the DMG arm's discriminator) and `read_carried` both fail to separate them. |
| lines 1..143 | **+5/−6** | trades 5 `halt/m0*_m0stat_*_ds_2` + `m0int_m0stat_scx5_ds_2` for 6 `lyc*`/`m0int_m0stat` `_ds_1` rows — **and does not recover `frame0` either**. |

So the two surviving guards are load-bearing and two-sided; only the line-144 arm
was vacuous. Separating these needs sub-dot read positions, not another arm.

### FF41 writes — DMG vs CGB

| Model | FF41-write behavior |
|---|---|
| DMG | the STAT-write glitch branch table (`stat_write_trigger_dmg`: hblank/vblank levels + held compare, old-enable suppression) **+** a dots-0/4 pulse re-decide (`m2_pulse_fires` retro; gbmicrotest `oam_int_if_level_d` is AGS-verified and reads the opposite way from the DMG-verified gambatte cell; it is no longer baselined — both sides pass) |
| CGB | `stat_write_trigger_cgb` (newly-enabled bits only: m0 enables fire in hblank but defer to a pending in-line m0 event; m1 in vblank except mode-1's last M-cycle; m2 only in the last M-cycle before a pulse; lyc anywhere the held compare matches) **+** a dot-0 retro |

### FF45 writes — DMG vs CGB

Both port `lycRegChangeTriggersStatIrq` (held-compare target tables, m0/m1 blocking, the simultaneous-inc exception).

| Model | FF45-write entry point | Extra |
|---|---|---|
| DMG | `write_lyc_dmg` | — |
| CGB | `write_lyc_cgb` | keeps the **+1 M-cycle IF** |

### Documented swaps & wired-OR survival

- 10 ds/lcdoffset `_1` rounds are **documented swaps** (see the 2026-06 block in `baselines/gambatte.txt`).
- The wired-OR level survives **only** for LCD-off writes (`legacy_level_edge`, `stat_lyc_onoff`).

## Post-boot LCD phase

| Model | Phase length | Pinned by |
|---|---|---|
| DMG / MGB / SGB | exactly **70164 dots** (60 before line-0 start) | gbmicrotest `poweron_stat`/`ly`/`oam`/`vram` tables, inside mooneye's `boot_hwio` window |
| CGB | 144·456+164 | gambatte `initstate` videoCycles (its DMG value equals 70164 exactly, anchoring the unit conversion); the `display_startstate` cgb04c rows pin it |
| AGB | 144·456+164 **+4** | same as CGB |

## CGB-C LY/STAT line timeline

(`ppu/mod.rs` §CGB-C deltas; 2026-06)

- Readable LYC flag holds the **previous line's** compare through dots 0-3 (no invalid gaps; line 153 holds 153 through dot 11).
- The IRQ side (`cmp_irq` vs the delayed `lyc_event` FF45 copy) keeps DMG windows, event-clocked.
- FF45 writes follow gambatte `lycRegChange` (4-dot event protection, boundary writes compare against the upcoming line, +1 M-cycle IF at single speed).
- line-0 dots 0-3 read **mode 1** with the vblank level extended.
- VRAM read block starts **dot 83**.
- CGB OAM writes blocked at line-start dots 0-3 of lines whose predecessor was visible; the DMG dots-80-83 writable gap does not exist.
- LY=153 loads **2 dots early at SS** / wraps at **dot 6 in ds**.
- FF41 m2-enable writes fire **only in the last M-cycle before a visible line**.

### Rows flipped

- 16 wilbertpol -C rows + age `ly`/`ly-ncm` + same-suite `hdma_mode0` + 74 gambatte rows.
- 16 gambatte sub-cycle/lcd-offset/ds rows are **documented swaps** (the "CGB-C LY/STAT timeline" block in `baselines/gambatte.txt`).

### Parked

- **Parked:** wilbertpol `ly_lyc_0-C` / `ly_new_frame-C` — cross-suite LY=153-window contradiction with age (see the wilbertpol baseline note).
- **Parked:** `hblank_ly_scx_timing-C` — needs the CGB mode-0 flip +1 dot in `render/mode0.rs`.

## Mode-0 end-of-line event grid

(The formerly **PARKED** flip/IF split, re-derived jointly.)

- The visible flip **AND** the mode-0 IRQ source rise together, via a stall/refill projection over committed renderer state that can **un-flip** when a late write arms a new stall (`m0_flip_events` / `m0_unflip` in `render/mode0.rs`).
- `pipe end = 256 + SCX%8 + penalties` stays the HDMA/palette anchor.

### Flip / m0-IRQ-rise offset from pipe end

| Line type | Offset |
|---|---|
| default / bare lines | pipe end −2 (bare lines flip 2 early) |
| double speed, window-stalled lines | pipe end −1 |
| DMG window-aborted lines | pipe end −0 |
| sprite-laden DMG lines | pipe end −3 |

- Sprite-line flips stay on their **mooneye-frozen dots** while the pop grid sits one dot later (mealybug `bgp_change_sprites`/`obp0_change` pin the pixels).

### First OBJ fetch cost

| Target | Cost |
|---|---|
| DMG blob | 6 dots |
| CGB-C | 5 dots |

### LCD-enable glitch line

- Starts its pipe at **dot 82** (VRAM/OAM blocking still 78): flip/IRQ at **252+SCX%8**.
- **CGB palette RAM locks at 81, not 78** (2026-08-02) — the anchor plus the same
  `PAL_M3START_OPEN` grace the normal dot-84 anchor gets on shifted frames
  (`Ppu::pal_ram_blocked`). `enable_display/ly0_late_cgbp{r,w}{,_ds}_1` access at
  dot 80 and want the palettes OPEN (read `$55` / write landed); their `_2`
  siblings access at 84 single speed and 82 double speed and want them locked.
  That brackets the lock to **81..=82** and only 81 is a derived value; a ROM
  accessing at exactly 81 would split it. Scores **+4/−0**; unit test
  `cgb_glitch_line_palette_lock_trails_the_anchor`. The VRAM and
  OAM predicates keep 78 — `ly0_late_vramr_{1,2,3}` and `ly0_late_oamw_{1,2}`
  pass on both speeds with the unchanged anchor, so the grace is palette-only.
- **VRAM locks carry the CGB grace too, and the DS grid drops one dot of it**
  (2026-08-02) — the glitch-line read and write locks were both the bare dot-78
  anchor. `enable_display/ly0_late_vram{r,w}_{1,2,3}` put a CGB read *and* write
  at dots 76/80 (both land) and 84 (both dropped), so the CGB lock is
  `78 + 3` like the normal line's read grace; the DMG legs block at 80 already
  and keep the bare anchor. `ly0_late_vramr_ds_{1,2}` open at 78 and block at
  80, one dot below the SS lock — the same relation the non-glitch DS lock (82)
  has to its SS twin (83). No ROM separates read from write on this line, so
  both take the identical anchor + grace (unlike the normal line, where the
  write lock trails at 84). Scores **+2/−0**; unit test
  `cgb_glitch_line_vram_locks_carry_the_grace` pins each arm. OAM is unaffected
  (`ly0_late_oamw_{1,2}{,_ds}` pass at the bare anchor on both speeds).

### IF rise timing

- The rise is dispatch-visible in its own M-cycle (no law) but **halt-late** when committed in the second half (`take_m0_rise` → `if_late`).

### Pinned by

- gbmicrotest `hblank_int`/`int_hblank`(+`halt`)/`ppu_sprite0`/`win*`/`sprite4` grids (+63 rows).
- mooneye `intr_2_mode0_timing`(+sprites)/`hblank_ly_scx-GS`/`lcdon-GS`.
- gambatte `window`/`m0enable`/`bgtile*` (+117 rows net of documented residuals).
- mealybug photos untouched (+2 legs).

### Parked / documented swaps

- Residual ±1-dot conflicts (gambatte `m2int_m0irq_scx2`/`5`-chained reads vs the gbmicrotest `_if` grid, DS sub-phase rows, wilbertpol 2016 `_nops` chains) are **documented swaps** in the baselines — **do not chase** them without sub-dot IF-flop modeling.
- **Parked:** the wilbertpol `_nops` chains in particular flip canonical `intr_2_mode0_timing` when chased.
