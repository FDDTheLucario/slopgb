# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps,
no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal only, a BGB-style
debugger UI).

**Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before touching core** — timing
contract (tick-then-access M-cycles), memory map, module ownership, mooneye +
game-boy-test-roms harness protocols.

This tree is the integration of two lines: the **SameBoy cycle-exact timing port**
(the accuracy-critical, actively-developed core — its State ladder below) and the
**BGB-style debugger frontend** (viewers, savestate, link, right-click menus). Core
accuracy is authoritative; the UI hooks are read-only introspection layered on top.

## The golden-safe law (the one invariant)

Every core change made *for the UI* is read-only `&self` debug introspection
(`slopgb_core::debug` + a few `GameBoy` accessors) that never advances a cycle or
mutates state; mutating hooks (link, profiler, exception mask, channel mute) are gated
off by default. So every UI path stays **byte-identical** to the golden. Verify any
core touch with `cargo test -p slopgb-core --test gbtr` (`golden_fingerprint`) + the
mooneye matrix.

**The C3 flip is DONE and the forks are collapsed (#11cu/#11cv + S7):** the
eager-value clock is now the **only** clock — the `leading_edge_reads`/`eager_value`/
`tier2_reclock` fork flags and both alternate paths (the disproven `read_deferred`
tier2 clock, the pre-flip OFF baseline) have been deleted. The golden/baselines are
the eager reference, NOT byte-identical to the pre-port core. Verify any core touch
with `golden_fingerprint` + mooneye; never `pkill` a build sharing a `CARGO_TARGET_DIR`
(corrupts the target → false failures).

## Where the detail lives

This file is a lean index. Implementation-state narratives live in dedicated dirs —
**read the matching file before touching that area, and write changes there, not here**
(see Rules).

| Dir / file | Holds |
|---|---|
| [`docs/hardware-state/`](docs/hardware-state/README.md) | **core** per-subsystem state, quirks, parked/disproven approaches (one file per subsystem) |
| [`docs/ui-state/`](docs/ui-state/README.md) | **frontend / bgb-UI** per-area state (menus, debugger, options, viewers, save-states + link, startup + boot, layout) |
| [`docs/bgb-reference/`](docs/bgb-reference/README.md) | real bgb screenshots + capture rig — **never invent bgb's UI, capture it** |
| [`docs/sameboy-port/`](docs/sameboy-port/PORT-PLAN.md) | the SameBoy cycle-exact port: `PORT-PLAN`, `STATE-HISTORY`, `C3-FLIP-CHECKLIST`, per-session measurement maps under `tools/measurements/` (the port + its measurement scaffolding are retired — history only) |
| `docs/*-plan.md` | forward-looking plans (clone/rclick-menu/menu-design/link/bootrom/exceptions/joypad/savestate/copy-clipboard/noload-startup/qa-fixes) |
| `crates/slopgb-core/tests/gbtr/baselines/gambatte.txt` header | floor-class index (A–H + lift conditions) — read before touching baselined behavior |

When a **hardware** question comes up, consult in order:

| Source | For |
|---|---|
| `docs/hardware-state/` | this emulator's per-subsystem state + quirks |
| gbctr (Gekkio, Complete Technical Reference) | CPU/MBC timing, micro-ops |
| Pan Docs | everything else |
| `test-roms-src/<failing test>.s` asm | what a failing mooneye test actually checks |
| `<suite>/game-boy-test-roms-howto.md` (in the collection) | each gbtr suite's pass protocol + verified devices |
| SameBoy / mooneye-gb / gambatte source | undocumented corners, tie-breaks |

## Rules

- TDD: failing test first. Every obscure hardware behavior gets a unit test.
- Never special-case test ROMs (mooneye or the game-boy-test-roms battery) — emulate
  the documented hardware behavior and cite the source in a comment when obscure.
- Before touching any baselined behavior, read the floor-class index header in
  `crates/slopgb-core/tests/gbtr/baselines/gambatte.txt`: every baselined cluster is an
  A/B-swept trade — one-sided "fixes" regress the now-green siblings.
- No new deps in core (std only); no unsafe anywhere (`forbid(unsafe_code)`); clippy
  `-D warnings` clean.
- No god files: keep every `.rs` **under 1000 lines**. Split a growing file into
  cohesive submodules (`foo.rs` + `foo/`, each a second `impl` block via
  `use super::*`; struct/fields/consts stay in the parent) and externalize inline
  tests to a `#[cfg(test)] #[path = "X_tests.rs"] mod tests;` sibling (split further
  into nested `#[path]` category modules if it too passes 1000). See
  [`docs/tdd-split-plan.md`](docs/tdd-split-plan.md) for the seam map.
- **Document state in the dedicated dirs, not here.** When you build or change a
  subsystem, write its state/quirks to the matching `docs/hardware-state/` (core) or
  `docs/ui-state/` (frontend) file — one file per subsystem/area. Keep CLAUDE.md a
  lean index: durable rules, commands, and pointers only.
- Commit + push frequently. **Every commit MUST be SSH-signed** (`commit.gpgsign=true`,
  `gpg.format=ssh`, key `~/.ssh/id_ed25519`, committer `richard@richardmoch.xyz`, verify
  `%G?`=G; `export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket`, commit `-S`). Signing
  fails with `ssh_askpass` → the agent is down; ask the user to start it in-session.
- Each iteration: run `/rust-diff-review` on that iteration's diff, fix every finding
  before the next iteration.
- **Enable the pre-commit gate once per clone: `git config core.hooksPath .githooks`.**
  It runs `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -D
  warnings` (the CI checks) on the pinned toolchain (`rust-toolchain.toml`, 1.97.0) and
  blocks the commit if either fails. Bump the pin + fix new lints in the same PR.
- Keep this file updated (and `/clean-docs`-clean) as the project evolves.

## Commands

```sh
test-roms/download.sh                                  # fetch both pinned ROM bundles (once)
cargo test -p slopgb-core --lib <module>               # core unit tests
cargo test -p slopgb --bins                            # frontend (binary crate) tests
cargo test -p slopgb-core --test mooneye               # full mooneye matrix
cargo test -p slopgb-core --test gbtr                  # game-boy-test-roms battery (~4 min)
cargo run -p slopgb-core --example run_mooneye -- <rom> [model]   # single ROM debug
cargo run --release -- [game.gb]                       # play (no ROM = blank LCD; load via menu/drag-drop)
```

Parallel cargo runs: set `CARGO_TARGET_DIR=target/<name>` to dodge lock contention.

## Mooneye protocol

Test ends on `LD B,B` (`GameBoy::debug_breakpoint_hit`). Pass ⇔ B,C,D,E,H,L =
3,5,8,13,21,34. Model from filename suffix (see ARCHITECTURE.md §Mooneye). Timeout 120
emulated s.

## State (2026-07-13 — the SameBoy port is COMPLETE and the clock forks are collapsed; eager is the only clock)

- **The C3 flip is DONE and the S7 fork-collapse is DONE, all on `main`.** The eager-value
  cycle-exact clock is the sole clock — the port's dual-clock scaffolding (the `port_probe`
  measurement harness, the disproven `tier2_reclock` fork, the pre-flip OFF baseline, and
  the `eager_value`/`leading_edge_reads`/`flip_hooks` fork flags) is deleted. slopgb runs at
  SameBoy-class accuracy — cycle-exact PPU (variable mode-3 length, fine-scroll, window,
  mid-mode-3 register views), sub-M-cycle interrupt/STAT timing, double-speed.
- **Baseline (all green):** mooneye 93/93; gbtr v7.0 battery **215/0** (golden = eager
  reference; the tier2-only test rows were retired with the fork); core lib + frontend
  green; clippy clean. Missing ROMs skip unless `SLOPGB_REQUIRE_ROMS=1` — run
  `test-roms/download.sh` first.
- **SGB support** (command set, SPC700, S-DSP+ICD2 audio, BIOS, border, palette) and the
  **bgb-UI clone** (debugger, viewers, Options, right-click menu, save states, link,
  opt-in boot ROM + MCP server) are merged — per-area state in
  [`docs/ui-state/`](docs/ui-state/README.md). All eight Options → System "Emulated
  system" radios are live; "GBC + initial SGB border" captures the game's own SGB border
  from an initial SGB run and shows it with GBC color (bgb-faithful). The file picker is
  slopfp only (no native-dialog shell-out).
- **Remaining (non-blocking):** the SRAM power-on init feature (deterministic fill +
  opt-in seeded xorshift). Known residuals (all SameBoy-FAIL/floored, NOT regressions):
  DS mid-dot render floor, halt-wake/HDMA levers.
- **History:** [`STATE-HISTORY.md`](docs/sameboy-port/STATE-HISTORY.md) +
  [`ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md); roadmap
  [`PORT-PLAN.md`](docs/sameboy-port/PORT-PLAN.md); maps in
  `docs/sameboy-port/tools/measurements/`.

