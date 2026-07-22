//! CGB single-speed dispatch/IRQ web pins. A CGB `GB_CONFLICT_WRITE_CPU`
//! engine write (FF41 STAT / FF0F IF / FF45 LYC) commits its engine-visible
//! effect one T into the M-cycle — [`Interconnect::write`] lands the commit at
//! the WriteCpu dot (M-cycle boundary + 1).

use super::*;

/// CGB single-speed FF41/FF0F/FF45 engine writes commit at the WriteCpu dot
/// (M-cycle boundary + 1), not the boundary. Covers both divergence directions:
/// a *missing* STAT bit — a disable that must NOT kill the source before its
/// latch (`ff41_disable_2` want 2, `lyc0_ff41_disable_2` want E2) — and a
/// *spurious* STAT bit — a disable/IF-clear that must land after/at the rise
/// (`lycdisable_ff41_2` want 0, `m2int_m0irq_scx3_ifw_2` want 0 via the
/// co-instant rise fold + squash, `lycint152_lyc153irq_ifw_2` want E0).
#[test]
fn eager_write_conflict_commit_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_write_conflict_commit",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // Twelve rows, one per divergence shape/register.
    let rows = [
        // Missing-bit: the disabling FF41 write must commit AFTER the LYC
        // latch, so the IRQ still fires.
        (
            "gambatte/lycEnable/ff41_disable_2_dmg08_out0_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/lycEnable/lyc0_ff41_disable_2_dmg08_cgb04c_outE2.gbc",
            "E2",
        ),
        // Spurious-bit: the disable/clear must commit at/after the rise.
        (
            "gambatte/lycEnable/late_ff41_enable_2_dmg08_out2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/lycEnable/lyc153_late_ff41_enable_2_dmg08_outE2_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/lycEnable/lyc153_late_m1disable_3_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/m0enable/lycdisable_ff41_2_dmg08_out2_cgb04c_out0.gbc",
            "0",
        ),
        // FF45 (LYC) WriteCpu.
        (
            "gambatte/m0enable/lycdisable_ff45_3_dmg08_out2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/m2enable/lyc0_late_m2enable_lycdisable_2_dmg08_out2_cgb04c_out0.gbc",
            "0",
        ),
        // FF0F (IF) WriteCpu + the co-instant-rise squash.
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_4_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/ly0/lycint152_lyc153irq_ifw_2_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_4_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}

/// Double-speed FF0F: a CGB DS bit1-clearing FF0F write must consume the mode-0
/// STAT rise landing 1-2 dots later. The `stat_if_squash` arm (the DS mode-0
/// squash window `w=2`, Δ1-2) consumes the rise for the `_ds_2` rows while the
/// `_ds_1` siblings (Δ3-4) survive. CGB + DS scoped.
#[test]
fn eager_ds_write_conflict_commit_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ds_write_conflict_commit",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // The two DS FF0F ifw rows (want 0 = the rise squashed).
    let rows = [
        "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_ds_2_cgb04c_out0.gbc",
        "gambatte/m2int_m0irq/m2int_m0irq_scx4_ifw_ds_2_cgb04c_out0.gbc",
    ];
    for rel in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out0 (eager DS): {e}"));
    }
}

/// A post-ack mode-0 STAT retrigger must stay CONSUMED by its dispatch's IF
/// clear. The `ack_squash_dots` LCD-bit window (SS 6, DS 3) re-consumes the
/// retrigger for the `_2` rows while the one-M-cycle-later `_1` siblings land
/// outside and DELIVER.
#[test]
fn eager_ack_squash_retrigger_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ack_squash_retrigger",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // The SS + DS irq_precedence mode-0 retriggers plus the mode-2 SS
    // retrigger family.
    let rows = [
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_2_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_scx1_2_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_ds_2_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_late_retrigger_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/m2int_m2irq/m2int_m2irq_late_retrigger_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}

/// DMG FF0F IF-clear write-commit. DMG is single-speed with the same 4-dot
/// M-cycle and 1-T WriteCpu commit as CGB SS, so a bit1-clearing FF0F write
/// (`Interconnect::write`, FF0F-only on DMG) commits at the WriteCpu dot and
/// folds a co-instant STAT rise into `intf` first, then squashes it. Recovers
/// the mode-0 IF-clear straddle (`m2int_m0irq_scx3_ifw_2` /
/// `m2int_m0irq_scx3_ifw_4` want 0) and the line-153 LYC IF-write
/// (`lycint152_lyc153irq_ifw_2` want E0). The FF41/FF45 WriteCpu commit is
/// CGB-only.
#[test]
fn eager_dmg_ff0f_write_commit_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_ff0f_write_commit",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rows = [
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_4_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/ly0/lycint152_lyc153irq_ifw_2_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}

/// CGB single-speed sub-M-cycle halt-wake. A CGB halt exiting on the mode-0
/// STAT rise wakes at the flip's own M-cycle boundary
/// (`Ppu::m0_stat_flip_reached`, a pure dot-space peek — no machine advance,
/// timer-safe); the resumed IME=1 dispatch's first FF41 read then rides the
/// re-fetch line boundary to mode 2 (`Ppu::halt_refetch_read_override`). The
/// wake peek separates the wake instant (scx2_3a dot 256 → mode-0 read,
/// scx3_3b dot 260 → mode-2 read) so the read override fires with zero
/// collateral. Targets: `_3a` want 0, `dec_2` want 6, m0irq/m0int `_3b` want 2;
/// the want-0 `_1a` sibling must stay 0 (the read override must not leak).
#[test]
fn eager_halt_wake_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_halt_wake",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rows = [
        // Targets.
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx2_3a_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx3_3a_dmg08_cgb04c_out0.gbc",
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
        // The coupled row: the wake peek + read override recover it.
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx3_3b_dmg08_out0_cgb04c_out2.gbc",
            "2",
        ),
        // A want-0 sibling on the same read boundary — the override must NOT
        // leak onto it (the sub-M-cycle wake keeps its read one dot short).
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx2_1a_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager halt-wake): {e}"));
    }
}

/// WINDOW mode-0 STAT-IF read-frame deliver. With a window active the mode-0
/// STAT source rises 2 dots (half an M-cycle) before the window-elevated
/// mode-0 flip R; a single-speed FF0F read landing in the `[R-2, R)` gap must
/// fold the bit ([`Ppu::ff0f_stat_peek`], `win_active`-gated) so
/// `m2int_wxA5/A6_m0irq_2` (read R-1) deliver 2/E2. The `win_active` gate is
/// the discriminator: non-window `m0irq` rows keep R at the rise, so their
/// `_1` polls (`m2int_m0irq_1`, `m2int_wxA6_m0irq_1`, want 0) must NOT deliver.
/// DS takes the same peek arm.
#[test]
fn eager_window_m0irq_deliver_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_window_m0irq_deliver",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expect, is_cgb). The `_2` targets DELIVER; the `_1`/non-window
    // guards must stay CLEAR (the win_active + [R-2, R) scope).
    let rows = [
        // Targets — the window `m0irq` reads land in the [R-2, R) deliver gap.
        (
            "gambatte/window/m2int_wxA5_m0irq_2_dmg08_cgb04c_out2.gbc",
            "2",
            true,
        ),
        (
            "gambatte/window/m2int_wxA5_m0irq_2_dmg08_cgb04c_out2.gbc",
            "2",
            false,
        ),
        (
            "gambatte/window/m2int_wxA6_m0irq_2_dmg08_cgb04c_out2.gbc",
            "2",
            false,
        ),
        (
            "gambatte/window/m2int_wxA6_m0irq2_2_dmg08_cgb04c_out2.gbc",
            "2",
            false,
        ),
        // Scope guards (want 0): a window `_1` poll (below R-2) and a
        // non-window poll (win_active off) must NOT deliver.
        (
            "gambatte/window/m2int_wxA6_m0irq_1_dmg08_cgb04c_out0.gbc",
            "0",
            false,
        ),
        (
            "gambatte/m2int_m0irq/m2int_m0irq_1_dmg08_cgb04c_out0.gbc",
            "0",
            false,
        ),
        (
            "gambatte/m2int_m0irq/m2int_m0irq_scx2_1_dmg08_cgb04c_out0.gbc",
            "0",
            true,
        ),
    ];
    for (rel, expect, is_cgb) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let model = if is_cgb { Model::Cgb } else { Model::Dmg };
        let mut gb = harness::boot(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, is_cgb).unwrap_or_else(|e| {
            panic!("{rel} [{model:?}] expected out{expect} (eager window m0irq): {e}")
        });
    }
}

/// DMG line-0 OAM-entry read-frame back-date: a line-0 dot<4 FF41 read maps to
/// line-0 dot 4-7 = the OAM scan (mode 2), the DMG twin of the `(1..144)`
/// line-start arm. Gated on `!line_render_done` — the discriminator vs the
/// mooneye `stat_lyc_onoff` LCD-enable poll (line-0 dot 0, want mode 0), which
/// resolves with the render done (no pending scan). Recovers
/// `ly0/lycint152_ly0stat_3` (want C2; its `_2` sibling reads the earlier
/// LY=153, untouched) and `enable_display/frame1_m2stat_count_2` (want 90).
#[test]
fn tier2_eager_dmg_ly0_oam_entry_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_eager_dmg_ly0_oam_entry",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rows = [
        (
            "gambatte/ly0/lycint152_ly0stat_3_dmg08_cgb04c_outC2.gbc",
            "C2",
        ),
        (
            "gambatte/enable_display/frame1_m2stat_count_2_dmg08_cgb04c_out90.gbc",
            "90",
        ),
        // The sibling that stays mode 0 (its verdict read is the earlier
        // LY=153 read, not the line-0 read the arm rewrites).
        (
            "gambatte/ly0/lycint152_ly0stat_2_dmg08_cgb04c_outC0.gbc",
            "C0",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager ly0 oam): {e}"));
    }
}

/// DMG line-153 FF41 write-commit. On line 153 the DMG FF41 engine-view
/// (`eng_stat`) write commits its disable ~2 dots later than the whole-dot cc+4
/// landing (the line-153 write quirk): the VBLANK-disable lands coincident with
/// the LYC=153 re-latch (dot 6), so the held LYC match keeps the STAT line high
/// across the disable → no fresh edge (want E0). The defer applies to only the
/// `eng_stat` view via the odd-half `Ppu::stat_update_half`, leaving the LYC
/// re-latch schedule the window/DMA/`_2` neighbours consume untouched. The CGB
/// siblings pass via the two-phase `eng_stat_pending` (pinned in
/// `eager_write_conflict_commit_passes`); this is the DMG twin. Pins
/// `lyc153_late_m1disable_3` / `lyc153_late_enable_m1disable_3` (out E0).
#[test]
fn eager_dmg_lyc153_m1disable_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_lyc153_m1disable",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rows = [
        "gambatte/lycEnable/lyc153_late_m1disable_3_dmg08_cgb04c_outE0.gbc",
        "gambatte/lycEnable/lyc153_late_enable_m1disable_3_dmg08_cgb04c_outE0.gbc",
    ];
    for rel in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "E0", false).unwrap_or_else(|e| {
            panic!("{rel} [Dmg] expected outE0 (eager line-153 m1disable): {e}")
        });
    }
}

/// CGB double-speed mid-mode-3 SCX (FF43) render commit. These 4
/// `scx_during_m3_ds` rows write SCX twice per line, both POST-match (after
/// this line's fine-scroll comparator lock `hunt_done` at `hunt_match_dot`=89;
/// write dots 90/232 and 96/226) — a pure coarse/tile shift with no
/// mode-3-length effect. The post-match commit uses render debt 2 (6hd) so it
/// lands on the correct dot (`regs/stage.rs`); post-match-scoped (the `_ds_1`
/// pre-match line-start write keeps 4), CGB + DS gated.
#[test]
fn eager_cgb_m3_render_scx_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_cgb_m3_render_scx_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        "gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_5.gbc",
        "gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_8.gbc",
        "gambatte/scx_during_m3/scx_0063c0/scx_during_m3_ds_5.gbc",
        "gambatte/scx_during_m3/scx_0063c0/scx_during_m3_ds_8.gbc",
    ];
    for rel in targets {
        assert_pixel_leg_eager(&root, rel, Model::Cgb);
    }
}

/// DMG line-153 LYC=153 IF-emission decouple + the LYC-153 window
/// sibling-cluster. The DMG `ly_for_comparison` line-153 table sets 153 only at
/// dot 6 (the READ frame, `GB_SLEEP(14,4)`), but the LYC STAT IF must emit at
/// dot 4 (the DISPATCH frame): `stat_irq/reclock.rs` (the C015 two-latch split)
/// emits `IF |= 2` at dot 4 while the register-read latch stays dot 6 — not a
/// dispatch move (mooneye `intr_2_*` green), pinned by `m1statwirq_3` (want 2).
/// The dot-4 emission moves every ISR-timed WY write in the shared LYC=153
/// handler 4 dots earlier, retuning the DMG window compensations: the
/// `win_extends_sb` deadline (`read_laws_exit.rs`) re-splits the mid-line `_2`
/// extend / `_3` bare siblings, and the `wy_xline_trig` classify dot (`regs.rs`)
/// re-splits the head/boundary family. DMG-scoped.
#[test]
fn eager_dmg_lyc153_cluster_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_lyc153_cluster",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rows = [
        // Mechanism 1: the dot-4 IF-emission decouple.
        ("gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb", "2"),
        // Mechanism 2: the mid-line late-WY `_2` extend / `_3` bare re-split.
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_3_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx3_3_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_wx0f_3_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // Mechanism 3: the cross-line/head-write `wy_xline_trig` re-classify.
        (
            "gambatte/window/arg/late_wy_FFto0_ly2_3_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto1_ly2_3_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_10to0_ly1_3_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // The WX=0 co-incident-trigger BARE exit (Arm D-wx0, read_laws_exit.rs).
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_wx00_3_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // The line-153 retrigger ack-squash window (speed.rs): `_2` squashes
        // (E0) while `_1` still delivers (E2).
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_late_retrigger_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // The DMG ly0 dot-4 OAM co-instant mask disable (ff0f.rs): `_2` reads
        // the pulse (out2), `_1` stays clear.
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_2_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // The (0,4) LYC-write compare-wrap un-block re-enable (lyc.rs): `_3`
        // fires at dot 4 (outE2), `_1`/`_2` stay blocked.
        (
            "gambatte/lycEnable/lycwirq_trigger_ly00_stat50_3_dmg08_cgb04c_outE2.gbc",
            "E2",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}
