//! Mid-mode-3 pixel-render pinned-behavior tests (`*_m3_render_*`).
//!
//! Submodule of `oam_vram`; see the module root for the split rationale.

use super::super::*;

/// The DMG palette (BGP/OBP FF47-49) mid-mode-3 render commit. The write stage
/// starts at the cc+0 leading edge (`interconnect::Bus::write` stages before
/// `tick_machine`), so an un-shifted commit lands the palette change ~4 dots
/// (8hd SS / 4hd DS) too early. The pure-render registers (SCY FF42 / palette
/// FF47-49) take the CGB render-frame stage offset on DMG too
/// (`regs/stage.rs::stage_write`), which is render-only: no mode-3 length or
/// FF41-read-law coupling (their read laws sample ARCH state, `commit_eff`
/// records no read-law input). The length-coupled registers (FF40/FF43/FF4B)
/// keep zero offset (an offset there breaks the `late_enable_afterVblank`
/// gambatte set). Pins the 5 mealybug palette/window legs below.
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

/// The WX (FF4B) mid-mode-3 render commit. Like the palette pin above, an
/// un-shifted `eff.wx` lands the WX activation/reactivation change too early
/// (the stage starts at cc+0). WX has the smallest render stage (dots=0 +1) so
/// it carries the largest offset (12hd SS) to reach the ~14hd absolute commit
/// the per-dot WX comparator wants (`regs/stage.rs::stage_write`). The
/// render/read split keeps the FF41 mode-3-length reads untouched: the un-catch
/// READ law's `wx_write_dot` is recorded in `Ppu::write` at cc+0, so the offset
/// shifts only the render view. Pins the mealybug m3_wx_4/5/6_change + _sprites
/// rows. (SCX FF43 is NOT offset — its `eff.scx` fine-scroll discard IS the
/// mode-3 length, so a render offset would trade the
/// `late_scx_late_disable_0`/`_1` SameBoy-PASS OCR sibling pair.)
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
