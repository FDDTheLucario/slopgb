# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps, no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal only).

**Read `docs/ARCHITECTURE.md` before touching core** — timing contract (tick-then-access M-cycles), memory map, module ownership, mooneye + game-boy-test-roms harness protocols.

**Debugger/viewer UI** (bgb functional-clone, branch `bgb-ui`): keys are **focus-dependent** like bgb. In the **game** window **F2/F3/F4** open the debugger / VRAM viewer / I/O map (or `SLOPGB_OPEN_TOOLS=debugger,vram,iomap`); in the **debugger** window they become bgb's keys — F2 toggle breakpoint, F3 step over, F7 trace (step), F4 run to cursor, Ctrl+G go to, F5/F10 open VRAM/iomap (F9 break/resume is global). bgb-style **right-click context menus** on every pane — Go to (modal hex prompt), set/clear breakpoint (red gutter dot; the paced loop free-runs to it via core `run_frame_until_breakpoint`), run-to-cursor, stay-on-bank pin, code/data hints (`db` markers that re-flow the disasm) (not-yet-wired items render greyed, matching the captures). Read-only introspection lives in `slopgb_core::debug` (std-only, side-effect-free `&self` — golden-safe); the frontend renders it on softbuffer — `ui::` toolkit (incl. `ui::menu` PopupMenu + `ui::dialog` modal) + `windows::` content (per-window `WinState` for VRAM + debugger, pure `layout`/`target_at`/`on_*_click` hit-tests) + `toolwin::`/`dbg::` (breakpoint set + `DebugAction`). The VRAM viewer is interactive (tab/checkbox/radio clicks + hover details).

**Never invent bgb's UI — capture it** (bgb runs under wine here). Plans: [`docs/bgb-clone-plan.md`](docs/bgb-clone-plan.md) (windows), [`docs/bgb-rclick-menu-plan.md`](docs/bgb-rclick-menu-plan.md) (menus); analysis-gated design decisions (keybinding routing, breakpoint/cursor state, save-state/reverse-exec/link scope) in [`docs/bgb-menu-design.md`](docs/bgb-menu-design.md). Real-screenshot spec + re-capture rig + gotchas: [`docs/bgb-reference/`](docs/bgb-reference/README.md) (captured bgb menus in [`menus/`](docs/bgb-reference/menus/)).

| To screenshot | How |
|---|---|
| slopgb's own windows | `import -window "slopgb — debugger"` (by title; `-window root` misses them under a compositor) |
| slopgb tab/checkbox UX | synthetic xdotool **clicks** reach winit windows (keys do **not**) |
| bgb's menus | synthetic wine clicks DO open them — `click 3` (right-click menu), menubar `click 1` (dropdown) |

## Rules

- TDD: failing test first. Every obscure hardware behavior gets a unit test.
- Never special-case test ROMs (mooneye or the game-boy-test-roms battery) — emulate the documented hardware behavior and cite the source in a comment when obscure.
- Before touching any baselined behavior, read the floor-class index header in `crates/slopgb-core/tests/gbtr/baselines/gambatte.txt`: every baselined cluster is an A/B-swept trade — one-sided "fixes" regress the now-green siblings.
- No new deps in core (std only); no unsafe anywhere (`forbid(unsafe_code)`); clippy `-D warnings` clean.
- No god files: keep every `.rs` **under 1000 lines**. Do split a growing file into cohesive submodules (`foo.rs` + `foo/`, each a second `impl` block via `use super::*`; struct/fields/consts stay in the parent) and externalize inline tests to a `#[cfg(test)] #[path = "X_tests.rs"] mod tests;` sibling (split that further into nested `#[path]` category modules if it too passes 1000). Don't let a module accrete unrelated concerns or a 1000-line inline `mod tests`. See `docs/tdd-split-plan.md` for the seam map.
- Commit + push frequently (after each phase/fix round). **Every commit MUST be SSH-signed** (`commit.gpgsign=true`, `gpg.format=ssh`, key `~/.ssh/id_ed25519`, committer `richard@richardmoch.xyz`). Never commit unsigned. If signing fails with `ssh_askpass`/"Could not open a connection to your authentication agent", the agent is down — ask the user to start it in-session: `! eval $(ssh-agent -s) && ssh-add ~/.ssh/id_ed25519 && echo "SSH_AUTH_SOCK=$SSH_AUTH_SOCK SSH_AGENT_PID=$SSH_AGENT_PID"`, then `export` the printed `SSH_AUTH_SOCK`/`SSH_AGENT_PID` in each Bash call (env doesn't persist across calls).
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

## State (2026-06-14)

All mooneye green (439/439 rom×model); game-boy-test-roms v7.0 battery green against ratcheted baselines (7041 cases = 6028 pass + 1013 baselined floor); 602 core unit tests. Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (CI) — run `test-roms/download.sh` first. Six class-F defect cases are exempted (not run): bully ×2 + strikethrough ×2 (defective Hacktix suites) and the scxly/mbc3-tester [Cgb] defective-reference legs — never drop a test SameBoy passes (blargg/mooneye/wilbertpol stay fully asserted).

**Per-subsystem hardware-behavior notes — timing laws, quirks, the test ROMs that pin each, and the parked/disproven approaches not to re-chase — live in [`docs/hardware-state/`](docs/hardware-state/README.md) (one file per subsystem). Read the relevant file before touching that subsystem.** The floor-class index (classes A–H with lift conditions) is the header of `tests/gbtr/baselines/gambatte.txt`.
