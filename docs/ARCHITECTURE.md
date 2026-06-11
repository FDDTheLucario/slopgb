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
- `Bus::pending`/`ack` are free (no time). The CPU samples `pending()` at the
  architecturally correct points (see CPU notes).
- The PPU is stepped per dot; the timer per M-cycle on the CPU clock
  (4 internal T-ticks); the APU per M-cycle with the DIV counter passed in
  (DIV-APU = falling edge of DIV bit 4, bit 5 in double speed).
- OAM DMA is an interconnect engine: 160 M-cycles + startup delay, restart
  semantics, source-range quirks, and bus conflicts (CPU reads of
  OAM/conflicting bus during DMA return 0xFF / DMA data per hardware).

## Memory map routing (interconnect)

| Range | Target |
|---|---|
| 0000-7FFF | `Cartridge::read_rom/write_rom` |
| 8000-9FFF | `Ppu` (VRAM, current VBK bank on CGB) |
| A000-BFFF | `Cartridge::read_ram/write_ram` |
| C000-DFFF | WRAM (CGB: D000 banked via SVBK, banks 1-7) |
| E000-FDFF | echo of C000-DDFF |
| FE00-FE9F | `Ppu` OAM (mode + DMA blocking) |
| FEA0-FEFF | prohibited area (model-specific reads; DMG: OAM-corruption-free 00/FF behavior — see Pan Docs) |
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
