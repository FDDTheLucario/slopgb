//! OAM / VRAM / sprite / mode-2-3 pinned-behavior tests.

use super::*;

/// The SPRITE-line analog of the kernel pair, on the flag-on path. A
/// sprite-laden line extends mode 3, shifting the visible mode→0 boundary; the
/// `vis_early` back-date for sprite/window lines (`lead + 4`, vs bare's
/// `lead + 3`) lands it at SameBoy's frame, so the two equal-`ldh` reads
/// straddle it: `10spritesPrLine_m3stat_1` reads mode 3 (out3) and `_m3stat_2`
/// reads mode 0 (out0) — the same out3/out0 split the kernel pair shows on a
/// bare line. Whole-dot production reads BOTH as mode 3 (the baselined floor);
/// this lifts 40 such sprite `m3stat_2` rows flag-on with zero regression.
/// Flag-OFF (production) is unchanged.
#[test]
fn sprite_kernel_pair_matches_sameboy_target() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "sprite_kernel_pair",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/sprites/10spritesPrLine_m3stat_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/sprites/10spritesPrLine_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let path = root.join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for model in [Model::Dmg, Model::Cgb] {
            let mut gb = harness::boot(&rom, model);
            gb.set_leading_edge_reads(true);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (flag-on): {e}")
            });
        }
    }
}

/// Mooneye `intr_2_mode0_timing_sprites` passes on the FULL deferred reclock
/// (`set_tier2_reclock`) for BOTH models. The test resolves each sprite
/// config's mode-3 length to whole M-cycles; our `proj` tracks a finer per-X
/// staircase that the cc+4 read quantizes back into the right buckets
/// (production passes every config), but the cc+0 leading-edge read exposes
/// the sub-M-cycle dispatch phase. The fix (`ppu/render/mode0.rs`) snaps the
/// sprite-line dispatch + `vis_early` to the CPU read grid (dot ≡ 0 mod 4)
/// with `early_lead = 0`, reproducing the cc+4 quantization — verified to pass
/// all 105 configs both models while the bare kernel pair, `intr_2_mode0_timing`
/// and `int_hblank_halt` keep passing (4/4 thesis triad). Production (flag-off)
/// is byte-identical: the snap is gated on `tier2_reclock` and `vis_early` is
/// never set without `leading_edge_reads`. SameBoy passes this ROM
/// (header `pass: DMG..AGS`).
#[test]
fn tier2_intr_2_sprites_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_intr_2_sprites",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rel = "mooneye-test-suite/acceptance/ppu/intr_2_mode0_timing_sprites.gb";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = harness::boot(&rom, model);
        gb.set_tier2_reclock(true);
        harness::run_until_breakpoint(&mut gb, 30_000_000)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        harness::check_fib(&gb)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
    }
}

/// Mooneye `intr_2_mode3_timing` on the deferred reclock.
/// The test counts STAT-read polls from the mode-2 IRQ to the mode-3 read;
/// the CPU-visible 2→3 entry boundary (`mode3_entry_dot`) was back-dated 4
/// dots (80) for the leading-edge-only frame, but the Tier-2 deferred read
/// samples the entry at the trailing frame, so 80 made it see mode 3 one
/// M-cycle early (`test_iter 2` → 1 poll, want 2). Restoring the flag-off 84
/// under `tier2_reclock` passes it both models. Production (flag-off) is
/// byte-identical — the tier2 branch only fires on the reclock path.
#[test]
fn tier2_intr_2_mode3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_intr_2_mode3",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rel = "mooneye-test-suite/acceptance/ppu/intr_2_mode3_timing.gb";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = harness::boot(&rom, model);
        gb.set_tier2_reclock(true);
        harness::run_until_breakpoint(&mut gb, 30_000_000)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        harness::check_fib(&gb)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
    }
}

/// The per-ISR read-POSITION PEEK. The first clean
/// read-position-decoupled slice: the double-speed OAM-STAT-ISR
/// (`m2int`) FF41 mode read lands +4 dots before SameBoy's cfl, so slopgb's
/// leading-edge read sees mode 3 (`got=3`) where SameBoy — reading 4 dots later,
/// past its bare mode-3 exit — sees mode 0 (`want=0`). The peek
/// (`stat_irq.rs::vis_mode_read`, armed by `interconnect.rs::dispatch_retime`
/// via `Ppu::read_carried`) shifts ONLY that read's VERDICT to `dot + off < SBex`
/// (`off` = the per-source offset, `SBex = 257 + SCX&7 + ds` the SameBoy bare
/// exit) — a transient sample, NOT a machine advance — so the counter-pinned
/// dispatch dot and IF delivery stay put (mooneye flag-on 91/91). SCOPED to the
/// carried read (`read_carried` one-shot), native mode 3 (excludes the m0stat/
/// m2stat/enable reads that probe a different boundary), and a non-window
/// (`!wy_trig_sb`) non-sprite bare line (excludes the co-temporal `late_disable`
/// render-length A/B pair). +6/−0 full-CGB two-bin; production (flag-off)
/// byte-identical.
#[test]
fn tier2_m2int_m3stat_ds_readpos_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m2int_m3stat_ds_readpos",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    for rel in [
        "gambatte/m2int_m3stat/m2int_m3stat_ds_2_cgb04c_out0.gbc",
        "gambatte/m2int_m3stat/scx/m2int_scx2_m3stat_ds_2_cgb04c_out0.gbc",
        "gambatte/m2int_m3stat/scx/m2int_scx8_m3stat_ds_2_cgb04c_out0.gbc",
        "gambatte/speedchange/m2int_m3stat_lcdoffds_2_cgb04c_out0.gbc",
    ] {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out0 (tier2 flag-on): {e}"));
    }
    // The full per-read carry also converges the polled post-DMA FF41
    // mode-3 reads (`off = 4` = the leading-edge default): gdma/hdma_cycles _ds_2
    // want 0. These are the +2 the global law adds over the carried-only peek.
    for rel in [
        "gambatte/dma/gdma_cycles_long_ds_2_cgb04c_out0.gbc",
        "gambatte/dma/hdma_cycles_ds_2_cgb04c_out0.gbc",
    ] {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out0 (tier2 flag-on): {e}"));
    }
    // The m0stat READ-FRAME slice: the m2int mode-2 OAM ISR line-start
    // mode0→2 flip peek (dot ≥ 2 → mode 2), +1/−0 (`m2int_m0stat_ds_2` wants 2).
    let rel = "gambatte/m2int_m0stat/m2int_m0stat_ds_2_cgb04c_out2.gbc";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
    run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
    check_hex_screen(gb.frame(), "2", true)
        .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out2 (tier2 flag-on): {e}"));
    // The same carried-read frame at the mode2→3 ENTRY (slopgb dot
    // 84): the DS mode-2 ISR pair straddles it at dots 80/82 (+2 carry →
    // 82/84, want 2/3); the entry is SCX-independent (`m2int_scx4_m2stat_ds`).
    for (rel, expect) in [
        ("gambatte/m2int_m2stat/m2int_m2stat_ds_1_cgb04c_out2.gbc", "2"),
        ("gambatte/m2int_m2stat/m2int_m2stat_ds_2_cgb04c_out3.gbc", "3"),
        (
            "gambatte/m2int_m2stat/m2int_scx4_m2stat_ds_2_cgb04c_out3.gbc",
            "3",
        ),
    ] {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The read-observer eighth-grid: the bare-line
/// `m2int_m3stat` reads converge flag-on when the mode-0 dispatch lands at cc2 of
/// its M-cycle (dot ≡ 1 mod 4 ⇔ SCX&7 ∈ {3,7}). A leading-edge FF41 read samples
/// at its M-cycle START and observes the flip at cc+2, so a same-M-cycle read
/// should see mode 0; the cc2 dispatch commits one dot past the start, so
/// `vis_early` is anticipated 1 dot (`early_lead = 1`) to that start
/// (`ppu/render/mode0.rs`). `m2int_scx3_m3stat_2` (DMG+CGB, dispatch 257, read
/// 256) and `m2int_nobg_scx7_m3stat_2` (CGB, dispatch 261, read 260) read mode 0
/// (out0); SameBoy reads mode 0 in the same M-cycle. cc1/cc3/cc4 keep el=0 so the
/// kernel (`m2int@252` dispatch 254 ≡2) and `lcdon` hold; the IRQ dispatch keys
/// on `line_render_done`, not `vis_early`, so it is untouched. Restricted to
/// window-free bare lines (`!wy_latch`), so the `window/late_disable_*` read-
/// collapse A/B pairs (both SameBoy-passing, slopgb renders one digit) are not
/// disturbed. Production (flag-off) byte-identical — `vis_early` never set there.
#[test]
fn tier2_m2int_m3stat_scx3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m2int_m3stat",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expect, models) — scx3 both models, nobg_scx7 CGB-only (cgb04c tag).
    let targets: [(&str, &str, &[Model]); 2] = [
        (
            "gambatte/m2int_m3stat/scx/m2int_scx3_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Dmg, Model::Cgb],
        ),
        (
            "gambatte/m2int_m3stat/nobg/m2int_nobg_scx7_m3stat_2_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// The read-observer accessibility coupling: the
/// OAM/VRAM read-accessibility unblock COINCIDES with the visible mode→0 flip
/// (`vis_early`) on SameBoy, not with the render-done dispatch (`line_render_done`)
/// one dot later. The deferred cc+0 read otherwise sees mode 0 yet OAM/VRAM still
/// locked, rendering "3" where SameBoy reads accessible (out0). The fix releases
/// `oam_read_blocked`/`vram_read_blocked` on `vis_early` under Tier-2
/// (`ppu/blocking.rs`). `vram_m3/postread_scx3_2` (DMG+CGB) and
/// `oam_access/postread_scx3_2` (CGB; the DMG leg is `xout1`-exempt) all read the
/// SCX&7=3 cc2 boundary at the M-cycle the visible flip lands. Production
/// (flag-off) byte-identical — `vis_early` is never set there. The `scx2`/`scx5`
/// siblings (the read lands ON the boundary M-cycle, blocked by the cc+2-MID
/// `m0_access_edge` stamp `vis_early` cannot release) are resolved separately by
/// `tier2_oam_vram_postread_scx2_scx5_passes` (the boundary-coincident release).
#[test]
fn tier2_oam_vram_postread_scx3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_vram_postread_scx3",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expect, models) — vram_m3 both models, oam_access CGB-only (DMG xout1).
    let targets: [(&str, &str, &[Model]); 2] = [
        (
            "gambatte/vram_m3/postread_scx3_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Dmg, Model::Cgb],
        ),
        (
            "gambatte/oam_access/postread_scx3_2_dmg08_xout1_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// The read-observer accessibility coupling, WRITE
/// side: the OAM/VRAM write-unblock at the mode3→0 boundary coincides with the
/// visible mode→0 flip (`vis_early`) on SameBoy, one dot before the render-done
/// dispatch (`line_render_done`). The deferred cc+0 write at the SCX&7=3 boundary's
/// M-cycle (slopgb dot 256 / SameBoy cfl 260) otherwise stays blocked
/// (`!line_render_done`) where SameBoy lands it (`blk=0`). The fix releases
/// `oam_write_blocked`/`vram_write_blocked` on `vis_early` under Tier-2, excluding
/// glitch lines (`write_unblocked_early`) so `lcdon_write_timing-GS` (the
/// line-start dots 80-83 gap) is untouched. `oam_access/postwrite_2_scx3` (out1)
/// and `vramw_m3end/vramw_m3end_scx3_5` (out3), both DMG+CGB. Production (flag-off)
/// byte-identical — `vis_early` is never set there.
#[test]
fn tier2_oam_vram_postwrite_scx3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_vram_postwrite_scx3",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str, &[Model]); 2] = [
        (
            "gambatte/oam_access/postwrite_2_scx3_dmg08_cgb04c_out1.gbc",
            "1",
            &[Model::Dmg, Model::Cgb],
        ),
        (
            "gambatte/vramw_m3end/vramw_m3end_scx3_5_dmg08_cgb04c_out3.gbc",
            "3",
            &[Model::Dmg, Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// The read-observer accessibility coupling, the
/// BOUNDARY-COINCIDENT release: the `scx2`/`scx5` siblings the scx3 pin left
/// "floored". Their deferred cc+0 OAM/VRAM read lands on the EXACT dot
/// `line_render_done` fires (the unblock M-cycle), where the production cc+2-MID
/// `m0_access_edge` stamp still reports the second-half unblock as blocked (mode 3)
/// — but SameBoy unblocks AT the boundary (reads accessible, out0). The `_1`
/// sibling reads 4 dots earlier (a different M-cycle, no stamp) and stays blocked,
/// so releasing only the boundary M-cycle's stamp is a clean separation (full-CGB
/// two-bin +4/−0 single speed). The fix pushes the M0Access edge to phase 0 under
/// Tier-2 single speed (`render/mode0.rs` `access_lead`); double speed is excluded
/// (the stamp gates the DS VRAM-WRITE path too — `vramw_m3end_scx5_ds` — the DS
/// read grid is its own separate reclock). Production (flag-off) byte-identical —
/// `bare_flip`/`tier2_reclock` never release it there.
#[test]
fn tier2_oam_vram_postread_scx2_scx5_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_vram_postread_scx2_scx5",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str, &[Model]); 4] = [
        (
            "gambatte/vram_m3/postread_scx2_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
        (
            "gambatte/vram_m3/postread_scx5_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
        (
            "gambatte/oam_access/postread_scx2_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
        (
            "gambatte/oam_access/postread_scx5_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// The render mode-3 LENGTH port, the DS line-END OAM-read
/// release. Under CGB double speed SameBoy releases the mode-3 OAM read-lock one
/// cycle later than single speed: it SKIPS the `if (!cgb_double_speed)` early
/// unblock (`display.c:2104-2111`) and drops through to `:2118`, which lands the
/// deferred cc+0 read's unblock at slopgb dot `254 + SCX&7`. slopgb's production
/// block ran to `line_render_done` (~2 dots later), so `oam_access/postread_ds_2`
/// (`ly135 dot254`, SameBoy accessible) read "3" (blocked) while its `_1` sibling
/// (dot252, still blocked) passed. The fix releases OAM reads at that anchor on
/// bare non-sprite non-window non-glitch DS lines (`ppu/blocking.rs::
/// ds_lineend_read_open`). OAM-only: the VRAM twin (`vram_m3/postread_ds_2`) is
/// co-temporal with the `vramw_m3end_ds_2` write-readback at the same dot254 (the
/// vramw write costs a CPU M-cycle SameBoy spreads across the read but slopgb's
/// deferred frame collapses), so a VRAM release is an A/B swap — the VRAM DS read
/// grid is a separate parked reclock. Full-CGB two-bin flag-on +1/−0; production
/// (flag-off) byte-identical (`tier2_reclock`/`ds` gated). The `_1` sibling is
/// asserted below as the regression guard.
#[test]
fn tier2_oam_postread_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_postread_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 2] = [
        // The fix: DS line-end OAM read at dot254 reads accessible (out0).
        ("gambatte/oam_access/postread_ds_2_cgb04c_out0.gbc", "0"),
        // Regression guard: the `_1` read (dot252) is still blocked (out3).
        ("gambatte/oam_access/postread_ds_1_cgb04c_out3.gbc", "3"),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// CGB lcd-offset, the line-start OAM-read window:
/// on CGB single-speed SameBoy keeps `oam_read_blocked = false` for the first few
/// T-cycles of each visible line (`display.c:1805-1810`: the mode-0/HBlank tail
/// runs 2+1 cycles before the mode-2 OAM lock engages at state 7). The lcd-offset
/// shifts the `oam_access/preread_lcdoffset1_1` deferred read into that window
/// (slopgb `ly2 dot2` vs SameBoy `ly2 cfl0 blk=0`), where slopgb — locking OAM
/// from dot 0 — read "3" (blocked) instead of out0 (accessible). The fix releases
/// `oam_read_blocked` for dots `1..CGB_LINESTART_OAM_OPEN` on CGB single-speed
/// under Tier-2 (`ppu/blocking.rs::cgb_linestart_oam_open`). CGB-only, single-
/// speed (the `_ds_` siblings are handled separately, the DMG base reads in real
/// mode-0 already). Production (flag-off) byte-identical — the window is never
/// open there.
///
/// The window EXCLUDES dot 0: the BASE `oam_access/preread_2` reads
/// `ly2 dot0` and wants BLOCKED (out3 — SameBoy's mode-2 OAM lock has engaged by
/// then; SBMODE `ly2 cfl0 dc8 vis=2`), while the lcd-offset variant's read is
/// shifted off the line start to `dot2`. Opening dots 0-3 served the offset read
/// but wrongly opened the base's dot-0 read; opening only dots 1-3 separates them
/// (full-CGB two-bin flag-on +1/−0, the lcd-offset pin held). Both rows asserted
/// below.
#[test]
fn tier2_oam_preread_lcdoffset1_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_preread_lcdoffset1",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expect) — the lcd-offset variant reads accessible (dot2, open), the
    // base reads blocked (dot0, excluded from the window).
    let targets: [(&str, &str); 2] = [
        (
            "gambatte/oam_access/preread_lcdoffset1_1_cgb04c_out0.gbc",
            "0",
        ),
        ("gambatte/oam_access/preread_2_dmg08_cgb04c_out3.gbc", "3"),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The mode-3 render reclock, mechanism 1 (SCY/palette): the pure-render
/// mid-mode-3 registers (SCY FF42, BGP/OBP FF47-FF49) take SCX's +4 render-frame
/// defer (dots=3) on the tier2 deferred write path. The deferred clock advances
/// the machine to the write's leading edge (cc+0) before the write; the eager
/// `commit_eff` there landed the value 4 dots EARLY of the render's
/// cc+4-calibrated fetch grid, so the pixel pipeline sampled the new SCY/palette
/// too soon (the `dmgpalette_during_m3` / `scy_during_m3` pixel-reference
/// flip-blockers). Staging 3 dots lets the strobe re-commit at the render frame
/// (the `regs.rs` `staged_pending` survive skip keeps `Ppu::write` from
/// clobbering it). SCY/palette are pure colour/row selection — no mode-3-length
/// or FF41-read-law coupling (those sample ARCH `self.scy`/`self.bgp`) — so this
/// is a render-only slice: CGB two-bin 291/291 zero-drift, mooneye 91/91 ON+OFF,
/// production byte-identical OFF. Verified against the pixel two-bin
/// (`gambatte_pixel_probe`, `SLOPGB_ROWLIST`): dmgpalette 6/6 + scy 26/27
/// flag-on (the sprite-stalled `scy_during_m3_spx08_2` is a separate penalty-grid
/// case). Representatives asserted via the suite's own frame comparator
/// (`expect_frame_png`), flag-on.
#[test]
fn tier2_dmg_m3_render_scy_palette_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_scy_palette",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("gambatte/dmgpalette_during_m3/dmgpalette_during_m3_1.gb", Model::Dmg),
        ("gambatte/dmgpalette_during_m3/scx3/dmgpalette_during_m3_3.gb", Model::Dmg),
        ("gambatte/scy/scy_during_m3_1.gbc", Model::Dmg),
        ("gambatte/scy/scy_during_m3_1.gbc", Model::Cgb),
        ("gambatte/scy/scx3/scy_during_m3_5.gbc", Model::Dmg),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The mode-3 render reclock, mechanism 2 (LCDC BG addressing): the BG/
/// window fetcher samples a DEFERRED LCDC view (`eff.render_lcdc`, bit3 BG map /
/// bit4 tile-data / bit6 win map) that lags the eager control commit by the
/// render frame (`RENDER_LCDC_DELAY`), so a mid-mode-3 bgtilemap/bgtiledata
/// toggle reaches the fetch grid at the production dot instead of the leading
/// edge. The window bit5 (abort/reenable/enable) side-effects + the FF41 read
/// laws keep the eager `eff.lcdc` (their tier2 window pins are calibrated to the
/// cc+0 control commit — a full LCDC defer regressed them); OBJ-enable / mode-3
/// length reads also keep the eager view (must not move the length). Fixes the
/// `bgtiledata` (21) + `bgtilemap` (26) pixel-reference flip-blockers + mealybug
/// `m3_lcdc_tile_sel_change`. Render-only slice: CGB two-bin 291/291 zero-drift,
/// mooneye 91/91 ON+OFF, tier2 window pins intact, production byte-identical OFF.
#[test]
fn tier2_dmg_m3_render_lcdc_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_lcdc",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("gambatte/bgtiledata/bgtiledata_spx08_1.gbc", Model::Dmg),
        ("gambatte/bgtiledata/bgtiledata_spx09_2.gbc", Model::Dmg),
        ("gambatte/bgtiledata/bgtiledata_spx08_2.gbc", Model::Cgb),
        ("gambatte/bgtilemap/bgtilemap_spx08_1.gbc", Model::Dmg),
        ("gambatte/bgtilemap/bgtilemap_spx08_1.gbc", Model::Cgb),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The mode-3 render reclock, mechanism 3 (SCX double-speed): SCX's
/// render-frame defer is +2 dots in double speed vs +4 (dots=3) in single speed
/// — the DS M-cycle is 2 PPU dots (vs 4), so the write-commit-to-fetch-grid
/// offset halves. dots=2 fixes the 5 `scx_during_m3_ds` fine-scroll pixel legs
/// AND holds `late_scx4`'s DS FF41 read law (see
/// `tier2_late_scx_writestrobe_passes`) — the single value that satisfies both
/// the render straddle and the read-verdict straddle. CGB two-bin zero-drift,
/// production byte-identical OFF.
#[test]
fn tier2_dmg_m3_render_scx_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_scx_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_5.gbc", Model::Cgb),
        ("gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_8.gbc", Model::Cgb),
        ("gambatte/scx_during_m3/scx_0063c0/scx_during_m3_ds_5.gbc", Model::Cgb),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The mode-3 render reclock, mechanisms 4+5 (mixer render-view LCDC
/// bits): the sprite↔BG mixer (`output_pixel`) reads its render-only LCDC bits
/// from the DEFERRED view (`eff.render_lcdc`), like mechanism 2's BG-fetch
/// addressing bits, so a mid-mode-3 toggle lands its column at the
/// production/SameBoy dot instead of the leading edge. Mech4 is bit0 (BG/window
/// priority): it strips BG priority at the toggle column
/// (m3_lcdc_bg_en_change/_change2 + bgoff_bgon_sprite_below_window). Mech5 is
/// bit1 (OBJ-enable draw-side): it suppresses an already-fetched sprite pixel at
/// the mix (m3_lcdc_obj_en_change, CGB only — DMG keeps the eager one-dot-ahead
/// mixer calibration). Both bits are render-only (bit0's BG fetch still runs;
/// bit1's draw-side is past the sprite fetch — the FETCH-side OBJ enable gating
/// the stall/length stays eager in `render.rs`). CGB two-bin zero-drift, mooneye
/// 91/91, production byte-identical OFF.
#[test]
fn tier2_dmg_m3_render_bg_priority_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_bg_priority",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change.gb", Model::Cgb),
        ("mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change2.gb", Model::Cgb),
        ("gambatte/bgen/bgoff_bgon_sprite_below_window.gbc", Model::Cgb),
        ("mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change.gb", Model::Cgb),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The DMG palette (BGP/OBP FF47-49) commit half-dot pop-grid: the last
/// palette-timing pixel-reference flip-blockers the whole-dot render-defer could
/// not land (89/100 was the whole-dot ceiling; these 5 need
/// half-dot precision). The mealybug `m3_bgp_change`/`_sprites`, `m3_obp0_change`
/// and `m3_window_timing`/`_wx_0` legs are BGP/OBP torture (m3_window_timing is a
/// BGP test, not a window one — its window render is byte-identical flag-on/off,
/// only `eff.bgp` at the pixel-pop differs). SameBoy commits the palette at the
/// write M-cycle's exact half-dot and the pixel pops at a half-dot; single speed
/// is whole-dot aligned so the commit lands at a whole (EVEN) dot, visible +2
/// dots from the pop. The tier2 deferred write's whole-dot leading edge loses
/// which side of the even grid it sits on — `dots = 2 + (leading_edge & 1)`
/// (`cycle.rs::write_deferred`) recovers it: EVEN leading edges (all the mealybug
/// legs, dual-traced LE=104) want +2, ODD (the gambatte dmgpalette legs, LE=183)
/// want +3, so the shared dots=3 was one column late for the mealybug set. DMG
/// only, render-only (colour selection, no length/read-law coupling): CGB two-bin
/// 291/291 zero-drift, mooneye 91/91 ON+OFF, all shipped dmgpalette/scy render
/// pins held, production byte-identical OFF. Pixel two-bin 89→94 (+5 / 0 dropped).
#[test]
fn tier2_dmg_m3_render_palette_halfdot_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_palette_halfdot",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("mealybug-tearoom-tests/ppu/m3_bgp_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_obp0_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_window_timing.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb", Model::Dmg),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The SCY (FF42) commit takes the DMG palette's EVEN-dot parity anchor
/// (`dots = 2 + (leading_edge & 1)`, `cycle.rs::write_deferred`), resolving the
/// sub-dot render-fetch grid the whole-dot defer=3 could not on a sprite-stalled
/// line. A sprite prefill stall (`scy_during_m3_spx08_2`, an X=8 OBJ) shifts the
/// BG fetch grid so a tile's Lo/Hi data read (`bg_tile_addr`, fine row = LY+SCY
/// & 7) lands EXACTLY on the deferred SCY-commit dot; production/SameBoy commits
/// the write at the M-cycle mid-point (visible +2 from an EVEN leading edge, +3
/// from ODD — the same round_up_even(LE)+2 the palette derives), so the per-tile
/// data read re-samples the NEW scroll while the latched tile NUMBER keeps the old
/// (the mealybug m3_scy_change mixed-fetch behaviour). Dual-traced: the sprite leg
/// lands an EVEN LE=236 → +2 (the flat defer=3 rendered the change one column
/// late); the objectless `scy_during_m3_{1,4,5,6}` writes land ODD LEs → +3 (held,
/// a flat +2 broke all 8). SCY is pure row selection (no length / FF41-read-law
/// coupling), so render-only: CGB two-bin 291/291 zero-drift (the CGB `spx08_2`
/// held), mooneye 91/91 ON+OFF, production byte-identical OFF. Pixel two-bin +1.
#[test]
fn tier2_dmg_m3_render_scy_spx08_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_scy_spx08",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // FIXED — the sprite-stalled SCY leg (even LE → parity +2).
        ("gambatte/scy/scy_during_m3_spx08_2.gbc", Model::Dmg),
        // HELD — the CGB sprite leg (already passed at defer=3, unperturbed).
        ("gambatte/scy/scy_during_m3_spx08_2.gbc", Model::Cgb),
        // HELD — an odd-LE objectless leg (parity +3, a flat +2 would drop it).
        ("gambatte/scy/scx3/scy_during_m3_4.gbc", Model::Dmg),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The DMG palette (BGP/OBP FF47-49) render commit RE-HOSTED onto the eager
/// clock (`eager_value`, the C3-flip target). The tier2 render laws fire under
/// `eager_value` too (`|| eager_value`), but on the eager clock the write stage
/// starts at the cc+0 leading edge (`interconnect::Bus::write` stages BEFORE
/// `tick_machine`), while the tier2 stage starts at the cc+4 leading edge
/// (`write_deferred` advances the machine first) — so the un-shifted eager
/// commit lands the palette change ~4 dots (8hd SS / 4hd DS) too EARLY. The
/// pure-render registers (SCY FF42 / palette FF47-49) take the CGB render-frame
/// debt on DMG too (`regs.rs::stage_write`), which is render-only: no mode-3
/// length or FF41-read-law coupling (their read laws sample ARCH state,
/// `commit_eff` records no read-law input), so EV DMG two-bin stays 102 and the
/// length-coupled registers (FF40/FF43/FF4B) keep zero debt (a debt there breaks
/// the `late_enable_afterVblank` gambatte set, #11ck). Same 5 legs as the tier2
/// `tier2_dmg_m3_render_palette_halfdot_passes` pin; production byte-identical
/// (`eager_value`-gated). Recovers the mealybug rows the flip regressed.
#[test]
fn eager_dmg_m3_render_palette_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_m3_render_palette",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("mealybug-tearoom-tests/ppu/m3_bgp_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_obp0_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_window_timing.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb", Model::Dmg),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_eager(&root, rel, model);
    }
}

/// CGB lcd-offset, the line-start OAM-read window,
/// DOUBLE-SPEED sibling of [`tier2_oam_preread_lcdoffset1_passes`]. Under DS the
/// deferred cc+0 read lands 2 dots earlier in the dot grid, so the accessible
/// `preread_ds*_1` read shifts to `dot0` and the slopgb-side window narrows with
/// it ([`CGB_LINESTART_OAM_OPEN_DS`] = 2). Both the base `preread_ds_1` and the
/// offset `preread_ds_lcdoffset1_1` read accessible (out0); their `_2` siblings
/// read `dot2` and stay blocked (out3 — a lcd-offset RENDER shift slopgb matches
/// via its mode-3 OAM block, NOT the OAM read, so the window must NOT extend to
/// them). The `_2` legs are pinned here as regression guards against a
/// widened window. Probe (3524 CGB rows, flag-on): +2/−0. Byte-identical OFF.
#[test]
fn tier2_oam_preread_ds_lcdoffset1_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_preread_ds_lcdoffset1",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("gambatte/oam_access/preread_ds_1_cgb04c_out0.gbc", "0"),
        ("gambatte/oam_access/preread_ds_2_cgb04c_out3.gbc", "3"),
        (
            "gambatte/oam_access/preread_ds_lcdoffset1_1_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/oam_access/preread_ds_lcdoffset1_2_cgb04c_out3.gbc",
            "3",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The read-observer DOUBLE-SPEED sprite m3stat
/// read-grid snap. The single-speed sprite read-grid snap snaps the
/// sprite-line mode-0 dispatch to the CPU read grid (`dot % 4 == 0`); that
/// `snap_ok` term applied in DOUBLE speed too. But the DS sprite-line FF41
/// mode-bit read does not use `vis_early` (which is `!self.ds`-gated, the wrong
/// direction here — these reads want the LAGGING mode 3, not an anticipated 0).
/// It rides the PRODUCTION `stat_mode_edge` override
/// (`interconnect/memory.rs`: a DS sprite-line m3→m0 flip holds the FF41 mode
/// bits at 3 for the read M-cycle), which is armed by the `m0_stat_flip` stamp
/// that ONLY `m0_flip_events` sets. The mod-4 snap pushed the DS sprite dispatch
/// past the pipe end (`advance_lx` lx=160), where the pipe-end fallback set
/// `m0_src` first and `m0_flip_events` early-returned — so the stamp was never
/// armed and the deferred cc+0 read saw the already-flipped visible mode 0 (digit
/// 0 where SameBoy reads 3). Fix (`render/mode0.rs`): gate the dispatch snap to
/// single speed (`snap_ok = !(tier2 && has_sprites && !ds) || dot % 4 == 0`), so
/// DS sprite lines flip at the natural dot, arm the stamp, and the deferred read
/// straddles the override. SameBoy verified: `..._ds_1` read mode 3, the in-cluster
/// `late_sizechange_*_ds_2` (out3) join the lift; the 3 `late_sizechange_*_ds_1`
/// (out0) are gambatte-reference floors (SameBoy reads mode 3, already baselined in
/// production). CGB DS only; vis_early untouched; byte-identical OFF. Probe (3524
/// CGB rows, flag-on): +87/−3 (net +84), 0 SameBoy-passing rows dropped.
#[test]
fn tier2_sprite_m3stat_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_sprite_m3stat_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // The DS sprite m3stat lift (want the lagging mode 3).
        (
            "gambatte/sprites/8spritesPrLine_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/sprites/10spritesprline_1xposa7_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/sprites/1spritesPrLine_1sprite8pBgPrior_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        // The in-cluster A/B winner (size-change `_2`, out3) — joins the lift.
        (
            "gambatte/sprites/late_sizechange_sp00_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        // The `_2` mode-0 sibling is the regression guard (must stay out0).
        (
            "gambatte/sprites/8spritesPrLine_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}
