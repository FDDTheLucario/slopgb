# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps, no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal only).

**Read `docs/ARCHITECTURE.md` before touching core** — timing contract (tick-then-access M-cycles), memory map, module ownership, mooneye protocol.

## Rules

- TDD: failing test first. Every obscure hardware behavior gets a unit test.
- Never special-case mooneye ROMs — emulate the documented hardware behavior and cite the source in a comment when obscure.
- No new deps in core (std only); no unsafe anywhere (`forbid(unsafe_code)`); clippy `-D warnings` clean.
- Commit + push frequently (after each phase/fix round). Repo-local `commit.gpgsign=false` (user's ssh key locked in non-interactive sessions).
- Each iteration: run `/rust-diff-review` on that iteration's diff, fix every finding before the next iteration.
- Keep this file updated (and `/clean-docs`-clean) as the project evolves.

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

## State (2026-06-11)

- Goal reached: every mooneye test green — 439/439 rom×model combos, CI-verified on linux/windows/macos. Breakpoint protocol for acceptance/emulator-only/misc; frame compare for sprite_priority *and* madness/mgb_oam_dma_halt_sprites (that ROM halts forever, never executes LD B,B; reference frames vendored as shade-class .bin under `crates/slopgb-core/tests/expected/`).
- All subsystems implemented; 412 unit tests; two diff-review rounds (56 findings) plus a full-codebase review round (23-task TDD fix plan: lock-free SPSC audio ring, PPU IRQ single-drain via `Ppu::write` return, CGB flag = header bit 7, API facade, `SLOPGB_REQUIRE_ROMS=1` gate in CI).
- Core public API is a curated facade (`GameBoy`, `Registers`, `Button`, `CartridgeError`, `Model` + screen/clock consts); internals `pub(crate)`, integration-test escape hatches are `#[doc(hidden)]`.
- Post-boot APU is warmed ~1 emulated second so the boot beep's envelope is decayed at hand-off (PCM12/FF76 reads $00, NR52 keeps ch1 status) — don't "simplify" the warmup away.
- CPU interrupt sampling is FROZEN: sampled at end of opcode fetch, dispatch aborts the fetched instruction (mooneye-gb prefetch semantics). Recalibrate dependents (PPU IRQ anchors), don't move the sampling.
- HALT/STOP gate the CPU core clock via `Bus::set_halted` — engaging only *after* the post-HALT prefetch M-cycle — and the OAM DMA engine freezes with it; while frozen, the MGB PPU's OAM scan renders the glitch sprite documented in `test-roms-src/madness/mgb_oam_dma_halt_sprites.s` (other models keep the plain frozen-OAM scan: no reference data).
