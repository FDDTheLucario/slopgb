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
(`slopgb_core::debug` + a few `GameBoy` accessors) — it never advances a cycle or
mutates state, so the gbtr golden frame-hash stays **byte-identical**. Mutating hooks
(link, profiler, exception mask, channel mute) are **gated off by default**
(`link_connected`/`None`/`0`) so every golden path is byte-identical. Verify any core
touch with `cargo test -p slopgb-core --test gbtr` (the `golden_fingerprint` case) +
the mooneye matrix. **The C3 flip is DONE (#11cu/#11cv):** the eager-value clock
(`leading_edge_reads`/`eager_value` = `true` in the `interconnect.rs` struct literal)
is now the production default — the SameBoy cycle-exact port is complete, TRUE
flip-floor 0 both models. Production is NO LONGER byte-identical to the pre-port core;
the golden/baselines are the eager-clock reference. The golden-safe law still binds
every *UI* hook (read-only `&self` introspection + default-off mutating hooks) to be
byte-identical against the eager golden. The tier2 SameBoy reclock stays **OFF** (the
disproven `read_deferred` variant); the pre-port OFF clock survives only as the
`cfg(test)`/`port_probe` two-bin baseline. Verify via `new()` (production), NOT the
`SLOPGB_EAGER` post-boot toggle (incoherent) — and never `pkill` a cargo build sharing
a `CARGO_TARGET_DIR` (corrupts the target → false failures).

## Where the detail lives

This file is a lean index. Implementation-state narratives live in dedicated dirs —
**read the matching file before touching that area, and write changes there, not here**
(see Rules).

| Dir / file | Holds |
|---|---|
| [`docs/hardware-state/`](docs/hardware-state/README.md) | **core** per-subsystem state, quirks, parked/disproven approaches (one file per subsystem) |
| [`docs/ui-state/`](docs/ui-state/README.md) | **frontend / bgb-UI** per-area state (menus, debugger, options, viewers, save-states + link, startup + boot, layout) |
| [`docs/bgb-reference/`](docs/bgb-reference/README.md) | real bgb screenshots + capture rig — **never invent bgb's UI, capture it** |
| [`docs/sameboy-port/`](docs/sameboy-port/PORT-PLAN.md) | the SameBoy cycle-exact port: `PORT-PLAN`, `STATE-HISTORY`, `C3-FLIP-CHECKLIST`, `PROBE-HARNESS` (the `--features port_probe` measurement traces/knobs), per-session measurement maps under `tools/measurements/` |
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
- Commit + push frequently (after each phase/fix round). **Every commit MUST be
  SSH-signed** (`commit.gpgsign=true`, `gpg.format=ssh`, key `~/.ssh/id_ed25519`,
  committer `richard@richardmoch.xyz`, verify `%G?` = G). Never commit unsigned.
  `export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket` (systemd agent; verify
  `ssh-add -l`), commit with `-S`. If signing fails with `ssh_askpass`/"Could not open
  a connection to your authentication agent", the agent is down — ask the user to start
  it in-session: `! eval $(ssh-agent -s) && ssh-add ~/.ssh/id_ed25519 && echo
  "SSH_AUTH_SOCK=$SSH_AUTH_SOCK SSH_AGENT_PID=$SSH_AGENT_PID"`, then `export` the
  printed `SSH_AUTH_SOCK`/`SSH_AGENT_PID` in each Bash call (env doesn't persist across
  calls).
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

## State (2026-07-12 — the SameBoy cycle-exact port is COMPLETE; eager clock is production default)

- **The C3 flip is DONE and on `main` (#11cu/#11cv).** The eager-value cycle-exact
  clock (`leading_edge_reads`/`eager_value` = `true` in the `interconnect.rs` struct
  literal; `tier2_reclock` stays OFF — the disproven `read_deferred` variant) is the
  production default. Flip-floor census = **TRUE floor 0 both models**: the eager clock
  drops ZERO SameBoy-pass rows across gambatte/gbmicrotest/mealybug/wilbertpol/age/
  mooneye; the ~43 residual gambatte flip-BUGs are all rows SameBoy itself fails
  (rebaseline-OK reference-value divergences). slopgb now runs at SameBoy-class accuracy
  — cycle-exact PPU (variable mode-3 length, fine-scroll, window, mid-mode-3 register
  views), sub-M-cycle interrupt/STAT timing, double-speed. The census-refuted
  "m1statwirq CPU-atomic dispatch wall" fell to a one-dot fix (line-153 LYC IF emits at
  dot 6/read-frame but belongs at dot 4/dispatch-frame; dot-4 emission decouple + 13-row
  LYC-153 cluster re-host, `5c91f68`), no dispatch move. `new()` == `new_with_eager`
  byte-identical (the `#11ds` post-boot re-arm construction is coherent).

- **Baseline (all green on `main`, production = eager):** mooneye 93/93 (full matrix);
  gbtr v7.0 battery **278/0** (golden = the eager reference); core lib + frontend tests
  green; clippy `-D warnings` clean. Missing ROMs skip silently unless
  `SLOPGB_REQUIRE_ROMS=1` — run `test-roms/download.sh` first. Verify a flip via `new()`
  (the production construction), NEVER the `SLOPGB_EAGER` post-boot toggle (incoherent).

- **SGB support** (full command set, SPC700 CPU, S-DSP + ICD2 audio, BIOS gating,
  default border + title palette) merged from the SGB line — state in `docs/ui-state/` +
  `docs/hardware-state/`.

- **bgb-UI functional clone** (debugger, VRAM/iomap viewers, Options, game-window
  right-click menu, save states, serial link, opt-in boot ROM, no-ROM startup, opt-in MCP
  server) — state per area in [`docs/ui-state/`](docs/ui-state/README.md). All UI core
  hooks are read-only `&self` introspection or default-off mutating hooks (golden-safe
  against the eager golden — see the golden-safe law above).

- **Toolchain + CI:** pinned to **Rust 1.97.0** (`rust-toolchain.toml` + `ci.yml`
  `dtolnay/rust-toolchain@1.97.0`) so CI and every local checkout run the identical
  rustc/clippy (no stable-drift lint breakage). Pre-commit gate `.githooks/pre-commit`
  runs the CI rustfmt + clippy checks and blocks the commit if either fails — enable per
  clone with `git config core.hooksPath .githooks`. Every `.rs` is under the 1000-line
  cap.

- **Remaining (non-blocking):** S7 scaffolding cleanup — delete the OFF/tier2 clock forks
  and retire the `cfg(test)`/`port_probe` `new_with_eager`/`new_with_reclock` now that
  eager is the default. The SRAM/RAM power-on init feature (deterministic fill default +
  opt-in seeded PRNG, golden-safe). Known accuracy residuals (all SameBoy-FAIL or floored
  — NOT regressions): the DS mid-dot render floor + the halt-wake/HDMA eager sub-levers.

- **History:** per-session port narrative in
  [`docs/sameboy-port/STATE-HISTORY.md`](docs/sameboy-port/STATE-HISTORY.md)
  (verbatim archive) and
  [`docs/hardware-state/ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md)
  (the measurement ladder); roadmap
  [`docs/sameboy-port/PORT-PLAN.md`](docs/sameboy-port/PORT-PLAN.md);
  per-session maps in `docs/sameboy-port/tools/measurements/`.

**Per-subsystem hardware-behavior notes — timing laws, quirks, the test ROMs that pin each, and the parked/disproven approaches not to re-chase — live in [`docs/hardware-state/`](docs/hardware-state/README.md) (one file per subsystem). Read the relevant file before touching that subsystem.** The floor-class index (classes A–H with lift conditions) is the header of `tests/gbtr/baselines/gambatte.txt`.

