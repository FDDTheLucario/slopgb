# slopgb architecture & contribution contract

slopgb is a cycle-accurate Game Boy (DMG) / Game Boy Color (CGB) emulator.

- `crates/slopgb-core` — the emulator. **No external (crates.io) dependencies — std
  only, plus one in-tree path dep (`slopgb-snes-apu`, re-exported for the shared
  save-state `Reader`/`Writer`/`StateError`). `forbid(unsafe_code)`, deterministic.**
  It emulates no SNES chip: the SGB border, palettes and command-packet handling
  are core-side HLE (`ppu/sgb.rs` + `ppu/sgb/`, `src/sgb/`; the packet *receiver*
  is in `joypad.rs`), but every SNES-side *chip* arrives from outside as a wasm
  coprocessor plugin.
- `crates/slopgb` — desktop frontend (winit + softbuffer + cpal + gilrs). Keeps deps
  minimal and pure-Rust.
- The rest of the workspace `members` (root `Cargo.toml`) is the plugin system and
  its support crates: `slopgb-plugin-api` (guest SDK), `slopgb-plugin-host` (the
  `wasmi` runtime — the only crate that depends on it, so core and the frontend
  stay off it), `slopgb-sgb-coprocessor`, the clean-room chip cores
  `slopgb-snes-apu` / `slopgb-w65c816` / `slopgb-snes-ppu` with their
  `slopgb-{spc700,w65c816,snes-ppu}-plugin` wasm wrappers, `slopgb-sf2` +
  `slopgb-sf2-plugin`, `slopgb-msu1-plugin`, `slopfp`, and `xtask`. Nothing below
  applies to them: this file is the **core** contract.

## Ground rules (all work packages)

1. **TDD.** Write the failing unit test first, then the implementation. Every
   obscure hardware behavior you implement must have a unit test that fails
   without it.
2. **Emulate hardware, not test ROMs.** Never special-case a code path to make
   a test ROM pass — mooneye or any game-boy-test-roms suite. Every behavior
   must be justified by documented hardware behavior (cite in a comment when
   obscure). The test-ROM corpus is the *oracle*, not the *spec*.
3. References, in order of authority:
   - *Game Boy: Complete Technical Reference* (Gekkio, "gbctr") — CPU timing,
     instruction micro-ops, MBC register maps.
   - Pan Docs (gbdev.io/pandocs) — everything else.
   - mooneye-test-suite ROM **source** (`test-roms-src/` if present, or the
     GitHub repo) — each test's asm states exactly what it checks; read it when
     a test fails.
   - SameBoy / mooneye-gb source — tie-breakers for undocumented corners.
4. No `unsafe`, no new external dependencies in core, rustfmt defaults, clippy clean
   (`cargo clippy --all-targets -- -D warnings`).
5. Unit tests live in a `#[cfg(test)] #[path = "X_tests.rs"] mod tests;` sibling of
   the module they cover, or in `crates/slopgb-core/tests/` for cross-module
   behavior. No `.rs` over 1000 lines (`tests/source_size.rs` enforces it), so
   inline `mod tests` blocks get externalized as a file grows.
6. **The golden-safe law.** Every core hook added for the UI is either read-only
   `&self` introspection or a default-off mutating hook (watchpoints, exception
   mask, profiler, CDL, link, channel mute, boot ROM, RAM init, Game Genie) that
   never perturbs emulation when disarmed — so the debugger/viewers stay
   byte-identical to the golden. A third class — explicit user-initiated mutations
   (`debug_set_reg`, `debug_write`, load-state) — changes state only on a direct
   user action, never on the passive frame loop. The debug surface lives in
   `lib/debug.rs`, `interconnect/{accessors,debug,link}.rs`, and the `debug/`
   module. See CLAUDE.md "The golden-safe law".

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
  The CPU-side clock underneath is SameBoy's deferred-commit `pending_cycles`
  clock (`crate::cycle_clock`, with SameBoy's per-IO-write `Conflict` classes),
  which puts a bus access's *sample instant* at the M-cycle's leading edge
  (cc+0). One address is routed to that instant instead of the post-tick view:
  FF41, sampled by `Interconnect::leading_edge_sample` before `tick_machine`.
  Everything else reads the post-tick value.
- `Bus::pending`/`pending_halt_wake`/`ack` are free (no time). The CPU samples
  `pending()` at the architecturally correct points (see CPU notes); the halt
  idle loop samples `pending_halt_wake()` instead, an earlier intra-cycle view
  that misses timer IF bits committed in the second half of the current
  M-cycle for one cycle (SameBoy `GB_cpu_run` halt path; gambatte tima/,
  wilbertpol timer_if).
- The PPU is stepped per **half-dot** (`Ppu::tick_half`, the 8 MHz grain: two
  half-dots per dot at single speed, one per dot in double speed). The first half
  does no structural work and the second runs the whole-dot body, so a run of
  aligned half-dots is byte-identical to a whole-dot advance; the grain exists so
  a mode-3-exit or read boundary can sit on the odd half-dot. IF bits and
  accessibility edges are folded only on a dot-completing half
  (`Interconnect::fold_ppu_events`). The timer is stepped per M-cycle on the CPU
  clock (4 internal T-ticks); the APU per M-cycle with the DIV counter passed in
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
  entirely while the vblank source enable is set (both rules live in
  `Ppu::stat_update_tick`) — gambatte
  `mstat_irq.h doM2Event` and the mealybug handlers' "line 0 timing is
  different by 4 cycles" compensation pin them.
- **The STAT IRQ side is a single level line with rising-edge detection**
  (`Ppu::stat_update_tick`, a port of SameBoy `GB_STAT_update`,
  `display.c:523`; the rising-edge core itself is the unit-tested
  `crate::stat_update::StatUpdate`). The line is the OR of the **one**
  mode source selected by `Ppu::mode_for_interrupt` and the LYC source,
  and `IF |= STAT` fires only on its 0→1 rise — the classic STAT-blocking
  model, so a second source joining an already-high line raises nothing.
  `mode_for_interrupt` carries a deliberate no-mode-source state
  (`MODE_FOR_INTERRUPT_NONE`) that holds the mode side low between
  transitions; the LYC input is the `Ppu::lyc_interrupt_line` latch,
  re-derived from the delayed `Ppu::ly_for_comparison` whenever that is a
  real line and held across those gaps. `Ppu::stat_update_half`
  re-evaluates the level on the odd half-dot so a coincident FF41
  write-commit / LYC re-latch / mode-0 rise resolves at its true sub-dot
  phase (idempotent on the aligned grid). The mode-1/mode-2 pulses are
  direct pokes on top of that level (`stat_update_vblank_oam_pulses`):
  OAM at line-start dot 0 on lines 1-143, at dot 4 on line 0, at 144:0
  on both families, and on DMG again at dot 12 of every later vblank
  line; mode 1 and the VBlank IF bit at 144:4. The *readable* FF41
  mode/LYC bits are a separate path (`Ppu::refresh_cmp` →
  `Ppu::vis_mode`/`vis_mode_read`), not the IRQ source. The dot-0
  pulses stay second-half commits: IF reads back at once, but both the
  halt-exit sampler (`Ppu::take_stat_halt_late` → `Interconnect::
  if_late`) and the running CPU's same-cycle interrupt sample
  (`Ppu::take_stat_late` → `if_stat_late`) miss them for one M-cycle —
  the CGB 144:0 pulse is exempt. Those emission masks have no
  `GB_STAT_update` equivalent and are set by
  `Ppu::stat_update_halt_masks`. Register writes raise IF only through
  the ported trigger predicates: the DMG STAT-write glitch branch table
  (`stat_write_trigger_dmg`) plus a dots-0/4 line-start pulse
  re-decide, the CGB newly-enabled-bits table
  (`stat_write_trigger_cgb`) plus a dot-0 re-decide, and the FF45
  tables (`write_lyc_dmg`/`write_lyc_cgb`, gambatte
  lycRegChangeTriggersStatIrq). The gambatte delayed event-register
  copies survive on that write side only (`stage_stat_copies` +
  `Ppu::m2_pulse_fires`, whose blocking the level-OR otherwise
  reproduces): on CGB the FF41/FF45 copies land 6/6/8 dots after the
  architectural commit — 2 in double speed — while DMG copies update
  immediately. Double-speed/lcd-offset sub-cells stay
  documented-swap baselines. (See the CGB-C deltas section in
  `ppu/mod.rs` for the per-model timeline: readable-LYC holds, the
  delayed FF45 event copy, line-0 mode-1 tail, VRAM/OAM blocking
  shifts, the LY=153 windows, and the boot LCD phase.) The **mode-0 flip/IRQ
  anchor** (formerly parked) is re-derived jointly: the visible flip
  (STAT mode bits, OAM/VRAM unblock) and the mode-0 IRQ source rise
  together **2 dots before the pipe end** — 254+SCX%8 on a bare line,
  with the pipe-end anchors (HBlank-DMA trigger, palette blocking)
  unmoved at 256+SCX%8. With the fetch grid's OBJ costs (DMG blob 6
  dots per fetch, CGB-C first-of-line 5 — see `obj_fetch_base`),
  sprite-laden DMG lines flip at pipe end −3, keeping every sprite-laden
  flip on its mooneye-frozen dot while bare lines flip 2 dots earlier
  (double speed: −1; DMG window-aborted lines: −0 — `m0_flip_events`).
  The rise is fully visible to the running
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
| 0000-7FFF | `Cartridge::read_rom/write_rom` (an opt-in boot ROM overlays the low region until it writes FF50 — `interconnect/boot_rom.rs`) |
| 8000-9FFF | `Ppu` (VRAM, current VBK bank on CGB) |
| A000-BFFF | `Cartridge::read_ram/write_ram` |
| C000-DFFF | WRAM (CGB: D000 banked via SVBK, banks 1-7) |
| E000-FDFF | echo of C000-DDFF |
| FE00-FE9F | `Ppu` OAM (mode + DMA blocking) |
| FEA0-FEFF | prohibited area (DMG: 00/FF; CGB-C: 24 B extra OAM RAM, 4× mirrored; AGB: nibble echo — Pan Docs) |
| FF00 | `Joypad` |
| FF01-FF02 | `Serial` |
| FF04-FF07 | `Timer` |
| FF0F | IF (upper 3 bits read 1) |
| FF10-FF3F | `Apu` |
| FF40-FF4B | `Ppu` regs (FF46 DMA register lives in interconnect) |
| FF4D KEY1, FF4F VBK, FF50 boot-off, FF51-55 HDMA, FF56 RP, FF68-6B palettes, FF6C OPRI, FF70 SVBK, FF72-77 | CGB regs (interconnect, palette regs routed to PPU) |
| FF80-FFFE | HRAM |
| FFFF | IE (all 8 bits writable/readable) |

Any CPU access with a $FE00-$FEFF value on the address bus during the
mode-2 OAM scan triggers the DMG-family OAM corruption bug (Pan Docs "OAM
Corruption Bug"): `Interconnect` gates on model/halt/DMA and routes to
`Ppu::oam_bug`; the 16-bit inc/dec-unit CPU cycles reach it through
`Bus::tick_addr`/`Bus::read_inc` (blargg `oam_bug/` is the oracle).

## Models

`Model = {Dmg0, Dmg, Mgb, Sgb, Sgb2, Cgb, Agb}`. By default no boot ROM is
executed (`GameBoy::new`; every golden and test path);
`Registers::post_boot(model)` + `Interconnect::apply_post_boot_state()` set
the exact PC=0x100 state including the internal 16-bit DIV counter (this is
what `boot_div*` ROMs measure). Values come from gbctr/mooneye-gb and are
verified by `boot_regs-*`/`boot_hwio-*`/`boot_div*` ROMs. On CGB/AGB the
hand-off moment depends on the cart type: the boot ROM's DMG-compat tail
runs 0x7D8 T-cycles longer than the CGB-cart path, so `apply_post_boot_state`
shifts DIV and the LCD phase together for CGB-flagged carts (mooneye ROMs —
DMG carts — pin one side, gambatte's `$143=$C0` ROMs the other; see the
model.rs table docs). A real boot ROM can be attached opt-in
(`GameBoy::new_with_boot`, the frontend's `--boot`); it overlays 0000-7FFF until
it writes FF50 and is inert on every other path (`interconnect/boot_rom.rs`).

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
The harness is `crates/slopgb-core/tests/mooneye.rs`; the model mapping itself
lives in `tests/common/mod.rs::models_for`, which maps every ROM under
`test-roms/` to the model(s) it applies to via its filename suffix:
`-dmg0`, `-dmgABC`, `-dmgABCmgb`, `-mgb`, `-sgb`, `-sgb2`, `-cgb`/`-cgbABCDE`,
`-agb`, plus the group letters `G`(=DMG+MGB) / `S`(=SGB+SGB2) / `C`(=CGB) /
`A`(=AGB) in any combination (`-GS`, `-C`, `-A`, …). `-cgb0` maps to the empty
model list — CGB revision 0 is not modeled, so no machine can pass it. With no
recognized suffix a ROM runs on DMG+MGB+SGB+SGB2+CGB+AGB (**not** DMG0), except
`misc/` (CGB+AGB — the suite README calls it CGB/AGB extras) and
`emulator-only/` (DMG+CGB: mapper tests are model-agnostic).
`manual-only/sprite_priority` and `madness/` are verified by frame compare
against a vendored reference image instead (`tests/expected/`); the latter halts
forever and never executes `LD B,B`.

## game-boy-test-roms battery (harness)

`crates/slopgb-core/tests/gbtr/` runs the pinned c-sp/game-boy-test-roms
v7.0 collection (fetched by `test-roms/download.sh` alongside the mooneye
bundle): one module per suite — acid, age, blargg, gambatte, gbmicrotest,
mealybug, mooneye2022, same-suite, smallsuites (bully/strikethrough/
turtle-tests/scribbltests/little-things-gb/mbc3-tester/rtc3test),
wilbertpol — each
implementing its suite's documented pass protocol (the
`game-boy-test-roms-howto.md` inside each suite directory) and asserting
its full rom×model matrix against an exact known-failure baseline
(`gbtr/harness.rs::assert_against_baseline`): an unlisted failure is a
regression, a now-passing or orphaned baseline entry fails the run too —
shrinking the baselines is the tracked progress. A whole-collection
inventory guard (`gbtr/inventory.rs`) pins that every `.gb`/`.gbc` on
disk is claimed or documented-exempt by exactly one suite, so a re-pinned
collection can never silently shrink coverage. `gbtr/golden.rs`'s
`golden_fingerprint` is the byte-identity gate the golden-safe law above is
verified against. The residual-failure floor
is classified — classes A, B, C, E, F, G, H: double-speed sub-cycle phase,
speed-switch/HDMA seams, APU write phase, 2016-era expectation chains,
asset/build defects, upstream tie-breaks, one-dot conflicts (class D, the
dot-serial OAM scan, was lifted) — in the
index header of `tests/gbtr/baselines/gambatte.txt` with per-class lift
conditions (several require sub-dot event modeling, i.e. a revision of
the whole-dot timing contract above). Read that index before touching any
baselined behavior: every cluster is an A/B-swept trade whose one-sided
"fix" regresses now-green siblings.

## Work package file ownership (parallel development)

| Package | Files (exclusive) |
|---|---|
| CPU | `src/cpu/execute.rs`, `src/cpu/registers.rs`, `src/cpu/mod.rs` (the `Bus` trait), `src/cycle_clock.rs` |
| Timer/serial/joypad | `src/timer.rs`, `src/serial.rs`, `src/joypad.rs` |
| Cartridge | `src/cartridge.rs` + `src/cartridge/` (`banking`/`header`/`mbc6`/`rtc`/`save`/`state`) |
| PPU | `src/ppu/mod.rs` (struct + driver) + submodules below, `src/stat_update.rs`, `src/mode_timeline.rs` |
| APU | `src/apu/mod.rs` + `envelope`/`length`/`noise`/`pulse`/`wave` |
| Interconnect | `src/interconnect.rs` (struct + sub-dot machinery) + submodules below, `src/model.rs`, `src/lib.rs` + `src/lib/`, `tests/` |
| SGB presentation | `src/ppu/sgb.rs` + `src/ppu/sgb/` (`commands`/`transfer`/`border`/`defaults`/`bios`), `src/sgb/` (the SNES-side seams), `src/lib/sgb_api.rs` |

Each god-file split keeps the struct, its fields, and shared consts in the
parent; every submodule is a second `impl` block (`use super::*`) owning one
concern. Module ownership (each file's `//!` header names its oracle suite):

| Parent | Submodule | Owns |
|---|---|---|
| `interconnect.rs` | `interconnect/bus.rs` | `impl Bus for Interconnect` (a trait impl cannot split across files, so its bodies live in the siblings below) |
| | `interconnect/boot.rs` | power-on / post-boot state install |
| | `interconnect/boot_rom.rs` | opt-in boot-ROM overlay (inert with none attached) |
| | `interconnect/cycle.rs` | deferred-commit clock driving + leading-edge (cc+0) read helpers |
| | `interconnect/phase.rs` | the eighth-grid sub-cc phase model (`EdgeKind`, the edge-stamp helpers) |
| | `interconnect/speed.rs` | STOP / speed switch, halt-wake samplers, IF ack, dispatch retime |
| | `interconnect/oam_dma.rs` | OAM DMA engine + bus conflicts |
| | `interconnect/hdma.rs` | CGB VRAM (HBlank/General) DMA request engine |
| | `interconnect/memory.rs` | memory-map routing + IO register read/write |
| | `interconnect/tick.rs` | per-M-cycle machine advance + HALT/STOP gate |
| | `interconnect/state.rs` | save-state (de)serialization |
| | `interconnect/accessors.rs`, `debug.rs`, `link.rs` | the golden-safe debug surface (see the golden-safe law above) |
| `ppu/mod.rs` | `ppu/engine.rs` | the half-dot tick driver (`tick_half`) + read-position helpers |
| | `ppu/stat_irq.rs` | mode readout, FF41 write triggers, delayed event copies |
| | `ppu/stat_irq/reclock.rs` | the production STAT IRQ engine (`stat_update_tick`, the `GB_STAT_update` port) |
| | `ppu/stat_irq/ff0f.rs` | the FF0F read-view / squash family |
| | `ppu/stat_irq/read_laws.rs`, `read_laws_exit.rs` | the CPU-visible FF41 mode read + its mode-3-exit table |
| | `ppu/lyc.rs` | LYC compare + FF45 write triggers |
| | `ppu/blocking.rs` | mode/DMA access-block predicates |
| | `ppu/access.rs` | accessibility queries + the HDMA trigger level |
| | `ppu/oam_bug.rs` | the DMG OAM-corruption patterns (free fns, not an `impl` block) |
| | `ppu/regs.rs` | FF40-FF4B read/write |
| | `ppu/regs/stage.rs` | mode-3 write strobe (`stage_write`/`commit_eff`/`strobe_tick`) |
| | `ppu/line_setup.rs` | per-line setup at a line boundary (`start_line`) |
| | `ppu/render.rs` | mode-3 pipeline driver (struct + `render_step`) |
| | `ppu/render/sprite.rs` | OAM scan + OBJ fetch/mix |
| | `ppu/render/window.rs` | window machine |
| | `ppu/render/mode0.rs` | BG fetcher + mode-0/IRQ end-of-line grid |
| | `ppu/state.rs` | save-state (de)serialization |

Public signatures in the skeleton are the inter-package API. If you must
change one, it's a coordination point — keep the change minimal and adjust
callers in your own package only when the file table above gives you the file.
