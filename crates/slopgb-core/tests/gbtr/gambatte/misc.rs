//! Speedchange / serial / timer / HDMA / engine misc pinned-behavior tests.

use super::*;

/// The executable red spec for the kernel pair. Both ROMs reduce to the
/// *same* `ldh a,(FF41)`; SameBoy's cycle-exact frame separates them with no
/// CPU-call-stack discriminator (leading-edge cc+0 sampling + a decoupled
/// `mode_for_interrupt` + the mode-2(−1)/mode-0(+1) anchor swing):
///   - `m2int_m3stat_1` → out3 (mode 3) — anchored off a *mode-2* STAT IRQ;
///     slopgb's whole-dot model currently reads mode 0 here, so this is the
///     one baselined floor row (`tests/gbtr/baselines/gambatte.txt`).
///   - `m0int_m3stat_2` → out0 (mode 0) — anchored off a *mode-0* STAT IRQ;
///     slopgb already passes this, and the port must keep it passing.
///
/// So the lift is *directional*: make `m2int` read 3 while `m0int` stays 0 —
/// exactly the `O<E ∧ O≥E` "contradiction" that only the decoupled-edge
/// rewrite resolves.
///
/// The spec runs on the **flag-on** SameBoy cycle-exact path
/// ([`GameBoy::set_leading_edge_reads`] — leading-edge cc+0 reads + the
/// `StatUpdate` engine + the `vis_early` back-date + the halt-late masks). With
/// those four pieces the kernel pair SEPARATES (`m2int`→3 ∧ `m0int`→0) on both
/// models while the canonical mooneye `intr_2_mode0_timing` also holds flag-on.
/// This is GREEN as a flag-on acceptance test; production (flag-off) is
/// unchanged — the global default flip + ~7000-row rebaseline is the remaining
/// work.
#[test]
fn kernel_pair_matches_sameboy_target() {
    let Some(root) = common::gbtr_root() else {
        // The collection is required to evaluate this spec; mirror the
        // suite's REQUIRE_ROMS contract rather than silently passing.
        common::skip_or_fail_gbtr("kernel_pair", "game-boy-test-roms collection not present");
        return;
    };
    // (relative ROM path, expected FF41 mode both models)
    let targets = [
        (
            "gambatte/m2int_m3stat/m2int_m3stat_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m0int_m3stat/m0int_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let path = root.join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for model in [Model::Dmg, Model::Cgb] {
            // Run the SameBoy cycle-exact flag-on path (the convergence the
            // whole-dot production model cannot represent); same 16-frame
            // protocol + OCR as `run_case`'s `Check::Hex` arm.
            let mut gb = harness::boot(&rom, model);
            gb.set_leading_edge_reads(true);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (flag-on): {e}")
            });
        }
    }
}

/// The kernel pair on the FULL deferred-commit reclock (`set_tier2_reclock`:
/// deferred machine advance + dispatch retime + `early_lead`−2), NOT just the
/// leading-edge hybrid the spec above runs. This is the make-or-break thesis
/// result: the deferred reclock makes the two equal `ldh a,(FF41)` reads
/// SEPARATE — `m2int_m3stat_1` reads mode 3 (out3) and `m0int_m3stat_2` reads
/// mode 0 (out0) — *while* mooneye `intr_2_mode0_timing` simultaneously passes
/// flag-on. That dissolves the mutual-exclusion the prior verdict claimed
/// ("m0int=0 forces intr_2 FAIL in the cc+4 frame"): m0int=0 and intr_2 now
/// co-hold. Production (both flags off) is byte-identical; the global default
/// flip + ~7000-row rebaseline + the two residuals (sprite-line dispatch lead +
/// the deferred halt-wake cc+2 mask) are the remaining work.
#[test]
fn tier2_kernel_pair_matches_sameboy_target() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_kernel_pair",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/m2int_m3stat/m2int_m3stat_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m0int_m3stat/m0int_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let path = root.join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for model in [Model::Dmg, Model::Cgb] {
            let mut gb = harness::boot(&rom, model);
            gb.set_tier2_reclock(true);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// The deferred-frame DIV/serial re-calibration. The
/// post-boot `div_counter` constants (`model.rs`) are calibrated for the eager
/// tick-then-access frame (register reads sample the timer at cc+4); the
/// deferred-commit reclock samples every read at the M-cycle leading edge
/// (cc+0), one M-cycle earlier, so the boot hand-off DIV phase advances 4 T
/// under `tier2_reclock` (`interconnect/boot.rs`) to land on SameBoy's
/// leading-edge frame. Without it `boot_div-*` reads positioned just after a
/// DIV-byte increment lose 1, and the DIV-derived serial clock fails
/// `boot_sclk_align`. SameBoy passes all of these reading at cc+0 (its
/// `div_counter` rides the same +1-M-cycle timeline). Production (flag-off) is
/// byte-identical — the +4 is gated on `tier2_reclock`, and leading-edge-only
/// leaves DIV at cc+4.
#[test]
fn tier2_boot_div_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_boot_div",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel path under the mooneye-test-suite, model legs the suffix runs on).
    let legs: &[(&str, &[Model])] = &[
        (
            "acceptance/boot_div-dmgABCmgb.gb",
            &[Model::Dmg, Model::Mgb],
        ),
        ("acceptance/boot_div-dmg0.gb", &[Model::Dmg0]),
        ("acceptance/boot_div-S.gb", &[Model::Sgb, Model::Sgb2]),
        ("acceptance/boot_div2-S.gb", &[Model::Sgb, Model::Sgb2]),
        ("misc/boot_div-A.gb", &[Model::Agb]),
        ("misc/boot_div-cgbABCDE.gb", &[Model::Cgb]),
        (
            "acceptance/serial/boot_sclk_align-dmgABCmgb.gb",
            &[Model::Dmg, Model::Mgb],
        ),
    ];
    for (rel, models) in legs {
        let path = root.join("mooneye-test-suite").join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in *models {
            // boot_div's DIV phase is decided at hand-off, so the reclock must
            // be on *before* the post-boot state lands — the runtime toggle is
            // too late (see `harness::boot_with_reclock`).
            let mut gb = harness::boot_with_reclock(&rom, model);
            harness::run_until_breakpoint(&mut gb, 30_000_000)
                .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
            harness::check_fib(&gb)
                .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        }
    }
}

/// The POST-SWITCH bare-exit 4-variable table:
/// `ppu/stat_irq.rs::vis_exit_hd` replaces the emergent bare exit
/// with `E = 504 + leave_k − 4*[lcd_enable_in_ds] + 2*(SCX&7)` rp (SS) /
/// `502 + leave_k + 2*(SCX&7)` (DS) for dances whose FIRST LCD-on switching
/// STOP sits mid-frame (`Ppu::stop_anchor_midframe`) — the speedchange
/// v1/2/3/4/5 anchor; the VBlank/boot-prologue frame every other tier2
/// constant absorbs is excluded (kernel `_ds`, lcd_offset, gdma all anchor
/// ly144, measured). All 120 family m3stat legs dual-traced (SBSTOP/SBACK/
/// SBREAD/SBMODE ↔ SLOPGB stop/leave/ff41/visexit): 120/120 offline fit,
/// zero conflicts; family probe +31/−0 (the 4 census blockers + 27 bonus;
/// the sole non-fix `speedchange2_nop_m2int_m3stat_scx1_1` is the
/// VBlank-anchored pre-seeded rebaseline joiner, out of scope).
/// lcd_offset/m2int_m3stat/dma guard probes byte-identical.
#[test]
fn tier2_speedchange_postswitch_exit_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_speedchange_postswitch_exit",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 5] = [
        // The census quartet (SS lcdoff class, `E = 502 + 2*scx`): the `_2`
        // read (rp 506) now sits AT the exit → mode 0.
        (
            "gambatte/speedchange/speedchange2_lcdoff_m2int_m3stat_scx2_2_cgb04c_out0.gbc",
            "0",
        ),
        // Regression guard: the `_1` read (rp 498) stays below it → mode 3.
        (
            "gambatte/speedchange/speedchange2_lcdoff_m2int_m3stat_scx2_1_cgb04c_out3.gbc",
            "3",
        ),
        // The k=6 (dsa7==4 leave) class guard: `E = 506 + 2*scx` — the earlier
        // blanket law's first casualty class, must keep reading 3.
        (
            "gambatte/speedchange/speedchange2_lcdoff_nop_m2int_m3stat_scx1_1_cgb04c_out3.gbc",
            "3",
        ),
        // The DS arm (v1: enter-only mid-frame dance, `E = 504 + 2*scx`).
        (
            "gambatte/speedchange/speedchange_ly44_m3_m3stat_2_cgb04c_outC0.gbc",
            "C0",
        ),
        // The replace-semantics witness: native-0 read the emergent m==0
        // hold over-held to 3 (rp 512 == law exit 512).
        (
            "gambatte/speedchange/speedchange4_ly44_m3_nop_m3stat_scx3_2_cgb04c_outC0.gbc",
            "C0",
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

/// The serial/timer completion frame (`interconnect/tick.rs`
/// `advance_machine_t`): the deferred path detects serial completions per
/// T-substep (the DIV-edge fall's true T) and squashes a dispatch-ack'd
/// timer/serial re-set by SameBoy's EXACT T-threshold
/// (`updateTimaIrq(cc + 2 + isCgb())` / `updateSerial(cc + 3 + isCgb())`)
/// instead of the whole-M-cycle window. Converges BOTH legs of the
/// `tima/tc00_irq_late_retrigger` and `serial/start_wait_trigger_int8_read_if`
/// pairs (SS+DS, 8 rows, MEASURED +8/−0): the `_1` re-set commits past the
/// threshold and is DELIVERED (E4/E8), the `_2` inside it is consumed (E0).
#[test]
fn tier2_serial_tima_completion_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_serial_tima_completion",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 4] = [
        (
            "gambatte/tima/tc00_irq_late_retrigger_1_dmg08_cgb04c_outE4.gbc",
            "E4",
        ),
        (
            "gambatte/tima/tc00_irq_late_retrigger_2_dmg08_outE4_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/serial/start_wait_trigger_int8_read_if_1_dmg08_cgb04c_outE8.gbc",
            "E8",
        ),
        (
            "gambatte/serial/start_wait_trigger_int8_read_if_2_dmg08_outE8_cgb04c_outE0.gbc",
            "E0",
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

/// The sub-M-cycle WAKE clock (`Bus::pending_halt_wake_mid`,
/// `interconnect.rs`): the DMG tier2 halt loop samples the wake condition at
/// the M-cycle head AND mid-cycle (SameBoy `GB_cpu_run` advance-2 → sample →
/// advance-2, `sm83_cpu.c:1621-1628`); a mid wake resumes the CPU 2 T into
/// the idle cycle and the handler's first FF41 read samples the STAT mode at
/// that true sub-M-cycle T (the `wake_skew`, consumed by the FF41 read /
/// repaid before any other IO read so the TIMA-counted `int_hblank_halt` and
/// LY-straddle `hblank_ly_scx` grids keep their aligned calibration).
/// MEASURED +11/−3 on the DMG `halt *_m0stat` wake-clock class. Pins two
/// fixed want-0 legs (the post-wake read lands on the line-start mode-0
/// window it previously missed) + a passing want-2 guard leg.
#[test]
fn tier2_halt_m0stat_wake_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_halt_m0stat_wake",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 3] = [
        // Fixed by the mid-cycle wake: the read lands in the line-start
        // mode-0 window (want 0).
        (
            "gambatte/halt/m0int_m0stat_scx2_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/halt/m0irq_m0stat_scx4_2_dmg08_out0_cgb04c_out2.gbc",
            "0",
        ),
        // Regression guard: the want-2 sibling stays mode 2.
        (
            "gambatte/halt/m0int_m0stat_scx2_2_dmg08_cgb04c_out2.gbc",
            "2",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The SameBoy-exact DMG halt-wake grid
/// for the mode-0 STAT rise: one iq sample per iteration at the mid point
/// (4k+2; the `just_halted` head sample has NO +2 slot — the jh gap), the
/// post-sample advance COMPLETES the M-cycle, and the woken instruction
/// RE-FETCHES (SameBoy's halt loop performs no prefetch — `GB_cpu_run`
/// sm83_cpu.c:1629-1642 + halt() :1036-1058). The rise's visibility is a
/// T-deadline (+4 on the LCD-enable glitch line, whose engine rise is emitted
/// at visexit where normal lines emit at visexit−3); HALT's own entry
/// IF-check observes the machine at the fetch's END (t0+4), arming the
/// halt-bug exactly when SameBoy does. Together these replace the M-quantized
/// `if_late`/`m0_halt_hold`/w2-skew model for the m0 rise AND dissolve the
/// `halt_ly_phase` carry (its only passing table under the exact grid is
/// all-zero). Full-DMG two-bin 167→158 (+9/−0); the four pinned rows cover
/// the four mechanisms: the jh-gap/steady-grid pair
/// (`late_m0irq_halt_m0stat_scx3_2b` want 2 · `m0irq_m0stat_scx3_2` want 0 —
/// same rise, same read distance, straddling wake slots), the re-fetch
/// (`m0int_m0stat_scx5_2`), and the entry check (`late_m0irq_halt_dec_scx2_2`
/// — the halt-bug's double-DEC). DMG-only; CGB byte-identical (its
/// head-sample grid is a separate port); production byte-identical.
#[test]
fn tier2_halt_wake_grid_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_halt_wake_grid",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 4] = [
        (
            "gambatte/halt/late_m0irq_halt_m0stat_scx3_2b_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/halt/m0irq_m0stat_scx3_2_dmg08_out0_cgb04c_out2.gbc",
            "0",
        ),
        (
            "gambatte/halt/m0int_m0stat_scx5_2_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/halt/late_m0irq_halt_dec_scx2_2_dmg08_cgb04c_out6.gbc",
            "6",
        ),
    ];
    let model = Model::Dmg;
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
            panic!("{rel} [{model:?}] expected dmg08 out{expect} (tier2 flag-on): {e}")
        });
    }
}

/// A racing DMA-register write (FF51-FF55 counters/arm, FF70
/// WRAM bank, FF4F VRAM bank) beats a same-advance HBlank-DMA steal: SameBoy
/// runs `GB_hdma_run` only after the current instruction completes
/// (sm83_cpu.c:1718), so the racing write's store is visible to the block.
/// slopgb's deferred write head-serviced the request flagged during the
/// write's own machine advance BEFORE the store (`hdma_late_destl_1`
/// dual-traced with the new SBWHDMA tracer: SameBoy order w54 → run dst=8010;
/// slopgb ran the block with the stale dst=8000). The steal now defers past
/// the scoped registers' store; a request already pending at the op's entry
/// still steals first. The scope is load-bearing: a GENERAL post-store
/// service broke `irq_precedence/hdma_vs_m0_scx2_halt` (base-passing) and
/// 60+ hdma rows. Pinned: destl `_1` (write wins) + `_2` (block-first
/// sibling), `wrambank_1` (the FF70 source-bank race), `disable_ds_2` (the
/// FF55 disarm race, DS). Full-CGB two-bin +5/−0; production byte-identical
/// (`write_deferred` is tier2-only).
#[test]
fn tier2_hdma_write_race_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_hdma_write_race",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 4] = [
        ("gambatte/dma/hdma_late_destl_1_cgb04c_out0.gbc", "0"),
        ("gambatte/dma/hdma_late_destl_2_cgb04c_out1.gbc", "1"),
        ("gambatte/dma/hdma_late_wrambank_1_cgb04c_out0.gbc", "0"),
        ("gambatte/dma/hdma_late_disable_ds_2_cgb04c_out1.gbc", "1"),
    ];
    let model = Model::Cgb;
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
            panic!("{rel} [{model:?}] expected cgb04c out{expect} (tier2 flag-on): {e}")
        });
    }
}

/// The CGB single-speed FF41 two-phase ENGINE write
/// (`GB_CONFLICT_STAT_CGB` seen from the hardware side): the engine's FF41
/// view (`Ppu::eng_stat`) transitions old → phase-1 (mode bits new, LYC enable
/// bit OLD) at commit+2 → final at commit+4, with hazard-free applications
/// (falls silent, the final rise continuity-gated + delivered through the CGB
/// `lyc_if_delay`) and externals edging against the armed phase-1
/// (`ff41_disable_2`\u{2019}s ly6 dot-4 LYC latch rise). The write-instant
/// gambatte LYC arms stay for MID-LINE writes (the `lyc_ff41_trigger_delay`
/// pair collapses to one deferred commit dot — only the calibrated arm splits
/// it); the engine owns the line-boundary region. All rows dual-traced
/// (SBWRITE two-phase prints + slopgb wff41/dispatch). CGB SS unshifted
/// Tier-2/LE only; production byte-identical OFF.
#[test]
fn tier2_ff41_twophase_engine_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_ff41_twophase",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // bit6-disable stays armed through the lyfc latch rise (fires).
        (
            "gambatte/lycEnable/ff41_disable_2_dmg08_out0_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/lycEnable/lyc0_ff41_disable_2_dmg08_cgb04c_outE2.gbc",
            "E2",
        ),
        // bit6-enable lands one T late and misses the closed match window.
        (
            "gambatte/lycEnable/late_ff41_enable_2_dmg08_out2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/lycEnable/lyc153_late_ff41_enable_2_dmg08_outE2_cgb04c_outE0.gbc",
            "E0",
        ),
        // GUARD — the one-M-earlier sibling still catches the held latch.
        (
            "gambatte/lycEnable/lyc153_late_ff41_enable_1_dmg08_cgb04c_outE2.gbc",
            "E2",
        ),
        // The m1→LYC handoff is hazard-free on hardware (SameBoy\u{2019}s
        // intersection form dips and reads E2 — hardware-truth row).
        (
            "gambatte/lycEnable/lyc153_late_enable_m1disable_3_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        // The final value evaluates at the T0+1T-instant mode: the sub-dot
        // dip re-fires the next line\u{2019}s OAM carryover rise.
        (
            "gambatte/m2enable/lyc1_m2irq_late_lycdisable_1_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // m0-flip fast-forward: the dying LYC hold dips before the mode-0
        // rise re-edges.
        (
            "gambatte/m0enable/lycdisable_ff41_scx1_1_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // GUARD — a stage still within a dot of the flip keeps the OLD view
        // (the dying enable catches its own rise).
        (
            "gambatte/m0enable/disable_2_dmg08_out0_cgb04c_out2.gbc",
            "2",
        ),
        // GUARD — the mid-line write-instant arm splits the trigger-delay
        // pair the deferred frame collapses.
        (
            "gambatte/lycEnable/lyc_ff41_trigger_delay_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/lycEnable/lyc_ff41_trigger_delay_2_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // The DS immediate-view analogue of the m0-flip dip: a bit6-drop
        // committing within one M of the DS mode-3→0 flip dies before the
        // mode-0 rise (fresh edge); the `_2` sibling drops after the flip
        // (seamless).
        (
            "gambatte/m0enable/lycdisable_ff41_ds_1_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/m0enable/lycdisable_ff41_ds_2_cgb04c_out0.gbc",
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

/// The FF0F read PEEK (group A) + write-race squash (group B), the
/// IF-register analogues of the FF41 verdict peek: verdict-only, no
/// machine advance, no dispatch move (the refuted sub-M FF0F sampling
/// moved the machine).
///
/// Group A: the deferred cc+0 FF0F read's verdict includes a deterministically
/// imminent STAT engine rise SameBoy's events-first `read_high_memory` frame
/// has already folded — the DS mode-0 flip one dot ahead (mode-2-ISR-anchored
/// reads only, `stat_rise_oam`) and the LYC latch half an M-cycle ahead
/// (`Ppu::ff0f_stat_peek`). Group B: a bit1-clearing FF0F write consumes a
/// STAT rise landing within the per-source window (DS mode-0 2 dots, SS LYC
/// 1 dot; everything else 0 — `GB_CONFLICT_WRITE_CPU`, strict-edge
/// no-re-raise). All windows dual-traced (SBIF/SBACK/SBREAD-ff0f fp vs
/// SLOPGB wff0f/ff0f/dispatch, 2026-07-03).
#[test]
fn tier2_ff0f_groupab_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_ff0f_groupab",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // Group A: the DS mode-0 rise one dot past the read (rise 255, read 254).
        ("gambatte/m2int_m0irq/m2int_m0irq_ds_2_cgb04c_out3.gbc", "3"),
        // GUARD — one M earlier (read 252) stays clear.
        ("gambatte/m2int_m0irq/m2int_m0irq_ds_1_cgb04c_out1.gbc", "1"),
        // GUARD — the identical dot-254/rise-255 geometry from an
        // LYC-anchored ISR reads clear (the `stat_rise_oam` anchor gate).
        (
            "gambatte/lyc0int_m0irq/lyc0int_m0irq_ds_1_cgb04c_out0.gbc",
            "0",
        ),
        // Group A: the SS LYC=153 latch two dots past the read (rise 6, read 4).
        (
            "gambatte/ly0/lycint152_lyc153irq_2_dmg08_cgb04c_outE2.gbc",
            "E2",
        ),
        // GUARDs — reads at dot 0 (SS) / dot 2 (DS) stay clear.
        (
            "gambatte/ly0/lycint152_lyc153irq_1_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/ly0/lycint152_lyc153irq_ds_1_cgb04c_outE0.gbc",
            "E0",
        ),
        // Group B: the DS mode-0 write-race (write 1-2 dots before the rise
        // consumes it; the strict edge never re-raises).
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx4_ifw_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — 3-4 dots clear survives.
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_ds_1_cgb04c_out2.gbc",
            "2",
        ),
        // GUARD — the SS mode-0 write-race window is 0 (Δ=1 survives).
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx4_ifw_1_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // Group B: the SS LYC write-race (write at dot 5, rise 6 — consumed).
        (
            "gambatte/ly0/lycint152_lyc153irq_ifw_2_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        // GUARDs — Δ=5 (SS) survives; the DS LYC window is 0 (Δ=2 survives);
        // the mode-2 pulse window is 0.
        (
            "gambatte/ly0/lycint152_lyc153irq_ifw_1_dmg08_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/ly0/lycint152_lyc153irq_ifw_ds_1_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/m2int_m2irq/m2int_m2irq_ifw_ds_1_cgb04c_out2.gbc",
            "2",
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

/// The dispatch-ack squash reclock: under tier2 the
/// whole-dot bit-0/1 `ack_squash_dots = 2` window is replaced by the PPU-side
/// per-SOURCE windows (`Ppu::ack_squash_ppu`, dots (SS, DS): mode-0 (0, 1) ·
/// mode-2 pulse (0, 0) · LYC / mode-1 / vblank-IF (2, 0) — all dual-traced
/// SBACK/SBIF fp vs SLOPGB vec/dispatch). SameBoy's ack is the bare IF clear
/// at the flushed pending−2 instant; a rise past the window survives and
/// re-sets IF (the retrigger `_1` legs), a rise inside it merges into the
/// dispatch (the `_2` legs).
#[test]
fn tier2_ack_squash_reclock_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_ack_squash",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // The six blockers: rise past the ack window survives.
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_ds_1_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_scx1_1_dmg08_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/m2int_m2irq/m2int_m2irq_late_retrigger_1_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_late_retrigger_1_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/m1/lycint143_m1irq_late_retrigger_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m1/lycint_vblankirq_late_retrigger_ds_1_cgb04c_out1.gbc",
            "1",
        ),
        // Bonus lifts (the DS twins the whole-dot squash also ate).
        (
            "gambatte/ly0/lycint152_lyc0irq_late_retrigger_ds_1_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/m2int_m2irq/m2int_m2irq_late_retrigger_ds_1_cgb04c_out2.gbc",
            "2",
        ),
        // GUARDs — a rise inside the per-source window stays consumed.
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_scx1_ds_2_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/ly0/lycint152_lyc0irq_late_retrigger_2_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/m1/lycint143_m1irq_late_retrigger_2_dmg08_cgb04c_out1.gbc",
            "1",
        ),
        (
            "gambatte/m1/lycint_vblankirq_late_retrigger_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // GUARDs — a rise folded at/before the ack is cleared by it.
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_ds_2_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/m2int_m2irq/m2int_m2irq_late_retrigger_2_dmg08_cgb04c_out0.gbc",
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

/// The glitch-line same-dot SCX hunt re-open + the
/// DS line-start carryover-enable level hold. (c): the LCD-enable glitch
/// line's CGB fine-scroll sample deadline is `83 + scx_init` INCLUSIVE — a
/// same-dot FF43 commit lands after that dot's render tick but hardware's
/// live comparator still honors it; re-opening the hunt lets it cycle to the
/// new SCX&7 (asm_enable ROW 5). (d): a DS dots-0-1 fresh LYC enable whose
/// old value armed HBlank joins a line still latched HIGH from the previous
/// line's mode-0 (SameBoy's natural 1→0 lands at dot 2) — no edge; the
/// engine level is seeded high so the next tick stays silent
/// (asm_m1_misc Rows 5-6). The (e) WY-deadline rows are frame-phase-marginal
/// and deliberately unpinned (two-bin covered).
#[test]
fn tier2_glitch_hunt_carryover_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_glitch_hunt_carryover",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/enable_display/ly0_late_scx7_m3stat_scx1_1_dmg08_cgb04c_out87.gbc",
            "87",
        ),
        // GUARD — one M later misses the re-open window.
        (
            "gambatte/enable_display/ly0_late_scx7_m3stat_scx1_2_dmg08_cgb04c_out84.gbc",
            "84",
        ),
        (
            "gambatte/miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_ds_2_cgb04c_outE0.gbc",
            "E0",
        ),
        // GUARDs — the dot-2 write lands after the natural drop and fires;
        // the dot-4 write finds both masks low.
        (
            "gambatte/miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_ds_3_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_ds_4_cgb04c_outE0.gbc",
            "E0",
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

/// The IME=1 halt-entry rewind on the EAGER clock (`halt_entry_rewind_impl`).
///
/// SameBoy's `halt()` (sm83_cpu.c:1043-1047) does not enter HALT when
/// `IE & IF` is already nonzero at the entry view: it clears `halted` and
/// decrements PC, so the dispatched ISR returns *into* the HALT and it
/// re-executes with the IF bit consumed. slopgb's production path instead
/// halts and wakes on the first idle check, which pushes the return address to
/// halt+1 — the ISR skips the re-halt and the whole post-wake stream runs one
/// halt round early.
///
/// The rewind was hosted on tier2 only; the eager clock ran the production
/// (halted+wake) shape. `ifandie_ei_halt_sra` is the row that separates them:
/// `EI; HALT` with `IE & IF` already set, so the entry view must rewind.
/// Hardware behaviour, not a clock artifact — hence `tier2_reclock ||
/// eager_value` rather than a new sub-flag. Production (both flags off) keeps
/// the halted+wake shape and stays byte-identical.
#[test]
fn eager_halt_entry_rewind_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_halt_entry_rewind",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rel = "gambatte/halt/ifandie_ei_halt_sra_dmg08_cgb04c_out0A.gbc";
    let path = root.join(rel);
    let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = harness::boot_eager(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0A", model.is_cgb())
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected out0A (eager): {e}"));
    }
}

/// The eager halt-entry `t0+4` VALUE peek (`Ppu::stat_m0_rise_within`).
///
/// SameBoy's `halt()` samples `IE & IF` *after* the prefetch `cycle_read` walked
/// the machine through the HALT fetch M-cycle (t0+4). The deferred clock reaches
/// that view by flushing its parked debt; the eager clock parks none, so its
/// entry sample sits at t0 and a mode-0 STAT rise landing inside the fetch is
/// invisible — the rewind never arms and the post-wake stream runs one halt
/// round early.
///
/// Traced on `late_m0int_halt_m0stat_scx3_3a` [Cgb]: OFF samples ly1 dot 332
/// (w=00, halts, `out0`); tier2 samples dot 260 (w=02 → rewind → re-entry 336,
/// `out0`); eager sampled dot 256 (w=00 → halts early, `out2`). The rise folds
/// at dot 257 — four dots.
///
/// Reconstructing the rise's VALUE at t0+4 rather than advancing the clock keeps
/// machine time honest (advancing would tick the timers 4 T early and break the
/// TIMA-counted `int_hblank_halt` rows). DMG-scoped: see the note in
/// `halt_entry_impl` on the CGB `_3b` skip-path.
#[test]
fn eager_halt_entry_m0_peek_passes_dmg() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_halt_entry_m0_peek",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // The six DMG halt rows the peek recovers — every one a SameBoy-PASS row
    // that was inside the TRUE flip bar.
    let rows = [
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx2_3a_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx3_3a_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx3_3b_dmg08_out0_cgb04c_out2.gbc",
            "0",
        ),
        (
            "gambatte/halt/late_m0irq_halt_dec_scx2_2_dmg08_cgb04c_out6.gbc",
            "6",
        ),
        (
            "gambatte/halt/late_m0irq_halt_dec_scx3_2_dmg08_cgb04c_out6.gbc",
            "6",
        ),
        (
            "gambatte/halt/late_m0irq_halt_m0stat_scx3_3b_dmg08_cgb04c_out2.gbc",
            "2",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_eager(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}

/// CGB DOUBLE-SPEED mode-2→3 ENTRY back-date RE-HOSTED onto the eager clock
/// (#11da, L1). The eager cc+0 FF41 value peek (`leading_edge_sample`) samples
/// the PPU pre-tick, a DS M-cycle (2 dots) before the trailing cc+4 view, so a
/// line-start FF41 read straddling the mode-2→3 boundary saw the un-shifted
/// dot-84 entry as mode 2 where SameBoy's cc+4 view reads mode 3. The DS entry
/// back-dates to 80 (as single speed) so the peek lands on mode 3
/// (`Ppu::mode3_entry_dot`, `eager_value && ds`-scoped). EV CGB two-bin
/// 353 → 348 (clean +5/−0; 4 SameBoy-pass bar + 1 lcd_offset gambatte-want
/// gain). The `_1` siblings (want 2) are the regression guards — they read
/// earlier and must stay mode 2. Tier2's deferred DS frame keeps 84;
/// `eager_value` off → byte-identical.
#[test]
fn eager_ds_mode3_entry_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ds_mode3_entry",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rows = [
        // Recovered (SameBoy-pass, was EV-fail):
        (
            "gambatte/m2int_m2stat/m2int_m2stat_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m2int_m2stat/m2int_scx4_m2stat_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/enable_display/frame0_m3stat_count_ds_2_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/enable_display/frame1_m3stat_count_ds_2_cgb04c_out90.gbc",
            "90",
        ),
        // Regression guards (the `_1` mode-2 siblings must stay blocked at 2):
        (
            "gambatte/m2int_m2stat/m2int_m2stat_ds_1_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/m2int_m2stat/m2int_scx4_m2stat_ds_1_cgb04c_out2.gbc",
            "2",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}
