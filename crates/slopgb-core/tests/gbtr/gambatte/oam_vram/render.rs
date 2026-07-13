//! Mid-mode-3 pixel-render reclock pinned-behavior tests (`*_m3_render_*`).
//!
//! Submodule of `oam_vram`; see the module root for the split rationale.

use super::super::*;

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
/// the `late_enable_afterVblank` gambatte set). Same 5 legs as the tier2
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
        (
            "mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb",
            Model::Dmg,
        ),
        ("mealybug-tearoom-tests/ppu/m3_obp0_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_window_timing.gb", Model::Dmg),
        (
            "mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb",
            Model::Dmg,
        ),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_eager(&root, rel, model);
    }
}

/// The WX (FF4B) mid-mode-3 render commit RE-HOSTED onto the eager clock. Like
/// the palette pin above, the un-shifted eager `eff.wx` lands the WX
/// activation/reactivation change too early (the eager stage starts at cc+0 vs
/// tier2's cc+4). WX has the SMALLEST render stage (dots=0 +1) so it needs the
/// LARGEST frame debt (12hd SS) to reach the ~14hd absolute commit the per-dot
/// WX comparator wants (`regs.rs::stage_write`). The render/read SPLIT
/// built for tier2 carries over: the un-catch READ law's `wx_write_dot` is
/// recorded in `Ppu::write` at the eager cc+0, so the debt shifts only the
/// render view — the FF41 mode-3-length reads are untouched (EV DMG two-bin
/// 102 → 96, clean +6/−0). Recovers the mealybug m3_wx_4/5/6_change + _sprites
/// rows the flip regressed. (SCX FF43 is NOT re-hosted — its `eff.scx`
/// fine-scroll discard IS the mode-3 length, so a render debt trades the
/// `late_scx_late_disable_0`/`_1` SameBoy-PASS OCR sibling pair; refuted, see
/// the eager-dmg-render-rehost map.)
#[test]
fn eager_dmg_m3_render_wx_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_m3_render_wx",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("mealybug-tearoom-tests/ppu/m3_wx_4_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_wx_5_change.gb", Model::Dmg),
        ("mealybug-tearoom-tests/ppu/m3_wx_6_change.gb", Model::Dmg),
        (
            "mealybug-tearoom-tests/ppu/m3_wx_4_change_sprites.gb",
            Model::Dmg,
        ),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_eager(&root, rel, model);
    }
}
