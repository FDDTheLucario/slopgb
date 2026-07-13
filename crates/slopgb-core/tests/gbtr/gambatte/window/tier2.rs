//! Window tier2-reclock pinned-behavior tests.

use super::super::*;

/// The tier2 SCX write-strobe render-frame
/// deferral. The deferred clock lands pipeline-register writes at SameBoy's
/// true commit instant, but the production render geometry (the fine-scroll
/// comparator hunt at dots 89-96) is calibrated to the cc+4 frame, 4 dots
/// late of that instant — so the pipeline-view SCX commit must lag the same
/// 4 dots (`write_deferred` stages 3 dots under tier2, the stage surviving
/// the architectural write) for a mid-hunt SCX write to straddle the
/// comparator like hardware. `late_scx4`: the `_1` leg's write beats the
/// first comparator sample (picks up SCX=4, mode 3 extends +4, read=3), the
/// `_2` leg (one M-cycle later) misses it (matches at 0, bare, read=0);
/// slopgb collapsed both onto the leading edge so both extended. SS+DS + the
/// scx_during_m3 extend + the late_scx_late_disable window pair — full-CGB
/// two-bin `+6/−0`. The glitch line keeps the immediate commit (its render
/// geometry carries its own glitch-line offsets: `ly0_late_scx7_m3stat_*`), and the
/// FF41-read shadow laws sample the ARCHITECTURAL `scx` (their calibration
/// frame) rather than the lagged pipeline view.
#[test]
fn tier2_late_scx_writestrobe_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_late_scx_writestrobe",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    for (rel, want) in [
        // The straddle pair, both speeds: `_1` extends (want 3), `_2` bare.
        (
            "gambatte/m2int_m3stat/scx/late_scx4_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m2int_m3stat/scx/late_scx4_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/m2int_m3stat/scx/late_scx4_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m2int_m3stat/scx/late_scx4_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // A mid-mode-3 SCX write extending the fine scroll (want 3).
        (
            "gambatte/scx_during_m3/scx_m3_extend_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        // The glitch-line immediate-commit guard (want mode 3 + LYC = 87).
        (
            "gambatte/enable_display/ly0_late_scx7_m3stat_scx3_1_dmg08_cgb04c_out87.gbc",
            "87",
        ),
    ] {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), want, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{want} (tier2 flag-on): {e}"));
    }
}

/// The CGB pre-draw window-abort BARE-exit slice
/// (`stat_irq.rs::vis_mode_read`, `regs.rs`/`render/window.rs` `win_predraw_abort`).
/// An LCDC.5 clear that lands BEFORE the enabled window's first fetch renders the
/// line bare on SameBoy but with the SCX fine-scroll penalty DROPPED (mattcurrie
/// §WIN_EN) → mode-3 exit cfl257, not 257+SCX&7; slopgb's whole-dot render
/// over-extended it. `late_disable_early_scx03_wx{0f,10,11,12}_1` (LCDC.5 cleared
/// at dot104, pre-first-tile) read out0 flag-on (+4/−0, MEASURED). The `_2`
/// siblings (dot108, post-first-tile) EXTEND mode 3 (want out3) — a per-config
/// window-tile-completion length left to the atomic render reclock, excluded by
/// the `win_predraw_abort_dot <= 105` pre-first-tile scope. CGB single-speed only.
#[test]
fn tier2_window_predraw_abort_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_predraw_abort",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 12] = [
        (
            "gambatte/window/late_disable_early_scx03_wx0f_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_early_scx03_wx10_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_early_scx03_wx11_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_early_scx03_wx12_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // The DOUBLE-SPEED twin (the `(89 + WX) & !1` first-fetch
        // M-cycle boundary; `_1` aborts commit before it → bare, `_2` at/
        // after → extends). All 8 legs pinned: the `_2` extends guard the
        // boundary from both sides.
        (
            "gambatte/window/late_disable_early_scx00_wx0f_ds_1_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_early_scx00_wx10_ds_1_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_early_scx00_wx11_ds_1_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_early_scx00_wx12_ds_1_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_early_scx00_wx0f_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/late_disable_early_scx00_wx10_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/late_disable_early_scx00_wx11_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/late_disable_early_scx00_wx12_ds_2_cgb04c_out3.gbc",
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

/// The CGB window-REENABLE mode-3 length slice
/// (`stat_irq.rs::vis_mode_read`, `Render::win_reenable_dot`). A window disabled
/// then re-enabled mid-mode-3 (`late_reenable`) redraws from the re-enable point;
/// its mode-3 extends past the read iff the re-enable beat the WX-match redraw
/// start (`re-enable_dot <= wx_match_dot - 3`, MEASURED). A LATE re-enable renders
/// the tail bare (mode0). slopgb's whole-dot render collapsed both legs to mode3.
/// `late_reenable_{2,scx2_2,scx3_2,wx0f_2}` read out0 flag-on (+4/−0). SCX&7 <= 3
/// (the fine-scroll deadline shift at high SCX is the atomic reclock's; scx5+
/// pass natively). CGB single-speed.
#[test]
fn tier2_window_reenable_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_reenable",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 4] = [
        ("gambatte/window/late_reenable_2_dmg08_cgb04c_out0.gbc", "0"),
        (
            "gambatte/window/late_reenable_scx2_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_reenable_scx3_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_reenable_wx0f_2_dmg08_cgb04c_out0.gbc",
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

/// The CGB late-WY UN-trigger bare slice
/// (`stat_irq.rs::vis_mode_read`, `Ppu::wy_trig_sb_raw`). SameBoy's `wy_check`
/// compares LY against the IMMEDIATE WY; a late WY→(non-LY) write un-triggers its
/// window (raw WY != LY at the mode-2 compare → bare line), while slopgb's render +
/// `wy_trig_sb` read the 6-dot-lagged `wy2` and trigger it (over-extend mode 3). The
/// raw-WY sticky shadow (immediate `self.wy`, gated `dot >= 4` = the settled-WY
/// compare window) re-derives SameBoy's trigger; `win_active && !wy_trig_sb_raw` forces
/// the bare mode-0 exit. `late_wy_{1toFF,2toFF}_1` (WY→FF at dot0) read out0 flag-on
/// (+3/−0); the `_3` siblings (WY→FF at dot8, sticky-triggered) keep mode 3. CGB SS.
#[test]
fn tier2_window_late_wy_untrigger_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_late_wy_untrigger",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 2] = [
        (
            "gambatte/window/arg/late_wy_1toFF_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_2toFF_1_dmg08_cgb04c_out0.gbc",
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


/// The window visible-mode-3 LENGTH law (the FF41-read
/// half of the atomic reclock). A triggering window's SameBoy mode-3→0 exit is
/// `SBex = 263 + SCX&7` (cfl); the CPU-visible FF41 exit is `SBex − read_offset`.
/// The deferred FF41 read samples +4 dots before SameBoy's read (MEASURED:
/// `m2int_wx03_scx5_m3stat_2` slopgb dot264 ↔ SameBoy cfl268=SBex), NOT the +3
/// dispatch frame — so the exit is `259 + SCX&7`. DECOUPLED from
/// `line_render_done` (the counter-pinned dispatch, config-dependently
/// mis-positioned vs SBex so slopgb over-extends — the `m2int_wx*_m3stat_2` reads
/// see mode 3 where SameBoy reads 0). Applied ONLY to the FF41 register read
/// (`stat_irq.rs::vis_mode_read`, NOT the STAT-line `vis_mode` consumers), CGB
/// normal-trigger ly≥1 windows (`win_active && !win_aborted && wy2!=ly && wy2<=143
/// && wx<0xA0 && !ds`). Line 0 / late-WY / WY-disable windows are EXCLUDED — their
/// reads de-mask an entangled read-frame error (the window length + read-frame
/// co-land in the atomic step); the normal windows have a correct read-frame,
/// so the length law fixes them cleanly. Full-CGB two-bin flag-on +9/−0 (+7
/// at exit 260, +2 more at 259 — the scx5 `_2` over-extend rows). Production
/// byte-identical OFF (`win_active`/`tier2` never fire there). The DMG legs keep
/// their floor (the offset is CGB-measured; `is_cgb` gate).
#[test]
fn tier2_window_m3stat_length_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_m3stat_length",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // All [Cgb] out0 — the normal-trigger window mode-3 read exits at 259+SCX&7
    // (SBex 263+SCX&7 − the measured +4 read offset). The scx5 `_2` legs
    // pin the 259 (vs 260) calibration; the scx0 legs read past the exit either way.
    // The wxA5/wxA6 legs pin the off-screen-window extension (the same
    // 259+SCX&7 exit applies to the off-screen-trigger window; sprite-free).
    let rels = [
        "gambatte/window/m2int_wx00_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx03_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx03_scx5_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx03_scx3_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx07_scx2_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wxA6_m3stat_3_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wxA5_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wxA6_scx5_m3stat_3_dmg08_cgb04c_out0.gbc",
    ];
    for rel in rels {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out0 (tier2 flag-on): {e}"));
    }
}

/// The window render-level **shadow WY-trigger** (the
/// late-WY half of the window model). SameBoy latches `wy_triggered` from a
/// *continuous* `WY == LY` compare (`display.c` `wy_check`), but slopgb's
/// production `wy_latch` samples only at the three gambatte weMaster dots (line 0
/// dot 2, dots 450/454) — so a *mid-line* late-WY write that SameBoy catches is
/// MISSED by slopgb's discrete sampler, and slopgb renders the line BARE where
/// SameBoy's window triggered and extended mode 3 to `263 + SCX&7` (the POLLED
/// read exit, +0). The shadow [`Ppu::wy_trig_sb`] re-derives SameBoy's decision
/// — sticky `WY == LY` latch + the WX-activation deadline ([`Render::wx_match_dot`]
/// `+ 2`, the wy2-copy phase slack) — purely for the FF41-read law
/// ([`Ppu::vis_mode_read`]), NOT `line_render_done`/the render. Fires ONLY when
/// the trigger latched on THIS line (`trig_line == ly`): the cross-line
/// (`trig_line < ly`) latch is left bare because (a) the line-boundary late-WY
/// writes (`10to0`/`FFto0`) land a line later in the deferred frame so the shadow
/// never latches them, and (b) a `!win_active` cross-line latch means the window
/// was aborted / its WX/LCDC.5 toggled late (`late_wx`/`late_reenable`/
/// `late_enable`) — SameBoy renders THOSE bare. Full-CGB two-bin flag-on **+5/−0**
/// (the `_1` mid-line late-WY rows; the `_2`/`_3` siblings + the toggled-window
/// case — the BOUNDARY-WY cross-line window extend
/// (`Ppu::wy_xline_trig`): a WY write landing in a line's tail/head whose
/// value matches the CURRENT line latches SameBoy's `wy_triggered`
/// (scheduled `wy_check`, old `current_line` compare); every later bare
/// line reads mode 3 to the polled window exit. The ly0 mid-line row rides
/// the same-line shadow's new line-0 inclusion. Guards: the boundary write
/// with a NON-matching value + the late-toggled window stay bare.
#[test]
fn tier2_window_boundary_wy_xline_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_boundary_wy_xline",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // FIXED — boundary-WY writes matching the old line extend cross-line.
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
        // FIXED — the ly0 mid-line late-WY trigger (line-0 shadow inclusion).
        (
            "gambatte/window/arg/late_wy_FFto0_ly0_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        // GUARD — the toggled-window want-0 rows stay bare (the xline latch
        // requires a boundary WY write, not an LCDC enable).
        (
            "gambatte/window/late_enable_afterVblank_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_reenable_scx5_2_dmg08_out3_cgb04c_out0.gbc",
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

/// rows stay bare). Production byte-identical OFF (`tier2`/`is_cgb` gated).
#[test]
fn tier2_window_late_wy_extend_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_late_wy_extend",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expected). The `_1` mid-line late-WY rows now extend mode 3 (out3);
    // the `_2` deadline siblings + the cross-line toggled-window rows stay bare
    // (out0) — the regression guards against an over-aggressive shadow (the +2
    // slack boundary, and the `trig_line == ly` gate that excludes late_wx /
    // late_reenable).
    let targets = [
        // FIXED — the shadow extends the missed mid-line late-WY trigger.
        (
            "gambatte/window/arg/late_wy_10to1_ly1_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx2_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx3_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_wx0f_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        // GUARD — the `_2` siblings miss the deadline (+2 slack): stay bare.
        (
            "gambatte/window/arg/late_wy_10to1_ly1_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx2_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — cross-line toggled-window rows: the shadow must NOT extend.
        ("gambatte/window/late_wx_1_dmg08_cgb04c_out0.gbc", "0"),
        (
            "gambatte/window/late_reenable_scx5_3_dmg08_cgb04c_out0.gbc",
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

/// The DMG WINDOW-LAW PORT. The CGB `vis_mode_read`
/// window arms (`model.is_cgb()`-gated length/shadow/pre-draw-abort/reenable/
/// un-catch/boundary laws) are re-derived on the DMG frame: the DMG deferred
/// FF41 read shares the SS −4 polled offset, but the DMG `wy2` lag (+2 vs CGB
/// +6) and the DMG-specific per-WX/SCX ship deadlines make the model DIVERGE
/// from CGB (the `_2` legs that render bare on CGB extend on DMG). 56 of the
/// 62 DMG window flip-blockers fixed; the 6 residual are the same
/// atomic classes CGB parks (wxA6/wxA5 carried-read sub-dot wall · scx5
/// non-linear deadline · mid-frame-SCX-rewrite `late_scx` · the render-trigger
/// late_enable/reenable-scx5 extend). Each new arm is `!is_cgb()`-scoped →
/// CGB two-bin byte-identical (291/0-new); production byte-identical OFF.
#[test]
fn tier2_dmg_window_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_window",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expected DMG out). Representatives of each shipped DMG arm + guard
    // rows (the parked-atomic residual stays at its native verdict).
    let targets = [
        // Arm D1 — the DMG triggering-window length law (259 + SCX&7 exit).
        (
            "gambatte/window/m2int_wx00_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wx07_scx3_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxA6_m3stat_2_dmg08_out0_cgb04c_out3.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxA6_spxA7_m3stat_2_dmg08_out0_cgb04c_out3.gbc",
            "0",
        ),
        // Arm D2 — the DMG mid-line late-WY shadow extend (263 + SCX&7).
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_wx0f_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        // Arm D6 — the DMG late-WY UN-trigger bare exit + the WY→FF release.
        (
            "gambatte/window/arg/late_wy_1toFF_2_dmg08_out0_cgb04c_out3.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_2toFF_2_dmg08_out0_cgb04c_out3.gbc",
            "0",
        ),
        // GUARD — the WY→FF `_3` sibling flips FF a compare later: extends.
        (
            "gambatte/window/arg/late_wy_1toFF_3_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        // Arm D3 — the DMG pre-draw window-abort (bare 253 / extend 259).
        (
            "gambatte/window/late_disable_1_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        (
            "gambatte/window/late_disable_early_scx03_wx0f_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_late_scx03_wx0f_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        // GUARD — the SCX-delayed pre-match clear kills a low-WX window (bare).
        (
            "gambatte/window/late_disable_scx3_0_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // Arm D3-spr — a pre-draw abort with an object on the window line.
        (
            "gambatte/window/late_disable_spx10_wx0f_2_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        // Arm D5 — the DMG reenable-too-late bare exit (SCX-termed deadline).
        ("gambatte/window/late_reenable_2_dmg08_cgb04c_out0.gbc", "0"),
        // GUARD — the SCX2 reenable at the same dot still catches: extends.
        (
            "gambatte/window/late_reenable_scx2_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        // Arm D-wx — the DMG WX-rewrite un-catch (scx ≥ 3 → bare).
        (
            "gambatte/window/late_wx_scx3_2_dmg08_out0_cgb04c_out3.gbc",
            "0",
        ),
        // Arm D7 (boundary head-latch) — a WY head-write matching the finished
        // line triggers the cross-line window (extends every later line).
        (
            "gambatte/window/arg/late_wy_10to0_ly1_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto1_ly2_2_dmg08_out3_cgb04c_out0.gbc",
            "3",
        ),
        // GUARD — the `_3` sibling commits past the head (dot 4): stays bare.
        (
            "gambatte/window/arg/late_wy_10to0_ly1_3_dmg08_cgb04c_out0.gbc",
            "0",
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

/// The WX (FF4B) render-VIEW defer + the un-catch SPLIT. In tier2 the
/// eager `Ppu::write` committed `eff.wx` at the write's leading edge (cc+0), 2-4
/// dots early of the render's per-dot WX comparator, so a mid-mode-3 WX rewrite
/// reached the window activation/reactivation gate at the wrong dot: `late_wx_ds`
/// (DS) — the eager cc+0 WX=255 pre-empted the wx=7 window activation at the next
/// dot → the window never drew (bare cols 0-7); `m3_wx_6` (SS) — the un-catch
/// straddle (a WX 6→5 rewrite must split the `pos_dot==wx+6` compares at pos_dot
/// 11/12 so neither matches) needs the change at the production frame, not early.
/// Fix: `eff.wx` now SURVIVES the arch write (`regs.rs` `staged_pending`) and
/// strobe-commits at leading+1 (the strobe runs at tick-start before `dot += 1`,
/// so the value is visible to `render_step` from leading+2 == production, both
/// speeds; `cycle.rs` FF4B → dots 0, +1 for the FF4B palette-class offset). The
/// SPLIT keeps the un-catch READ law's `wx_write_dot` (FF41 mode-3 length) at its
/// cc+0 leading edge (`regs.rs` `Ppu::write` FF4B, not `commit_eff`) so
/// `tier2_window_late_wx_uncatch_passes` is unperturbed. Render-view only: CGB
/// two-bin 291/291 zero-drift, mooneye 91/91 ON+OFF, production byte-identical OFF.
/// Pixel two-bin +3 (`late_wx_ds_1` Cgb + `m3_wx_5`/`m3_wx_6` Dmg).
#[test]
fn tier2_dmg_m3_render_wx_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_wx",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // FIXED — the DS window-activation pre-empt (Cgb).
        ("gambatte/window/on_screen/late_wx_ds_1.gbc", Model::Cgb),
        // FIXED — the SS mid-draw reactivation / un-catch straddle (Dmg).
        ("mealybug-tearoom-tests/ppu/m3_wx_5_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_wx_6_change.gb", Model::Dmg),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The window-ABORT render/read-law SPLIT: a mid-mode-3 LCDC.5 clear ends
/// the drawn window's RENDER re-anchor at the deferred render frame while its
/// mode-3-length READ-LAW flags fire at the eager cc+0. In tier2 `eff.lcdc`
/// committed the bit5 clear at the write's leading edge (cc+0), so `window_abort`
/// ended the drawn window 2 dots / 2 pixels early of production
/// (`m3_lcdc_win_en_change_multiple`: the abort at lx≈51 stopped the window at
/// cols 50-51 instead of 52-53). Fix: `window_abort` is split — `window_abort_flags`
/// (`win_predraw_abort` / DMG `win_aborted`, the FF41 read-law inputs calibrated
/// to cc+0) stays eager in `regs.rs::commit_eff`; `window_abort_render` (the
/// drawn-window end + BG-fetch tile-boundary re-anchor) fires at the `render_lcdc`
/// bit5 1→0 catch-up (`ppu/mod.rs`), the same deferred fetch view mech2 uses. The
/// window ACTIVATION gate + `win_reenable_dot`/`win_enable_dot` stay eager (a
/// render-view activation defer was BUILT + REFUTED — it dropped
/// `late_enable_ly0_ds_2` / `late_reenable_scx2_2`, SameBoy-passes: the activation
/// dot IS the mode-3 length). Render-only: CGB two-bin 291/291 IDENTICAL SET,
/// mooneye 91/91 ON+OFF, `tier2_window_enable_deadline` + `tier2_dmg_window` held,
/// production byte-identical OFF. Pixel two-bin +2 (Dmg + Cgb) → the full 100/100.
#[test]
fn tier2_dmg_m3_render_win_abort_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_win_abort",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple.gb",
            Model::Dmg,
        ),
        (
            "mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple.gb",
            Model::Cgb,
        ),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The WINDOW family ported to DOUBLE-SPEED: the visible-mode-3
/// length law AND the shadow WY-trigger, with the DS exit/deadline
/// recalibrated. The `vis_mode_read` length law (the `m2int_wx*_m3stat` shorten)
/// and the late-WY shadow extend were both `!ds`-gated; under DS the deferred
/// cc+0 FF41 read lands +1 dot vs SS (the ISR read offset is +3 not +4), so the
/// **length-law exit is `260 + SCX&7`** (`259 + ds`) and the **shadow exit is
/// `264 + SCX&7`** (`263 + ds`). MEASURED: `m2int_wxA6_scx5_m3stat_ds` reads `_1`
/// dot264 / `_2` dot266 → only exit 265 (=260+5) separates them (and does NOT
/// drop the off-screen `_1` SameBoy-pass); `late_wy_FFto2_ly2_scx5_ds_1` reads
/// dot268 → the shadow exit must clear it. The shadow **deadline slack is +4** in
/// DS (the wy2-copy lands the trigdot 2 dots later: `late_wy_FFto2_ly2_ds` `_1`
/// trigdot 101 / `_2` 103 vs wxmatch 97). DS additionally **excludes
/// sprite-laden lines** from BOTH laws (`!ds || n_sprites == 0`) — with sprites
/// the real mode-3 end extends past the bare exit and the DS read frame straddles
/// it (`sprites/space/10spritesPrLine_wx*_m3stat_ds_1` would drop, a SameBoy-pass;
/// that is the DS sprite read-grid, separate). Full-CGB two-bin flag-on
/// **+8/−0** (7 length-law `_2` + the shadow `FFto2_ly2_ds_1`). SS legs
/// byte-identical (the `ds` terms are 0 in single speed); production byte-identical
/// OFF. The `scx5_ds_1` length `_1` rows + `late_wy_*_ds` boundary/disable rows
/// stay atomic (the same SCX-non-linear deadline / deferred-frame walls as the
/// single-speed shadow WY-trigger).
#[test]
fn tier2_window_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // FIXED — DS length law (`m2int_wx*_m3stat_ds_2`, want0): shorten at 260+SCX&7.
        (
            "gambatte/window/m2int_wx03_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wx07_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxDefault_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // FIXED — the off-screen wxA6 DS pair (`_2` shortens; `_1` must stay mode3).
        (
            "gambatte/window/m2int_wxA6_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxA6_scx5_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — the off-screen `_1` (exit 265 separates it from `_2` dot266).
        (
            "gambatte/window/m2int_wxA6_scx5_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        // FIXED — DS shadow WY-trigger (`late_wy_FFto2_ly2_ds_1`, want3): extend.
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        // GUARD — the DS `_2` deadline sibling stays bare (slack +4 boundary).
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — the DS-sprite exclusion: this `_1` (want3) must NOT be shortened.
        (
            "gambatte/sprites/space/10spritesPrLine_wx0_m3stat_ds_1_cgb04c_out3.gbc",
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

/// A late-ENABLE-triggered window whose LCDC.5 write lands
/// past the line's fetch-catch deadline (dot > 94, DS) renders BARE on
/// SameBoy (the window misses the line) while slopgb's whole-dot render still
/// activates and extends. The `late_enable_ly0_ds` want-pair reads the
/// IDENTICAL dot 260 with opposite wants — the enable dot (94 vs 96) is the
/// only discriminator, so this is a render-length law keyed on
/// `Render::win_enable_dot` (a FIRST enable: window neither active nor
/// aborted), not a read-position one. `_1` (enable 94) holds natively; `_2`
/// (enable 96) takes the DS bare exit. +1/−0 on the window family;
/// production byte-identical (tier2+CGB+DS law input only).
#[test]
fn tier2_window_enable_deadline_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_enable_deadline",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 2] = [
        ("gambatte/window/late_enable_ly0_ds_1_cgb04c_out3.gbc", "3"),
        ("gambatte/window/late_enable_ly0_ds_2_cgb04c_out0.gbc", "0"),
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

/// A mid-line FF4B (WX) rewrite committing AT/BEFORE the WX
/// match dot un-catches the window on SameBoy at SCX&7 == 5 (the fine-scroll
/// phase pushes the effective catch past the write): `late_wx_scx5_1` (write
/// and match both dot 97) renders BARE (want 0) where slopgb's whole-dot
/// render catches and extends; `_2` (write dot 101) is caught on both. The
/// scope is measured: at scx0/2/3 SameBoy still catches the same
/// write≤match race (the un-scoped arm dropped 8 want-3 rows).
/// `Render::wx_write_dot` + the SS bare-exit arm; +1/−0; production
/// byte-identical.
#[test]
fn tier2_window_late_wx_uncatch_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_late_wx_uncatch",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 2] = [
        ("gambatte/window/late_wx_scx5_1_dmg08_cgb04c_out0.gbc", "0"),
        ("gambatte/window/late_wx_scx5_2_dmg08_cgb04c_out3.gbc", "3"),
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

/// Three window-line slices off one root cause: the win-line
/// render clock sits +2 late in slopgb's frame; the FF41 `vis_mode_read` laws
/// already compensate but three OTHER flip consumers read the raw render
/// clock (dual-traced fp both emulators, 2026-07-03). (1) the win-line mode-0
/// ENGINE rise now leads the flip 2 dots (`m2int_wxA5_m0irq_2`); (2) the
/// wxA6 quirk-window VRAM read release co-moves with the CGB visible exit
/// (`259 + SCX&7`), wxA6-scoped — the generic release was the vramw A/B, and
/// the m0 IF rise fires while VRAM is still locked so it must not key the
/// release (`m2int_wxA6_vrambusyread_3`); (3) the sprite-at-window-X
/// abort-slot removal: an object at OAM X = WX+1 occupies the fetcher's
/// GET_TILE_T1 after the window restart, removing the late CGB abort slot,
/// so an LCDC.5 clear at wx_match−1 leaves the line fully extended
/// (`late_disable_spx10_wx0f_2`).
#[test]
fn tier2_window_line_flip_consumers_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_line_flip",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/window/m2int_wxA5_m0irq_2_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // GUARD — the one-M-earlier read stays clear of the led rise.
        (
            "gambatte/window/m2int_wxA5_m0irq_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // GUARDs — the FF41 read law was already right; must not double-move.
        (
            "gambatte/window/m2int_wxA5_m3stat_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/m2int_wxA5_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxA6_vrambusyread_3_dmg08_cgb04c_out5.gbc",
            "5",
        ),
        // GUARDs — the two earlier reads stay blocked (the CGB exit is one
        // bucket later than DMG; the release must not fire early).
        (
            "gambatte/window/m2int_wxA6_vrambusyread_2_dmg08_out5_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxA6_vrambusyread_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/late_disable_spx10_wx0f_2_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        // GUARD — the one-slot-earlier clear genuinely aborts.
        (
            "gambatte/window/late_disable_spx10_wx0f_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // Bonus lift (the wxA6+sprite enable row rides the same rise lead).
        (
            "gambatte/m0enable/enable_wxA6_2x_spxA7_1_dmg08_cgb04c_out2.gbc",
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

/// The UNSHIFTED CGB SS carryover-tail m0-enable hand-off: with
/// the two-phase engine view owning FF41 writes (`eng_lyc`), a line-boundary
/// m0 enable committing on the next line's dots 0-1 catches nothing — the
/// phase-1 evaluation lands where `mode_for_interrupt` reads the line-start
/// OAM carry (hardware's `ttnl > 4` dead-tail, asm_enable ROW 3). The
/// write-instant carryover fire stays for the SHIFTED frames it was built on
/// and DS.
#[test]
fn tier2_late_enable_dead_tail_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_late_enable_dead_tail",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/m0enable/late_enable_2_dmg08_out2_cgb04c_out0.gbc",
            "0",
        ),
        // GUARDs — the mid-line enable still fires (via the engine phase-1);
        // the next-line mode-2-zone commit stays silent; the DS boundary
        // enable keeps the write-instant fire.
        ("gambatte/m0enable/late_enable_1_dmg08_cgb04c_out2.gbc", "2"),
        ("gambatte/m0enable/late_enable_3_dmg08_cgb04c_out0.gbc", "0"),
        ("gambatte/m1/ly143_late_m0enable_ds_1_cgb04c_out3.gbc", "3"),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}
