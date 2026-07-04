# slopgb

Cycle-accurate GB/GBC emulator. Workspace: `crates/slopgb-core` (emulator, zero deps, no unsafe) + `crates/slopgb` (frontend: winit/softbuffer/cpal only).

**Read `docs/ARCHITECTURE.md` before touching core** — timing contract (tick-then-access M-cycles), memory map, module ownership, mooneye + game-boy-test-roms harness protocols.

## Rules

- TDD: failing test first. Every obscure hardware behavior gets a unit test.
- Never special-case test ROMs (mooneye or the game-boy-test-roms battery) — emulate the documented hardware behavior and cite the source in a comment when obscure.
- Before touching any baselined behavior, read the floor-class index header in `crates/slopgb-core/tests/gbtr/baselines/gambatte.txt`: every baselined cluster is an A/B-swept trade — one-sided "fixes" regress the now-green siblings.
- No new deps in core (std only); no unsafe anywhere (`forbid(unsafe_code)`); clippy `-D warnings` clean.
- No god files: keep every `.rs` **under 1000 lines**. Do split a growing file into cohesive submodules (`foo.rs` + `foo/`, each a second `impl` block via `use super::*`; struct/fields/consts stay in the parent) and externalize inline tests to a `#[cfg(test)] #[path = "X_tests.rs"] mod tests;` sibling (split that further into nested `#[path]` category modules if it too passes 1000). Don't let a module accrete unrelated concerns or a 1000-line inline `mod tests`. See `docs/tdd-split-plan.md` for the seam map.
- Commit + push frequently (after each phase/fix round). SSH-sign every commit: `export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket` (systemd agent; verify `ssh-add -l`), commit with `-S`, committer email `richard@richardmoch.xyz`, verify `%G?` = G.
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

## State (2026-07-03, #11bk)

- **Baseline (all-green, defaults NOT flipped):** mooneye 439/439 rom×model;
  gbtr v7.0 battery green vs ratcheted baselines (full run 237/0); lib 660
  unit tests; clippy `-D warnings` clean. Missing ROMs skip silently unless
  `SLOPGB_REQUIRE_ROMS=1` — run `test-roms/download.sh` first. Six class-F
  defect cases exempted (defective suites/reference legs) — never drop a
  test SameBoy passes.
- **SameBoy cycle-exact port (Phase B / S5):** flag-gated behind
  `tier2_reclock` (implies `leading_edge_reads`); production byte-identical
  OFF. Flag-on two-bin: ON 291 / OFF 486 on the 3422-row full-CGB list;
  **census of SameBoy-pass CGB blockers = 0** (unchanged by #11bj/#11bk — the
  DMG window + hblank-IF arms are all `!is_cgb()`-scoped, CGB two-bin 291/291
  zero-drift); 53 tier2 pins; mooneye 91/91 flag-on
  (`SLOPGB_MOONEYE_RECLOCK=1`) AND flag-off AND with defaults temp-flipped.
- **#11bk — DMG hblank_int mode-0 STAT-IF two-latch SHIPPED (+16 flag-on).**
  The §3b engine `hblank_int` family the #11bj classification called "atomic /
  single-edge peek" is REFINED: the `if_c`/`if_d` legs' READ frame decouples
  from the counter-pinned dispatch (like `vis_mode_read`), needing the
  two-latch DELIVER + SERVICE-CLEAR edges. The tier2 deferred `ldh a,(FF0F)`
  reads cc+0 (4 dots before production's cc+4), straddling the mode-0 rise
  `R = 254 + SCX&7`: DELIVER `[R-4, R)` returns the STAT bit set (the read's
  true cc+4 position crossed R — `ff0f_stat_peek` arm a-dmg, `if_c`);
  SERVICE-CLEAR `[R, R+4)` returns 0 (the dispatch clears IF at the read's own
  cycle — `if_d`, ISR `CP 0`), gated on `intf & ie & STAT` to separate the
  pure poll `hblank_scx2_if_a` (DI+IE=0, wants the bit set). verdict-only,
  `tier2`+`!is_cgb`+SS scoped → production/CGB byte-identical. gbmicrotest DMG
  flag-on 409→425 (ZERO of 513 regressed); pin `tier2_dmg_hblank_if_passes`.
  The `if_b`/`nops`/`hblank_scx3`/`int_scx7` siblings (27) need the dispatch
  to MOVE (parked). Map: `measurements/dmg-hblank-if-2026-07-03.md`.
- **C3 flip status (#11bj — the §3b DMG side worked):** the §3b DMG-OCR
  window blocker count was UNDER-reported by the #11bi census (want-regex
  missed 33 shared-want rows → true count 62). **Ported 56/62 DMG window
  blockers** (`tier2_dmg_window_passes`; the CGB `vis_mode_read` arms
  re-derived on the DMG frame — DMG `wy2` lag +2 vs CGB +6, per-WX/SCX ship
  deadlines; all `!is_cgb()`-scoped). **The §3b engine set (gbmicrotest 68 +
  wilbertpol 10 + age 1) MEASURED as the counter-pinned dispatch/boot-frame/
  read-clock atomic core** — no flag-gated slice; they land with the flip's
  global dispatch reclock. **The 195 pixel-reference legs CLASSIFIED**
  (`tools/classify_pixel.py`): 100 SameBoy-PASS flip-blockers (all mode-3
  render-reclock atomic, none law-reachable), 13 DMG rebaseline, 12
  golden-review. §3b now = 6 residual DMG window (atomic, same classes CGB
  parks) + 8 non-window DMG-OCR singles + the engine 63 residual
  (dispatch-atomic; #11bk shipped 16 of the 79 — the `hblank_int` `if_c`/`if_d`
  read-frame legs) + the 100 render-atomic pixel blockers + golden regen.
  Execute
  `docs/sameboy-port/C3-FLIP-CHECKLIST.md` top-to-bottom when §3b clears; do
  NOT flip defaults in any pushed commit. Maps:
  `measurements/dmg-window-port-2026-07-03.md` +
  `dmg-engine-set-classify-2026-07-03.md` + `pixel-classify-2026-07-03.md` +
  `dmg-hblank-if-2026-07-03.md` (#11bk).
- **History:** per-session port narrative in
  [`docs/sameboy-port/STATE-HISTORY.md`](docs/sameboy-port/STATE-HISTORY.md)
  (verbatim archive) and
  [`docs/hardware-state/ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md)
  (the measurement ladder); roadmap
  [`docs/sameboy-port/PORT-PLAN.md`](docs/sameboy-port/PORT-PLAN.md);
  per-session maps in `docs/sameboy-port/tools/measurements/`.

**Per-subsystem hardware-behavior notes — timing laws, quirks, the test ROMs that pin each, and the parked/disproven approaches not to re-chase — live in [`docs/hardware-state/`](docs/hardware-state/README.md) (one file per subsystem). Read the relevant file before touching that subsystem.** The floor-class index (classes A–H with lift conditions) is the header of `tests/gbtr/baselines/gambatte.txt`.
