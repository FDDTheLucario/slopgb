# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps,
no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal + gilrs for game
controllers, a BGB-style debugger UI) + `crates/slopgb-plugin-api` (guest SDK for
Rust→wasm plugins) + `crates/slopgb-plugin-host` (the wasmi runtime — the one place
`wasmi` is a dep, isolated so core stays zero-dep and the frontend keeps its lean
dep set).

**Plugins have three peer capability tiers, one loader each** (see
[`crates/slopgb-plugin-host/CLAUDE.md`](crates/slopgb-plugin-host/CLAUDE.md)):
tier-1 `INTROSPECTION` (`PluginHost` per-frame pump, `--plugins`), tier-2 tool
(`LoadedTool`, MCP), tier-3 `SUBSYSTEM` (`LoadedCoprocessor`): the SGB
coprocessor auto-loads `spc700.wasm` + `w65c816.wasm` from the `--plugins` dir on
SGB models; MSU-1 loads from a `--msu1` pack. **Subsystem plugins (SPC700 / 65C816
/ MSU-1 — `slopgb-*-plugin`, built by `cargo xtask stage-plugins`) are
first-class**: the host supports every valid subsystem type via the generic
coprocessor ABI. They load through their own seam — the tier-1 `--plugins`
*scanner* skips them (a loader mismatch, not an invalid plugin), even though the
SGB coprocessor reads its plugins from that same directory.

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
mutates state; every mutating hook is gated off by default — watchpoints, the
exception mask, the profiler, CDL, link, channel mute, the opt-in boot ROM, RAM
init, and Game Genie patches (all default-empty/off, verified per gate in a unit
test). A third class — explicit user-initiated mutations (`debug_set_reg`,
`debug_write`, load-state) — changes state only on a direct user action, never on
the passive frame loop. So every UI path stays **byte-identical** to the golden.
Verify any core touch with `cargo test -p slopgb-core --test gbtr`
(`golden_fingerprint`) + the mooneye matrix; the armed-hook half is pinned by
`armed_debug_hooks_do_not_perturb_emulation` + `cdl_logging_does_not_perturb_emulation`.

The SameBoy port is complete: the eager cycle-exact clock is the **only** clock (the
dual-clock fork scaffolding was deleted). The golden/baselines are the eager reference,
**NOT** byte-identical to the pre-port core. Never `pkill` a build sharing a
`CARGO_TARGET_DIR` (corrupts the target → false failures).

## Where the detail lives

This file is a lean index. Implementation-state narratives live in dedicated dirs —
**read the matching file before touching that area, and write changes there, not here**
(see Rules).

| Dir / file | Holds |
|---|---|
| [`docs/hardware-state/`](docs/hardware-state/README.md) | **core** per-subsystem state, quirks, parked/disproven approaches (one file per subsystem) |
| [`docs/ui-state/`](docs/ui-state/README.md) | **frontend / bgb-UI** per-area state (menus, debugger, options, viewers, save-states + link, startup + boot, layout) |
| [`docs/bgb-reference/`](docs/bgb-reference/README.md) | real bgb screenshots + capture rig — **never invent bgb's UI, capture it** |
| [`docs/msu1-plugin-plan.md`](docs/msu1-plugin-plan.md) | MSU-1 streaming-audio coprocessor plugin, wired into a running machine (`--msu1`; registers at `$A000-$A007`, frontend-owned, golden-safe) + the resident-handler/polled-mailbox pattern |
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
  `-D warnings` clean. The sole external runtime dep (`wasmi`, the plugin engine)
  is quarantined in `crates/slopgb-plugin-host`; the guest SDK uses
  `deny(unsafe_code)` + a scoped `allow` for two wasm linkage markers only (no
  `unsafe` blocks).
- No god files: keep every `.rs` **under 1000 lines**. Split a growing file into
  cohesive submodules (`foo.rs` + `foo/`, each a second `impl` block via
  `use super::*`; struct/fields/consts stay in the parent) and externalize inline
  tests to a `#[cfg(test)] #[path = "X_tests.rs"] mod tests;` sibling (split further
  into nested `#[path]` category modules if it too passes 1000).
- **Document state in the dedicated dirs, not here.** When you build or change a
  subsystem, write its state/quirks to the matching `docs/hardware-state/` (core) or
  `docs/ui-state/` (frontend) file — one file per subsystem/area. Keep CLAUDE.md a
  lean index: durable rules, commands, and pointers only.
- **No jargon comments; keep comments fresh.** A comment explains what the *current*
  code does + *why* the hardware behaves so — never process narrative. Forbidden in
  comments: fork/session codenames (`tier2`, `eager`, `flag-on/off`, `byte-identical
  OFF`, `#11x`, `S5`, `C3`, "port stage N"), A/B-sweep stories, and "inert/dead/never-
  called/off" claims you haven't just verified. Put narrative in the commit or `docs/`;
  keep the pinning ROM + hardware citation inline. **When you touch a comment, re-verify
  it against the code**: every named symbol must exist (grep it); a "removing this is
  byte-identical" claim must be *probed* (force the value off, run `golden_fingerprint`
  — if golden changes it is LIVE and the claim is false). A stale/false core comment is
  a regression trap: a defect, not a nit.
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

## State

Baseline (all green, on `main`): mooneye **93/93**, gbtr v7.0 **215/0**, core lib +
frontend green, clippy clean. Missing ROMs skip unless `SLOPGB_REQUIRE_ROMS=1` (run
`test-roms/download.sh` first). The SameBoy cycle-exact port, SGB support (SPC700 +
S-DSP audio, BIOS, border, and the SNES-side LLE: 65C816 + SNES-PPU plugins, GP-DMA,
autopoll, the arcade-takeover runtime — Space Invaders' ARCADE mode runs its own
SNES program end to end) and the bgb-UI clone (debugger, viewers, Options, link,
opt-in boot ROM, MCP) are all merged — per-area detail in
[`docs/ui-state/`](docs/ui-state/README.md) + [`docs/hardware-state/`](docs/hardware-state/README.md).
UI theming (contemporary Light default / Dark / Classic + custom-theme API; colour-only,
`T` toggles Light↔Dark): [`docs/ui-state/theming.md`](docs/ui-state/theming.md).
Known residuals (all SameBoy-FAIL/floored, NOT regressions): DS mid-dot render floor,
halt-wake/HDMA levers.

