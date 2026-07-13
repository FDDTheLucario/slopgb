//! OAM / VRAM / sprite access + mode-2/3 STAT-read pinned-behavior tests.
//!
//! Submodule of `oam_vram`; see the module root for the split rationale.

use super::super::*;

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

/// CGB DOUBLE-SPEED accessibility RE-HOSTED onto the eager clock (L1).
/// Under `eager_value` the OAM/VRAM/palette read still resolved against the
/// production `m0_access_edge`/`pal_access_edge` whole-M-cycle straddle stamp,
/// which is mis-framed at double speed (the eager mode-0 flip lands at the
/// reclocked render dot). The DS line-end read releases (`254 + SCX&7`), the
/// OAM-write release, and the palette-pipe-end unblock all already live in the
/// ported `Ppu::{oam,vram,pal}_*_blocked` laws (`|| eager_value`-gated); the
/// fix routes eager DS accessibility through them by taking the same stamp
/// bypass the eager clock already takes (`Interconnect::ev_ds_access`,
/// `interconnect/memory.rs`). EV CGB two-bin 358 → 353 (clean +5/−0). Single
/// speed keeps the stamp; production byte-identical. The `_1`
/// siblings are the regression guards (must stay blocked). The lcd-offset
/// `preread_ds_lcdoffset1_1` accessibility row stays parked (the STOP-shift
/// `lcd_shift_dots` frame is unported on the eager clock).
#[test]
fn eager_ds_access_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ds_access",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // Recovered (SameBoy-pass, was EV-fail):
        (
            "gambatte/oam_access/postread_scx5_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/oam_access/postwrite_scx1_ds_2_cgb04c_out1.gbc",
            "1",
        ),
        ("gambatte/vram_m3/postread_scx5_ds_2_cgb04c_out0.gbc", "0"),
        ("gambatte/cgbpal_m3/cgbpal_m3end_ds_2_cgb04c_out0.gbc", "0"),
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx5_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // Regression guards (the `_1` siblings must stay blocked):
        (
            "gambatte/oam_access/postread_scx5_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/oam_access/postwrite_scx1_ds_1_cgb04c_out0.gbc",
            "0",
        ),
        ("gambatte/vram_m3/postread_scx5_ds_1_cgb04c_out3.gbc", "3"),
        ("gambatte/cgbpal_m3/cgbpal_m3end_ds_1_cgb04c_out7.gbc", "7"),
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx5_ds_1_cgb04c_out7.gbc",
            "7",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}

/// The eager-clock CGB SINGLE-SPEED accessibility residual: the palette
/// whole-M-cycle stamp bypass + the STOP-shift law-frame, both clean
/// re-hosts. Traced (`postread_scx3_2` @dot256, `preread_lcdoffset1_1` @dot83):
///
/// * **Palette** — the CGB `pal_access_edge` stamp is a WHOLE-M-cycle block that
///   `access_lead` does not disarm; SS eager kept it while `tier2`/DS bypass it,
///   re-blocking `cgbpal_m3end_scx{2,3,5}_2` reads that land past the pipe end
///   where `Ppu::pal_ram_blocked` (already `|| eager_value`-gated) reads open.
///   Fix: extend the `interconnect/memory.rs` FF69/FF6B bypass to all
///   `eager_value` (`!self.eager_value` supersedes `!self.ev_ds_access()`).
/// * **STOP-shift** — `Ppu::vram_read_blocked`'s law position used the raw
///   `self.dot` under eager, not the `law_pos()` STOP-shift frame `tier2` and
///   `pal_ram_blocked` already take, so `preread_lcdoffset1_1`'s law-dot82 read
///   blocked (raw dot83 ≥ 83). Fix: `d = law_pos().1` under `tier2 || eager`.
///
/// EV CGB two-bin 323 → 318 (clean +5/−0, DMG 74 unchanged). The `_1`/`_2`
/// siblings separate WHOLE-DOT (`preread_lcdoffset1_2` law-dot86 stays blocked;
/// the palette `_1` entry reads stay blocked), so no render-length move is
/// needed — NOT the `vis_early` flip-dot family (whose eager `early_lead` is
/// mis-framed on scx0/scx5). Production + tier2-off byte-identical.
#[test]
fn eager_ss_access_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ss_access",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // Recovered (SameBoy-pass, was EV-fail):
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx2_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx3_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx5_2_cgb04c_out0.gbc",
            "0",
        ),
        ("gambatte/vram_m3/preread_lcdoffset1_1_cgb04c_out0.gbc", "0"),
        (
            "gambatte/vram_m3/preread_ds_lcdoffset1_1_cgb04c_out0.gbc",
            "0",
        ),
        // Regression guards (the `_1`/`_2` siblings must keep their verdict):
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx2_1_cgb04c_out7.gbc",
            "7",
        ),
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx3_1_cgb04c_out7.gbc",
            "7",
        ),
        (
            "gambatte/cgbpal_m3/cgbpal_m3end_scx5_1_cgb04c_out7.gbc",
            "7",
        ),
        ("gambatte/vram_m3/preread_lcdoffset1_2_cgb04c_out3.gbc", "3"),
        (
            "gambatte/vram_m3/preread_ds_lcdoffset1_2_cgb04c_out3.gbc",
            "3",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}
