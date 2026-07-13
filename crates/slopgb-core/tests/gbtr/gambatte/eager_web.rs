//! Eager-clock CGB single-speed dispatch/IRQ web pins: the
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

/// The double-speed extension of the write-conflict commit port: a CGB
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

/// The eager ack-squash port: a post-ack mode-0 STAT retrigger must
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

/// The DMG re-host of the FF0F IF-clear write-conflict borrow. DMG is
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

/// The eager CGB sub-M-cycle halt-wake port: the last bounded C3-flip
/// piece. A CGB halt exiting on the mode-0 STAT rise wakes at the flip's own
/// M-cycle boundary (`Ppu::m0_stat_flip_reached`, a pure dot-space peek — no
/// machine advance, timer-safe), not the whole-M-cycle IF commit that collapses
/// two SCX-shifted flips onto one boundary; the resumed IME=1 dispatch's first
/// FF41 read then rides the re-fetch line boundary to mode 2
/// (`Ppu::halt_refetch_read_override`). The two coupled: the wake peek separates
/// the wake instant (scx2_3a dot 256 → mode-0 read, scx3_3b dot 260 → mode-2
/// read) so the read override fires with zero collateral — where the entry peek
/// or the read shift ALONE each dropped a SameBoy-pass row.
/// The bar targets (`_3a` want0, `dec_2` want6, m0irq `_3b` want2) AND the row
/// the coupling saves (m0int `_3b` want2, dropped by the entry peek alone) all
/// pass; the want-0 `_1a` sibling must stay 0 (the read override must not leak).
/// +14 SameBoy-PASS BUG rows, zero drops (`classify_cgb_regr.py` → BUG=14).
/// Reverting either the wake peek or the read override makes this pin fail.
/// Eager+CGB single-speed scoped → production/tier2/DMG byte-identical.
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
        // The five true-bar targets.
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
        // The row the coupling saves: the entry peek alone drops this,
        // the read override recovers it — the discriminator the
        // whole port turns on.
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
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager halt-wake): {e}"));
    }
}

/// The eager WINDOW mode-0 STAT-IF read-frame DELIVER: a window-line
/// `m0irq` poll of FF0F must observe the STAT bit SameBoy's cc+4 events-first
/// frame delivers. With a window active the eager clock keeps the PRODUCTION,
/// window-elevated mode-0 flip R, but SameBoy (and tier2's render-length
/// reclock) rises the mode-0 STAT source 2 dots (half an M-cycle) earlier at
/// R-2; the eager cc+0 read landing in the `[R-2, R)` gap reads clear (E0/0)
/// where SameBoy has already delivered (`m2int_wxA5/A6_m0irq_2`: read R-1,
/// want 2/E2). Arm (a-eager) in [`Ppu::ff0f_stat_peek`] folds the bit in.
/// The `win_active` gate is the discriminator: NON-window `m0irq` rows keep
/// R == SameBoy's rise (no back-date) so their `_1` polls (want 0) must NOT
/// deliver — the scope guard below (`m2int_m0irq_1`, `m2int_wxA6_m0irq_1`)
/// stays 0 both with and without the arm; a broadened arm (dropping the
/// `win_active` gate or widening past `[R-2, R)`) re-sets their bit and fails
/// this pin. Recovered: 1 CGB BUG + 3 DMG BUG (`classify_{cgb,dmg}` → BUG),
/// zero drops both models. Reverting the arm makes the `_2` targets read 0.
/// Eager + SS scoped (DS takes the (a) arm) → production/tier2 byte-identical.
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
        let mut gb = harness::boot_eager(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, is_cgb).unwrap_or_else(|e| {
            panic!("{rel} [{model:?}] expected out{expect} (eager window m0irq): {e}")
        });
    }
}

/// The eager DMG line-0 OAM-entry read-frame back-date: a line-0
/// dot<4 FF41 read appearing on the eager clock (cc+0) maps to its cc+4 =
/// line-0 dot 4-7 = the OAM scan (mode 2), the DMG twin of the `(1..144)`
/// line-start arm. Gated on `!line_render_done` — the discriminator vs the
/// mooneye `stat_lyc_onoff` LCD-enable poll (line-0 dot 0, want mode 0), which
/// resolves `lrd=1` (no pending scan). Recovers two SameBoy-PASS bar rows:
/// `ly0/lycint152_ly0stat_3` (want C2 — its A/B sibling `_2` reads eager
/// LY=153, untouched) and `enable_display/frame1_m2stat_count_2` (want 90).
/// Reverting the `line == 0 && !line_render_done` arm makes this pin fail (the
/// reads fall back to native mode 0). Eager + DMG scoped → production/tier2/CGB
/// byte-identical; mooneye 3-clock green (`stat_lyc_onoff` holds via the guard).
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
        // The A/B sibling that must stay mode 0 (its verdict read is the
        // earlier eager LY=153 read, not the line-0 read the arm rewrites).
        (
            "gambatte/ly0/lycint152_ly0stat_2_dmg08_cgb04c_outC0.gbc",
            "C0",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_eager(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager ly0 oam): {e}"));
    }
}

/// HALFDOT — the DMG line-153 FF41 write-commit half-dot, the coupled
/// odd-half STAT engine's first wall-1 recovery. On line 153 the DMG FF41
/// engine-view (`eng_stat`) write commits its disable ~2 dots later than the
/// eager cc+4 whole-dot landing (the line-153 write quirk): SameBoy's
/// VBLANK-disable lands COINCIDENT with the LYC=153 re-latch (dot 6), so the
/// held LYC match keeps the STAT line high across the disable → no fresh edge
/// (`want E0`). slopgb's whole-dot cc+4 commit dropped the line 2 dots before
/// the LYC re-latch → spurious edge → E2. The deferral is applied to ONLY the
/// engine `eng_stat` view via the odd-half `Ppu::stat_update_half`, leaving the
/// LYC re-latch schedule the window/DMA/`_2` neighbours consume UNTOUCHED — so
/// this recovers the exact target pair with ZERO family shuffle (the
/// whole-dot LYC back-date measured netted +3 DMG / +17 CGB). The CGB
/// siblings of these two rows already pass via the two-phase `eng_stat_pending`
/// (pinned in `eager_write_conflict_commit_passes`); this is the DMG twin.
/// Reverting the `eng_stat_half` line-153 defer makes this pin fail (the commit
/// lands at dot 4, the line dips → E2). Eager + DMG + line-153 scoped →
/// production/tier2/CGB byte-identical; EV DMG 54 → 52 clean, EV CGB 295
/// unchanged, mooneye 93×3.
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
        let mut gb = harness::boot_eager(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "E0", false).unwrap_or_else(|e| {
            panic!("{rel} [Dmg] expected outE0 (eager line-153 m1disable): {e}")
        });
    }
}

/// The SCX (FF43) CGB DOUBLE-SPEED mid-mode-3 render commit RE-HOSTED onto the
/// eager clock — the DS extension of the shipped single-speed DMG SCX
/// write-commit cracks. These 4 `scx_during_m3_ds` rows write SCX
/// twice per line, both POST-match (after this line's fine-scroll comparator lock
/// `hunt_done`, at `hunt_match_dot`=89; write dots 90/232 and 96/226) — a pure
/// coarse/tile shift with no mode-3-length effect. On the DS grid the uniform CGB
/// DS render debt (4hd) over-shoots the eager cc+0 commit by exactly one whole dot
/// (eager stage 90 + 8hd = dot 94, but OFF/tier2 commit dot 93); the post-match
/// arm's debt 2 (6hd) lands the eager commit on the exact OFF/tier2 dot
/// (`regs/stage.rs`). Post-match-scoped (the `_ds_1` pre-match line-start write
/// keeps 4), `eager_value`+`is_cgb`+`ds`-gated → production + tier2 byte-identical
/// (golden unchanged, tier2 CGB 291 untouched, OCR flagon_probe EV 287/287
/// zero-drift). Pixel two-bin EV: these 4 recovered, 0 OFF-passing rows dropped
/// (`scy_during_m3_ds_5` stays the pre-existing floor — SameBoy matches neither
/// ref nor eager). Reverting the arm makes this pin fail (8px, +1 dot late). See
/// `eager-ds-scx-2026-07-12.md`.
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

/// The eager line-153 LYC=153 IF-emission decouple + the LYC-153 window
/// sibling-cluster re-host. `m1statwirq_3` fails on the eager clock
/// because the DMG `ly_for_comparison` line-153 table sets 153 only at slopgb
/// dot 6 (the READ frame, `GB_SLEEP(14,4)`), so the `stat_update` engine emits
/// the LYC STAT IRQ at dot 6 — mid-M-cycle — and the eager CPU recognises it one
/// M-cycle late, carrying the offset to the FF41 glitch write (got 0, want 2).
/// SameBoy sets `IF |= 2` at `display_cycles == 4` (the DISPATCH frame); the fix
/// emits the IF at dot 4 (`stat_irq/reclock.rs`, the C015 two-latch split) while
/// the register-read latch stays dot 6 — NOT a dispatch move (mooneye `intr_2_*`
/// all green). That dot-4 emission moves every ISR-timed WY write in the shared
/// LYC=153 handler 4 dots earlier, tipping the DMG window compensations: the
/// `win_extends_sb` deadline (`read_laws_exit.rs`, `+2 → −2`) re-splits the
/// mid-line `_2` extend / `_3` bare siblings, and the `wy_xline_trig` classify
/// dot (`regs.rs`, `+4` read-debt) re-splits the head/boundary family. All
/// `eager_value && !is_cgb`-scoped → production + tier2 byte-identical (golden
/// unchanged, DMG flagon_probe EV 46, CGB EV 287/287). Reverting any of the
/// three arms makes this pin fail. See `eager-lyc153-cluster-rehost-2026-07-12.md`.
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
        // The target: the dot-4 IF-emission decouple (mechanism 1).
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
        // The line-153 retrigger ack-squash widen (speed.rs 6→10): `_2` squash
        // (E0) while `_1` still delivers (E2).
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_late_retrigger_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // The DMG ly0 dot-4 OAM co-instant mask disable under eager
        // (ff0f.rs): `_2` reads the pulse (out2), `_1` stays clear.
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_2_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // The (0,4) LYC-write compare-wrap un-block re-enable under eager
        // (lyc.rs): `_3` fires at dot 4 (outE2), `_1`/`_2` stay blocked.
        (
            "gambatte/lycEnable/lycwirq_trigger_ly00_stat50_3_dmg08_cgb04c_outE2.gbc",
            "E2",
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
