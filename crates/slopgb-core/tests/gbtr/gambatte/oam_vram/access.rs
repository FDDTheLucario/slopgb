//! OAM / VRAM / sprite access + mode-2/3 STAT-read pinned-behavior tests.
//!
//! Submodule of `oam_vram`; see the module root for the split rationale.

use super::super::*;

/// The SPRITE-line analog of the kernel pair. A sprite-laden line extends
/// mode 3, shifting the visible mode→0 boundary; the `vis_early` back-date for
/// sprite/window lines (`lead + 4`, vs a bare line's `lead + 3`) matches
/// SameBoy's frame, so the two equal-`ldh` reads straddle it:
/// `10spritesPrLine_m3stat_1` reads mode 3 (out3) and `_m3stat_2` reads mode 0
/// (out0) — the same out3/out0 split the kernel pair shows on a bare line.
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
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb())
                .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected out{expect}: {e}"));
        }
    }
}

/// CGB DOUBLE-SPEED OAM/VRAM/palette accessibility. The whole-M-cycle
/// `m0_access_edge`/`pal_access_edge` straddle stamps are mis-framed at double
/// speed (the mode-0 flip lands at the render dot), so DS accessibility
/// bypasses the stamp (`Interconnect::ev_ds_access`, `interconnect/memory.rs`)
/// and resolves through the `Ppu::{oam,vram,pal}_*_blocked` laws: the DS
/// line-end read release (`254 + SCX&7`), the OAM-write release, and the
/// palette-pipe-end unblock. Single speed keeps the stamp. The `_1` siblings
/// are the regression guards (must stay blocked).
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
        // Open-access rows (SameBoy-pass):
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
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}

/// CGB SINGLE-SPEED accessibility: the palette stamp bypass + the STOP-shift
/// law frame (traced `postread_scx3_2` @dot256, `preread_lcdoffset1_1` @dot83):
///
/// * **Palette** — the CGB `pal_access_edge` stamp is a WHOLE-M-cycle block
///   that `access_lead` does not disarm; the `interconnect/memory.rs`
///   FF69/FF6B bypass resolves the read through `Ppu::pal_ram_blocked`
///   instead, which reads open past the pipe end
///   (`cgbpal_m3end_scx{2,3,5}_2`).
/// * **STOP-shift** — `Ppu::vram_read_blocked` takes the `law_pos()`
///   STOP-shift frame (as `pal_ram_blocked` does), not the raw `self.dot`, so
///   `preread_lcdoffset1_1`'s law-dot82 read is open (raw dot83 would block).
///
/// The `_1`/`_2` siblings separate on whole dots (`preread_lcdoffset1_2`
/// law-dot86 stays blocked; the palette `_1` entry reads stay blocked), so no
/// mode-3 render-length change is involved.
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
        // Open-access rows (SameBoy-pass):
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
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}
