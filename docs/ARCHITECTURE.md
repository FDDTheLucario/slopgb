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
- OAM DMA is an interconnect engine: 160 M-cycles + startup delay, restart
  semantics (an FF46 rewrite retargets the in-flight run immediately),
  source-range quirks (CGB sources ≥ $E0 read $FF; DMG re-reads WRAM), and
  bus conflicts mirroring gambatte-core memory.cpp: per-source-class page
  masks decide which CPU accesses collide; conflicted reads return the
  in-flight byte, conflicted *writes* derail into the in-flight OAM slot
  (DMG WRAM sources wire-AND), and CGB redirects WRAM-region accesses to
  the WRAM page picked by FF46 bit 4 (gambatte `oamdma/` is the oracle).

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
verified by `boot_regs-*`/`boot_hwio-*`/`boot_div*` ROMs.

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
*documented expected-fail list* (asserted, never silently skipped) — first
candidate: same-suite `channel_1_sweep_restart_2` (passes only on real
CGB-E; even SameBoy-E fails it).

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
