# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps,
no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal only).

**Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before touching core** — timing
contract (tick-then-access M-cycles), memory map, module ownership, mooneye +
game-boy-test-roms harness protocols.

## The golden-safe law (the one invariant)

Every core change made *for the UI* is read-only `&self` debug introspection
(`slopgb_core::debug` + a few `GameBoy` accessors) — it never advances a cycle or
mutates state, so the gbtr golden frame-hash stays **byte-identical**. Mutating hooks
(link, profiler, exception mask, channel mute) are **gated off by default**
(`link_connected`/`None`/`0`) so every golden path is byte-identical. Verify any core
touch with `cargo test -p slopgb-core --test gbtr` (the `golden_fingerprint` case) +
the mooneye matrix.

## Where the detail lives

This file is a lean index. Implementation-state narratives live in dedicated dirs —
**read the matching file before touching that area, and write changes there, not here**
(see Rules).

| Dir / file | Holds |
|---|---|
| [`docs/hardware-state/`](docs/hardware-state/README.md) | **core** per-subsystem state, quirks, parked/disproven approaches (one file per subsystem) |
| [`docs/ui-state/`](docs/ui-state/README.md) | **frontend / bgb-UI** per-area state (menus, debugger, options, viewers, save-states + link, startup + boot, layout) |
| [`docs/bgb-reference/`](docs/bgb-reference/README.md) | real bgb screenshots + capture rig — **never invent bgb's UI, capture it** |
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
  lean index (≤150 lines, `/clean-docs`-clean): durable rules, commands, and pointers
  only — never a dense per-feature narrative.
- Commit + push frequently (after each phase/fix round). **Every commit MUST be
  SSH-signed** (`commit.gpgsign=true`, `gpg.format=ssh`, key `~/.ssh/id_ed25519`,
  committer `richard@richardmoch.xyz`). Never commit unsigned. If signing fails with
  `ssh_askpass`/"Could not open a connection to your authentication agent", the agent
  is down — ask the user to start it in-session: `! eval $(ssh-agent -s) && ssh-add
  ~/.ssh/id_ed25519 && echo "SSH_AUTH_SOCK=$SSH_AUTH_SOCK SSH_AGENT_PID=$SSH_AGENT_PID"`,
  then `export` the printed `SSH_AUTH_SOCK`/`SSH_AGENT_PID` in each Bash call (env
  doesn't persist across calls).
- Each iteration: run `/rust-diff-review` on that iteration's diff, fix every finding
  before the next iteration.

## Commands

```sh
test-roms/download.sh                                  # fetch both pinned ROM bundles (once)
cargo test -p slopgb-core --lib <module>               # unit tests
cargo test -p slopgb-core --test mooneye               # full mooneye matrix
cargo test -p slopgb-core --test gbtr                  # game-boy-test-roms battery (~4 min)
cargo run -p slopgb-core --example run_mooneye -- <rom> [model]   # single ROM debug
cargo run --release -- [game.gb]                       # play (no ROM = blank LCD; load via menu/drag-drop)
```

Parallel cargo runs: set `CARGO_TARGET_DIR=target/<name>` to dodge lock contention.
The frontend is a binary crate — test it with `cargo test -p slopgb --bins`.

## Mooneye protocol

Test ends on `LD B,B` (`GameBoy::debug_breakpoint_hit`). Pass ⇔ B,C,D,E,H,L =
3,5,8,13,21,34. Model from filename suffix (see ARCHITECTURE.md §Mooneye). Timeout 120
emulated s.

## State (2026-06-20)

- All mooneye green (439/439 rom×model); game-boy-test-roms v7.0 battery green against
  ratcheted baselines (7041 cases = 6028 pass + 1013 baselined floor); 602+ core unit
  tests, 370 frontend tests.
- Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (CI) — run
  `test-roms/download.sh` first. Six class-F defect cases are exempted (not run):
  bully ×2 + strikethrough ×2 (defective Hacktix suites) and the scxly/mbc3-tester
  [Cgb] defective-reference legs — never drop a test SameBoy passes (blargg / mooneye /
  wilbertpol stay fully asserted).
- **bgb-UI functional clone** (debugger, VRAM/iomap viewers, Options, game-window
  right-click menu, save states, serial link, opt-in boot ROM, no-ROM startup) — state
  per area in [`docs/ui-state/`](docs/ui-state/README.md).
- **Per-subsystem core hardware notes** (timing laws, quirks, the test ROMs that pin
  each, parked approaches) in [`docs/hardware-state/`](docs/hardware-state/README.md).
  The floor-class index is the header of `tests/gbtr/baselines/gambatte.txt`.
