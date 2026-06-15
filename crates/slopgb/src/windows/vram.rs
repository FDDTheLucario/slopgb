//! The bgb VRAM viewer window (Layer C): a tabbed BG map / Tiles / OAM /
//! Palettes view. Pure content renderers composing `ui` over the
//! `slopgb_core::debug` VRAM decoders; the winit surface comes with B12b.

use slopgb_core::debug;

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::line_height;
use crate::ui::widgets::{checkbox_rect, radio_rects, swatch, tab_rects, tab_strip};

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

    /// The four tab labels, in [`VramTab::ALL`] order.
    #[must_use]
    pub fn labels() -> [&'static str; 4] {
        VramTab::ALL.map(VramTab::label)
    }
}

/// BG-map source-select radio labels (`Map` row): auto-detect, or force a base.
pub const MAP_SRC: [&str; 3] = ["Auto", "9800", "9C00"];
/// BG-map tile-data source radio labels (`Tiles` row): auto, signed, unsigned.
pub const TILE_SRC: [&str; 3] = ["Auto", "8800", "8000"];

/// Width reserved for the right-hand details/controls panel.
pub const PANEL_W: i32 = 150;

/// Persistent interactive state for the VRAM viewer window: which tab is shown
/// and every control's value. Owned per-window by the event loop; mutated by
/// [`on_click`] / [`on_hover`] and read by the renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VramState {
    pub tab: VramTab,
    /// Overlay tile/cell grid lines.
    pub grid: bool,
    /// Apply the guessed game palette (vs the neutral greyscale ramp).
    pub show_paletted: bool,
    /// Draw the screen viewport rectangle on the BG map.
    pub scxy: bool,
    /// `Map` source radio index into [`MAP_SRC`].
    pub map_src: u8,
    /// `Tiles` source radio index into [`TILE_SRC`].
    pub tile_src: u8,
    /// Cursor position (window pixels) while it is over the content area, for
    /// the hovered-cell details panel; `None` when outside.
    pub hover: Option<(i32, i32)>,
}

impl Default for VramState {
    fn default() -> Self {
        // bgb's defaults (03-vram.png): Tiles tab, paletted on, grid on, viewport on.
        Self {
            tab: VramTab::Tiles,
            grid: true,
            show_paletted: true,
            scxy: true,
            map_src: 0,
            tile_src: 0,
            hover: None,
        }
    }
}

/// Geometry of the VRAM window: tab hit-rects, the grid content area, the
/// details/controls panel, and every control's hit-rect. A pure function of the
/// window rect, shared by [`render`](super::render_vram) and the click/hover
/// hit-tests so they can never disagree.
pub struct VramLayout {
    pub tabs: Vec<Rect>,
    pub content: Rect,
    pub details: Rect,
    pub grid_box: Rect,
    pub paletted_box: Rect,
    pub scxy_box: Rect,
    pub map_src: Vec<Rect>,
    pub tile_src: Vec<Rect>,
}

/// Compute the VRAM window layout for `area`.
#[must_use]
pub fn layout(area: Rect) -> VramLayout {
    let lh = line_height();
    let tabs = tab_rects(area.x + 2, area.y + 2, &VramTab::labels());
    let top = tabs.first().map_or(area.y, |r| r.bottom()) + 2;
    let content = Rect::new(area.x + 2, top, area.w - PANEL_W - 6, area.bottom() - top);
    let dx = content.right() + 4;
    let details = Rect::new(dx, top, area.right() - dx - 2, area.bottom() - top);
    // Controls fill the lower five rows of the details column, top-down.
    let mut cy = details.bottom() - 1 - 5 * lh;
    let map_src = radio_rects(dx, cy, &MAP_SRC);
    cy += lh;
    let tile_src = radio_rects(dx, cy, &TILE_SRC);
    cy += lh;
    let scxy_box = checkbox_rect(dx, cy, "scxy");
    cy += lh;
    let paletted_box = checkbox_rect(dx, cy, "show paletted");
    cy += lh;
    let grid_box = checkbox_rect(dx, cy, "Grid");
    VramLayout {
        tabs,
        content,
        details,
        grid_box,
        paletted_box,
        scxy_box,
        map_src,
        tile_src,
    }
}

/// Handle a left-click at window-pixel `(px, py)`: switch tab, toggle a
/// checkbox, or select a BG-map source radio. Returns whether `state` changed
/// (i.e. a redraw is needed).
pub fn on_click(state: &mut VramState, area: Rect, px: i32, py: i32) -> bool {
    let l = layout(area);
    for (i, r) in l.tabs.iter().enumerate() {
        if r.contains(px, py) {
            return set_if_changed(&mut state.tab, VramTab::ALL[i]);
        }
    }
    // The Grid box is drawn on every tab except Palettes.
    if state.tab != VramTab::Palettes && l.grid_box.contains(px, py) {
        state.grid = !state.grid;
        return true;
    }
    if l.paletted_box.contains(px, py) {
        state.show_paletted = !state.show_paletted;
        return true;
    }
    // scxy + the source radios only apply on the BG map tab (where they show).
    if state.tab == VramTab::BgMap {
        if l.scxy_box.contains(px, py) {
            state.scxy = !state.scxy;
            return true;
        }
        for (i, r) in l.map_src.iter().enumerate() {
            if r.contains(px, py) {
                return set_if_changed(&mut state.map_src, i as u8);
            }
        }
        for (i, r) in l.tile_src.iter().enumerate() {
            if r.contains(px, py) {
                return set_if_changed(&mut state.tile_src, i as u8);
            }
        }
    }
    false
}

/// Handle cursor motion to window-pixel `(px, py)`: remember it while it is over
/// the content grid (for the details panel), else clear. Returns whether the
/// hovered position changed (so the loop only redraws on a real change).
pub fn on_hover(state: &mut VramState, area: Rect, px: i32, py: i32) -> bool {
    let l = layout(area);
    let new = l.content.contains(px, py).then_some((px, py));
    set_if_changed(&mut state.hover, new)
}

/// Set `slot` to `val`, returning whether it actually changed.
fn set_if_changed<T: PartialEq>(slot: &mut T, val: T) -> bool {
    if *slot != val {
        *slot = val;
        true
    } else {
        false
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

/// Absolute tile index (0..=383) for a BG-map tile number under the LCDC tile
/// data area: unsigned 0x8000 method (`n`), or signed 0x8800 method where `n`
/// is taken as `i8` relative to tile 256 (`0x9000`).
#[must_use]
pub fn tile_index(n: u8, signed: bool) -> usize {
    if signed {
        (256 + i16::from(n as i8)) as usize
    } else {
        n as usize
    }
}

/// Render the BG map tab: the 32×32 tilemap at `base` (0x9800/0x9C00), each
/// cell's tile (resolved via the `signed` tile-data area) drawn at `scale`. When
/// `viewport` is set, the screen rectangle is outlined at `(scx, scy)` (the
/// `scxy` toggle). Clipped to `rect`.
#[allow(clippy::too_many_arguments)]
pub fn render_bgmap(
    c: &mut Canvas,
    rect: Rect,
    vram: &[u8],
    base: u16,
    signed: bool,
    scx: u8,
    scy: u8,
    palette: &[u32; 4],
    scale: i32,
    viewport: bool,
    theme: &Theme,
) {
    let saved = c.push_clip(rect);
    let map = debug::bg_map(vram, base);
    for (i, cell) in map.iter().enumerate() {
        let col = i as i32 % 32;
        let row = i as i32 / 32;
        let px = rect.x + col * 8 * scale;
        let py = rect.y + row * 8 * scale;
        let pixels = debug::tile_pixels(vram, 0, tile_index(cell.tile, signed));
        c.blit_tile(px, py, &pixels, palette, scale);
    }
    // Screen viewport: 160×144 map pixels at (scx, scy); 1 map pixel = `scale`.
    if viewport {
        c.outline_rect(
            Rect::new(
                rect.x + i32::from(scx) * scale,
                rect.y + i32::from(scy) * scale,
                160 * scale,
                144 * scale,
            ),
            theme.breakpoint,
        );
    }
    c.set_clip(saved);
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
