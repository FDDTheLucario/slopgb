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

## State (2026-07-04, #11bo)

- **Baseline (all-green, defaults NOT flipped):** mooneye 439/439 rom×model;
  gbtr v7.0 battery green vs ratcheted baselines (full run 237/0); lib 660
  unit tests; clippy `-D warnings` clean. Missing ROMs skip silently unless
  `SLOPGB_REQUIRE_ROMS=1` — run `test-roms/download.sh` first. Six class-F
  defect cases exempted (defective suites/reference legs) — never drop a
  test SameBoy passes.
- **SameBoy cycle-exact port (Phase B / S5):** flag-gated behind
  `tier2_reclock` (implies `leading_edge_reads`); production byte-identical
  OFF. Flag-on two-bin: ON 291 / OFF 486 on the 3422-row full-CGB list;
  **census of SameBoy-pass CGB blockers = 0** (unchanged by #11bj/#11bk/#11bl/
  #11bm — the DMG window + hblank-IF + poweron + co-instant arms are all
  `!is_cgb()`-scoped, CGB two-bin 291/291 zero-drift; **#11bo added the mode-3
  RENDER reclock, also 291/291 zero-drift — its CGB slices (LCDC BG-addr, SCX-DS,
  BG-priority) touch only the pixel view, never an OCR verdict**); 59 tier2 pins;
  mooneye 91/91 flag-on (`SLOPGB_MOONEYE_RECLOCK=1`) AND flag-off AND with defaults
  temp-flipped.
- **#11bo — the tier2 MODE-3 PIXEL-RENDER reclock SHIPPED: 88/100 render-atomic
  legs in 4 flag-gated slices; 12 residuals classified.** The read-frame vein
  (#11bk/bl/bm) drained to reach the DIFFERENT subsystem — the pixel fetcher, not
  the read laws. Root cause: the tier2 deferred write path advances the render to
  the write's leading edge (cc+0) BEFORE the eager `commit_eff`, landing a
  mid-mode-3 SCY/SCX/BGP/OBP/LCDC change into the pixel view `eff` 4 dots EARLY of
  the render's cc+4-calibrated fetch grid (`dmgpalette`/`scy`/`bgtiledata`/
  `bgtilemap`/`m3_lcdc_*` boundary-column shift). SEPARABLE from the read laws (they
  sample ARCH `self.scy`/`self.lcdc`; the render samples `eff`), so each slice is
  render-only — CGB two-bin 291/291 IDENTICAL SET, mooneye 91/91 ON+OFF, byte-
  identical OFF. **Mech1** SCY/palette (FF42/FF47-49): SCX's `dots=3` survive-defer
  (`staged_pending` skip) — dmgpalette 6 + scy 26 = **+32** (`cef8471`,
  `tier2_dmg_m3_render_scy_palette_passes`). **Mech2** LCDC BG-addr (FF40 bit3/4):
  a SPLIT view — `eff.lcdc` stays eager (window bit5 abort/reenable laws + FF41
  reads + OBJ-enable/length), a new `eff.render_lcdc` read only by the BG fetcher
  lags `RENDER_LCDC_DELAY=3` (a full LCDC defer regressed 5 window pins — #11bb
  "LCDC +4 net-neg") — bgtiledata 21 + bgtilemap 26 + m3_lcdc_tile_sel = **+48**
  (`c26efdf`, `tier2_dmg_m3_render_lcdc_passes`). **Mech3** SCX double-speed
  (FF43 DS): dots=**2** not 3 (DS M-cycle = 2 dots, offset halves) — the single
  value straddling both the render AND `late_scx4`'s DS read law — scx_during_m3_ds
  **+5** (`380cbcd`, `tier2_dmg_m3_render_scx_ds_passes`). **Mech4** LCDC bit0
  BG-priority (mixer): reads `render_lcdc` too (no length coupling) — m3_lcdc_bg_en
  ×2 + bgoff_bgon = **+3** (`e1cd243`, `tier2_dmg_m3_render_bg_priority_passes`).
  Pixel two-bin `gambatte_pixel_probe` (flag-on framebuffer ↔ reference PNG via the
  suite comparator): OFF 100/100, ON 88/100. **12 residuals CLASSIFIED (not
  shipped):** WX window-trigger/length 5 (m3_wx_5/6, m3_window_timing×2, late_wx_ds
  — the WX-match dot IS the window activation = length; a swept defer broke
  `tier2_window_late_wx_uncatch`) + palette OR-quirk 3 (m3_bgp/obp — the DMG
  "old|new for one dot" boundary; no palette-dots value fixes both dmgpalette AND
  the OR-quirk column) + window-enable/length 2 (m3_lcdc_win_en_multiple) + OBJ-
  enable/length 1 (m3_lcdc_obj_en, bit1 gates the sprite fetch) + sprite-penalty
  grid 1 (scy_spx08_2). All are the render-length / sprite-grid class that lands
  WITH the length port. Map: `measurements/dmg-m3-render-reclock-2026-07-04.md`.
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
- **#11bl — DMG power-on boot-frame read law SHIPPED (+20 flag-on).** The 21
  `poweron_*` rows #11bj parked as "the C0 boot-DIV read-frame CHAIN … atomic"
  are the SAME read-frame decoupling as #11bk one frame earlier (boot). The
  tier2 deferred read of STAT (FF41) / OAM / VRAM / LY on the pristine boot
  hand-off frame samples cc+0, 4 dots before production's cc+4; the
  NOP-sled-timed `poweron_*` reads land exactly 4 dots before a boot mode
  transition, returning the pre-transition value (`poweron_stat_007` reads m0 at
  ly0 dot0, want m2 — the true cc+4 position dot4 is past the line-start hold).
  `Ppu::boot_read` restores the value at the read's true (cc+4) position — the
  current (line, dot) advanced +4 dots on the 154×456 grid (STAT mode +
  LYC-coincidence, OAM/VRAM mode locks, LY all re-derived there; the line-153
  LY=0 quirk via `self.ly`). ONE offset fits all four registers. **CRUX: the
  boot READ is SEPARABLE from the `+4` boot DIV** (PPU sample vs timer counter,
  different subsystems) — `tier2_boot_div_passes` HELD, so it SHIPS, not parks.
  verdict-only, `tier2`+`!is_cgb`+`frame_count<=2`+`!lcd_regs_written` scoped
  (the last isolates poweron's pristine-frame reads from every other early
  reader — `lcdon`/`oam_read`/`sprite`/`win`/kernel/halt all write a PPU
  register first) → production/CGB byte-identical, ZERO of 513 regressed. gbmicro
  DMG flag-on 425→445; pin `tier2_dmg_poweron_passes`. Map:
  `measurements/dmg-poweron-boot-read-2026-07-04.md`.
- **#11bm — the 8 non-window DMG-OCR singles CHARACTERIZED; +1 read-frame leg
  SHIPPED, 60 measured parks.** The #11bi "8 singles" was an UNDER-count (same
  want-regex miss as the window 29→62): a fresh census + `classify_dmg` finds
  **61 SameBoy-pass** non-window flip-blockers across the 7 categories. Only ONE
  clean read-frame leg remained (the vein #11bk/#11bl mostly drained):
  `enable_display/ly0_m0irq_scx1_1` (glitch-line mode-0 co-instant FF0F read).
  A DI/IE=0 poll reading EXACTLY on the recorded mode-0 flip dot
  (slopgb `dot253 == flip_dot253` == SameBoy cfl257): SameBoy orders the read
  BEFORE the STAT rise at that shared instant → E0; slopgb's whole-dot frame
  folds the rise first → E2. `Ppu::ff0f_dmg_m0_coincident_mask` masks IF_STAT
  off the verdict at `dot == flip_dot` (EXACT — the `_2`/`scx0_2` siblings read
  past the flip, keep E2). **Verdict-only — the rise/dispatch never moves**, so
  the co-located `int_hblank_halt` halt-wake grid the #11ad park cited as the
  atomicity is untouched (the #11bk/#11bl decoupling); CORRECTS the #11ad
  `tier2_glitch_m0irq_dispatch_passes` "DMG byte-identical floor". `tier2`+
  `!is_cgb`+`glitch_line`+SS scoped. +1 full-DMG two-bin / 0 dropped; gbmicro
  445 held; pin `tier2_dmg_m0_coincident_passes`. The 60 parks (measured):
  tima 45 + serial 1 = S6 timer/serial-completion (#11ai, C0-DIV refuted);
  `frame*_m0irq_count` 6 = dispatch-COUNT (cc+0 loses the mode-0 dispatch);
  sprites 2 = inverted IF lifecycle (render-reclock); the line-start STAT
  service class (m2enable 1 + lycEnable `lycwirq_stat50` 1 + miscmstatirq 1) =
  **BUILD-MEASURED dispatch-coupled** (the LYC service-clear candidate #11bn
  BUILT + two-binned = REGRESSED 38 SameBoy-passes wanting E2 from the identical
  gate=true/`lyc_interrupt_line` state; m2enable `_1`/`_2` co-temporal identical
  read state — reverted); ff40_disable 1 = LCD-disable timing;
  `ly0_late_scx7_m3stat` 2 = render-length atomic (identical read state, opposite
  want). `reclock.rs` split (→848) with the FF0F
  read-view/squash family to `ppu/stat_irq/ff0f.rs` (<1000 cap). Map:
  `measurements/dmg-ocr-singles-2026-07-04.md`.
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
  parks) + the non-window DMG-OCR singles **CHARACTERIZED #11bm (true count 61,
  not 8; +1 shipped `ly0_m0irq_scx1_1` co-instant mask, 60 measured parks —
  timer/serial-completion + dispatch-count + render-length + co-temporal)** +
  the engine 43 residual (dispatch-atomic; #11bk shipped 16 + #11bl shipped 20
  of the 79 — the `hblank_int` `if_c`/`if_d` read-frame legs and the 20
  `poweron_*` boot-read rows) + the 100 render-atomic pixel blockers + golden
  regen.
  Execute
  `docs/sameboy-port/C3-FLIP-CHECKLIST.md` top-to-bottom when §3b clears; do
  NOT flip defaults in any pushed commit. Maps:
  `measurements/dmg-window-port-2026-07-03.md` +
  `dmg-engine-set-classify-2026-07-03.md` + `pixel-classify-2026-07-03.md` +
  `dmg-hblank-if-2026-07-03.md` (#11bk) +
  `dmg-poweron-boot-read-2026-07-04.md` (#11bl) +
  `dmg-ocr-singles-2026-07-04.md` (#11bm).
- **History:** per-session port narrative in
  [`docs/sameboy-port/STATE-HISTORY.md`](docs/sameboy-port/STATE-HISTORY.md)
  (verbatim archive) and
  [`docs/hardware-state/ppu-subdot-ladder.md`](docs/hardware-state/ppu-subdot-ladder.md)
  (the measurement ladder); roadmap
  [`docs/sameboy-port/PORT-PLAN.md`](docs/sameboy-port/PORT-PLAN.md);
  per-session maps in `docs/sameboy-port/tools/measurements/`.

**Per-subsystem hardware-behavior notes — timing laws, quirks, the test ROMs that pin each, and the parked/disproven approaches not to re-chase — live in [`docs/hardware-state/`](docs/hardware-state/README.md) (one file per subsystem). Read the relevant file before touching that subsystem.** The floor-class index (classes A–H with lift conditions) is the header of `tests/gbtr/baselines/gambatte.txt`.
