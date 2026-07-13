//! Mid-mode-3 pixel-render reclock pinned-behavior tests (`*_m3_render_*`).
//!
//! Submodule of `oam_vram`; see the module root for the split rationale.

use super::super::*;

/// The mode-3 render reclock, mechanism 1 (SCY/palette): the pure-render
/// mid-mode-3 registers (SCY FF42, BGP/OBP FF47-FF49) take SCX's +4 render-frame
/// defer (dots=3) on the tier2 deferred write path. The deferred clock advances
/// the machine to the write's leading edge (cc+0) before the write; the eager
/// `commit_eff` there landed the value 4 dots EARLY of the render's
/// cc+4-calibrated fetch grid, so the pixel pipeline sampled the new SCY/palette
/// too soon (the `dmgpalette_during_m3` / `scy_during_m3` pixel-reference
/// flip-blockers). Staging 3 dots lets the strobe re-commit at the render frame
/// (the `regs.rs` `staged_pending` survive skip keeps `Ppu::write` from
/// clobbering it). SCY/palette are pure colour/row selection — no mode-3-length
/// or FF41-read-law coupling (those sample ARCH `self.scy`/`self.bgp`) — so this
/// is a render-only slice: CGB two-bin 291/291 zero-drift, mooneye 91/91 ON+OFF,
/// production byte-identical OFF. Verified against the pixel two-bin
/// (`gambatte_pixel_probe`, `SLOPGB_ROWLIST`): dmgpalette 6/6 + scy 26/27
/// flag-on (the sprite-stalled `scy_during_m3_spx08_2` is a separate penalty-grid
/// case). Representatives asserted via the suite's own frame comparator
/// (`expect_frame_png`), flag-on.
#[test]
fn tier2_dmg_m3_render_scy_palette_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_scy_palette",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/dmgpalette_during_m3/dmgpalette_during_m3_1.gb",
            Model::Dmg,
        ),
        (
            "gambatte/dmgpalette_during_m3/scx3/dmgpalette_during_m3_3.gb",
            Model::Dmg,
        ),
        ("gambatte/scy/scy_during_m3_1.gbc", Model::Dmg),
        ("gambatte/scy/scy_during_m3_1.gbc", Model::Cgb),
        ("gambatte/scy/scx3/scy_during_m3_5.gbc", Model::Dmg),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The mode-3 render reclock, mechanism 2 (LCDC BG addressing): the BG/
/// window fetcher samples a DEFERRED LCDC view (`eff.render_lcdc`, bit3 BG map /
/// bit4 tile-data / bit6 win map) that lags the eager control commit by the
/// render frame (`RENDER_LCDC_DELAY`), so a mid-mode-3 bgtilemap/bgtiledata
/// toggle reaches the fetch grid at the production dot instead of the leading
/// edge. The window bit5 (abort/reenable/enable) side-effects + the FF41 read
/// laws keep the eager `eff.lcdc` (their tier2 window pins are calibrated to the
/// cc+0 control commit — a full LCDC defer regressed them); OBJ-enable / mode-3
/// length reads also keep the eager view (must not move the length). Fixes the
/// `bgtiledata` (21) + `bgtilemap` (26) pixel-reference flip-blockers + mealybug
/// `m3_lcdc_tile_sel_change`. Render-only slice: CGB two-bin 291/291 zero-drift,
/// mooneye 91/91 ON+OFF, tier2 window pins intact, production byte-identical OFF.
#[test]
fn tier2_dmg_m3_render_lcdc_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_lcdc",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("gambatte/bgtiledata/bgtiledata_spx08_1.gbc", Model::Dmg),
        ("gambatte/bgtiledata/bgtiledata_spx09_2.gbc", Model::Dmg),
        ("gambatte/bgtiledata/bgtiledata_spx08_2.gbc", Model::Cgb),
        ("gambatte/bgtilemap/bgtilemap_spx08_1.gbc", Model::Dmg),
        ("gambatte/bgtilemap/bgtilemap_spx08_1.gbc", Model::Cgb),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The mode-3 render reclock, mechanism 3 (SCX double-speed): SCX's
/// render-frame defer is +2 dots in double speed vs +4 (dots=3) in single speed
/// — the DS M-cycle is 2 PPU dots (vs 4), so the write-commit-to-fetch-grid
/// offset halves. dots=2 fixes the 5 `scx_during_m3_ds` fine-scroll pixel legs
/// AND holds `late_scx4`'s DS FF41 read law (see
/// `tier2_late_scx_writestrobe_passes`) — the single value that satisfies both
/// the render straddle and the read-verdict straddle. CGB two-bin zero-drift,
/// production byte-identical OFF.
#[test]
fn tier2_dmg_m3_render_scx_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_scx_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_5.gbc",
            Model::Cgb,
        ),
        (
            "gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_8.gbc",
            Model::Cgb,
        ),
        (
            "gambatte/scx_during_m3/scx_0063c0/scx_during_m3_ds_5.gbc",
            Model::Cgb,
        ),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The mode-3 render reclock, mechanisms 4+5 (mixer render-view LCDC
/// bits): the sprite↔BG mixer (`output_pixel`) reads its render-only LCDC bits
/// from the DEFERRED view (`eff.render_lcdc`), like mechanism 2's BG-fetch
/// addressing bits, so a mid-mode-3 toggle lands its column at the
/// production/SameBoy dot instead of the leading edge. Mech4 is bit0 (BG/window
/// priority): it strips BG priority at the toggle column
/// (m3_lcdc_bg_en_change/_change2 + bgoff_bgon_sprite_below_window). Mech5 is
/// bit1 (OBJ-enable draw-side): it suppresses an already-fetched sprite pixel at
/// the mix (m3_lcdc_obj_en_change, CGB only — DMG keeps the eager one-dot-ahead
/// mixer calibration). Both bits are render-only (bit0's BG fetch still runs;
/// bit1's draw-side is past the sprite fetch — the FETCH-side OBJ enable gating
/// the stall/length stays eager in `render.rs`). CGB two-bin zero-drift, mooneye
/// 91/91, production byte-identical OFF.
#[test]
fn tier2_dmg_m3_render_bg_priority_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_bg_priority",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change.gb",
            Model::Cgb,
        ),
        (
            "mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change2.gb",
            Model::Cgb,
        ),
        (
            "gambatte/bgen/bgoff_bgon_sprite_below_window.gbc",
            Model::Cgb,
        ),
        (
            "mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change.gb",
            Model::Cgb,
        ),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The DMG palette (BGP/OBP FF47-49) commit half-dot pop-grid: the last
/// palette-timing pixel-reference flip-blockers the whole-dot render-defer could
/// not land (89/100 was the whole-dot ceiling; these 5 need
/// half-dot precision). The mealybug `m3_bgp_change`/`_sprites`, `m3_obp0_change`
/// and `m3_window_timing`/`_wx_0` legs are BGP/OBP torture (m3_window_timing is a
/// BGP test, not a window one — its window render is byte-identical flag-on/off,
/// only `eff.bgp` at the pixel-pop differs). SameBoy commits the palette at the
/// write M-cycle's exact half-dot and the pixel pops at a half-dot; single speed
/// is whole-dot aligned so the commit lands at a whole (EVEN) dot, visible +2
/// dots from the pop. The tier2 deferred write's whole-dot leading edge loses
/// which side of the even grid it sits on — `dots = 2 + (leading_edge & 1)`
/// (`cycle.rs::write_deferred`) recovers it: EVEN leading edges (all the mealybug
/// legs, dual-traced LE=104) want +2, ODD (the gambatte dmgpalette legs, LE=183)
/// want +3, so the shared dots=3 was one column late for the mealybug set. DMG
/// only, render-only (colour selection, no length/read-law coupling): CGB two-bin
/// 291/291 zero-drift, mooneye 91/91 ON+OFF, all shipped dmgpalette/scy render
/// pins held, production byte-identical OFF. Pixel two-bin 89→94 (+5 / 0 dropped).
#[test]
fn tier2_dmg_m3_render_palette_halfdot_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_palette_halfdot",
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
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

/// The SCY (FF42) commit takes the DMG palette's EVEN-dot parity anchor
/// (`dots = 2 + (leading_edge & 1)`, `cycle.rs::write_deferred`), resolving the
/// sub-dot render-fetch grid the whole-dot defer=3 could not on a sprite-stalled
/// line. A sprite prefill stall (`scy_during_m3_spx08_2`, an X=8 OBJ) shifts the
/// BG fetch grid so a tile's Lo/Hi data read (`bg_tile_addr`, fine row = LY+SCY
/// & 7) lands EXACTLY on the deferred SCY-commit dot; production/SameBoy commits
/// the write at the M-cycle mid-point (visible +2 from an EVEN leading edge, +3
/// from ODD — the same round_up_even(LE)+2 the palette derives), so the per-tile
/// data read re-samples the NEW scroll while the latched tile NUMBER keeps the old
/// (the mealybug m3_scy_change mixed-fetch behaviour). Dual-traced: the sprite leg
/// lands an EVEN LE=236 → +2 (the flat defer=3 rendered the change one column
/// late); the objectless `scy_during_m3_{1,4,5,6}` writes land ODD LEs → +3 (held,
/// a flat +2 broke all 8). SCY is pure row selection (no length / FF41-read-law
/// coupling), so render-only: CGB two-bin 291/291 zero-drift (the CGB `spx08_2`
/// held), mooneye 91/91 ON+OFF, production byte-identical OFF. Pixel two-bin +1.
#[test]
fn tier2_dmg_m3_render_scy_spx08_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_m3_render_scy_spx08",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // FIXED — the sprite-stalled SCY leg (even LE → parity +2).
        ("gambatte/scy/scy_during_m3_spx08_2.gbc", Model::Dmg),
        // HELD — the CGB sprite leg (already passed at defer=3, unperturbed).
        ("gambatte/scy/scy_during_m3_spx08_2.gbc", Model::Cgb),
        // HELD — an odd-LE objectless leg (parity +3, a flat +2 would drop it).
        ("gambatte/scy/scx3/scy_during_m3_4.gbc", Model::Dmg),
    ];
    for (rel, model) in targets {
        assert_pixel_leg_flagon(&root, rel, model);
    }
}

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
/// the `late_enable_afterVblank` gambatte set, #11ck). Same 5 legs as the tier2
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
/// WX comparator wants (`regs.rs::stage_write`). The render/read SPLIT #11bq
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
