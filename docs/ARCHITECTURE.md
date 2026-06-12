# slopgb architecture & contribution contract

slopgb is a cycle-accurate Game Boy (DMG) / Game Boy Color (CGB) emulator.

- `crates/slopgb-core` — the emulator. **Zero dependencies, `forbid(unsafe_code)`, deterministic.**
- `crates/slopgb` — desktop frontend (winit + softbuffer + cpal). Keeps deps minimal and pure-Rust.

## Ground rules (all work packages)

1. **TDD.** Write the failing unit test first, then the implementation. Every
   obscure hardware behavior you implement must have a unit test that fails
   without it.
2. **Emulate hardware, not test ROMs.** Never special-case a code path to make
   a mooneye ROM pass. Every behavior must be justified by documented hardware
   behavior (cite in a comment when obscure). The mooneye suite is the
   *oracle*, not the *spec*.
3. References, in order of authority:
   - *Game Boy: Complete Technical Reference* (Gekkio, "gbctr") — CPU timing,
     instruction micro-ops, MBC register maps.
   - Pan Docs (gbdev.io/pandocs) — everything else.
   - mooneye-test-suite ROM **source** (`test-roms-src/` if present, or the
     GitHub repo) — each test's asm states exactly what it checks; read it when
     a test fails.
   - SameBoy / mooneye-gb source — tie-breakers for undocumented corners.
4. No `unsafe`, no new dependencies in core, rustfmt defaults, clippy clean
   (`cargo clippy --all-targets -- -D warnings`).
5. Unit tests live in the same file (`#[cfg(test)] mod tests`) or in
   `crates/slopgb-core/tests/` for cross-module behavior.

## Timing model (the contract everything hangs on)

- 1 M-cycle = 4 T-cycles (dots). CGB double speed: CPU/timer/serial/DMA run
  2× — i.e. one CPU M-cycle = **2** dots of PPU/APU time.
- The CPU is clock master. `cpu::Bus::read`/`write`/`tick` each:
  1. advance every peripheral by one M-cycle (`Interconnect` internals:
     timer first, then DMA engine, PPU dots, APU, serial, joypad IRQ
     collection — IF bits OR-ed in as they are produced),
  2. then perform the memory access (if any).
  So a read observes peripheral state *after* the cycle's ticks; this is the
  same ordering mooneye-gb uses and the mooneye timing tests expect.
- `Bus::pending`/`pending_halt_wake`/`ack` are free (no time). The CPU samples
  `pending()` at the architecturally correct points (see CPU notes); the halt
  idle loop samples `pending_halt_wake()` instead, an earlier intra-cycle view
  that misses timer IF bits committed in the second half of the current
  M-cycle for one cycle (SameBoy `GB_cpu_run` halt path; gambatte tima/,
  wilbertpol timer_if).
- The PPU is stepped per dot; the timer per M-cycle on the CPU clock
  (4 internal T-ticks); the APU per M-cycle with the DIV counter passed in
  (DIV-APU = falling edge of DIV bit 4, bit 5 in double speed).
- **Mode-3 write strobe** (a refinement *inside* tick-then-access, not an
  exception to it): the CPU drives the data bus during the second half of a
  write M-cycle (gbctr "Memory access timing"), which the dot-clocked pixel
  pipeline can observe mid-cycle. `Bus::write` therefore *stages*
  rendering-register writes (FF40, FF42/FF43, FF47-FF4B) with the PPU
  before ticking (`Ppu::stage_write`); the staged value expires into a
  separate pipeline-view register copy (`Ppu::eff`) 2 dots (1 in double
  speed) before the architectural commit, with pre-CGB palette registers
  reading old|new on the transition dot (mealybug README: "BGP takes the
  value old OR new for one cycle"). Everything the tick-then-access
  contract calibrates — STAT/LYC/IRQ machinery, access blocking, LCDC.7,
  CPU reads — keeps using the architectural registers committed by
  `Ppu::write` after the tick, so nothing mooneye observes moves.
  Calibrated against the mealybug `m3_*` reference photographs and
  gambatte `dmgpalette_during_m3`/`scx_during_m3`/`scy`.
- **Window machine**: the WX comparator runs every mode-3 dot including
  the 8-dot prefill, edge-triggered, against the pause-aware position
  counter (sprite stalls freeze it, so a WX 0-7 match shifts later by
  the stall instead of being skipped), and a match sharing a dot with a
  sprite trigger starts the window first; the window line counter
  follows gambatte's winYPos (reset 0xFF per frame, incremented per
  activation); LCDC.5 mid-line disables abort the window at the
  pipeline-view commit with the BG resuming on a live-computed tile
  column (mattcurrie comprehensive-ppu-doc §WIN_EN; gambatte ppu.cpp
  setLcdc/Tile::f0); the WY condition is sampled at discrete dots
  (gambatte weMaster) with a live comparison against a delayed WY copy
  (wy2). WX writes commit to the pipeline one dot later than the
  palette class. DMG WX=166 matches carry a window-start request into
  the next line. Sprites with OAM X 0-7 are fetched during the
  pause-aware prefill walk that also drives the SCX comparator hunt.
- **BG/window fetch grid** (mealybug fetch cluster): every fetch VRAM
  access samples the pipeline-view registers (`eff`) at its read dot
  on both families (the gambatte bgtiledata cgb04c rows pin the clean
  commit on CGB-C; the CGB-C photo residue is documented in
  baselines/mealybug.txt). The BG fetcher
  free-runs through sprite-fetch stalls — the alignment penalty *is*
  the fetcher finishing its tile row in real time, prefill included —
  with the line's first push gated on the pause-aware startup walk
  (pixel 0 stays on its stall-shifted dot). The DMG blob pays the full
  6 dots for every OBJ fetch (no first-fetch discount; the mode-0
  flip leads sprite-extended pipe ends by 3 dots so every mooneye/
  gbmicrotest anchor keeps its dot), CGB-C discounts the line's first
  fetch to 5. LCDC.1 also gates sprite pixels at the mix, not just the
  fetch trigger. Calibrated against the mealybug m3_lcdc_*/m3_scy/
  m3_scx/m3_bgp/m3_obp0 reference photographs.
- One IF bit has sub-cycle dispatch semantics: the line-0 OAM STAT rise is
  readable through FF0F immediately but misses the CPU's interrupt sample
  for the M-cycle it was raised in (`Interconnect::if_stat_late`, the same
  shape as the timer's `if_late` halt-wake mask), and it is blocked
  entirely while the vblank source enable is set — gambatte
  `mstat_irq.h doM2Event` and the mealybug handlers' "line 0 timing is
  different by 4 cycles" compensation pin both rules.
- **STAT IRQs are per-source events with predicates**
  (`Ppu::stat_events_tick`, a function-by-function port of gambatte
  mstat_irq.h `MStatIrqEvent` + lyc_irq.cpp `LycIrq`; the truth table
  lives in its doc comment): there is no wired-OR STAT line on the IRQ
  side — each source fires at its own dot (m2 pulses at line-start dot
  0 on lines 1-144, dot 4 on line 0, DMG dot 12 on lines 145-153; m1 at
  144:4; LYC at (N,4) and (153,12); m0 at the visible-flip dot), gated
  by the *other* sources' enables sampled through delayed FF41/FF45
  copies (the `statRegChange`/`lycRegChange` guard windows, staged
  6/6/8 dots at single speed, 2 in double speed, with per-event fresh
  views). m0 is blocked only by a matching delayed LYC — never by the
  m2 enable; m1 by delayed m2en|m0en; per-line m2 pulses are routed
  away entirely while m0en is live (mode2IrqSchedule) and are
  lyc-blocked against the previous line's compare; LYC events are
  m2-blocked for values 1-144 and m1-blocked otherwise. The dot-0
  pulses stay second-half commits: IF reads back at once, but both the
  halt-exit sampler (`Ppu::take_stat_halt_late` → `Interconnect::
  if_late`) and the running CPU's same-cycle interrupt sample
  (`Ppu::take_stat_late` → `if_stat_late`) miss them for one M-cycle —
  the CGB 144:0 pulse is exempt. Register writes raise IF only through
  the ported trigger predicates: the DMG STAT-write glitch branch table
  (`stat_write_trigger_dmg`) plus a dots-0/4 line-start pulse
  re-decide, the CGB newly-enabled-bits table
  (`stat_write_trigger_cgb`) plus a dot-0 re-decide, and the FF45
  tables (`write_lyc_dmg`/`write_lyc_cgb`, gambatte
  lycRegChangeTriggersStatIrq). Double-speed/lcd-offset sub-cells stay
  documented-swap baselines. (See the CGB-C deltas section in
  `ppu/mod.rs` for the per-model timeline: readable-LYC holds, the
  delayed FF45 event copy, line-0 mode-1 tail, VRAM/OAM blocking
  shifts, the LY=153 windows, and the boot LCD phase.) The **mode-0 flip/IRQ
  anchor** (formerly parked) is re-derived jointly: the visible flip
  (STAT mode bits, OAM/VRAM unblock) and the mode-0 IRQ source rise
  together **2 dots before the pipe end** — 254+SCX%8 on a bare line,
  with the pipe-end anchors (HBlank-DMA trigger, palette blocking)
  unmoved at 256+SCX%8 — and the first OBJ fetch of a line costs 5 dots
  (not 3), which keeps every sprite-laden flip on its old dot while bare
  lines flip 2 dots earlier. The rise is fully visible to the running
  CPU's interrupt sample in its own M-cycle (no dispatch law), but a
  rise in the second half of the M-cycle is missed by the halt-exit
  sampler for one cycle (the timer-`if_late` shape). The LCD-enable
  glitch line starts its pipe at dot 82 (blocking still at 78), putting
  its flip/IRQ at 252+SCX%8. Pinned jointly by the gbmicrotest
  hblank_int/int_hblank(+_halt)/ppu_sprite0/win/sprite4 grids, mooneye
  intr_2_mode0_timing(+_sprites)/hblank_ly_scx_timing-GS/lcdon_timing-GS
  and the mealybug photos (whose dispatch anchors stay bit-identical at
  SCX=0); gambatte's xpos-166/167 event pair folds to the same single
  dot under its cc+2 access offset.
- OAM DMA is an interconnect engine: 160 M-cycles + startup delay, restart
  semantics (an FF46 rewrite retargets the in-flight run immediately),
  source-range quirks (CGB sources ≥ $E0 read $FF; DMG re-reads WRAM), and
  bus conflicts mirroring gambatte-core memory.cpp: per-source-class page
  masks decide which CPU accesses collide; conflicted reads return the
  in-flight byte, conflicted *writes* derail into the in-flight OAM slot
  (DMG WRAM sources wire-AND), and CGB redirects WRAM-region accesses to
  the WRAM page picked by FF46 bit 4 (gambatte `oamdma/` is the oracle).
  Each copied byte commits to OAM at its cycle's *end*
  (`oam_dma_commit_pending`), and while the controller owns OAM — running
  or halt-frozen — the PPU's dot-serial mode-2 scan is disconnected and
  latches $FF per entry (`Ppu::oam_dma_active`; gambatte switches its
  OamReader source to rdisabledRam — the `oamdma/late_sp*` families pin
  both window edges per sprite slot).
- CGB VRAM DMA (FF51-FF55) is a *request* engine mirroring gambatte-core:
  the dot-exact mode-0 entry (led by one dot, `Ppu::hdma_trigger_level`)
  or an FF55 write flags a request, which steals the bus at the head of
  the CPU's next bus operation — an in-flight write commits first, a read
  in the trigger cycle yields — copying 2 bytes per stolen M-cycle (1 in
  double speed) plus one teardown M-cycle. FF55 is the live register; the
  full 16-bit destination counter terminates at the 0x10000 crossing.
  Blocks never run while the core clock is gated: HALT/STOP defer a
  pending block and the wake re-fires it (gambatte `haltHdmaState_`); the
  STOP speed switch aborts it entering double speed and defers it
  leaving. The STOP tail itself (skipped-byte read cycle, DIV-reset cell,
  the ~0x8000-M-cycle pause on the new clock) lives in `Bus::stop`
  (gambatte `Memory::stop`; `dma/` + `speedchange/` are the oracle).

## Memory map routing (interconnect)

| Range | Target |
|---|---|
| 0000-7FFF | `Cartridge::read_rom/write_rom` |
| 8000-9FFF | `Ppu` (VRAM, current VBK bank on CGB) |
| A000-BFFF | `Cartridge::read_ram/write_ram` |
| C000-DFFF | WRAM (CGB: D000 banked via SVBK, banks 1-7) |
| E000-FDFF | echo of C000-DDFF |
| FE00-FE9F | `Ppu` OAM (mode + DMA blocking) |
| FEA0-FEFF | prohibited area (DMG: 00/FF; CGB-C: 24 B extra OAM RAM, 4× mirrored; AGB: nibble echo — Pan Docs) |

Any CPU access with a $FE00-$FEFF value on the address bus during the
mode-2 OAM scan triggers the DMG-family OAM corruption bug (Pan Docs "OAM
Corruption Bug"): `Interconnect` gates on model/halt/DMA and routes to
`Ppu::oam_bug`; the 16-bit inc/dec-unit CPU cycles reach it through
`Bus::tick_addr`/`Bus::read_inc` (blargg `oam_bug/` is the oracle).
| FF00 | `Joypad` |
| FF01-FF02 | `Serial` |
| FF04-FF07 | `Timer` |
| FF0F | IF (upper 3 bits read 1) |
| FF10-FF3F | `Apu` |
| FF40-FF4B | `Ppu` regs (FF46 DMA register lives in interconnect) |
| FF4D KEY1, FF4F VBK, FF50 boot-off, FF51-55 HDMA, FF56 RP, FF68-6B palettes, FF6C OPRI, FF70 SVBK, FF72-77 | CGB regs (interconnect, palette regs routed to PPU) |
| FF80-FFFE | HRAM |
| FFFF | IE (all 8 bits writable/readable) |

## Models

`Model = {Dmg0, Dmg, Mgb, Sgb, Sgb2, Cgb, Agb}`. No boot ROM is executed;
`Registers::post_boot(model)` + `Interconnect::apply_post_boot_state()` set
the exact PC=0x100 state including the internal 16-bit DIV counter (this is
what `boot_div*` ROMs measure). Values come from gbctr/mooneye-gb and are
verified by `boot_regs-*`/`boot_hwio-*`/`boot_div*` ROMs. On CGB/AGB the
hand-off moment depends on the cart type: the boot ROM's DMG-compat tail
runs 0x7D8 T-cycles longer than the CGB-cart path, so `apply_post_boot_state`
shifts DIV and the LCD phase together for CGB-flagged carts (mooneye ROMs —
DMG carts — pin one side, gambatte's `$143=$C0` ROMs the other; see the
model.rs table docs).

## CGB revision policy (Model::Cgb)

`Model::Cgb` models **one** CGB revision: **CPU CGB C** (the CGB-CPU-04 SoC).
There is no revision parameter; revision-incompatible ROMs/references are
model-skips, exactly like `-dmg0` ROMs on `Model::Dmg` today.

Why C: the reference corpus pins it. gambatte's 3,352 `cgb04c`-tagged
expectations were captured on CGB-CPU-04; mealybug-tearoom's `_cgb_c`
screenshots are the only complete CGB reference set (no `_cgb_e` refs exist
anywhere); age-test-roms' `-cgbBC` variants and blargg `cgb_sound` (real
CGB-C passes) align. age proves real single-speed LY/STAT divergence between
B/C and E silicon — pinning E (SameBoy's default) would put gambatte's
~1,000+ dot-timing ROMs at legitimate-fail risk with no way to tell a real
bug from revision skew.

**Companion rule (load-bearing):** do **not** implement C-only quirks whose
behavior upstream documents as not-understood — canonically the CGB≤C
PCM12/PCM34 same-M-cycle read glitch (same-suite apu/README "To Do"). With
clean PCM reads, same-suite's E-verified channel tests pass on this model;
implementing the glitch would break them and therefore requires the revision
split first (trigger T1 below).

### DMG revision

`Model::Dmg` likewise pins **one** DMG revision for reference selection:
late-DMG silicon — the "blob" (DMG-C-ish) capture series. This is consistent
across the corpus: age routes its `-dmgC` variants to `Model::Dmg`,
gambatte's `dmg08` expectations come from a DMG-CPU-08 (late-revision)
board, and mooneye's `-dmgABC` ROMs pass on this model. mealybug-tearoom is
the one suite that also ships early-revision screenshots: its two
`_dmg_b.png` references differ from the `_dmg_blob` series and stay
**parked** — the policy picks blob for corpus consistency, exactly like the
parked `_cgb_d.png` series. A future `Model::DmgB` split would follow the
same upgrade shape as the CGB one below.

### Reference selection per suite

| Suite | On `Model::Cgb` run / compare against | Revision-skips (empty model list, loud note) |
|---|---|---|
| mooneye | `-C`/`-cgb`/`-cgbABCDE` (C ∈ every set — matrix unchanged) | `-cgb0` (pre-existing) |
| gambatte | ROMs with a `cgb04c` name segment; that tag's `_out<hex>`/`_outaudio`/PNG expectation | none (suite is CGB-C); `dmg08`-only → Dmg; `*_dumper.gbc` manual |
| same-suite | unsuffixed (E-verified, pass via the no-PCM-glitch rule) + `-cgb0BC` | `-cgb0`, `-cgb0B`, `-cgbB`, `-cgbDE`; `-A` → Agb. extra_length_clocking has **no** C-compatible variant: known hole |
| mealybug ppu | the 27 ROMs with `*_cgb_c.png` | `_cgb_d.png` parked (future CgbE/D); DMG-ref-only ROMs (`m3_wx_4/5/6_change`, `…multiple_wx`) → Dmg only; `win_without_bg` has no ref: never run |
| mealybug dma | `hdma_during_halt-C`, `hdma_timing-C` | none |
| age | `-cgbBC(E)`, `-dmgC-cgbBC(E)` CGB leg, `-ncmBC(E)`, unsuffixed `m3-bg-*`, `-ds` | `-cgbE` ×6, `-ncmE` ×3 (each has a running `-cgbBC` sibling); `-nocgb`/`-dmgC` → Dmg |
| blargg | `cgb_sound` (real C passes; only B fails case 3) | none |
| cgb-acid2 / acid-hell | single upstream reference (revision-agnostic) | none |

Failures that triage to genuine C-vs-E silicon divergence go on a
*documented expected-fail list* (asserted, never silently skipped). The
one-time first candidate, same-suite `channel_1_sweep_restart_2` (passes
only on real CGB-E; even SameBoy-E fails it), in fact passes here via the
SameBoy sweep-calculation machinery under this core's tick-then-access
write conventions — the list is currently empty.

### Escalation triggers (when to parameterize the revision)

- **T1:** we implement any C-only quirk (PCM12/34 read glitch foremost) that
  breaks an E-targeted expectation we currently pass.
- **T2:** baseline triage attributes **>10** rom×reference failures to
  genuine C-vs-E divergence that suffix/reference routing cannot absorb.

Upgrade shape: keep `Model::Cgb` ≡ CGB-C (all existing baselines, vendored
references and the mooneye matrix stay valid), add `Model::CgbE` behind the
facade, put per-revision deltas in small `match`es at the divergence sites —
no speculative per-revision behavior tables for unbaselined behaviors.

## Mooneye test protocol (harness)

A test ends by executing `LD B,B` (opcode 0x40, exposed as
`GameBoy::debug_breakpoint_hit()`).
Pass ⇔ registers are the Fibonacci sequence B=3, C=5, D=8, E=13, H=21, L=34.
Anything else (or 120 emulated seconds without the breakpoint) is a failure.
The harness (`crates/slopgb-core/tests/mooneye.rs`) maps every ROM under
`test-roms/` to the model(s) it applies to via its filename suffix:
`-dmg0`, `-dmgABC(mgb)`, `-mgb`, `-S`(=SGB+SGB2), `-sgb`, `-sgb2`, `-GS`(=DMG+SGB families),
`-C`/`-cgb*`(=CGB), `-A`(=AGB), no suffix = every supported model.
`manual-only/sprite_priority` is verified by frame compare against a
reference image instead.

## Work package file ownership (parallel development)

| Package | Files (exclusive) |
|---|---|
| CPU | `src/cpu/execute.rs`, `src/cpu/registers.rs`, `src/cpu/mod.rs` |
| Timer/serial/joypad | `src/timer.rs`, `src/serial.rs`, `src/joypad.rs` |
| Cartridge | `src/cartridge.rs` (may become `src/cartridge/`) |
| PPU | `src/ppu.rs` (may become `src/ppu/`) |
| APU | `src/apu.rs` (may become `src/apu/`) |
| Interconnect | `src/interconnect.rs`, `src/model.rs`, `src/lib.rs`, `tests/` |

Public signatures in the skeleton are the inter-package API. If you must
change one, it's a coordination point — keep the change minimal and adjust
callers in your own package only when the file table above gives you the file.
