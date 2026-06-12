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

## State (2026-06-12)

- All mooneye tests green — 439/439 rom×model combos, CI-verified on linux/windows/macos. Breakpoint protocol for acceptance/emulator-only/misc; frame compare for sprite_priority and madness/mgb_oam_dma_halt_sprites (that ROM halts forever, never executes LD B,B; reference frames vendored under `crates/slopgb-core/tests/expected/`).
- All subsystems implemented; 487 unit tests. Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (set in CI) — run `test-roms/download.sh` first.
- Serial clock is a master flip-flop toggled by DIV bit-7 (CGB fast: bit-2) falling edges; shifts on the high→low toggle, and **any SC write resets the flip-flop** (first bit lands on the *second* edge after the write — SameBoy `GB_serial_master_edge`, gambatte serial/ green except trigger_int8). FF04 writes reach the serial within the cycle via `Serial::div_write` (the sampled tick would miss the fast clock's reset edge).
- SGB joypad (`src/joypad.rs` `Sgb`): ICD2 command-packet receiver + MLT_REQ multiplexing, gated on Sgb/Sgb2 *and* the header SGB flag (`Cartridge::supports_sgb`: $146=$03 ∧ $14B=$33). Joypad-ID increments on JOYP bit-5 rising edges; the glitched MLT_REQ mode 2 is pinned by SameSuite sgb/ (both green). Only MLT_REQ executes — other commands are SNES-side only.
- MBC30 (MBC3 cart with >2 MiB ROM or >32 KiB RAM, SameBoy detection): 8-bit ROM-bank register, 8 RAM banks — mbc3-tester [Dmg] green; its [Cgb] reference PNG green contradicts the suite's own howto (asset defect, see `tests/gbtr/smallsuites.rs`).
- DMG OAM corruption bug implemented (`Ppu::oam_bug` + `Bus::tick_addr`/`read_inc`; DMG-family only, suppressed while halted/during OAM DMA). Window + patterns are CRC-calibrated against blargg `oam_bug/` — all green except 7-timing_effect, a defective single build that self-destructs on real hardware too (see baseline note in `tests/gbtr/blargg.rs`).
- Core public API is a curated facade (`GameBoy`, `Registers`, `Button`, `CartridgeError`, `Model` + screen/clock consts); keep internals `pub(crate)`, new integration-test escape hatches go behind `#[doc(hidden)]`.
- Audio frontend uses a hand-rolled lock-free SPSC ring (`crates/slopgb/src/audio.rs`) — the cpal callback must never lock or allocate; keep it that way.
- PPU raises STAT/VBlank IRQs via `Ppu::write`'s return value (single drain) — when adding a PPU register path, OR the returned IF bits into `intf` like the existing interconnect call sites.
- Post-boot APU is warmed ~1 emulated second so the boot beep's envelope is decayed at hand-off (PCM12/FF76 reads $00, NR52 keeps ch1 status) — don't "simplify" the warmup away.
- APU follows SameBoy's countdown model (`src/apu/`): pulse/noise step on a machine 2 MHz grid (`Apu::phase`, bit 1 = `lf_div`), triggers anchor to it, the duty bit/LFSR sample is LATCHED at expiries, NR52 power-on resets the divider chain (+DIV-event skip glitch when the DIV-APU bit is high), envelopes use the countdown + rising-edge-arming + lock scheme, noise runs a free-running 14-bit counter that triggers do NOT reset. Same-suite apu is green except sweep micro-timing, freq_change_timing revision variants, ch4 align/freq_change (NR43 corruption tables: upstream-documented non-deterministic) — see the baseline comments before touching.
- CPU interrupt sampling is FROZEN: sampled at end of opcode fetch, dispatch aborts the fetched instruction (mooneye-gb prefetch semantics). Recalibrate dependents (PPU IRQ anchors), don't move the sampling.
- Halt wake uses a separate, earlier intra-cycle sample (`Bus::pending_halt_wake`, both IME states): a timer IF committed in the second half of the M-cycle is missed for one cycle (SameBoy `GB_cpu_run`). Timer-only on purpose — the PPU IRQ anchors absorb the offset for STAT/vblank; masking those breaks mooneye `intr_2_*`/`halt_ime1_timing2-GS`. The CGB/AGB start-of-cycle staleness for first-half PPU commits stays unmodelled (gambatte `halt/*_cgb04c` split rows) pending a per-model PPU-anchor recalibration.
- HALT/STOP gate the CPU core clock via `Bus::set_halted` — engaging only *after* the post-HALT prefetch M-cycle — and the OAM DMA engine freezes with it; while frozen, the MGB PPU's OAM scan renders the glitch sprite documented in `test-roms-src/madness/mgb_oam_dma_halt_sprites.s` (other models keep the plain frozen-OAM scan: no reference data).
- The first frame after an LCD enable is presented blank (`Ppu::frame_skip`, Pan Docs LCDC.7 / SameBoy frame-skip) — frame-compare harnesses must sample ≥2 vblanks after the ROM's re-enable. CGB DMG-compat boot palettes are the real boot-ROM *defaults* (BG table ≠ OBJ table, `interconnect.rs`); the Nintendo-licensee title-hash table is deliberately unmodelled.
