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
    /// Which CGB VRAM bank the Tiles tab shows (0/1); ignored on DMG.
    pub tile_bank: u8,
    /// BG map tab: show the window tilemap (LCDC bit 6 select) + the WX/WY box
    /// instead of the BG tilemap + the SCX/SCY box. Auto stays BG-only.
    pub show_window: bool,
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
            tile_bank: 0,
            show_window: false,
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
    /// Tiles-tab CGB VRAM-bank-1 toggle.
    pub tile_bank_box: Rect,
    /// BG-map-tab BG⇄window toggle.
    pub win_box: Rect,
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
    // Controls fill the lower rows of the details column, top-down. Each tab
    // shows only the subset that applies (gated in render/click).
    let mut cy = details.bottom() - 1 - 7 * lh;
    let tile_bank_box = checkbox_rect(dx, cy, "VRAM bank 1");
    cy += lh;
    let win_box = checkbox_rect(dx, cy, "window");
    cy += lh;
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
        tile_bank_box,
        win_box,
    }
}

/// Handle a left-click at window-pixel `(px, py)`: switch tab, toggle a
/// checkbox, or select a BG-map source radio. `cgb` gates the CGB-only Tiles
/// bank toggle. Returns whether `state` changed (i.e. a redraw is needed).
pub fn on_click(state: &mut VramState, area: Rect, px: i32, py: i32, cgb: bool) -> bool {
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
    // The Tiles tab's bank toggle is CGB-only (DMG has a single VRAM bank).
    if state.tab == VramTab::Tiles && cgb && l.tile_bank_box.contains(px, py) {
        state.tile_bank ^= 1;
        return true;
    }
    // scxy + the source radios + the BG⇄window toggle only apply on the BG map
    // tab (where they show).
    if state.tab == VramTab::BgMap {
        if l.win_box.contains(px, py) {
            state.show_window = !state.show_window;
            return true;
        }
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

/// Largest integer scale at which a `natural_w × natural_h` image fits inside a
/// `content_w × content_h` area — so the VRAM content grows on integer steps as
/// the window resizes, never fractionally (which would break the 1px tile
/// borders). Always ≥ 1 so the content is never zero-scaled.
#[must_use]
pub fn fit_scale(content_w: i32, content_h: i32, natural_w: i32, natural_h: i32) -> i32 {
    if natural_w <= 0 || natural_h <= 0 {
        return 1;
    }
    (content_w / natural_w).min(content_h / natural_h).max(1)
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

/// OAM-grid horizontal cell pitch at `scale`: an 8-px tile plus a proportional
/// 2-px gap (so a cell is `10 * scale`; 20 px at the default scale 2, as bgb
/// shows). Shared by [`render_oam`] and the OAM hover hit-test so they can't drift.
#[must_use]
pub fn oam_cell(scale: i32) -> i32 {
    10 * scale
}

/// OAM-grid vertical cell pitch at `scale`: the sprite height (8 or 16 px in
/// 8×16 mode) plus the 2-px gap, so 8×16 sprites get room for both stacked tiles.
#[must_use]
pub fn oam_cell_h(scale: i32, tall: bool) -> i32 {
    (if tall { 18 } else { 10 }) * scale
}

/// Mirror an 8×8 tile's pixels horizontally and/or vertically (the OAM/BG-map
/// attribute X/Y-flip bits). Pure — `pixels[row][col]`, so an x-flip mirrors the
/// column and a y-flip the row.
#[must_use]
pub fn flip_tile(pixels: [[u8; 8]; 8], xflip: bool, yflip: bool) -> [[u8; 8]; 8] {
    let mut out = [[0u8; 8]; 8];
    for (r, row) in pixels.iter().enumerate() {
        let dr = if yflip { 7 - r } else { r };
        for (c, &px) in row.iter().enumerate() {
            let dc = if xflip { 7 - c } else { c };
            out[dr][dc] = px;
        }
    }
    out
}

/// Render the OAM tab: the 40 sprites in an 8×5 grid, each honoring its OAM
/// attribute byte — per-sprite VRAM bank (CGB bit 3), X/Y flip (bits 5/6), and
/// palette: on CGB the OBJ palette (bits 0-2) indexes `palettes`, on DMG the
/// DMG-palette bit (bit 4) picks `palettes[0/1]` (OBP0/OBP1). `tall` (LCDC bit 2)
/// draws 8×16 sprites as two stacked tiles (`tile&!1` over `tile|1`, order
/// swapped on Y-flip). Empty slots (y == x == 0) are blank. Clipped to `rect`.
#[allow(clippy::too_many_arguments)]
pub fn render_oam(
    c: &mut Canvas,
    rect: Rect,
    oam: &[u8],
    vram: &[u8],
    palettes: &[[u32; 4]],
    cgb: bool,
    tall: bool,
    scale: i32,
) {
    const COLS: i32 = 8;
    let (cw, ch) = (oam_cell(scale), oam_cell_h(scale, tall));
    let saved = c.push_clip(rect);
    for (i, s) in debug::oam_sprites(oam).iter().enumerate() {
        if s.y == 0 && s.x == 0 {
            continue;
        }
        let px = rect.x + (i as i32 % COLS) * cw;
        let py = rect.y + (i as i32 / COLS) * ch;
        let bank = if cgb { usize::from(s.attr >> 3 & 1) } else { 0 };
        let pal_idx = if cgb {
            usize::from(s.attr & 0x07)
        } else {
            usize::from(s.attr >> 4 & 1)
        };
        let Some(palette) = palettes.get(pal_idx).or_else(|| palettes.first()) else {
            continue;
        };
        let (xf, yf) = (s.attr & 0x20 != 0, s.attr & 0x40 != 0);
        if tall {
            // Top tile is the even index, bottom the odd; Y-flip swaps them.
            let halves = if yf {
                [s.tile | 1, s.tile & !1]
            } else {
                [s.tile & !1, s.tile | 1]
            };
            for (k, &t) in halves.iter().enumerate() {
                let pixels = flip_tile(debug::tile_pixels(vram, bank, t as usize), xf, yf);
                c.blit_tile(px, py + k as i32 * 8 * scale, &pixels, palette, scale);
            }
        } else {
            let pixels = flip_tile(debug::tile_pixels(vram, bank, s.tile as usize), xf, yf);
            c.blit_tile(px, py, &pixels, palette, scale);
        }
    }
    c.set_clip(saved);
}

/// One DMG palette row for the Palettes tab: a register (`BGP`/`OBP0`/`OBP1`),
/// its raw value, and the four shades it maps colour IDs 0..=3 to (through the
/// [`GREYS`] ramp).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmgPalRow {
    pub name: &'static str,
    pub reg: u8,
    pub colors: [u32; 4],
}

/// The three DMG palette registers as swatch rows, so a DMG game's BGP/OBP0/OBP1
/// shade mappings (`rBGP`/`rOBP`) are inspectable — the CGB palette-RAM rows are
/// meaningless on DMG. Each colour ID is mapped to its shade via
/// [`debug::dmg_palette_shades`], then to the neutral [`GREYS`] ramp.
#[must_use]
pub fn dmg_palette_rows(bgp: u8, obp0: u8, obp1: u8) -> [DmgPalRow; 3] {
    [("BGP", bgp), ("OBP0", obp0), ("OBP1", obp1)].map(|(name, reg)| DmgPalRow {
        name,
        reg,
        colors: debug::dmg_palette_shades(reg).map(|s| GREYS[s as usize]),
    })
}

/// Render the DMG Palettes tab: the BGP/OBP0/OBP1 rows from [`dmg_palette_rows`],
/// each a `NAME XX` label then four shade swatches. Clipped to `rect`.
pub fn render_palettes_dmg(c: &mut Canvas, rect: Rect, bgp: u8, obp0: u8, obp1: u8, theme: &Theme) {
    use crate::ui::text::{draw_text, measure};
    let sw = 14;
    let lh = line_height();
    // Widest label is "OBP0 XX" (7 glyphs); the trailing space leaves a one-glyph
    // gap before the swatches (the font is 7px wide, so a fixed 48 clipped it).
    let label_w = measure("OBP0 XX ");
    let saved = c.push_clip(rect);
    for (i, row) in dmg_palette_rows(bgp, obp0, obp1).iter().enumerate() {
        let py = rect.y + i as i32 * (sw + 2).max(lh);
        draw_text(
            c,
            rect.x,
            py,
            &format!("{} {:02X}", row.name, row.reg),
            theme.text,
        );
        for (ci, &color) in row.colors.iter().enumerate() {
            let px = rect.x + label_w + ci as i32 * sw;
            swatch(c, Rect::new(px, py, sw, sw), color, theme);
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

/// Split a `start`-anchored `len`-long span over a `modulus`-wide axis into the
/// 1 or 2 contiguous pieces it occupies once it wraps past the edge.
fn wrap_spans(start: i32, len: i32, modulus: i32) -> Vec<(i32, i32)> {
    let start = start.rem_euclid(modulus);
    if start + len <= modulus {
        vec![(start, len)]
    } else {
        vec![(start, modulus - start), (0, start + len - modulus)]
    }
}

/// The screen viewport (a `vw`×`vh` map-pixel box at `(scx, scy)`) split into up
/// to four `scale`-multiplied rectangles as it wraps around a `map_px`-wide map —
/// so the box shows both edges of the wrap instead of a single clipped rect.
/// Rectangles are in content-local pixels (the caller offsets by the pane origin).
#[must_use]
pub fn bgmap_viewport_segments(
    scx: u8,
    scy: u8,
    vw: i32,
    vh: i32,
    map_px: i32,
    scale: i32,
) -> Vec<Rect> {
    let xs = wrap_spans(i32::from(scx), vw, map_px);
    let ys = wrap_spans(i32::from(scy), vh, map_px);
    let mut out = Vec::with_capacity(xs.len() * ys.len());
    for &(x, w) in &xs {
        for &(y, h) in &ys {
            out.push(Rect::new(x * scale, y * scale, w * scale, h * scale));
        }
    }
    out
}

/// The on-screen-visible portion of the window layer, as a content-local rect for
/// the window-map view's `rWX`/`rWY` indicator. The window is displayed from its
/// own top-left at screen `(WX-7, WY)`, so the visible slice is a box at the map
/// origin sized `min(160, 167-WX)` × `144-WY`. `None` when the window is wholly
/// off-screen (`WX ≥ 167` or `WY ≥ 144`).
#[must_use]
pub fn window_region_rect(wx: u8, wy: u8, scale: i32) -> Option<Rect> {
    let w = (167 - i32::from(wx)).clamp(0, 160);
    let h = 144 - i32::from(wy);
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(Rect::new(0, 0, w * scale, h * scale))
}

/// The outline drawn over the BG-map tab: nothing, the screen viewport (SCX/SCY,
/// wrapping at the map edges), or the visible-window region (WX/WY).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapOverlay {
    None,
    Screen { scx: u8, scy: u8 },
    Window { wx: u8, wy: u8 },
}

impl MapOverlay {
    /// The content-local outline rectangles for this overlay at `scale`.
    #[must_use]
    pub fn rects(self, scale: i32) -> Vec<Rect> {
        match self {
            MapOverlay::None => Vec::new(),
            MapOverlay::Screen { scx, scy } => {
                bgmap_viewport_segments(scx, scy, 160, 144, 256, scale)
            }
            MapOverlay::Window { wx, wy } => window_region_rect(wx, wy, scale).into_iter().collect(),
        }
    }
}

/// Render the BG map tab: the 32×32 tilemap at `base` (0x9800/0x9C00), each
/// cell's tile (resolved via the `signed` tile-data area) drawn at `scale`. On
/// CGB (`cgb`) each cell honors its attribute byte — BG palette (bits 0-2) into
/// `palettes`, tile VRAM bank (bit 3), and X/Y flip (bits 5/6); on DMG it uses
/// `palettes[0]` (BGP) with no flips. `overlay` outlines the screen/window box.
/// Clipped to `rect`.
#[allow(clippy::too_many_arguments)]
pub fn render_bgmap(
    c: &mut Canvas,
    rect: Rect,
    vram: &[u8],
    base: u16,
    signed: bool,
    palettes: &[[u32; 4]],
    cgb: bool,
    scale: i32,
    overlay: MapOverlay,
    theme: &Theme,
) {
    let saved = c.push_clip(rect);
    let map = debug::bg_map(vram, base);
    for (i, cell) in map.iter().enumerate() {
        let px = rect.x + (i as i32 % 32) * 8 * scale;
        let py = rect.y + (i as i32 / 32) * 8 * scale;
        let (pal_idx, bank, xf, yf) = if cgb {
            (
                usize::from(cell.attr & 0x07),
                usize::from(cell.attr >> 3 & 1),
                cell.attr & 0x20 != 0,
                cell.attr & 0x40 != 0,
            )
        } else {
            (0, 0, false, false)
        };
        let Some(palette) = palettes.get(pal_idx).or_else(|| palettes.first()) else {
            continue;
        };
        let pixels = flip_tile(
            debug::tile_pixels(vram, bank, tile_index(cell.tile, signed)),
            xf,
            yf,
        );
        c.blit_tile(px, py, &pixels, palette, scale);
    }
    // Viewport/window outline: 1 map pixel = `scale`; the screen box wraps.
    for b in overlay.rects(scale) {
        c.outline_rect(
            Rect::new(rect.x + b.x, rect.y + b.y, b.w, b.h),
            theme.breakpoint,
        );
    }
    c.set_clip(saved);
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
