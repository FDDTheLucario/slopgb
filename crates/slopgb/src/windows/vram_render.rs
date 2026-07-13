//! VRAM viewer rendering: tiles / BG-map / OAM / palette panes, hover
//! detail panels, and the palette/overlay helpers. Split out of
//! `windows.rs` for the size cap; a set of free fns over the `vram`
//! submodule state, reached from the `render` dispatcher via
//! [`render_vram`].

use super::*;

/// Per-tab VRAM geometry: the integer render scale fitted to the content area
/// (so content grows on resize), the grid cell pitch, the bounded drawn extent
/// (so the grid + frame hug the actual map, not the whole content rect — QA "bg
/// map should be bounded"), and whether the tab has a tile grid.
struct VramGeom {
    scale: i32,
    cell_w: i32,
    cell_h: i32,
    extent: Rect,
    grid: bool,
}

/// Compute [`VramGeom`] for `tab` inside the `content` area. Natural pixel sizes:
/// Tiles 16×24 tiles (128×192), BG map 32×32 (256×256), OAM an 8×5 grid of
/// 10-px cells (8-px tile + 2-px gap). Palettes has no grid.
fn vram_geom(tab: VramTab, content: Rect, tall: bool) -> VramGeom {
    let tiled = |cols: i32, rows: i32, cell_w: i32, cell_h: i32, scale: i32| VramGeom {
        scale,
        cell_w,
        cell_h,
        extent: Rect::new(content.x, content.y, cols * cell_w, rows * cell_h),
        grid: true,
    };
    match tab {
        VramTab::Tiles => {
            let s = vram::fit_scale(content.w, content.h, 16 * 8, 24 * 8);
            tiled(16, 24, 8 * s, 8 * s, s)
        }
        VramTab::BgMap => {
            let s = vram::fit_scale(content.w, content.h, 32 * 8, 32 * 8);
            tiled(32, 32, 8 * s, 8 * s, s)
        }
        VramTab::Oam => {
            // 8×16 mode needs a taller row pitch so the stacked tiles don't overlap.
            let (nw, nh) = (8 * vram::oam_cell(1), 5 * vram::oam_cell_h(1, tall));
            let s = vram::fit_scale(content.w, content.h, nw, nh);
            tiled(8, 5, vram::oam_cell(s), vram::oam_cell_h(s, tall), s)
        }
        VramTab::Palettes => VramGeom {
            scale: 1,
            cell_w: 0,
            cell_h: 0,
            extent: content,
            grid: false,
        },
    }
}

/// Two-column Tiles layout (CGB): bank 0 grid left, bank 1 grid right, each a
/// 16×24 tile grid fitted to half the `content` width with a small gutter
/// between. Returns `(left, right, scale)`. Shared by the render and the hover
/// hit-test so they can't drift.
fn tiles_two_col(content: Rect) -> (Rect, Rect, i32) {
    const GUTTER: i32 = 6;
    let half_w = (content.w - GUTTER).max(0) / 2;
    let s = vram::fit_scale(half_w, content.h, 16 * 8, 24 * 8);
    let (gw, gh) = (16 * 8 * s, 24 * 8 * s);
    let left = Rect::new(content.x, content.y, gw, gh);
    let right = Rect::new(content.x + half_w + GUTTER, content.y, gw, gh);
    (left, right, s)
}

/// Two-column BG-map layout: BG tilemap left, window tilemap right, each a 32×32
/// tile grid fitted to half the `content` width with a small gutter. Mirrors
/// [`tiles_two_col`]; shared by the render and the hover hit-test.
fn bgmap_two_col(content: Rect) -> (Rect, Rect, i32) {
    const GUTTER: i32 = 6;
    let half_w = (content.w - GUTTER).max(0) / 2;
    let s = vram::fit_scale(half_w, content.h, 32 * 8, 32 * 8);
    let g = 32 * 8 * s;
    let left = Rect::new(content.x, content.y, g, g);
    let right = Rect::new(content.x + half_w + GUTTER, content.y, g, g);
    (left, right, s)
}

pub(super) fn render_vram(
    gb: &GameBoy,
    c: &mut Canvas,
    area: Rect,
    theme: &Theme,
    state: &VramState,
) {
    let l = vram::layout(area);
    vram::render_tabs(c, area.x + 2, area.y + 2, state.tab, theme);
    let cgb = gb.model().is_cgb();
    let tall = gb.debug_read(0xFF40) & 0x04 != 0;
    let g = vram_geom(state.tab, l.content, tall);
    // CGB has two VRAM banks; the Tiles tab shows both side by side (bank 0 left,
    // bank 1 right), so its geometry differs from the single-grid vram_geom (each
    // grid fits half the content width). DMG has one bank → None.
    let tiles_two = (state.tab == VramTab::Tiles && cgb).then(|| tiles_two_col(l.content));
    // The BG-map tab shows the BG tilemap (left) and window tilemap (right) side
    // by side, like the two-bank Tiles view.
    let bgmap_two = (state.tab == VramTab::BgMap).then(|| bgmap_two_col(l.content));
    match state.tab {
        VramTab::Tiles => {
            // A raw tile has no inherent palette, so bgb renders the Tiles grid
            // in a neutral grey ramp rather than through one game palette. On CGB
            // both banks show at once (bank 0 left, bank 1 right); DMG has one.
            if let Some((left, right, s)) = tiles_two {
                vram::render_tiles(c, left, gb.vram(), 0, &vram::GREYS, s);
                vram::render_tiles(c, right, gb.vram(), 1, &vram::GREYS, s);
            } else {
                vram::render_tiles(c, l.content, gb.vram(), 0, &vram::GREYS, g.scale);
            }
        }
        VramTab::Oam => {
            let (pals, n) = obj_palettes(gb, state.show_paletted);
            vram::render_oam(
                c,
                l.content,
                gb.oam(),
                gb.vram(),
                &pals[..n],
                cgb,
                tall,
                g.scale,
            );
        }
        VramTab::BgMap => {
            let (bg_base, win_base, signed) = bgmap_bases(gb, state);
            let (pals, n) = bg_palettes(gb, state.show_paletted);
            let (left, right, s) = bgmap_two.expect("bgmap_two set on the BG map tab");
            // Left = BG tilemap with the screen viewport box; right = window
            // tilemap with the WX/WY region box (both gated by `scxy`).
            vram::render_bgmap(
                c,
                left,
                gb.vram(),
                bg_base,
                signed,
                &pals[..n],
                cgb,
                s,
                screen_overlay(gb, state.scxy),
                theme,
            );
            vram::render_bgmap(
                c,
                right,
                gb.vram(),
                win_base,
                signed,
                &pals[..n],
                cgb,
                s,
                window_overlay(gb, state.scxy),
                theme,
            );
        }
        VramTab::Palettes => {
            // On a monochrome model the CGB palette RAM is meaningless; show the
            // BGP/OBP0/OBP1 shade mappings instead (so rBGP/rOBP are inspectable).
            // CGB/AGB use the palette RAM path below.
            if !gb.model().is_cgb() {
                vram::render_palettes_dmg(
                    c,
                    l.content,
                    gb.debug_read(0xFF47),
                    gb.debug_read(0xFF48),
                    gb.debug_read(0xFF49),
                    theme,
                );
            } else {
                let (bg, obj) = gb.cgb_palette_ram();
                vram::render_palettes(c, l.content, bg, obj, theme);
            }
        }
    }
    // bgb frames the grid and the details column as separate panels. The grid
    // tabs frame the *bounded* extent (so the map doesn't bleed grid lines into
    // empty space); Palettes frames the whole content area. The two-grid Tiles /
    // BG-map views frame each grid separately.
    let two = tiles_two.or(bgmap_two);
    if let Some((left, right, s)) = two {
        let cell = 8 * s;
        if state.grid {
            draw_grid(c, left, cell, cell, theme);
            draw_grid(c, right, cell, cell, theme);
        }
        c.outline_rect(left, theme.border);
        c.outline_rect(right, theme.border);
    } else {
        if state.grid && g.grid {
            draw_grid(c, g.extent, g.cell_w, g.cell_h, theme);
        }
        c.outline_rect(if g.grid { g.extent } else { l.content }, theme.border);
    }
    c.outline_rect(l.details, theme.border);
    render_vram_controls(c, &l, state, cgb, theme);
    render_vram_details(gb, c, &l, state, g.scale, two, theme);
}

/// The BG-map tab's 8 BG palettes (CGB) or single BGP palette (DMG) as RGB888,
/// or a single neutral grey ramp when `show_paletted` is off.
/// Expand `cram`'s 8 CGB palettes (BG or OBJ) into `out` as RGB888.
fn cgb_palettes(cram: &[u8], out: &mut [[u32; 4]; 8]) {
    for (p, slot) in out.iter_mut().enumerate() {
        *slot = debug::cgb_palette_words(cram, p).map(xrgb);
    }
}

/// A DMG palette register (`BGP`/`OBP*`) as four RGB888 shades.
fn dmg_palette(gb: &GameBoy, reg: u16) -> [u32; 4] {
    debug::dmg_palette_shades(gb.debug_read(reg)).map(|s| vram::GREYS[s as usize])
}

fn bg_palettes(gb: &GameBoy, show_paletted: bool) -> ([[u32; 4]; 8], usize) {
    let mut out = [vram::GREYS; 8];
    if !show_paletted {
        (out, 1)
    } else if gb.model().is_cgb() {
        cgb_palettes(gb.cgb_palette_ram().0, &mut out);
        (out, 8)
    } else {
        out[0] = dmg_palette(gb, 0xFF47);
        (out, 1)
    }
}

/// The OAM tab's 8 OBJ palettes (CGB) or the OBP0/OBP1 pair (DMG) as RGB888, or
/// a single neutral grey ramp when `show_paletted` is off. Returns a fixed array
/// + the live count (no per-redraw allocation).
fn obj_palettes(gb: &GameBoy, show_paletted: bool) -> ([[u32; 4]; 8], usize) {
    let mut out = [vram::GREYS; 8];
    if !show_paletted {
        (out, 1)
    } else if gb.model().is_cgb() {
        cgb_palettes(gb.cgb_palette_ram().1, &mut out);
        (out, 8)
    } else {
        out[0] = dmg_palette(gb, 0xFF48);
        out[1] = dmg_palette(gb, 0xFF49);
        (out, 2)
    }
}

/// The BG grid's screen viewport (SCX/SCY) box when `on`, else no overlay.
fn screen_overlay(gb: &GameBoy, on: bool) -> vram::MapOverlay {
    if on {
        vram::MapOverlay::Screen {
            scx: gb.debug_read(0xFF43),
            scy: gb.debug_read(0xFF42),
        }
    } else {
        vram::MapOverlay::None
    }
}

/// The window grid's WX/WY region box when `on`, else no overlay.
fn window_overlay(gb: &GameBoy, on: bool) -> vram::MapOverlay {
    if on {
        vram::MapOverlay::Window {
            wx: gb.debug_read(0xFF4B),
            wy: gb.debug_read(0xFF4A),
        }
    } else {
        vram::MapOverlay::None
    }
}

/// A 15-bit BGR555 word as an XRGB8888 pixel.
fn xrgb(word: u16) -> u32 {
    let (r, g, b) = debug::rgb555_to_rgb888(word);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Resolve the two BG-map bases (BG tilemap, window tilemap) + shared tile
/// addressing from the source radios, falling back to LCDC auto-detection: BG uses
/// the BG tilemap select (bit 3), window the window tilemap select (bit 6).
// ponytail: an explicit `Map` radio (9800/9C00) forces BOTH grids to that base
// (they then show the same region) — `Auto` is the useful default that shows the
// two distinct maps.
fn bgmap_bases(gb: &GameBoy, state: &VramState) -> (u16, u16, bool) {
    let lcdc = gb.debug_read(0xFF40);
    let base_of = |auto_9c00: bool| match state.map_src {
        1 => 0x9800,
        2 => 0x9C00,
        _ if auto_9c00 => 0x9C00,
        _ => 0x9800,
    };
    let signed = match state.tile_src {
        1 => true,
        2 => false,
        _ => lcdc & 0x10 == 0,
    };
    (base_of(lcdc & 0x08 != 0), base_of(lcdc & 0x40 != 0), signed)
}

/// Overlay grid lines at `cell_w`×`cell_h` pitch over the content area (the OAM
/// tab's cells are taller than wide in 8×16 mode).
fn draw_grid(c: &mut Canvas, content: Rect, cell_w: i32, cell_h: i32, theme: &Theme) {
    let saved = c.push_clip(content);
    let mut x = content.x;
    while x <= content.right() && cell_w > 0 {
        c.vline(x, content.y, content.h, theme.hilight);
        x += cell_w;
    }
    let mut y = content.y;
    while y <= content.bottom() && cell_h > 0 {
        c.hline(content.x, y, content.w, theme.hilight);
        y += cell_h;
    }
    c.set_clip(saved);
}

/// Draw the checkboxes/radios in the details column, reflecting `state`. `cgb`
/// gates the CGB-only Tiles bank toggle.
fn render_vram_controls(
    c: &mut Canvas,
    l: &VramLayout,
    state: &VramState,
    cgb: bool,
    theme: &Theme,
) {
    if state.tab == VramTab::Tiles && cgb {
        checkbox(
            c,
            l.tile_bank_box.x,
            l.tile_bank_box.y,
            state.tile_bank != 0,
            "VRAM bank 1",
            theme,
        );
    }
    if state.tab == VramTab::BgMap {
        radio_group(
            c,
            l.map_src[0].x,
            l.map_src[0].y,
            &vram::MAP_SRC,
            state.map_src as usize,
            theme,
        );
        radio_group(
            c,
            l.tile_src[0].x,
            l.tile_src[0].y,
            &vram::TILE_SRC,
            state.tile_src as usize,
            theme,
        );
        checkbox(c, l.scxy_box.x, l.scxy_box.y, state.scxy, "scxy", theme);
    }
    checkbox(
        c,
        l.paletted_box.x,
        l.paletted_box.y,
        state.show_paletted,
        "show paletted",
        theme,
    );
    if state.tab != VramTab::Palettes {
        checkbox(c, l.grid_box.x, l.grid_box.y, state.grid, "Grid", theme);
    }
}

/// Draw the hovered-cell field list (bgb's right panel) for the active tab.
/// `scale` is the tab's live render scale ([`vram_geom`]), so the hover hit-test
/// matches the drawn cell size at any window size.
fn render_vram_details(
    gb: &GameBoy,
    c: &mut Canvas,
    l: &VramLayout,
    state: &VramState,
    scale: i32,
    two: Option<(Rect, Rect, i32)>,
    theme: &Theme,
) {
    let Some((hx, hy)) = state.hover else {
        return;
    };
    let (lx, ly) = (hx - l.content.x, hy - l.content.y);
    if lx < 0 || ly < 0 {
        return;
    }
    let m8 = state.tile_hex_8bit;
    let lines = match state.tab {
        VramTab::Tiles => match two {
            Some((left, right, s)) => tile_details_two(lx, ly, left, right, s, m8),
            None => tile_details(lx, ly, scale, m8),
        },
        VramTab::Oam => oam_details(gb, lx, ly, scale, m8),
        VramTab::BgMap => match two {
            Some((left, right, s)) => bgmap_details_two(gb, state, lx, ly, left, right, s, m8),
            None => Vec::new(),
        },
        VramTab::Palettes => return,
    };
    let mut y = l.details.y;
    for line in lines {
        draw_text(c, l.details.x, y, &line, theme.text);
        y += line_height();
    }
}

/// A count shown decimal with its hex in parens, bgb-style: `10 ($0A)`,
/// `383 ($17F)`. Min two hex digits, widening as needed (tiles reach 383). When
/// `mask8` (Options → Debug "8-bit tile hex", matching tools that show the raw
/// tilemap byte) the hex wraps to the low 8 bits, so `383 ($7F)`.
fn dec_hex(n: u32, mask8: bool) -> String {
    let hex = if mask8 { n & 0xFF } else { n };
    format!("{n} (${hex:02X})")
}

/// Tiles-tab details: the tile under `(lx, ly)` in the 16-wide grid at `scale`.
/// The content area is wider than the grid, so an out-of-column hover has no tile.
fn tile_details(lx: i32, ly: i32, scale: i32, mask8: bool) -> Vec<String> {
    let col = lx / (8 * scale);
    let tile = (ly / (8 * scale)) * 16 + col;
    if col >= 16 || !(0..384).contains(&tile) {
        return Vec::new();
    }
    vec![
        format!("Tile No. {}", dec_hex(tile as u32, mask8)),
        format!("Tile Address 0:{:04X}", 0x8000 + tile * 16),
    ]
}

/// Two-bank Tiles hover (CGB): resolve content-relative `(lx, ly)` to a tile in
/// the left (bank 0) or right (bank 1) grid — geometry from [`tiles_two_col`] —
/// and print the real bank in the `bank:addr` label. A hover in the gutter or
/// off-grid yields no tile.
fn tile_details_two(
    lx: i32,
    ly: i32,
    left: Rect,
    right: Rect,
    scale: i32,
    mask8: bool,
) -> Vec<String> {
    let (bank, gx) = if lx < left.w {
        (0, lx)
    } else {
        let rx = lx - (right.x - left.x);
        if (0..right.w).contains(&rx) {
            (1, rx)
        } else {
            return Vec::new(); // gutter between the two grids
        }
    };
    let col = gx / (8 * scale);
    let tile = (ly / (8 * scale)) * 16 + col;
    if col >= 16 || !(0..384).contains(&tile) {
        return Vec::new();
    }
    vec![
        format!("Tile No. {}", dec_hex(tile as u32, mask8)),
        format!("Tile Address {bank}:{:04X}", 0x8000 + tile * 16),
    ]
}

/// OAM-tab details: the sprite under `(lx, ly)` in the 8-wide cell grid at `scale`.
fn oam_details(gb: &GameBoy, lx: i32, ly: i32, scale: i32, mask8: bool) -> Vec<String> {
    let tall = gb.debug_read(0xFF40) & 0x04 != 0;
    let (col, row) = (
        lx / vram::oam_cell(scale),
        ly / vram::oam_cell_h(scale, tall),
    );
    let idx = (row * 8 + col) as usize;
    if col >= 8 || idx >= 40 {
        return Vec::new();
    }
    let s = debug::oam_sprites(gb.oam())[idx];
    vec![
        format!("OAM addr FE{:02X}", idx * 4),
        format!("X-loc {}", s.x),
        format!("Y-loc {}", s.y),
        format!("Tile No {}", dec_hex(u32::from(s.tile), mask8)),
        format!("Attribute {:02X}", s.attr),
        format!("X-flip {}", u8::from(s.attr & 0x20 != 0)),
        format!("Y-flip {}", u8::from(s.attr & 0x40 != 0)),
        format!("Palette OBJ {}", s.attr & 0x07),
    ]
}

/// BG-map-tab details: resolve content-relative `(lx, ly)` to a cell in the left
/// (BG tilemap) or right (window tilemap) grid — geometry from [`bgmap_two_col`] —
/// and print which map it is + its address. A hover in the gutter or off-grid
/// yields no cell.
#[allow(clippy::too_many_arguments)]
fn bgmap_details_two(
    gb: &GameBoy,
    state: &VramState,
    lx: i32,
    ly: i32,
    left: Rect,
    right: Rect,
    scale: i32,
    mask8: bool,
) -> Vec<String> {
    let (is_window, gx) = if lx < left.w {
        (false, lx)
    } else {
        let rx = lx - (right.x - left.x);
        if (0..right.w).contains(&rx) {
            (true, rx)
        } else {
            return Vec::new(); // gutter between the two grids
        }
    };
    let (col, row) = (gx / (8 * scale), ly / (8 * scale));
    if col >= 32 || row >= 32 {
        return Vec::new();
    }
    let (bg_base, win_base, signed) = bgmap_bases(gb, state);
    let base = if is_window { win_base } else { bg_base };
    let idx = (row * 32 + col) as usize;
    let cell = debug::bg_map(gb.vram(), base)[idx];
    let tile = vram::tile_index(cell.tile, signed);
    vec![
        format!(
            "{}  X {col}  Y {row}",
            if is_window { "Window" } else { "BG" }
        ),
        format!("Tile No. {}", dec_hex(u32::from(cell.tile), mask8)),
        format!("Attribute {:02X}", cell.attr),
        format!("Map address {:04X}", base as usize + idx),
        format!("Tile address 0:{:04X}", 0x8000 + tile * 16),
        format!("X-flip {}", u8::from(cell.attr & 0x20 != 0)),
        format!("Y-flip {}", u8::from(cell.attr & 0x40 != 0)),
        format!("palette BG {}", cell.attr & 0x07),
    ]
}

#[cfg(test)]
#[path = "vram_render_tests.rs"]
mod tests;
