//! Layer C: the bgb tool windows (Debugger / VRAM viewer / I/O map). Each is a
//! pure content renderer composing the `ui` widgets over `slopgb_core::debug`
//! introspection, unit-tested headless; the event loop (B12b) feeds each one a
//! real softbuffer surface and routes its input.

pub mod debugger;
pub mod iomap;
pub mod mainwin;
pub mod options;
pub mod vram;

use std::rc::Rc;

use slopgb_core::{GameBoy, debug};

use crate::dbg::Breakpoints;
use crate::symbols::SymbolTable;
use crate::ui::canvas::Rect;
use crate::ui::dialog::InputDialog;
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::{checkbox, radio_group};
use crate::ui::{Canvas, Theme, ToolWindow};
use debugger::DebuggerState;
use vram::{VramLayout, VramState, VramTab};

/// Per-window interactive state. The VRAM viewer and the debugger carry view
/// state (active tab / hover, or disasm cursor + open menu); the I/O map is
/// stateless.
#[derive(Clone, Debug)]
pub enum WinState {
    Stateless,
    Vram(VramState),
    // Boxed: `DebuggerState` is much larger than the other variants (disasm/menu/
    // dialog/bookmark state), and only ever a handful of `WinState`s exist (one
    // per open tool window), so the indirection costs nothing and keeps the enum
    // small. Deref coercion makes the box transparent at the match sites.
    Debugger(Box<DebuggerState>),
    Memory(MemoryView),
}

impl WinState {
    /// The initial state a freshly-opened window of `kind` owns.
    #[must_use]
    pub fn new(kind: ToolWindow) -> Self {
        match kind {
            ToolWindow::Vram => WinState::Vram(VramState::default()),
            ToolWindow::Debugger => WinState::Debugger(Box::default()),
            ToolWindow::IoMap => WinState::Stateless,
            ToolWindow::MemoryViewer => WinState::Memory(MemoryView::default()),
        }
    }
}

/// State for the standalone memory viewer window: the visible base address and
/// the loaded symbols (for the status bar). Navigated with the wheel / arrows /
/// PageUp-Down like the debugger's integrated pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryView {
    pub mem_base: u16,
    pub symbols: Rc<SymbolTable>,
    /// Open `Go to…` prompt (Ctrl+G); `None` when idle. Mirrors the integrated
    /// debugger pane's modal, scoped to this standalone window.
    pub goto: Option<InputDialog>,
}

impl Default for MemoryView {
    fn default() -> Self {
        Self {
            mem_base: 0xFF00,
            symbols: Rc::new(SymbolTable::default()),
            goto: None,
        }
    }
}

impl MemoryView {
    /// Scroll the base by `rows` rows of 16 bytes (negative = lower addresses),
    /// wrapping the 64 KiB space — same model as the debugger memory pane.
    pub fn scroll(&mut self, rows: i32) {
        self.mem_base = self.mem_base.wrapping_add(rows.wrapping_mul(16) as u16);
    }

    /// Apply a `Go to…` entry: a loaded symbol name resolves to its address,
    /// else a hex parse (accepting `$`/`0x` prefixes). Returns whether the entry
    /// resolved; an empty/garbage entry leaves `mem_base` unchanged. Mirrors the
    /// integrated pane's accept_dialog Goto arm.
    pub fn apply_goto(&mut self, text: &str) -> bool {
        let t = text.trim();
        let hex = t.trim_start_matches('$').trim_start_matches("0x");
        if let Some(addr) = self.symbols.resolve(t).or_else(|| u16::from_str_radix(hex, 16).ok()) {
            self.mem_base = addr;
            true
        } else {
            false
        }
    }
}

/// Render a tool window's full content into `c` from the live machine and its
/// persistent UI `state` — the single entry point the event loop's redraw calls
/// (B12b). Pure (`&GameBoy`), so it tests headless against a real machine; the
/// winit layer only has to hand it a surface buffer + the window's state.
pub fn render(
    kind: ToolWindow,
    gb: &GameBoy,
    c: &mut Canvas,
    theme: &Theme,
    state: &WinState,
    bps: &Breakpoints,
) {
    let area = c.bounds();
    c.fill_rect(area, theme.bg);
    match kind {
        ToolWindow::Debugger => {
            let default = DebuggerState::default();
            let st = match state {
                WinState::Debugger(s) => s,
                _ => &default,
            };
            render_debugger(gb, c, area, theme, st, bps);
        }
        ToolWindow::Vram => {
            let default = VramState::default();
            let st = match state {
                WinState::Vram(s) => s,
                _ => &default,
            };
            render_vram(gb, c, area, theme, st);
        }
        ToolWindow::IoMap => render_iomap(gb, c, area, theme),
        ToolWindow::MemoryViewer => {
            let default = MemoryView::default();
            let st = match state {
                WinState::Memory(s) => s,
                _ => &default,
            };
            render_memory_window(gb, c, area, theme, st);
        }
    }
}

/// Render the standalone memory viewer: the hex dump from the base address
/// filling the window above a one-line status bar showing the nearest preceding
/// symbol (`Name+offset`, or `----` with no symbols loaded).
fn render_memory_window(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme, st: &MemoryView) {
    let lh = line_height();
    let body = Rect::new(area.x, area.y, area.w, (area.h - lh).max(0));
    debugger::render_memory(c, body, |a| gb.debug_read(a), st.mem_base, theme, &st.symbols);
    let bar_y = area.bottom() - lh;
    c.hline(area.x, bar_y, area.w, theme.border);
    let status = match st.symbols.nearest_before(st.mem_base) {
        Some((name, base)) => format!("{:04X}  {name}+{:X}", st.mem_base, st.mem_base - base),
        None => format!("{:04X}  ----", st.mem_base),
    };
    draw_text(c, area.x + 2, bar_y + 1, &status, theme.text);
    if let Some(dlg) = &st.goto {
        crate::ui::dialog::render(c, area, dlg, theme);
    }
}

fn regs_view(gb: &GameBoy, clock_base: u64) -> debugger::RegsView {
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
        // Emulated cycles since the last user-clock reset (RM14); low 32 bits.
        cnt: gb.cycles().wrapping_sub(clock_base) as u32,
        rom_bank: gb.rom_bank(),
        ram_bank: gb.ram_bank(),
    }
}

fn render_debugger(
    gb: &GameBoy,
    c: &mut Canvas,
    area: Rect,
    theme: &Theme,
    st: &DebuggerState,
    bps: &Breakpoints,
) {
    let l = debugger::DebuggerLayout::for_size(area.w, area.h);
    let pc = gb.cpu_regs().pc;
    // The menu bar across the top, highlighting an open dropdown's parent label.
    debugger::render_menubar(c, l.menu, st.menu.as_ref().and_then(|m| m.bar), theme);
    // Disasm follows PC (or the pinned base); memory + stack from their bases.
    let start = st.disasm_start(pc);
    let rows = debugger::render_disasm(
        c,
        l.disasm,
        |a| gb.debug_read(a),
        start,
        pc,
        bps,
        &st.data_hints,
        st.disasm_fmt,
        &st.symbols,
        theme,
    );
    // Profiler: overlay per-line execution counts while logging (MB5).
    if gb.profiling() {
        debugger::render_profile_counts(c, l.disasm, &rows, |a| gb.profile_count(a), theme);
    }
    debugger::render_regs(c, l.regs, &regs_view(gb, st.clock_base), theme);
    let stack_rows = (l.stack.h / line_height()).max(0) as usize;
    debugger::render_stack(c, l.stack, &gb.stack(stack_rows), theme);
    debugger::render_memory(c, l.memory, |a| gb.debug_read(a), st.mem_base, theme, &st.symbols);
    // The open context menu / modal draws last, on top of every pane.
    if let Some(om) = &st.menu {
        crate::ui::menu::render(c, om.origin, &om.items, om.hovered, theme);
    }
    if let Some(gd) = &st.dialog {
        crate::ui::dialog::render(c, area, &gd.input, theme);
    }
}

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

fn render_vram(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme, state: &VramState) {
    let l = vram::layout(area);
    vram::render_tabs(c, area.x + 2, area.y + 2, state.tab, theme);
    let cgb = gb.model().is_cgb();
    let tall = gb.debug_read(0xFF40) & 0x04 != 0;
    let g = vram_geom(state.tab, l.content, tall);
    match state.tab {
        VramTab::Tiles => {
            // A raw tile has no inherent palette, so bgb renders the Tiles grid
            // in a neutral grey ramp (its "guessed palette" field stays empty)
            // rather than mapping every tile through one game palette. The CGB
            // bank toggle picks which 8 KiB half to show (DMG has one bank).
            let bank = if cgb { usize::from(state.tile_bank) } else { 0 };
            vram::render_tiles(c, l.content, gb.vram(), bank, &vram::GREYS, g.scale);
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
            let (base, signed) = bgmap_source(gb, state);
            let (pals, n) = bg_palettes(gb, state.show_paletted);
            vram::render_bgmap(
                c,
                l.content,
                gb.vram(),
                base,
                signed,
                &pals[..n],
                cgb,
                g.scale,
                bgmap_overlay(gb, state),
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
    if state.grid && g.grid {
        draw_grid(c, g.extent, g.cell_w, g.cell_h, theme);
    }
    // bgb frames the grid and the details column as separate panels. The grid
    // tabs frame the *bounded* extent (so the map doesn't bleed grid lines into
    // empty space); Palettes frames the whole content area.
    c.outline_rect(if g.grid { g.extent } else { l.content }, theme.border);
    c.outline_rect(l.details, theme.border);
    render_vram_controls(c, &l, state, cgb, theme);
    render_vram_details(gb, c, &l, state, g.scale, theme);
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

/// The BG-map overlay box: the window's WX/WY region in window-view mode, else
/// the screen's SCX/SCY viewport — both gated by the `scxy` toggle.
fn bgmap_overlay(gb: &GameBoy, state: &VramState) -> vram::MapOverlay {
    if !state.scxy {
        return vram::MapOverlay::None;
    }
    if state.show_window {
        vram::MapOverlay::Window {
            wx: gb.debug_read(0xFF4B),
            wy: gb.debug_read(0xFF4A),
        }
    } else {
        vram::MapOverlay::Screen {
            scx: gb.debug_read(0xFF43),
            scy: gb.debug_read(0xFF42),
        }
    }
}

/// A 15-bit BGR555 word as an XRGB8888 pixel.
fn xrgb(word: u16) -> u32 {
    let (r, g, b) = debug::rgb555_to_rgb888(word);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Resolve the BG-map base + tile-addressing from the source radios, falling
/// back to LCDC auto-detection (`Auto`). In window-view mode the auto base is the
/// window tilemap select (LCDC bit 6) instead of the BG select (bit 3).
fn bgmap_source(gb: &GameBoy, state: &VramState) -> (u16, bool) {
    let lcdc = gb.debug_read(0xFF40);
    let auto_select = if state.show_window {
        lcdc & 0x40 != 0
    } else {
        lcdc & 0x08 != 0
    };
    let base = match state.map_src {
        1 => 0x9800,
        2 => 0x9C00,
        _ if auto_select => 0x9C00,
        _ => 0x9800,
    };
    let signed = match state.tile_src {
        1 => true,
        2 => false,
        _ => lcdc & 0x10 == 0,
    };
    (base, signed)
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
        checkbox(
            c,
            l.win_box.x,
            l.win_box.y,
            state.show_window,
            "window",
            theme,
        );
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
        VramTab::Tiles => tile_details(lx, ly, scale),
        VramTab::Oam => oam_details(gb, lx, ly, scale),
        VramTab::BgMap => bgmap_details(gb, state, lx, ly, scale),
        VramTab::Palettes => return,
    };
    let mut y = l.details.y;
    for line in lines {
        draw_text(c, l.details.x, y, &line, theme.text);
        y += line_height();
    }
}

/// Tiles-tab details: the tile under `(lx, ly)` in the 16-wide grid at `scale`.
/// The content area is wider than the grid, so an out-of-column hover has no tile.
fn tile_details(lx: i32, ly: i32, scale: i32) -> Vec<String> {
    let col = lx / (8 * scale);
    let tile = (ly / (8 * scale)) * 16 + col;
    if col >= 16 || !(0..384).contains(&tile) {
        return Vec::new();
    }
    vec![
        format!("Tile No. {tile}"),
        format!("Tile Address 0:{:04X}", 0x8000 + tile * 16),
    ]
}

/// OAM-tab details: the sprite under `(lx, ly)` in the 8-wide cell grid at `scale`.
fn oam_details(gb: &GameBoy, lx: i32, ly: i32, scale: i32) -> Vec<String> {
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
        format!("Tile No {}", s.tile),
        format!("Attribute {:02X}", s.attr),
        format!("X-flip {}", u8::from(s.attr & 0x20 != 0)),
        format!("Y-flip {}", u8::from(s.attr & 0x40 != 0)),
        format!("Palette OBJ {}", s.attr & 0x07),
    ]
}

/// BG-map-tab details: the map cell under `(lx, ly)` in the 32×32 grid at `scale`.
fn bgmap_details(gb: &GameBoy, state: &VramState, lx: i32, ly: i32, scale: i32) -> Vec<String> {
    let (col, row) = (lx / (8 * scale), ly / (8 * scale));
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
    let lh = line_height();
    let col_w = area.w / 4;
    let x = |i: i32| area.x + 2 + i * col_w;
    let y0 = area.y + 2;
    let label = |c: &mut Canvas, x: i32, y: i32, s: &str| draw_text(c, x, y, s, theme.text);

    // Col 0: LCD registers, then the LCDC bit breakdown.
    let after_lcd = iomap::render_group(c, x(0), y0, &read, iomap::LCD, theme);
    label(c, x(0), after_lcd + lh, "LCDC (FF40)");
    iomap::render_bits(
        c,
        x(0),
        after_lcd + 2 * lh,
        read(0xFF40),
        &iomap::LCDC_BITS,
        7,
        theme,
    );

    // Col 1: the "various" registers, then the STAT bit breakdown, then the
    // cartridge ROM/RAM bank indicator (distinct from VBK/SVBK above it).
    let after_var = iomap::render_group(c, x(1), y0, &read, iomap::VARIOUS, theme);
    label(c, x(1), after_var + lh, "STAT (FF41)");
    iomap::render_bits(
        c,
        x(1),
        after_var + 2 * lh,
        read(0xFF41),
        &iomap::STAT_BITS,
        6,
        theme,
    );
    label(
        c,
        x(1),
        after_var + (2 + iomap::STAT_BITS.len() as i32 + 1) * lh,
        &iomap::bank_line(gb.rom_bank(), gb.ram_bank()),
    );

    // Col 2: the sound channels + master control.
    iomap::render_group(c, x(2), y0, &read, iomap::SOUND, theme);

    // Col 3: GBC DMA + palette ports, then the IF/IE interrupt vectors.
    let after_dma = iomap::render_group(c, x(3), y0, &read, iomap::GBC_DMA, theme);
    let after_pal = iomap::render_group(c, x(3), after_dma + lh, &read, iomap::GBC_PAL, theme);
    label(c, x(3), after_pal + lh, "IF, IE");
    iomap::render_vectors(
        c,
        x(3),
        after_pal + 2 * lh,
        read(0xFF0F),
        read(0xFFFF),
        theme,
    );

    // Wave pattern (FF30–3F): full-width row along the bottom. Sourced from the
    // raw wave-RAM buffer (the gated FF3x read path is unreliable while ch3 plays).
    let wy = area.bottom() - lh - 2;
    label(c, x(0), wy, "wave (FF3x)");
    draw_text(
        c,
        x(0) + 11 * 8,
        wy,
        &iomap::wave_row(&gb.wave_ram()),
        theme.text,
    );
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
