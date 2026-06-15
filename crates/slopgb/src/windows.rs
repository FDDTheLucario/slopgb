//! Layer C: the bgb tool windows (Debugger / VRAM viewer / I/O map). Each is a
//! pure content renderer composing the `ui` widgets over `slopgb_core::debug`
//! introspection, unit-tested headless; the event loop (B12b) feeds each one a
//! real softbuffer surface and routes its input.
#![allow(dead_code, unused_imports)] // scaffolding; wired to winit surfaces in B12b.

pub mod debugger;
pub mod iomap;
pub mod vram;

use slopgb_core::GameBoy;

use crate::ui::canvas::Rect;
use crate::ui::text::line_height;
use crate::ui::{Canvas, Theme, ToolWindow};

/// Render a tool window's full content into `c` from the live machine — the
/// single entry point the event loop's redraw calls (B12b). Pure (`&GameBoy`),
/// so it tests headless against a real machine; the winit layer only has to
/// hand it a surface buffer.
pub fn render(kind: ToolWindow, gb: &GameBoy, c: &mut Canvas, theme: &Theme) {
    let area = c.bounds();
    c.fill_rect(area, theme.bg);
    match kind {
        ToolWindow::Debugger => render_debugger(gb, c, area, theme),
        ToolWindow::Vram => render_vram(gb, c, area, theme),
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

fn render_vram(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme) {
    let tabs = vram::render_tabs(c, area.x + 2, area.y + 2, vram::VramTab::Tiles, theme);
    let top = tabs.first().map_or(area.y, |r| r.bottom()) + 2;
    let content = Rect::new(area.x + 2, top, area.w - 4, area.bottom() - top);
    vram::render_tiles(c, content, gb.vram(), 0, &vram::GREYS, 2);
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
