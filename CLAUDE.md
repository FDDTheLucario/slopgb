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
the mooneye matrix. The tier2 SameBoy reclock is **OFF by default** — production is
byte-identical to the pre-port core; never flip the `interconnect.rs` defaults in a
pushed commit.

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

## State (2026-07-07, #11bv — integration on `main`)

- **#11bv — the census NO-GO is OVERTURNED; the C3 flip is a TRACTABLE
  re-host, not a thrice-refuted wall.** `main` already contains the full port
  (`integration` ⊂ main; nothing to merge). The #11bt "98 DMG SameBoy-pass
  blockers, unfixable" verdict was founded entirely on the DEFERRED CPU clock
  (`read_deferred` shifts dispatch+reads to cc+0 → the DMG dispatch/timer rows
  break). **Measured (three-way gambatte-OCR two-bin OFF/ON/LE): the EAGER clock
  recovers the entire DMG blocker set the deferred clock breaks — 86–87 rows
  incl. ALL 45 tima.** The DMG blockers are production-CORRECT rows the deferred
  clock self-inflicts. Fix: eager clock (dispatch cc+4, count-safe) + the CGB
  read/render laws as cc+0 value peeks → flip = CGB +232 / DMG +0 (pure gain =
  GO) vs tier2's −98 (NO-GO). EV v0 measured the read frame must be the LE
  back-dated **80** (intr_2 passes LE-only), NOT the deferred artifact 84.
  Remaining: re-host the ~232 CGB laws onto the LE/frame-80 base (multi-session,
  tractable). Full map + plan:
  `docs/sameboy-port/tools/measurements/eager-clock-foundation-2026-07-07.md`.

- **Baseline (all-green, defaults NOT flipped):** mooneye 439/439 rom×model;
  gbtr v7.0 battery green vs ratcheted baselines (full run 237/0); core lib
  unit tests + frontend tests all green; clippy `-D warnings` clean. Missing
  ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` — run
  `test-roms/download.sh` first. Six class-F defect cases exempted (defective
  suites/reference legs) — never drop a test SameBoy passes.
- **bgb-UI functional clone** (debugger, VRAM/iomap viewers, Options, game-window
  right-click menu, save states, serial link, opt-in boot ROM, no-ROM startup,
  opt-in MCP server) — state per area in
  [`docs/ui-state/`](docs/ui-state/README.md). All UI core hooks are read-only
  `&self` introspection (`slopgb_core::debug` + `GameBoy` accessors) or default-off
  mutating hooks — golden-safe (see the golden-safe law above). The MCP server
  (`--mcp-port`, off by default) hosts 8 debug tools over hand-rolled HTTP/JSON-RPC
  for an LLM agent to drive the live debugger — see
  [`docs/ui-state/mcp-server.md`](docs/ui-state/mcp-server.md).
- **SameBoy cycle-exact port (Phase B / S5):** flag-gated behind
  `tier2_reclock` (implies `leading_edge_reads`); production byte-identical
  OFF. Flag-on two-bin: ON 291 / OFF 486 on the 3422-row full-CGB list;
  **census of SameBoy-pass CGB blockers = 0** (unchanged by #11bj/#11bk/#11bl/
  #11bm — the DMG window + hblank-IF + poweron + co-instant arms are all
  `!is_cgb()`-scoped, CGB two-bin 291/291 zero-drift; **#11bo added the mode-3
  RENDER reclock, also 291/291 zero-drift — its CGB slices (LCDC BG-addr, SCX-DS,
  BG-priority) touch only the pixel view, never an OCR verdict; #11bp added the
  DMG palette half-dot commit pop-grid, DMG-only + render-only, CGB 291/291
  zero-drift; #11bq added the last 6 pixel legs (SCY parity + WX defer/split +
  window-abort split) — render-view only, CGB 291/291 zero-drift TWICE**); 63
  tier2 pins;
  mooneye 91/91 flag-on (`SLOPGB_MOONEYE_RECLOCK=1`) AND flag-off AND with defaults
  temp-flipped.
- **#11bq — the LAST 6 pixel-render residuals SHIPPED; §3b RENDER half COMPLETE
  (pixel two-bin 94→100).** Three flag-gated, production byte-identical mechanisms
  (`09a9f5e` + `d3d7d40`): (1) SCY parity (`dots = 2 + (leading_edge & 1)` for
  FF42 SS); (2) WX render-view defer + un-catch SPLIT (`eff.wx` survives the arch
  write, strobe-commits at leading+2; the un-catch READ law's `wx_write_dot` stays
  cc+0 — the split); (3) window-abort render/read-law SPLIT (`window_abort_flags`
  stays eager, `window_abort_render` fires at the `render_lcdc` bit5 catch-up). The
  activation gate stays eager (a render-view activation defer was BUILT + REFUTED).
  Gates: pixel ON 94→100, OFF 100/100; CGB two-bin 291/291 IDENTICAL SET (twice);
  mooneye 91/91 ON+OFF; lib clean; clippy clean. Map:
  `measurements/dmg-m3-render-reclock-2026-07-04.md` (#11bq).
- **#11bp — the DMG palette HALF-DOT commit pop-grid SHIPPED (+5 pixel legs,
  89→94).** The sub-dot info is recovered by a whole-dot PARITY term
  (`dots = 2 + (leading_edge & 1)` for FF47-49), `tier2`+`!is_cgb`+`!glitch`
  scoped, render-only. Pin `tier2_dmg_m3_render_palette_halfdot_passes` (`f45ab02`).
- **#11bo — the tier2 MODE-3 PIXEL-RENDER reclock SHIPPED: 89/100 render-atomic
  legs in 5 flag-gated slices.** Root cause: the deferred write path advances the
  render to cc+0 before `commit_eff`, landing a mid-mode-3 SCY/SCX/BGP/OBP/LCDC
  change into `eff` 4 dots early of the cc+4 fetch grid. Separable from the read
  laws (they sample ARCH state; the render samples `eff`). Mechanisms: SCY/palette
  survive-defer (`cef8471`); LCDC BG-addr split view with `eff.render_lcdc`
  (`c26efdf`); SCX-DS dots=2 (`380cbcd`); LCDC bit0 BG-priority (`e1cd243`); LCDC
  bit1 OBJ-enable draw-side (`04d4425`). Map:
  `measurements/dmg-m3-render-reclock-2026-07-04.md`.
- **#11bk — DMG hblank_int mode-0 STAT-IF two-latch SHIPPED (+16 flag-on).** The
  `if_c`/`if_d` read frame decouples from the counter-pinned dispatch: DELIVER
  `[R-4, R)` returns the STAT bit set, SERVICE-CLEAR `[R, R+4)` returns 0, gated on
  `intf & ie & STAT`. verdict-only, `tier2`+`!is_cgb`+SS scoped. Pin
  `tier2_dmg_hblank_if_passes`. Map: `measurements/dmg-hblank-if-2026-07-03.md`.
- **#11bl — DMG power-on boot-frame read law SHIPPED (+20 flag-on).** The
  `poweron_*` reads on the pristine boot frame sample cc+0; `Ppu::boot_read`
  restores the value at the read's true cc+4 position (STAT mode + LYC + OAM/VRAM
  locks + LY re-derived on the 154×456 grid). SEPARABLE from the +4 boot DIV
  (`tier2_boot_div_passes` held). `tier2`+`!is_cgb`+`frame_count<=2`+
  `!lcd_regs_written` scoped. Pin `tier2_dmg_poweron_passes`. Map:
  `measurements/dmg-poweron-boot-read-2026-07-04.md`.
- **#11bm — the non-window DMG-OCR singles CHARACTERIZED; +1 read-frame leg
  SHIPPED, 60 measured parks.** `enable_display/ly0_m0irq_scx1_1` (glitch-line
  mode-0 co-instant FF0F read): `Ppu::ff0f_dmg_m0_coincident_mask` masks IF_STAT
  off the verdict at `dot == flip_dot`. verdict-only. Pin
  `tier2_dmg_m0_coincident_passes`. `reclock.rs` split (→848) with the FF0F
  read-view/squash family to `ppu/stat_irq/ff0f.rs`. Map:
  `measurements/dmg-ocr-singles-2026-07-04.md`.
- **C3 flip status (#11bj):** ported 56/62 DMG window blockers
  (`tier2_dmg_window_passes`, `!is_cgb()`-scoped). The §3b engine set MEASURED as
  the counter-pinned dispatch/boot-frame/read-clock atomic core. §3b now = 6
  residual DMG window + the non-window DMG-OCR singles (#11bm) + the engine
  residual (#11bk +16, #11bl +20) + the pixel blockers (#11bo +89, #11bp +5 =
  94/100) + golden regen. Execute
  [`docs/sameboy-port/C3-FLIP-CHECKLIST.md`](docs/sameboy-port/C3-FLIP-CHECKLIST.md)
  top-to-bottom when §3b clears; do NOT flip defaults in any pushed commit.
- **#11br — the ENGINE DISPATCH-ATOMIC CORE re-characterized + the dispatch lever
  BUILD-MEASURED ATOMIC (no code shipped).** The residual dispatch core ≈ 39
  flip-blockers. The imminent-rise dispatch fold gained +22 but dropped 9
  SameBoy-passes and broke `intr_2_0_timing` — an incoherent frame (dispatch at
  cc+4 while reads stay cc+0). The dispatch must CO-MOVE with the read frame (one
  coherent retime = HALFDOT Part A). REVERTED; tree byte-identical @ d3d7d40. Plan:
  `measurements/dispatch-retime-plan-2026-07-04.md`.
- **#11bu — the S6 TIMER/SERIAL COMPLETION family BUILD-MEASURED as
  READ-FRAME-WELDED; the last flag-gated slice is EXHAUSTED (no code shipped, tree
  byte-identical @ d3d7d40).** A completion peek reconstructs the reload reads but
  is welded (`full` +21/−28); co-temporal proof shows no read-time discriminator
  separates fix from drop. The flip-gated attack surface is now exhausted across
  all three families (render DONE, dispatch atomic #11br, S6 completion welded
  #11bu); the flip is gated SOLELY on the coherent per-T retime (HALFDOT Part A).
  Map: `measurements/s6-completion-weld-refuted-2026-07-04.md`.
- **#11bw — the EAGER-VALUE (EV) atomic core BUILD-MEASURED as HALF-DOT-BLOCKED on
  the read-frame (no code shipped, tree byte-identical @ ace4d31).** The eager
  clock (dispatch cc+4) + tier2 read-laws as cc+0 peeks: enabling `vis_mode_read`
  under `eager_value` is frame-mismatched — EV CGB two-bin 578→601 (+0hd) /
  585 (+4hd) / 578 (+8hd == native), a MONOTONE whole-dot curve that never dips
  below 578. The window `vis_exit_hd` arms are FIXED deferred-frame (`259+SCX&7`);
  the bare arm is EMERGENT eager-frame (`2*flip+2`) — they need DIFFERENT frames,
  i.e. the read must resolve to its true HALF-DOT (odd-hd `dhalf`), UNREACHABLE
  while the eager read keeps `dhalf==0`. intr_2 is SAFE at +8hd (entry stays
  frame-80). Slice #2a write-commit is net-zero on EV alone (atomicity). Next
  lever: wire the half-dot read (`tick_half`/`dhalf`) on the eager `Bus::read`
  (HALFDOT Part B), THEN gate `vis_mode_read` on `eager_value`. Map:
  `measurements/eager-atomic-core-2026-07-07.md`.
- **#11by — the COUPLED render-length ∧ read-exit slice SHIPPED: EV SS
  convergence STARTED (EV CGB 578→553, clean +25/−0).** Overturns #11bw's
  "half-dot-blocked" read: the SS window `_1`/`_2` pairs separate WHOLE-DOT once
  the render-LENGTH laws (`vis_hold_until`, `wx_match_dot`/`win_predraw_abort`
  latches, `wy_trig_sb` shadow) are coupled to the read-EXIT web
  (`vis_exit_hd`), enabled `eager_value && !ds` with a PRINCIPLED `+8hd`
  read-debt (cc+0→cc+4) — NOT the half-dot. Recovery: window 21 (arm1/D1 length
  + arm2 shadow + D3 abort + D6 un-trigger), accessibility 4 (`access_lead=-8`).
  EV DMG 172→147 bonus. Entry stays 80 (intr_2-safe); `early_lead`/`snap_ok`
  NOT enabled (they move the sprite-line dispatch → break `intr_2_*_sprites`).
  DS `!ds`-scoped (returns native; the `lcd_shift_dots`/`sb_dsa8` DS half-dot
  alignment is the next sub-lever — un-scoped reaches 539 via a DS pair-shuffle,
  parked). Gates: golden byte-identical, mooneye 91/91 OFF+tier2, tier2 two-bin
  291 unchanged, eager intr_2_mode0/mode3/sprites PASS, clippy clean. Map:
  `measurements/eager-coupled-slice-2026-07-07.md`.
- **#11bz–#11cb — EV convergence 553 → 516 → 462 → 428, each clean flag-gated
  read-frame port.** #11bz DS read-debt + accessibility/palette (553→516,
  `eager-ds-debt-slice`); #11ca STOP-shift/lcd-offset frame install (516→462,
  `eager-stopshift-slice`); **#11cb the line-start mode-2 back-date** (462→428,
  `6666d9d` SS + `bc68a24` DS, `eager-linestart-mode2-slice-2026-07-08.md`) — the
  eager cc+0 read's `[0,4)` mode-0 window was the ONE boundary never back-dated by
  the read-debt (the mode-2→3 entry `84→80` + mode-3 exit `read_pos_hd` already
  are), so mode-0-ISR line-start reads read 0 where SameBoy's cc+4 view reads
  mode 2; CGB+`eager_value`-scoped, CLEAN 34/0. The `vis_early` accessibility
  release was REVERTED as a shuffle (needs the reclocked dot, not a gate flip).
  Residual 428 = counter-pinned dispatch reads (129, C3-flip), DS mid-dot floor
  (94), halt-wake (unported wake clock), HDMA DMA-service. Gates all hold
  (golden/tier2-291/mooneye/EV-DMG-147/intr_2). Next: the halt-wake clock port +
  the HDMA `defer_steal` eager replication.
- **History:** per-session port narrative in
  [`docs/sameboy-port/STATE-HISTORY.md`](docs/sameboy-port/STATE-HISTORY.md)
  (verbatim archive) and
  [`docs/hardware-state/ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md)
  (the measurement ladder); roadmap
  [`docs/sameboy-port/PORT-PLAN.md`](docs/sameboy-port/PORT-PLAN.md);
  per-session maps in `docs/sameboy-port/tools/measurements/`.

**Per-subsystem hardware-behavior notes — timing laws, quirks, the test ROMs that pin each, and the parked/disproven approaches not to re-chase — live in [`docs/hardware-state/`](docs/hardware-state/README.md) (one file per subsystem). Read the relevant file before touching that subsystem.** The floor-class index (classes A–H with lift conditions) is the header of `tests/gbtr/baselines/gambatte.txt`.
