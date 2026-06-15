# TDD plan — split god files (no file > 1000 lines)

Scope: **every file > 1000 lines** (the "god file" anti-pattern). Behavior-preserving
extraction only — no logic edits. Current offenders (2026-06-14, code-vs-test split):

| File | Total | Code | Inline `mod tests` |
|---|---|---|---|
| `interconnect.rs` | 4737 | ~2031 | 2032-4708 + `pcm_decay_probe` 4709-4737 |
| `ppu/mod.rs` | 3391 | ~2197 | 2198-3391 |
| `ppu/render.rs` | 3178 | ~1298 | 1299-3178 |
| `cpu/execute_tests.rs` | 2054 | — (all test, 77 #[test]) | already `#[path]` sibling |
| `apu/mod.rs` | 1855 | ~670 | 671-1855 |
| `cartridge.rs` | 1655 | ~685 | 686-1655 |
| `tests/common/png.rs` | 1071 | mixed | — |
| `tests/common/mod.rs` | 1020 | mixed | — |

**Key insight**: inline test modules are the bulk of the bloat. Externalizing them
alone drops apu→671 and cartridge→686 under the limit. The established project pattern
is `cpu/execute.rs`: `#[cfg(test)] #[path = "execute_tests.rs"] mod tests;`.

Strategy: Rust-2018 `foo.rs` + `foo/` directory submodules (the parent `foo.rs` keeps
the struct, its fields, and the load-bearing sub-dot consts; methods move to
`impl Foo` blocks in `foo/<name>.rs`). Shared `#[cfg(test)]` helpers (`ic()`,
`dmg()`, `run_to()`, …) move to the parent file as `#[cfg(test)]` free fns so every
split test mod reaches them via `super::`.

Safety net (the characterization test for every task — must stay byte-identical):
`cargo clippy --workspace --all-targets -- -D warnings` + `cargo test -p slopgb-core
--lib` + `--test mooneye` (439/439) + `--test gbtr` (baseline ratchet: unlisted-fail
= panic, stale-pass = panic). The ratchet **is** the regression oracle. Baseline
captured green at `3378864`.

```xml
<plan goal="no file over 1000 lines; behavior byte-identical; god-file rule in CLAUDE.md">
  <!-- PHASE A: externalize inline test modules (mechanical, safe) -->
  <task id="A1" model="haiku" deps="none">
    <do>apu/mod.rs: move `#[cfg(test)] mod tests` (671-1855) to apu/tests.rs; replace with `#[cfg(test)] #[path = "tests.rs"] mod tests;`.</do>
    <test>cargo test -p slopgb-core --lib apu:: passes identical count; clippy clean.</test>
    <done>apu/mod.rs <= 700 lines; apu/tests.rs holds the test mod; lib tests green.</done>
    <why>Pure verbatim move of a self-contained `mod tests` with `use super::*` — zero logic.</why>
  </task>
  <task id="A2" model="haiku" deps="none">
    <do>cartridge.rs: move `mod tests` (686-1655) to cartridge_tests.rs via `#[path]`.</do>
    <test>cargo test -p slopgb-core --lib cartridge:: identical; clippy clean.</test>
    <done>cartridge.rs <= 700 lines; cartridge_tests.rs holds the mod; lib green.</done>
    <why>Verbatim move, same pattern as cpu/execute_tests.</why>
  </task>
  <task id="A3" model="haiku" deps="none">
    <do>ppu/render.rs: move `mod tests` (1299-3178) to ppu/render_tests.rs via `#[path]` (keep the cfg(test) helper method at 279 in place).</do>
    <test>cargo test -p slopgb-core --lib ppu::render identical; clippy clean.</test>
    <done>ppu/render.rs <= 1300 lines; render_tests.rs holds the mod; lib green.</done>
    <why>Verbatim move; the `use super::*`/`use super::super::Ppu` paths resolve unchanged.</why>
  </task>
  <task id="A4" model="haiku" deps="none">
    <do>ppu/mod.rs: move `mod tests` (2198-3391) to ppu/mod_tests.rs via `#[path]` (keep the cfg(test) helper methods 2094-2117 in place).</do>
    <test>cargo test -p slopgb-core --lib ppu:: identical; clippy clean.</test>
    <done>ppu/mod.rs <= 2200 lines; mod_tests.rs holds the mod; lib green.</done>
    <why>Verbatim move with `use super::*`.</why>
  </task>
  <task id="A5" model="sonnet" deps="none">
    <do>interconnect.rs: move `mod tests` (2032-4708) AND `mod pcm_decay_probe` (4709-4737) to interconnect_tests.rs via two `#[path]` mods.</do>
    <test>cargo test -p slopgb-core --lib interconnect:: identical count; clippy clean.</test>
    <done>interconnect.rs <= 2050 lines; interconnect_tests.rs holds both mods; lib green.</done>
  </task>

  <!-- PHASE B: split remaining code into submodules (struct + consts stay in parent) -->
  <analysis id="B0" model="opus" deps="A5">
    <do>Confirm the interconnect seam: which methods move to boot/oam_dma/hdma/memory/speed; verify no field/const moves out of the parent; the `impl Bus` trait block stays one block in the parent and delegates (e.g. `stop` -> inherent `stop_impl` in speed.rs).</do>
    <done>Seam table final; each method assigned once; Bus trait kept intact.</done>
    <why>Trait impl cannot split across files; dense IRQ/DMA timing — wrong seam = pub(crate) churn.</why>
  </analysis>
  <task id="B1" model="sonnet" deps="B0">
    <do>Extract OAM-DMA engine into interconnect/oam_dma.rs (DmaSrcKind/OamDmaRun/DmaConflict/OamDmaStart enums + oam_dma_commit_pending/oam_dma_tick/oam_dma_promote_start/oam_dma_bus_capture/dma_src_kind/oam_dma_source_read/in_dma_conflict_area/dma_redirect_wram_index as `impl Interconnect`).</do>
    <test>gbtr oamdma/* + blargg oam_bug rows unchanged; lib+mooneye+gbtr ratchet green.</test>
    <done>oam_dma.rs holds the engine; interconnect.rs shrinks; suite byte-identical.</done>
  </task>
  <task id="B2" model="sonnet" deps="B1">
    <do>Extract VRAM/HDMA request engine into interconnect/hdma.rs (HdmaMode/VramDmaReq/HaltHdmaState + service_vram_dma/run_vram_dma/vram_dma_unhalt/vram_dma_source_read/hdma5_write).</do>
    <test>gbtr hdma_* + same-suite hdma rows unchanged; ratchet green.</test>
    <done>hdma.rs holds DMA-req engine; suite byte-identical.</done>
  </task>
  <task id="B3" model="sonnet" deps="B1">
    <do>Extract boot/post-boot state into interconnect/boot.rs (apply_post_boot_state/install_power_on_wram/install_boot_logo_vram + CGB_COMPAT palette consts if local).</do>
    <test>gbtr boot_* + mooneye boot rows unchanged; ratchet green.</test>
    <done>boot.rs holds post-boot install; suite byte-identical.</done>
  </task>
  <task id="B4" model="sonnet" deps="B1">
    <do>Extract memory-map routing into interconnect/memory.rs (read_no_tick/write_no_tick/io_read/io_write/wram_index/prohibited_read/prohibited_write/extra_oam_index/maybe_oam_bug + sgb_header_zero_bits free fn).</do>
    <test>gbtr + mooneye full matrix unchanged; ratchet green.</test>
    <done>memory.rs holds IO routing; interconnect.rs core = struct + sub-dot consts + tick_machine + Bus impl; suite byte-identical.</done>
  </task>
  <task id="B5" model="sonnet" deps="B1">
    <do>Extract CGB speed switch into interconnect/speed.rs (Bus::stop body -> inherent stop_impl; KEY1 tail/0x20000-pause helpers).</do>
    <test>gbtr age spsw + speedchange + tima/div STOP rows unchanged; ratchet green.</test>
    <done>speed.rs holds the stop tail; Bus::stop delegates; suite byte-identical.</done>
  </task>
  <task id="B6" model="opus" deps="A4">
    <do>Extract STAT-IRQ event machinery into ppu/stat_irq.rs (stat_events_tick + stat_ev/lyc/staging predicates, m2_pulse_fires, stage_stat_copies/flush_stat_copies/stat_ev_fresh/lyc_ev_m_fresh, stat_write_trigger_dmg/cgb, stat_line_level, refresh_cmp/legacy_level_edge).</do>
    <test>gbtr m2int/m0irq/lycm2int + gbmicrotest hblank_int/oam_int + mooneye intr_2_* + stat_irq_blocking rows unchanged; ratchet green.</test>
    <done>stat_irq.rs holds the per-source predicate engine; ppu/mod.rs shrinks; suite byte-identical.</done>
    <why>Densest interdependence in the codebase (delayed FF41/FF45 copies, per-source predicates, halt-late commits); highest drift risk.</why>
  </task>
  <task id="B7" model="sonnet" deps="B6">
    <do>Extract LYC machinery into ppu/lyc.rs (write_lyc_dmg/write_lyc_cgb, compare_ly/compare_ly_irq, lyc_cmp_held/lyc_period).</do>
    <test>gbtr age ly/ly-ncm + wilbertpol -C LY + mooneye LYC rows unchanged; ratchet green.</test>
    <done>lyc.rs holds LYC compare/write; suite byte-identical.</done>
  </task>
  <task id="B8" model="sonnet" deps="A4">
    <do>Extract OAM scan + corruption into ppu (scan via render.rs already; here move oam_bug/oam_bug_row + block predicates oam_read_blocked/oam_write_blocked/vram_read_blocked/vram_write_blocked/pal_ram_blocked into ppu/blocking.rs).</do>
    <test>gbtr oamdma/late_sp* + blargg oam_bug + mooneye sprite rows unchanged; ratchet green.</test>
    <done>blocking.rs holds access-block predicates + oam_bug; suite byte-identical.</done>
  </task>
  <task id="B9" model="sonnet" deps="A3">
    <do>Extract sprite fetcher into ppu/render/sprite.rs (SpritePixel, sprite_penalty, fetch_sprite, output_pixel mix, cgb_color, oam_scan/oam_scan_step/oam_scan_entry/mgb_dma_freeze_glitch_entry).</do>
    <test>gbtr sprites/* + mooneye intr_2_mode0_timing_sprites + mealybug obj photos unchanged; ratchet green.</test>
    <done>sprite.rs holds OBJ fetch+mix+scan; render.rs shrinks; suite byte-identical.</done>
  </task>
  <task id="B10" model="sonnet" deps="B9">
    <do>Extract window machine into ppu/render/window.rs (window_trigger_step, window_abort).</do>
    <test>gbtr m3_wx_*/window/m0enable + mealybug m3_window_timing* rows unchanged; ratchet green.</test>
    <done>window.rs holds window logic; suite byte-identical.</done>
  </task>
  <task id="B11" model="sonnet" deps="B9">
    <do>Extract mode-0/flip + fetcher grid into ppu/render/mode0.rs (m0_flip_events/m0_unflip, advance_lx, fetcher_step/push_allowed/push_bg_row/bg_tile_addr, stall_tick).</do>
    <test>gbtr bgtile*/m0enable + gbmicrotest hblank_int/int_hblank + mooneye intr_2_mode0_timing rows unchanged; ratchet green.</test>
    <done>mode0.rs holds flip/IRQ + BG fetch; render.rs core = Render struct + render_init/render_step driver; suite byte-identical.</done>
  </task>

  <!-- PHASE C: split oversized test files by category (shared helpers -> parent cfg(test) fns) -->
  <task id="C1" model="sonnet" deps="A5">
    <do>Split interconnect_tests.rs (~2700) into category files (dma/hdma, boot/post-boot, io/memory-map, stat-mode/sub-dot, speed/stop) each `#[cfg(test)] #[path] mod`; move shared helpers (test_rom/ic/ic_cgb_mode/ticks) to interconnect.rs as `#[cfg(test)]` fns.</do>
    <test>same total test count, all green; clippy clean.</test>
    <done>each interconnect test file <= 1000 lines; lib green identical count.</done>
  </task>
  <task id="C2" model="sonnet" deps="A4">
    <do>Split ppu/mod_tests.rs (~1190) into category files (stat-irq, lyc, blocking/access, misc) if > 1000; shared helpers (dmg/cgb/tick_n/run_to) to ppu/mod.rs as cfg(test) fns.</do>
    <test>same count, green; clippy clean.</test>
    <done>each ppu test file <= 1000 lines; lib green.</done>
  </task>
  <task id="C3" model="sonnet" deps="A3">
    <do>Split ppu/render_tests.rs (~1880) into category files (sprite, window, mode0/fetch, mealybug) ; shared helpers (run_to/render_line/px/dmg_on/set_tile_row) reachable via super.</do>
    <test>same count, green; clippy clean.</test>
    <done>each render test file <= 1000 lines; lib green.</done>
  </task>
  <task id="C4" model="sonnet" deps="none">
    <do>Split cpu/execute_tests.rs (2054, 77 tests) into category files (load, alu, control-flow/jumps, bit-ops, misc/timing) via `#[path]` mods; shared TestBus/helpers (cpu/bus/fl) to a shared cfg(test) location reachable by all.</do>
    <test>same 77 tests green; clippy clean.</test>
    <done>each cpu test file <= 1000 lines; lib green.</done>
  </task>
  <task id="C5" model="sonnet" deps="none">
    <do>Split tests/common/png.rs (1071) and tests/common/mod.rs (1020) into focused submodules under tests/common/ (e.g. png decode vs compare; harness vs suffix-mapping).</do>
    <test>cargo test -p slopgb-core --test mooneye --test gbtr green; clippy clean.</test>
    <done>each common file <= 1000 lines; suites green.</done>
  </task>

  <!-- PHASE D: docs + the rule -->
  <task id="D1" model="haiku" deps="B1,B2,B3,B4,B5,B6,B7,B8,B9,B10,B11">
    <do>Add a //! header to each new submodule (what it owns, oracle suite); add a module-ownership table to docs/ARCHITECTURE.md.</do>
    <test>cargo doc -p slopgb-core builds; clippy clean; each new file's //! names its oracle.</test>
    <done>every submodule self-describes ownership; ARCHITECTURE.md indexes them.</done>
    <why>Mechanical doc relocation, content already exists in comments.</why>
  </task>
  <task id="D2" model="sonnet" deps="D1">
    <do>Update CLAUDE.md: add a paired do/don't rule forbidding god files (no file > 1000 lines; externalize tests to `#[path]` siblings; split code into submodules) + cite docs/tdd-split-plan.md; run /clean-docs.</do>
    <test>/clean-docs passes; CLAUDE.md links resolve; grep finds the rule.</test>
    <done>CLAUDE.md forbids the anti-pattern with an enforceable line limit.</done>
  </task>
</plan>
```

Summary: **23 tasks (4 haiku, 16 sonnet, 1 opus analysis, 2 opus) across 4 phases** —
A externalize tests (safe), B split code into submodules, C split oversized test files,
D docs + the god-file rule. Critical path: A5 → B0 → B4 → C1 (interconnect spine) and
A4 → B6 → C2 (ppu STAT-IRQ, the densest drift risk). Every task gated on the gbtr
ratchet staying byte-identical green.
