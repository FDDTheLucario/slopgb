//! Layer C: the bgb tool windows (Debugger / VRAM viewer / I/O map). Each is a
//! pure content renderer composing the `ui` widgets over `slopgb_core::debug`
//! introspection, unit-tested headless; the event loop (B12b) feeds each one a
//! real softbuffer surface and routes its input.

pub mod debugger;
pub mod iomap;
pub mod vram;

use slopgb_core::{GameBoy, Model, debug};

use crate::ui::canvas::Rect;
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::{checkbox, radio_group};
use crate::ui::{Canvas, Theme, ToolWindow};
use vram::{VramLayout, VramState, VramTab};

/// Per-window interactive state. Only the VRAM viewer is stateful so far; the
/// debugger and I/O map carry no UI state yet (their interaction lands later).
#[derive(Clone, Debug)]
pub enum WinState {
    Stateless,
    Vram(VramState),
}

impl WinState {
    /// The initial state a freshly-opened window of `kind` owns.
    #[must_use]
    pub fn new(kind: ToolWindow) -> Self {
        match kind {
            ToolWindow::Vram => WinState::Vram(VramState::default()),
            _ => WinState::Stateless,
        }
    }
}

/// Render a tool window's full content into `c` from the live machine and its
/// persistent UI `state` — the single entry point the event loop's redraw calls
/// (B12b). Pure (`&GameBoy`), so it tests headless against a real machine; the
/// winit layer only has to hand it a surface buffer + the window's state.
pub fn render(kind: ToolWindow, gb: &GameBoy, c: &mut Canvas, theme: &Theme, state: &WinState) {
    let area = c.bounds();
    c.fill_rect(area, theme.bg);
    match kind {
        ToolWindow::Debugger => render_debugger(gb, c, area, theme),
        ToolWindow::Vram => {
            let default = VramState::default();
            let st = match state {
                WinState::Vram(s) => s,
                _ => &default,
            };
            render_vram(gb, c, area, theme, st);
        }
        ToolWindow::IoMap => render_iomap(gb, c, area, theme),
    }
}

fn regs_view(gb: &GameBoy) -> debugger::RegsView {
    let r = gb.cpu_regs();
    debugger::RegsView {
        af: r.af(),
        bc: r.bc(),
        de: r.de(),
        hl: r.hl(),
        sp: r.sp,
        pc: r.pc,
        ime: gb.ime(),
        ima: gb.ime_pending(),
        lcdc: gb.debug_read(0xFF40),
        stat: gb.debug_read(0xFF41),
        ly: gb.debug_read(0xFF44),
        ie: gb.debug_read(0xFFFF),
        iflag: gb.debug_read(0xFF0F),
        double_speed: gb.double_speed(),
    }
}

fn render_debugger(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme) {
    let l = debugger::DebuggerLayout::for_size(area.w, area.h);
    let pc = gb.cpu_regs().pc;
    // Disasm from PC; memory dump from the stack page; stack from SP.
    debugger::render_disasm(c, l.disasm, |a| gb.debug_read(a), pc, pc, theme);
    debugger::render_regs(c, l.regs, &regs_view(gb), theme);
    let stack_rows = (l.stack.h / line_height()).max(0) as usize;
    debugger::render_stack(c, l.stack, &gb.stack(stack_rows), theme);
    debugger::render_memory(c, l.memory, |a| gb.debug_read(a), 0xFF00, theme);
}

/// Tile-grid scale for the Tiles tab, and OAM-cell preview scale.
const TILE_SCALE: i32 = 2;
const OAM_SCALE: i32 = 2;

fn render_vram(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme, state: &VramState) {
    let l = vram::layout(area);
    vram::render_tabs(c, area.x + 2, area.y + 2, state.tab, theme);
    let pal = display_palette(gb, state.show_paletted);
    // Draw the active tab into the content area; `cell` is its grid pitch (0 =
    // no grid, e.g. Palettes).
    let cell = match state.tab {
        VramTab::Tiles => {
            // A raw tile has no inherent palette, so bgb renders the Tiles grid
            // in a neutral grey ramp (its "guessed palette" field stays empty)
            // rather than mapping every tile through one game palette — which
            // would tint unrelated tiles with whatever colours BG palette 0
            // happens to hold. Match that: grey regardless of `show paletted`.
            vram::render_tiles(c, l.content, gb.vram(), 0, &vram::GREYS, TILE_SCALE);
            8 * TILE_SCALE
        }
        VramTab::Oam => {
            vram::render_oam(c, l.content, gb.oam(), gb.vram(), &pal, OAM_SCALE);
            8 * OAM_SCALE + 4
        }
        VramTab::BgMap => {
            let (base, signed) = bgmap_source(gb, state);
            let scx = gb.debug_read(0xFF43);
            let scy = gb.debug_read(0xFF42);
            vram::render_bgmap(
                c,
                l.content,
                gb.vram(),
                base,
                signed,
                scx,
                scy,
                &pal,
                1,
                state.scxy,
                theme,
            );
            8
        }
        VramTab::Palettes => {
            let (bg, obj) = gb.cgb_palette_ram();
            vram::render_palettes(c, l.content, bg, obj, theme);
            0
        }
    };
    if state.grid && cell > 0 {
        draw_grid(c, l.content, cell, theme);
    }
    // bgb frames the grid and the details column as separate panels.
    c.outline_rect(l.content, theme.border);
    c.outline_rect(l.details, theme.border);
    render_vram_controls(c, &l, state, theme);
    render_vram_details(gb, c, &l, state, theme);
}

/// The 4-colour display palette: the neutral grey ramp, or — when `show_paletted`
/// is on — the game's guessed palette (DMG: BGP shades; CGB/AGB: BG palette 0).
fn display_palette(gb: &GameBoy, show_paletted: bool) -> [u32; 4] {
    if !show_paletted {
        return vram::GREYS;
    }
    match gb.model() {
        Model::Dmg => {
            debug::dmg_palette_shades(gb.debug_read(0xFF47)).map(|s| vram::GREYS[s as usize])
        }
        _ => {
            let (bg, _obj) = gb.cgb_palette_ram();
            debug::cgb_palette_words(bg, 0).map(xrgb)
        }
    }
}

/// A 15-bit BGR555 word as an XRGB8888 pixel.
fn xrgb(word: u16) -> u32 {
    let (r, g, b) = debug::rgb555_to_rgb888(word);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Resolve the BG-map base + tile-addressing from the source radios, falling
/// back to LCDC auto-detection (`Auto`).
fn bgmap_source(gb: &GameBoy, state: &VramState) -> (u16, bool) {
    let lcdc = gb.debug_read(0xFF40);
    let base = match state.map_src {
        1 => 0x9800,
        2 => 0x9C00,
        _ if lcdc & 0x08 != 0 => 0x9C00,
        _ => 0x9800,
    };
    let signed = match state.tile_src {
        1 => true,
        2 => false,
        _ => lcdc & 0x10 == 0,
    };
    (base, signed)
}

/// Overlay grid lines at `cell` pitch over the content area.
fn draw_grid(c: &mut Canvas, content: Rect, cell: i32, theme: &Theme) {
    let saved = c.push_clip(content);
    let mut x = content.x;
    while x <= content.right() {
        c.vline(x, content.y, content.h, theme.hilight);
        x += cell;
    }
    let mut y = content.y;
    while y <= content.bottom() {
        c.hline(content.x, y, content.w, theme.hilight);
        y += cell;
    }
    c.set_clip(saved);
}

/// Draw the checkboxes/radios in the details column, reflecting `state`.
fn render_vram_controls(c: &mut Canvas, l: &VramLayout, state: &VramState, theme: &Theme) {
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
fn render_vram_details(
    gb: &GameBoy,
    c: &mut Canvas,
    l: &VramLayout,
    state: &VramState,
    theme: &Theme,
) {
    let Some((hx, hy)) = state.hover else {
        return;
    };
    let (lx, ly) = (hx - l.content.x, hy - l.content.y);
    if lx < 0 || ly < 0 {
        return;
    }
    let lines = match state.tab {
        VramTab::Tiles => tile_details(lx, ly),
        VramTab::Oam => oam_details(gb, lx, ly),
        VramTab::BgMap => bgmap_details(gb, state, lx, ly),
        VramTab::Palettes => return,
    };
    let mut y = l.details.y;
    for line in lines {
        draw_text(c, l.details.x, y, &line, theme.text);
        y += line_height();
    }
}

/// Tiles-tab details: the tile under `(lx, ly)` in the 16-wide grid. The content
/// area is wider than the 256-px grid, so an out-of-column hover has no tile.
fn tile_details(lx: i32, ly: i32) -> Vec<String> {
    let col = lx / (8 * TILE_SCALE);
    let tile = (ly / (8 * TILE_SCALE)) * 16 + col;
    if col >= 16 || !(0..384).contains(&tile) {
        return Vec::new();
    }
    vec![
        format!("Tile No. {tile}"),
        format!("Tile Address 0:{:04X}", 0x8000 + tile * 16),
    ]
}

/// OAM-tab details: the sprite under `(lx, ly)` in the 8-wide cell grid.
fn oam_details(gb: &GameBoy, lx: i32, ly: i32) -> Vec<String> {
    let pitch = 8 * OAM_SCALE + 4;
    let (col, row) = (lx / pitch, ly / pitch);
    let idx = (row * 8 + col) as usize;
    if col >= 8 || idx >= 40 {
        return Vec::new();
    }
    let s = debug::oam_sprites(gb.oam())[idx];
    vec![
        format!("OAM addr FE{:02X}", idx * 4),
        format!("X-loc {}", s.x),
        format!("Y-loc {}", s.y),
        format!("Tile No {}", s.tile),
        format!("Attribute {:02X}", s.attr),
        format!("X-flip {}", u8::from(s.attr & 0x20 != 0)),
        format!("Y-flip {}", u8::from(s.attr & 0x40 != 0)),
        format!("Palette OBJ {}", s.attr & 0x07),
    ]
}

/// BG-map-tab details: the map cell under `(lx, ly)` in the 32×32 grid.
fn bgmap_details(gb: &GameBoy, state: &VramState, lx: i32, ly: i32) -> Vec<String> {
    let (col, row) = (lx / 8, ly / 8);
    if col >= 32 || row >= 32 {
        return Vec::new();
    }
    let (base, signed) = bgmap_source(gb, state);
    let idx = (row * 32 + col) as usize;
    let cell = debug::bg_map(gb.vram(), base)[idx];
    let tile = vram::tile_index(cell.tile, signed);
    vec![
        format!("X {col}  Y {row}"),
        format!("Tile No. {}", cell.tile),
        format!("Attribute {:02X}", cell.attr),
        format!("Map address {:04X}", base as usize + idx),
        format!("Tile address 0:{:04X}", 0x8000 + tile * 16),
        format!("X-flip {}", u8::from(cell.attr & 0x20 != 0)),
        format!("Y-flip {}", u8::from(cell.attr & 0x40 != 0)),
        format!("palette BG {}", cell.attr & 0x07),
    ]
}

fn render_iomap(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme) {
    let read = |a: u16| gb.debug_read(a);
    let col_w = area.w / 3;
    iomap::render_group(c, area.x + 2, area.y + 2, &read, iomap::LCD, theme);
    iomap::render_group(c, area.x + col_w, area.y + 2, &read, iomap::VARIOUS, theme);
    iomap::render_group(
        c,
        area.x + 2 * col_w,
        area.y + 2,
        &read,
        iomap::SOUND,
        theme,
    );
    // LCDC / STAT bit breakdowns under the first column.
    let bits_y = area.y + 2 + (iomap::LCD.len() as i32 + 1) * line_height();
    iomap::render_bits(
        c,
        area.x + 2,
        bits_y,
        read(0xFF40),
        &iomap::LCDC_BITS,
        7,
        theme,
    );
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
