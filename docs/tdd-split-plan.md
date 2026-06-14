# TDD plan — split large files + improve bus-factor

Scope: 3 worst files — `interconnect.rs` (4125), `ppu/mod.rs` (3276), `ppu/render.rs` (3147).
Strategy: behavior-preserving extraction into Rust submodules (`impl Foo` split across files; struct + fields stay in `mod.rs`). No logic edits.
Safety net (the characterization test for every refactor task): all must stay green & byte-identical —
`cargo clippy --workspace --all-targets -- -D warnings` + `cargo test -p slopgb-core --lib` + `--test mooneye` (439/439) + `--test gbtr` (baseline ratchet: unlisted-fail = panic, stale-pass = panic). The ratchet is the regression oracle.

```xml
<plan goal="split 3 monolith files into cohesive submodules; raise bus-factor via seam map + module docs">
  <analysis id="1" model="opus" deps="none">
    <do>Map exact cut lines: for each of interconnect/ppu/render, list which methods+helpers move to which new submodule, what stays in mod.rs, names + visibility. Record as the refactor contract.</do>
    <done>Seam table written (per file: submodule name -> method list -> deps); every method assigned exactly once; no field moves out of the owning struct.</done>
    <why>Cross-cutting architecture call in dense timing code; wrong seam = circular pub(crate) churn or behavior drift. Needs judgment, no test.</why>
  </analysis>
  <analysis id="2" model="opus" deps="1">
    <do>Gap-check the safety net: for each planned seam, confirm a test exercises it; flag any behavior crossing a cut with NO guarding test.</do>
    <done>Coverage map per seam; uncovered seams listed as "add characterization test first" before their extraction task runs.</done>
    <why>Refactor safety depends entirely on the net catching drift; silent gaps in dense IRQ/DMA timing are exactly where extraction breaks.</why>
  </analysis>

  <task id="3" model="sonnet" deps="1,2">
    <do>Extract OAM-DMA engine (DmaSrcKind/OamDmaRun/DmaConflict/OamDmaStart + bus-conflict methods) from interconnect.rs into interconnect/oam_dma.rs as a second impl Interconnect block.</do>
    <test>Pre-extract run captures green; post-extract re-run must match: gbtr oamdma/* rows unchanged, lib tests + mooneye green, clippy clean.</test>
    <done>oam_dma.rs holds the engine; interconnect.rs shrinks by that block; full suite byte-identical green.</done>
  </task>
  <task id="4" model="sonnet" deps="3">
    <do>Extract HDMA/VRAM-DMA request engine (HdmaMode/VramDmaReq/HaltHdmaState + vram_dma_req/halt_hdma methods) into interconnect/hdma.rs.</do>
    <test>gbtr hdma_* + same-suite hdma_mode0 rows unchanged; suite green post-extract vs pre-extract capture.</test>
    <done>hdma.rs holds DMA-req engine; suite green unchanged.</done>
  </task>
  <task id="5" model="sonnet" deps="3">
    <do>Extract CGB speed switch (Bus::stop / KEY1 tail / 0x20000-pause) into interconnect/speed.rs.</do>
    <test>gbtr age spsw + tima/div STOP pair rows unchanged; mooneye green; suite matches capture.</test>
    <done>speed.rs holds stop tail; suite green unchanged.</done>
  </task>
  <task id="6" model="sonnet" deps="3">
    <do>Extract CGB register routing (BCPS/BCPD/OCPS/OCPD palette RAM, VBK/SVBK, OPRI, FF72-77) into interconnect/cgb_regs.rs.</do>
    <test>gbtr CGB palette + boot-palette rows unchanged; suite matches capture.</test>
    <done>cgb_regs.rs holds CGB IO routing; interconnect.rs core = memory map + IF/IE + Bus impl only; suite green.</done>
  </task>

  <task id="7" model="opus" deps="1,2">
    <do>Extract STAT-IRQ event machinery (stat_events_tick + MStatIrqEvent/LycIrq port: stat_ev/stat_lyc_ev staging, predicates, FF41 write triggers) from ppu/mod.rs into ppu/stat_irq.rs.</do>
    <test>gbtr m2int/m0irq/lycm2int + gbmicrotest hblank_int/oam_int + mooneye intr_2_* + stat_irq_blocking rows ALL unchanged vs pre-extract capture.</test>
    <done>stat_irq.rs holds the per-source predicate engine; ppu/mod.rs shrinks; suite byte-identical green.</done>
    <why>Densest interdependence in the codebase (delayed FF41/FF45 copies, per-source predicates, halt-late commits); highest drift risk of any extraction.</why>
  </task>
  <task id="8" model="sonnet" deps="7">
    <do>Extract LYC machinery (write_lyc_dmg/cgb, lycRegChange port, held-compare tables, cmp_irq) into ppu/lyc.rs.</do>
    <test>gbtr age ly/ly-ncm + wilbertpol -C LY rows + mooneye LYC rows unchanged vs capture.</test>
    <done>lyc.rs holds LYC compare/write logic; suite green unchanged.</done>
  </task>
  <task id="9" model="sonnet" deps="1,2">
    <do>Extract OAM scan (dot-serial scan_latch, oam_dma_active disconnect) + OAM-bug (oam_bug + patterns) into ppu/oam.rs.</do>
    <test>gbtr oamdma/late_sp* + blargg oam_bug/* + mooneye sprite rows unchanged vs capture.</test>
    <done>oam.rs holds scan + corruption; suite green unchanged.</done>
  </task>

  <task id="10" model="sonnet" deps="1,2">
    <do>Extract sprite fetcher (SpritePixel, obj_fetch_base, sprite FetchPhase steps, stall penalties) from render.rs into ppu/render/sprite.rs.</do>
    <test>gbtr sprites/* + mooneye intr_2_mode0_timing_sprites + mealybug obj photos unchanged vs capture.</test>
    <done>sprite.rs holds OBJ fetch+mix; render.rs shrinks; suite green unchanged.</done>
  </task>
  <task id="11" model="sonnet" deps="10">
    <do>Extract window machine (WX comparator, win_line/winYPos, window_abort, win_start_pending, WY weMaster sampling) into ppu/render/window.rs.</do>
    <test>gbtr m3_wx_*/window/m0enable + mealybug m3_window_timing* rows unchanged vs capture.</test>
    <done>window.rs holds window logic; suite green unchanged.</done>
  </task>
  <task id="12" model="sonnet" deps="10">
    <do>Extract mode-0 end-of-line grid (m0_flip_events/m0_unflip, pipe-end projection, SCX hunt_idx) into ppu/render/mode0.rs.</do>
    <test>gbtr bgtile*/m0enable + gbmicrotest hblank_int/int_hblank + mooneye intr_2_mode0_timing rows unchanged vs capture.</test>
    <done>mode0.rs holds flip/IRQ-dot logic; render.rs core = BG fetcher + FIFO only; suite green unchanged.</done>
  </task>

  <task id="13" model="haiku" deps="3,4,5,6,7,8,9,10,11,12">
    <do>Add a //! module-doc header to each new submodule: what it owns, which FROZEN contract it touches, oracle suite. Move the relevant CLAUDE.md state bullet's pointer to cite the new file.</do>
    <test>cargo doc -p slopgb-core builds; clippy clean; each new file's //! names its oracle suite (grep check).</test>
    <done>Every extracted submodule self-describes its ownership + oracle; suite green.</done>
    <why>Mechanical doc-writing, content already exists in CLAUDE.md/comments — just relocate to the code.</why>
  </task>
  <task id="14" model="sonnet" deps="3,4,5,6,7,8,9,10,11,12">
    <do>Add a module-ownership map (file -> owns -> oracle suite -> FROZEN contracts) to docs/ARCHITECTURE.md and update CLAUDE.md "Read before touching core" pointer to it.</do>
    <test>ARCHITECTURE.md lists all new submodules; /clean-docs passes; CLAUDE.md links resolve.</test>
    <done>New contributor can locate any subsystem from one table; bus-factor raised from prose-scattered to indexed.</done>
  </task>
</plan>
```

Out of scope (already decent): `apu/mod.rs` (1855) already splits into envelope/length/noise/pulse/wave submodules; `cartridge.rs` (1655) — optional per-mapper split (MBC1/3/5) deferrable, not a monolith risk.

Summary: **14 tasks (1 haiku, 10 sonnet, 3 opus)**. Critical path: 1 → 2 → 7 → 8 → 13 → 14. Opus reserved for seam design (1), safety-net gap analysis (2), and STAT-IRQ extraction (7, densest drift risk). Every refactor task is gated on the suite staying byte-identical green — the ratchet harness *is* the test.
