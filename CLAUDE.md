# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps, no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal only).

**Read `docs/ARCHITECTURE.md` before touching core** — timing contract (tick-then-access M-cycles), memory map, module ownership, mooneye protocol.

## Rules

- TDD: failing test first. Every obscure hardware behavior gets a unit test.
- Never special-case mooneye ROMs — emulate the documented hardware behavior and cite the source in a comment when obscure.
- No new deps in core (std only); no unsafe anywhere (`forbid(unsafe_code)`); clippy `-D warnings` clean.
- Commit + push frequently (after each phase/fix round). Repo-local `commit.gpgsign=false` (user's ssh key locked in non-interactive sessions).
- Keep this file updated as the project evolves.

When a hardware question comes up, consult in order:

| Source | For |
|---|---|
| gbctr (Gekkio, Complete Technical Reference) | CPU/MBC timing, micro-ops |
| Pan Docs | everything else |
| `test-roms-src/<failing test>.s` asm | what a failing mooneye test actually checks |
| SameBoy / mooneye-gb source | undocumented corners, tie-breaks |

## Commands

```sh
test-roms/download.sh                                  # fetch pinned mooneye ROMs (once)
cargo test -p slopgb-core --lib <module>               # unit tests
cargo test -p slopgb-core --test mooneye               # full mooneye matrix
cargo run -p slopgb-core --example run_mooneye -- <rom> [model]   # single ROM debug
cargo run --release -- game.gb                         # play
```

Parallel cargo runs: set `CARGO_TARGET_DIR=target/<name>` to dodge lock contention.

## Mooneye protocol

Test ends on `LD B,B` (`GameBoy::debug_breakpoint_hit`). Pass ⇔ B,C,D,E,H,L = 3,5,8,13,21,34. Model from filename suffix (see ARCHITECTURE.md §Mooneye). Timeout 120 emulated s.

## State (2026-06-10)

- Goal: every mooneye test green (acceptance, emulator-only, misc, madness, sprite_priority via frame compare). Fully featured emulator, not test-ROM golf.
- Done: scaffold, CPU, timer/serial/joypad, cartridge (MBC1(+M)/2/3+RTC/5), APU, harness, UI frontend, CI.
- In flight: PPU (dot-accurate FIFO), then interconnect/model tables, then mooneye fix loop.
- Pending core API for UI: `GameBoy::set_sample_rate`, `GameBoy::set_dmg_palette`.
