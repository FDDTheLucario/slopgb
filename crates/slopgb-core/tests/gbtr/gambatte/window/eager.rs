//! Window pinned-behavior tests.

use super::super::*;

/// DMG late-WY write-side latches in FF4A writes: the head-write cross-line
/// extend (`value + 1 == line` → `wy_xline_trig`, feeds arm 7) and the
/// single-speed trigger-line un-latch (`old_wy == ly && value != ly` →
/// `wy_trig_sb_raw = false`, feeds arm D6). Pins the `_2` boundary pairs:
/// `late_wy_10to0_ly1_2`/`FFto0_ly2_2`/`FFto1_ly2_2` (out3),
/// `1toFF_2`/`2toFF_2` (out0). The `_1` siblings are baselined (floor-class
/// index).
#[test]
fn eager_dmg_late_wy_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_late_wy",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 5] = [
        (
            "gambatte/window/arg/late_wy_10to0_ly1_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto0_ly2_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto1_ly2_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_1toFF_2_dmg08_out0_cgb04c_out3.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_2toFF_2_dmg08_out0_cgb04c_out3.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}

/// DMG late-WY `_1` boundary siblings. The `_1` writes commit one dot past the
/// head, so the write-latch alone (`eager_dmg_late_wy_passes`) does not cover
/// them; two DMG-scoped mechanisms close the gap. UN-trigger (`1toFF_1` /
/// `2toFF_1`, out0): the dot-0 WY→FF write latched the wy2-lagged shadow
/// (`wy_trig_sb`) at line start; the render never draws (`win_active` false),
/// so the sticky shadow blocked the arm-8 emergent bare exit and the read
/// over-held — the `regs.rs` single-speed un-latch releases the shadow +
/// commits wy2 (mirror of the DS un-latch). EXTEND (`10to0_ly1_1` /
/// `FFto0_ly2_1` / `FFto1_ly2_1`, out3): the render over-triggers the
/// cross-line seam (`win_active`), so arm D1 uses the cross-line 263 (not the
/// steady 259) when `wy_xline_trig`. The `scx2`/`scx3` extend siblings and the
/// `late_disable`/`reenable`/`m2int` window residual are baselined (floor-class
/// index).
#[test]
fn eager_dmg_late_wy1_rehost_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_late_wy1_rehost",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 5] = [
        (
            "gambatte/window/arg/late_wy_1toFF_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_2toFF_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_10to0_ly1_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto0_ly2_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto1_ly2_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}

/// Three DMG window-exit latch recalibrations. Each `_1`/`_2` (or `_0`/`_2`)
/// sibling pair differs by a whole-M-cycle NOP that shifts a WRITE, latched as
/// a representable render dot.
///  (1) `late_reenable{,_wx0f}_2` (out0): arm-D5 reenable — the bare threshold
///      takes +4 off `win_reenable_dot` (reen 94 + 4 > wxm 97 → bare).
///  (2) `late_scx_late_disable_0` (out0): arm-D3 pre-draw abort — the mid-line
///      SCX rewrite is admitted; the fetch-ship deadline K widens to 8 (fscx-4
///      fine-scroll) and the bare exit back-dates one dot (253→252) so the
///      early-abort `_0` reads mode 0.
///  (3) `late_wy_FFto2_ly2_scx{2,3}_1` (out3): arm-2 shadow — the DMG
///      first-window-line (`wy2 == ly`) trigger extends even when the render
///      activated (`win_active`), on-screen WX only.
/// DMG-scoped.
#[test]
fn eager_dmg_window_latch_recalib_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_window_latch_recalib",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 5] = [
        ("gambatte/window/late_reenable_2_dmg08_cgb04c_out0.gbc", "0"),
        (
            "gambatte/window/late_reenable_wx0f_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_scx_late_disable_0_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx2_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx3_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}

/// Five DMG STAT-source re-latch rows. Each `_1`/`_2` (or `_2`/`_3`) sibling
/// pair differs by a whole-M-cycle NOP that shifts a WRITE, latched as a
/// representable render dot; the consuming law resolves at the cc+4 read/write
/// frame.
///  (1) `m2enable/late_enable_2` (out0) + `late_enable_after_lycint_disable_2`
///      (dmg08 out0): the FF41 m2-enable RETRO pulse-reach window `{0,4}` + the
///      data-only dot-0 lycen add the +4 read-debt (`rd = dot + 4`) so the
///      retro resolves at cc+4 (`regs.rs` 0xFF41 write).
///  (2) `m2enable/late_enable_m0disable_2` (dmg08 out0): a fresh OAM enable in
///      the lines-1-143 mode-2 carry window (dots 0-3) is seeded HIGH (STAT
///      blocking) so the dot-engine raises no spurious mid-mode-2 IF (`regs.rs`).
///  (3) `lycEnable/lycwirq_trigger_ly00_stat50_2` (dmg08 outE0): the line-0
///      vblank-carry → LYC=0 seamless handoff — a matching LYC write the m1
///      block suppresses seeds the engine line HIGH (`lyc.rs write_lyc_dmg`),
///      firing `_3` in the visible branch.
///  (4) `m2int_m3stat/scx/late_scx4_2` (out0): the spurious mid-mode-3 SCX
///      fine-scroll extension is backed out of the BARE-line exit verdict
///      (`read_laws_exit.rs` Arm 8).
/// DMG-scoped.
#[test]
fn eager_dmg_stat_relatch_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_stat_relatch",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 5] = [
        ("gambatte/m2enable/late_enable_2_dmg08_cgb04c_out0.gbc", "0"),
        (
            "gambatte/m2enable/late_enable_after_lycint_disable_2_dmg08_out0_cgb04c_out2.gbc",
            "0",
        ),
        (
            "gambatte/m2enable/late_enable_m0disable_2_dmg08_out0_cgb04c_out2.gbc",
            "0",
        ),
        (
            "gambatte/lycEnable/lycwirq_trigger_ly00_stat50_2_dmg08_outE0_cgb04c_outE2.gbc",
            "E0",
        ),
        (
            "gambatte/m2int_m3stat/scx/late_scx4_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}

/// Eight CGB double-speed STAT-bar rows, the DS twins of the SS/DMG re-latch
/// families. Each resolves to a representable whole-M-cycle latch.
/// (a) the glitch-line mode-0 read-view mask (`ff0f_cgb_ds_glitch_m0_mask`):
/// the DS frame emits the glitch mode-0 STAT early, so a poll before the true
/// rise reads the pre-rise value (`ly0_m0irq_scx0/1_ds_1`,
/// `frame0_m0irq_count_scx2/3_ds_1`). (b) the DS carried mode-2 line-start read
/// debt (`read_laws.rs` `read_pos_hd >= 4`, not raw `dot >= 2`):
/// `m2int_m0stat_ds_2`. (c) the shifted-frame flip twin (`read_laws.rs`
/// lcd_offset arm): `offset1_lyc99int_m0stat_count_scx2_ds_1`. (d) the DS
/// window+sprite emergent exit (`read_laws_exit.rs` Arm 8-spr, `2*flip + 1`):
/// `10spritesPrLine_wx7_m3stat_ds_2`. (e) the DS mode-0 ack-squash window 4 for
/// the HBLANK retrigger family (`speed.rs` `stat_src_hblank`):
/// `late_m0irq_retrigger_scx1_ds_2`. CGB + DS scoped.
#[test]
fn eager_cgb_ds_relatch_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_cgb_ds_relatch",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 8] = [
        (
            "gambatte/enable_display/ly0_m0irq_scx0_ds_1_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/enable_display/ly0_m0irq_scx1_ds_1_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/enable_display/frame0_m0irq_count_scx2_ds_1_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/enable_display/frame0_m0irq_count_scx3_ds_1_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/m2int_m0stat/m2int_m0stat_ds_2_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/lcd_offset/offset1_lyc99int_m0stat_count_scx2_ds_1_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/sprites/space/10spritesPrLine_wx7_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/irq_precedence/late_m0irq_retrigger_scx1_ds_2_cgb04c_outE0.gbc",
            "E0",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}
