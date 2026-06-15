//! The bgb VRAM viewer window (Layer C): a tabbed BG map / Tiles / OAM /
//! Palettes view. Pure content renderers composing `ui` over the
//! `slopgb_core::debug` VRAM decoders; the winit surface comes with B12b.

use slopgb_core::debug;

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::widgets::{swatch, tab_strip};

/// The four VRAM-viewer tabs, in bgb's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VramTab {
    BgMap,
    Tiles,
    Oam,
    Palettes,
}

impl VramTab {
    pub const ALL: [VramTab; 4] = [
        VramTab::BgMap,
        VramTab::Tiles,
        VramTab::Oam,
        VramTab::Palettes,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            VramTab::BgMap => "BG map",
            VramTab::Tiles => "Tiles",
            VramTab::Oam => "OAM",
            VramTab::Palettes => "Palettes",
        }
    }
}

/// A neutral 4-grey palette for the tile/BG views when no game palette is
/// chosen (white → black), matching bgb's default "show paletted off" look.
pub const GREYS: [u32; 4] = [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000];

/// Draw the tab strip; returns each tab's hit-rect (index = [`VramTab::ALL`]).
pub fn render_tabs(c: &mut Canvas, x: i32, y: i32, active: VramTab, theme: &Theme) -> Vec<Rect> {
    let labels: Vec<&str> = VramTab::ALL.iter().map(|t| t.label()).collect();
    let active_idx = VramTab::ALL.iter().position(|&t| t == active).unwrap_or(0);
    tab_strip(c, x, y, &labels, active_idx, theme)
}

/// Render the Tiles tab: all 384 tiles of `bank` in a 16-wide grid through
/// `palette` at integer `scale`, top-left at `rect`. Clipped to `rect`.
pub fn render_tiles(
    c: &mut Canvas,
    rect: Rect,
    vram: &[u8],
    bank: usize,
    palette: &[u32; 4],
    scale: i32,
) {
    const COLS: usize = 16;
    let saved = c.push_clip(rect);
    for tile in 0..384 {
        let px = rect.x + (tile % COLS) as i32 * 8 * scale;
        let py = rect.y + (tile / COLS) as i32 * 8 * scale;
        let pixels = debug::tile_pixels(vram, bank, tile);
        c.blit_tile(px, py, &pixels, palette, scale);
    }
    c.set_clip(saved);
}

/// Render the OAM tab: the 40 sprites in an 8×5 grid, each cell its tile (from
/// `vram` bank 0) drawn through `palette` at `scale`; empty slots (y/x == 0)
/// are left blank. Clipped to `rect`.
pub fn render_oam(
    c: &mut Canvas,
    rect: Rect,
    oam: &[u8],
    vram: &[u8],
    palette: &[u32; 4],
    scale: i32,
) {
    const COLS: i32 = 8;
    let cell = 8 * scale + 4;
    let saved = c.push_clip(rect);
    for (i, s) in debug::oam_sprites(oam).iter().enumerate() {
        let col = i as i32 % COLS;
        let row = i as i32 / COLS;
        let px = rect.x + col * cell;
        let py = rect.y + row * cell;
        if s.y != 0 || s.x != 0 {
            let pixels = debug::tile_pixels(vram, 0, s.tile as usize);
            c.blit_tile(px, py, &pixels, palette, scale);
        }
    }
    c.set_clip(saved);
}

/// Render the Palettes tab: 8 BG + 8 OBJ palettes, each a row of four swatches
/// from the CGB palette RAM (`bg`, `obj` — 64 bytes each). Clipped to `rect`.
pub fn render_palettes(c: &mut Canvas, rect: Rect, bg: &[u8], obj: &[u8], theme: &Theme) {
    let sw = 14;
    let saved = c.push_clip(rect);
    for (block, cram) in [bg, obj].into_iter().enumerate() {
        let base_x = rect.x + block as i32 * (sw * 5 + 12);
        for pal in 0..8 {
            let words = debug::cgb_palette_words(cram, pal);
            let py = rect.y + pal as i32 * (sw + 2);
            for (ci, &word) in words.iter().enumerate() {
                let (r, g, b) = debug::rgb555_to_rgb888(word);
                let color = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
                let px = base_x + ci as i32 * sw;
                swatch(c, Rect::new(px, py, sw, sw), color, theme);
            }
        }
    }
    c.set_clip(saved);
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
