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
