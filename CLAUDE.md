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

All mooneye green (439/439 rom×model); game-boy-test-roms v7.0 battery green against ratcheted baselines (7041 cases = 6028 pass + 1013 baselined floor); 614 core unit tests. Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (CI) — run `test-roms/download.sh` first. Six class-F defect cases are exempted (not run): bully ×2 + strikethrough ×2 (defective Hacktix suites) and the scxly/mbc3-tester [Cgb] defective-reference legs — never drop a test SameBoy passes (blargg/mooneye/wilbertpol stay fully asserted).

**SameBoy cycle-exact port — BEGUN (2026-06-21); the half-dot "irreducible" verdict is REVERSED.** The prior "class-A/B floor is irreducible / a CPU-call-stack discriminator no timing model can represent" conclusion was **wrong**. Built SameBoy 1.0.2's headless tester + gap-finder and verified: **SameBoy passes ~420 of our baselined-failing gambatte rows** via a cycle-exact (T-cycle) timing model — including the kernel `m2int_m3stat_1` (renders `3`, non-blank OCR — re-verified this session) AND mooneye `intr_2_mode0_timing` simultaneously, so it is NOT a cross-oracle contradiction, just our whole-dot / tick-then-access model being too coarse. The resolver (no call-stack inspection): SameBoy samples reads at the M-cycle **leading edge** (cc+0, deferred-commit `pending_cycles`) vs our cc+4; keeps a **decoupled `mode_for_interrupt`** field separate from the CPU-visible mode; and fires the mode-2 IRQ 1 dot *before* / the mode-0 IRQ 1 dot *after* their visible edges (a 2-dot swing). The lift is an **atomic multi-session rewrite** (deferred-commit reads + boundary re-derivation land together — intermediate states are RED; recalibrates the cc-phase cluster to SameBoy's frame). **In-emulator the kernel pair now provably SEPARATES** (2026-06-21 #2, instrumented + reverted, `ppu-subdot-ladder.md`): leading-edge reads land m2int's FF41 read at line-1 dot 248 and m0int's at dot 252 — already in the right order — so back-dating the *visible* mode→0 flip into (248,252] (decoupled from the IRQ dispatch, which stays put) makes m2int read 3 ∧ m0int read 0 together. The residual is that the cc+0 read needs the **dispatch** at SameBoy's frame (~248) too; left at our cc+4 dot 254 it mis-frames the canonical mooneye `intr_2_mode0_timing` — so the dispatch reclock (the global ~7000-row rebaseline) is the next coordinated lever, not a missing within-frame coordination. Every shortcut in our cc+4 model is a measured A/B swap (R3 cc+2-read +19/−23; S3-boundary +5/−12; **M0LAG single-speed mode-0 IRQ +1 split +14/−25**; and the **combined kernel fix (IRQ-split + leading-edge FF41 read) +33/−48 that cascade-breaks mooneye2022 + gbmicrotest + wilbertpol + age + 146 golden frames** — both this session). The combo is the conclusive proof: the lift moves the IRQ-dispatch dot (pinned by counter-based gbmicrotest/mooneye2022 tests that an FF41-read-phase change cannot compensate) AND drifts rendered frames, so it is inseparable from the global cc-exact reclock. **Shipped this session (branch-green, byte-identical):** the full source-grounded port spec [`docs/sameboy-port/`](docs/sameboy-port/) (cpu/ppu/slopgb-core maps + `PORT-PLAN.md` staged roadmap + TDD plan) and the validated, inert deferred-commit foundation: [`src/cycle_clock.rs`](crates/slopgb-core/src/cycle_clock.rs) (the CPU-side `pending_cycles` clock, 5 tests pinning SameBoy `sm83_cpu.c` §2.1/§3/§6) + [`src/mode_timeline.rs`](crates/slopgb-core/src/mode_timeline.rs) (the PPU-side decoupled visible/interrupt-mode model, 5 tests pinning `display.c` mode-3 length + the mode-2/mode-0 anchor swing that separates the kernel pair) — both wired at port Stage S2+S3. The finer-resolution event-phase model is the half-dot grid's documented continuation (the class-A lift condition: "re-clock observable event commits to a [finer] grid"). The `event_phase`/`lead_eighths` half-dot scaffold is a correct-but-insufficient approximation, retired at port Stage S7. Continuation roadmap: [`docs/sameboy-port/PORT-PLAN.md`](docs/sameboy-port/PORT-PLAN.md). History + per-lever measurements: [`docs/hardware-state/ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md).

**PHASE B (the cc-exact reclock) — 3/4 PARTIAL CONFIRM (2026-06-23 #9). The kernel/`intr_2` mutual-exclusion DISSOLVES; the `int_hblank` residual is now FIXED.** Wired the deferred-commit machine advance (B1 — PPU/timer/APU/serial slaved to the deferred clock's paid debt, the S2+S3 foundation) + the dispatch retime (B2) + `early_lead`−2 (B3a) + the deferred mode-0 halt-wake `if_late` re-derivation (B4, `m0_halt_hold`), all flag-gated behind a NEW `tier2_reclock` flag (separate from `leading_edge_reads`; production + the S0 leading-edge specs byte-identical). **Result: the kernel pair SEPARATES (m2int=3 ∧ m0int=0) WHILE mooneye `intr_2_mode0_timing` PASSES AND `int_hblank_halt_scx0-7` PASSES — all on both models / DMG — so the A8 "m0int=0 forces intr_2 FAIL" exclusion is dissolved and the L1 residual is cleared.** Pinned by gbtr `tier2_kernel_pair_matches_sameboy_target` + `tier2_int_hblank_halt_passes_dmg` (flag-on). Corrections to the prior plan, all measured: **B2 (retime) is INERT** (B1+B3a do the kernel work); the prescribed "dispatch 254→252" is **wrong-direction**; and the planned **"S5 mode-0-raise −1" is REFUTED** for int_hblank (the wake is M-cycle-quantized to the deferred halt-loop grid, not the engine raise dot — #8). The L1 fix is the prescribed if_late re-derivation: 2 uniform masked M-cycles + `mask{rise cc==4}` (the deferred `cc=eager+1` rotation), the mode-0 halt mask being free to recalibrate (intr_2 wakes on mode-2, the kernel reads FF41). **ONE residual remains: `intr_2_mode0_timing_sprites`** (sprite-line deferred `early_lead`, model-split CGB 0 / DMG −1 + the DMG X=167 sprite-edge geometry — #7). Did NOT flip the defaults (4/4 + the ~7000 rebaseline remain). Detail: [`docs/hardware-state/ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md) "THESIS RESULT #8/#9".

**Per-subsystem hardware-behavior notes — timing laws, quirks, the test ROMs that pin each, and the parked/disproven approaches not to re-chase — live in [`docs/hardware-state/`](docs/hardware-state/README.md) (one file per subsystem). Read the relevant file before touching that subsystem.** The floor-class index (classes A–H with lift conditions) is the header of `tests/gbtr/baselines/gambatte.txt`.
