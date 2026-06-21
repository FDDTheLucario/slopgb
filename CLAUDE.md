# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps, no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal only).

**Read `docs/ARCHITECTURE.md` before touching core** — timing contract (tick-then-access M-cycles), memory map, module ownership, mooneye + game-boy-test-roms harness protocols.

## Rules

- TDD: failing test first. Every obscure hardware behavior gets a unit test.
- Never special-case test ROMs (mooneye or the game-boy-test-roms battery) — emulate the documented hardware behavior and cite the source in a comment when obscure.
- Before touching any baselined behavior, read the floor-class index header in `crates/slopgb-core/tests/gbtr/baselines/gambatte.txt`: every baselined cluster is an A/B-swept trade — one-sided "fixes" regress the now-green siblings.
- No new deps in core (std only); no unsafe anywhere (`forbid(unsafe_code)`); clippy `-D warnings` clean.
- No god files: keep every `.rs` **under 1000 lines**. Do split a growing file into cohesive submodules (`foo.rs` + `foo/`, each a second `impl` block via `use super::*`; struct/fields/consts stay in the parent) and externalize inline tests to a `#[cfg(test)] #[path = "X_tests.rs"] mod tests;` sibling (split that further into nested `#[path]` category modules if it too passes 1000). Don't let a module accrete unrelated concerns or a 1000-line inline `mod tests`. See `docs/tdd-split-plan.md` for the seam map.
- Commit + push frequently (after each phase/fix round). Repo-local `commit.gpgsign=false` (user's ssh key locked in non-interactive sessions).
- Each iteration: run `/rust-diff-review` on that iteration's diff, fix every finding before the next iteration.
- Keep this file updated (and `/clean-docs`-clean) as the project evolves.

When a hardware question comes up, consult in order:

| Source | For |
|---|---|
| `docs/hardware-state/` | this emulator's per-subsystem implementation state, quirks, and parked/disproven approaches (one file per subsystem; see its README index) |
| gbctr (Gekkio, Complete Technical Reference) | CPU/MBC timing, micro-ops |
| Pan Docs | everything else |
| `test-roms-src/<failing test>.s` asm | what a failing mooneye test actually checks |
| `<suite>/game-boy-test-roms-howto.md` (in the collection) | each gbtr suite's pass protocol + verified devices |
| SameBoy / mooneye-gb / gambatte source | undocumented corners, tie-breaks |

## Commands

```sh
test-roms/download.sh                                  # fetch both pinned ROM bundles (once)
cargo test -p slopgb-core --lib <module>               # unit tests
cargo test -p slopgb-core --test mooneye               # full mooneye matrix
cargo test -p slopgb-core --test gbtr                  # game-boy-test-roms battery (~4 min)
cargo run -p slopgb-core --example run_mooneye -- <rom> [model]   # single ROM debug
cargo run --release -- game.gb                         # play
```

Parallel cargo runs: set `CARGO_TARGET_DIR=target/<name>` to dodge lock contention.

## Mooneye protocol

Test ends on `LD B,B` (`GameBoy::debug_breakpoint_hit`). Pass ⇔ B,C,D,E,H,L = 3,5,8,13,21,34. Model from filename suffix (see ARCHITECTURE.md §Mooneye). Timeout 120 emulated s.

## State (2026-06-21)

All mooneye green (439/439 rom×model); game-boy-test-roms v7.0 battery green against ratcheted baselines (7041 cases = 6028 pass + 1013 baselined floor); 609 core unit tests. Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (CI) — run `test-roms/download.sh` first. Six class-F defect cases are exempted (not run): bully ×2 + strikethrough ×2 (defective Hacktix suites) and the scxly/mbc3-tester [Cgb] defective-reference legs — never drop a test SameBoy passes (blargg/mooneye/wilbertpol stay fully asserted).

**SameBoy cycle-exact port — BEGUN (2026-06-21); the half-dot "irreducible" verdict is REVERSED.** The prior "class-A/B floor is irreducible / a CPU-call-stack discriminator no timing model can represent" conclusion was **wrong**. Built SameBoy 1.0.2's headless tester + gap-finder and verified: **SameBoy passes ~420 of our baselined-failing gambatte rows** via a cycle-exact (T-cycle) timing model — including the kernel `m2int_m3stat_1` (renders `3`, non-blank OCR — re-verified this session) AND mooneye `intr_2_mode0_timing` simultaneously, so it is NOT a cross-oracle contradiction, just our whole-dot / tick-then-access model being too coarse. The resolver (no call-stack inspection): SameBoy samples reads at the M-cycle **leading edge** (cc+0, deferred-commit `pending_cycles`) vs our cc+4; keeps a **decoupled `mode_for_interrupt`** field separate from the CPU-visible mode; and fires the mode-2 IRQ 1 dot *before* / the mode-0 IRQ 1 dot *after* their visible edges (a 2-dot swing). The lift is an **atomic multi-session rewrite** (deferred-commit reads + boundary re-derivation land together — intermediate states are RED; recalibrates the cc-phase cluster to SameBoy's frame). Every single-lever shortcut in our cc+4 model is a measured A/B swap (R3 cc+2-read +19/−23; S3-boundary +5/−12; **M0LAG single-speed mode-0 IRQ +1 split +14/−25**, this session). **Shipped this session (branch-green, byte-identical):** the full source-grounded port spec [`docs/sameboy-port/`](docs/sameboy-port/) (cpu/ppu/slopgb-core maps + `PORT-PLAN.md` staged roadmap + TDD plan) and the validated, inert deferred-commit clock foundation [`src/cycle_clock.rs`](crates/slopgb-core/src/cycle_clock.rs) (`pending_cycles`, 5 passing tests pinning SameBoy `sm83_cpu.c` §2.1/§3/§6 — wired at port Stage S2+S3). The `event_phase`/`lead_eighths` half-dot scaffold is a correct-but-insufficient approximation, retired at port Stage S7. Continuation roadmap: [`docs/sameboy-port/PORT-PLAN.md`](docs/sameboy-port/PORT-PLAN.md). History + per-lever measurements: [`docs/hardware-state/ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md).

**Per-subsystem hardware-behavior notes — timing laws, quirks, the test ROMs that pin each, and the parked/disproven approaches not to re-chase — live in [`docs/hardware-state/`](docs/hardware-state/README.md) (one file per subsystem). Read the relevant file before touching that subsystem.** The floor-class index (classes A–H with lift conditions) is the header of `tests/gbtr/baselines/gambatte.txt`.
