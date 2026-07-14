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

STAT IRQs are **per-source events with predicates** (`Ppu::stat_events_tick`, a function-by-function port of gambatte `mstat_irq.h` `MStatIrqEvent` + `lyc_irq.cpp` `LycIrq` — the truth table is the doc comment there).

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

### FF41 writes — DMG vs CGB

| Model | FF41-write behavior |
|---|---|
| DMG | the STAT-write glitch branch table (`stat_write_trigger_dmg`: hblank/vblank levels + held compare, old-enable suppression) **+** a dots-0/4 pulse re-decide (`m2_pulse_fires` retro; gbmicrotest `oam_int_if_level_d` is AGS-verified and contradicts the DMG-verified gambatte cell — baselined) |
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
- CGB OAM writes blocked at line-start dots 0-3 and through 80-83.
- LY=153 loads **2 dots early at SS** / wraps at **dot 6 in ds**.
- FF41 m2-enable writes fire **only in the last M-cycle before a visible line**.

### Rows flipped

- 16 wilbertpol -C rows + age `ly`/`ly-ncm` + same-suite `hdma_mode0` + 74 gambatte rows.
- 36 gambatte sub-cycle/lcd-offset/ds rows are **documented swaps** (see the 2026-06 block in `baselines/gambatte.txt`).

### Parked

- **Parked:** wilbertpol `ly_lyc_0-C` / `ly_new_frame-C` — cross-suite LY=153-window contradiction with age (see the wilbertpol baseline note).
- **Parked:** `hblank_ly_scx_timing-C` — needs the CGB mode-0 flip +1 dot in `render.rs`.

## Mode-0 end-of-line event grid

(The formerly **PARKED** flip/IF split, re-derived jointly.)

- The visible flip **AND** the mode-0 IRQ source rise together, via a stall/refill projection over committed renderer state that can **un-flip** when a late write arms a new stall (`m0_flip_events` / `m0_unflip` in `render.rs`).
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

- Starts its pipe at **dot 82** (blocking still 78): flip/IRQ at **252+SCX%8**.

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
