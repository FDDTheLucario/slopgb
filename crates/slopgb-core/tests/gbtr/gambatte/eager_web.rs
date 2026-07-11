//! Eager-clock CGB single-speed dispatch/IRQ web pins (#11dd): the
//! write-conflict commit port. A CGB `GB_CONFLICT_WRITE_CPU` engine write
//! (FF41 STAT / FF0F IF / FF45 LYC) commits its engine-visible effect one T
//! into the M-cycle; the eager whole-M-cycle tick lands it at the boundary
//! instead. The [`Interconnect::write`] borrow moves the commit to the
//! WriteCpu dot (D+1) under `eager_value`, recovering these SameBoy-pass rows.

use super::*;

/// The eager write-conflict-commit port: CGB single-speed FF41/FF0F/FF45
/// engine writes commit at the WriteCpu dot (M-cycle boundary + 1), not the
/// boundary. Covers both divergence directions — a *missing* STAT bit (a
/// disable that must NOT kill the source before its latch: `ff41_disable_2`
/// want 2, `lyc0_ff41_disable_2` want E2) and a *spurious* STAT bit (a
/// disable/IF-clear that must land after/at the rise: `lycdisable_ff41_2`
/// want 0, `m2int_m0irq_scx3_ifw_2` want 0 via the co-instant rise fold +
/// squash, `lycint152_lyc153irq_ifw_2` want E0). Each is a SameBoy-PASS row
/// inside the true C3-flip bar (`classify_cgb_regr.py` → BUG). Reverting the
/// borrow makes this pin fail (the commit lands a dot early). Eager+CGB SS
/// scoped → production/tier2/DMG byte-identical.
#[test]
fn eager_write_conflict_commit_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_write_conflict_commit",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // Twelve BUG rows the borrow recovers, one per divergence shape/register.
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
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}

/// The double-speed extension of the write-conflict commit port (#11df): a CGB
/// DS bit1-clearing FF0F write must consume the mode-0 STAT rise landing 1-2
/// dots later. At DS SameBoy's WriteCpu commits half a dot into the M-cycle,
/// but the eager whole-M-cycle tick already lands the commit on the SAME dot as
/// the tier2 deferred path (measured), so no commit-dot borrow is needed — only
/// the `stat_if_squash` arm, which the SS borrow gated behind `!double_speed`.
/// With the arm, the DS mode-0 squash window (`w=2`, Δ1-2) consumes the rise for
/// the `_ds_2` rows while the `_ds_1` siblings (Δ3-4) survive. Both are
/// SameBoy-PASS BUG rows (`classify_cgb_regr.py` → BUG). Reverting the DS arm
/// makes this pin fail (the rise re-sets IF → got 2). Eager+CGB+DS scoped →
/// production/tier2/single-speed byte-identical.
#[test]
fn eager_ds_write_conflict_commit_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ds_write_conflict_commit",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // The two DS FF0F ifw rows the arm recovers (want 0 = the rise squashed).
    let rows = [
        "gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_ds_2_cgb04c_out0.gbc",
        "gambatte/m2int_m0irq/m2int_m0irq_scx4_ifw_ds_2_cgb04c_out0.gbc",
    ];
    for rel in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out0 (eager DS): {e}"));
    }
}

/// The eager ack-squash port (#11de): a post-ack mode-0 STAT retrigger must
/// stay CONSUMED by its dispatch's IF clear. The eager read-frame enters the
/// STAT/OAM ISR the read-debt earlier than gambatte's cc+4 frame (+8hd = 4
/// dots SS / 2 dots DS), so the eager ack fires that far before the fixed-dot
/// retrigger and the production 2-dot `ack_squash_dots` window no longer
/// reaches it; the `_2` rows wrongly re-deliver IF (got E2, want E0). Widening
/// the eager LCD-bit window by the shift (SS 6, DS 3) re-consumes the retrigger
/// while the one-M-cycle-later `_1` siblings still land outside and DELIVER.
/// Each row is a SameBoy-PASS BUG (`classify_cgb_regr.py` → BUG). Reverting the
/// window widen (back to `2`) makes this pin fail (the retrigger re-delivers).
/// Eager-scoped → production/tier2 byte-identical.
#[test]
fn eager_ack_squash_retrigger_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ack_squash_retrigger",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // Five BUG rows the widened window re-consumes: the SS + DS irq_precedence
    // mode-0 retriggers plus the mode-2 SS retrigger family.
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
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}

/// The DMG re-host of the FF0F IF-clear write-conflict borrow (#11dj). DMG is
/// single-speed with the same 4-dot M-cycle and 1-T WriteCpu commit as CGB SS,
/// so the identical whole-dot borrow (`Interconnect::write`, un-scoped from
/// is_cgb for addr FF0F only) moves a bit1-clearing FF0F write's commit to the
/// WriteCpu dot (D+1) and folds a co-instant STAT rise into `intf` first, then
/// squashes it. Three SameBoy-PASS BUG rows (`classify_dmg.py` → BUG) recover:
/// the mode-0 IF-clear straddle (`m2int_m0irq_scx3_ifw_2` want 0,
/// `m2int_m0irq_scx3_ifw_4` want 0) and the line-153 LYC IF-write
/// (`lycint152_lyc153irq_ifw_2` want E0). The FF41/FF45 WriteCpu borrow does
/// NOT cross to DMG (a net-negative A/B swap on the `m0enable/lycdisable_*`
/// siblings), so the DMG borrow is FF0F-only. Reverting the DMG FF0F scope
/// makes this pin fail (the rise re-sets IF → got 2/8/E2). Eager+DMG+SS scoped
/// → production/tier2/CGB byte-identical.
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
        let mut gb = harness::boot_eager(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}
