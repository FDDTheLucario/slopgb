//! The bgb debugger window (Layer C): composes the `ui` widgets over
//! `slopgb_core::debug` introspection. This module is the window *content* —
//! pure rendering into a [`Canvas`], unit-tested headless; the winit surface
//! wiring (B12b) feeds it a real buffer later.

use slopgb_core::debug;

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::line_height;
use crate::ui::widgets::scroll_list;

/// The four panes of the debugger body, partitioned from the window size to
/// match bgb's layout (see `docs/bgb-reference/02-debugger.png`): a thin menu
/// bar, the disassembly pane filling the upper-left, the registers panel
/// top-right with the stack list below it, and the memory hex dump across the
/// bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebuggerLayout {
    pub menu: Rect,
    pub disasm: Rect,
    pub regs: Rect,
    pub stack: Rect,
    pub memory: Rect,
}

impl DebuggerLayout {
    /// Partition a `w × h` window. Proportions mirror bgb's ~1172×786 layout:
    /// menu bar fixed-height, memory pane ~38 % of the body at the bottom,
    /// registers/stack a right-hand column ~⅓ wide, registers the top ~30 % of
    /// that column.
    #[must_use]
    pub fn for_size(w: i32, h: i32) -> Self {
        let menu_h = 18.min(h);
        let body_top = menu_h;
        let mem_h = ((h - menu_h) * 38 / 100).max(0);
        let body_bot = h - mem_h;
        let right_w = (w * 33 / 100).clamp(0, w);
        let left_w = w - right_w;
        let body_h = body_bot - body_top;
        let regs_h = (body_h * 30 / 100).max(0);
        Self {
            menu: Rect::new(0, 0, w, menu_h),
            disasm: Rect::new(0, body_top, left_w, body_h),
            regs: Rect::new(left_w, body_top, right_w, regs_h),
            stack: Rect::new(left_w, body_top + regs_h, right_w, body_h - regs_h),
            memory: Rect::new(0, body_bot, w, mem_h),
        }
    }
}

/// One decoded disassembly line: its address and the formatted bgb text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisasmRow {
    pub addr: u16,
    pub text: String,
}

/// A coarse bank label from the address region. Precise ROM/VRAM/WRAM bank
/// numbers are a deferred accessor (see handoff); this gives bgb's `ROM0:` for
/// the fixed bank and a best-effort tag elsewhere.
fn region_label(addr: u16) -> &'static str {
    match addr {
        0x0000..=0x3FFF => "ROM0",
        0x4000..=0x7FFF => "ROMX",
        0x8000..=0x9FFF => "VRAM",
        0xA000..=0xBFFF => "SRAM",
        0xC000..=0xCFFF => "WRA0",
        0xD000..=0xDFFF => "WRAX",
        0xE000..=0xFDFF => "ECHO",
        0xFE00..=0xFE9F => "OAM ",
        0xFEA0..=0xFEFF => "??? ",
        0xFF00..=0xFF7F => "I/O ",
        0xFF80..=0xFFFE => "HRAM",
        0xFFFF => "IE  ",
    }
}

/// Disassemble `count` instructions from `start`, each formatted as a bgb
/// disasm line `LABEL:ADDR  bytes  mnemonic  ;m-cycles`. `read(addr)` yields the
/// byte at `addr` (use `GameBoy::debug_read`). Exact column widths are tuned in
/// the C8 visual diff; the content (addr/bytes/mnemonic/cycles) is final.
pub fn disasm_rows(read: impl Fn(u16) -> u8, start: u16, count: usize) -> Vec<DisasmRow> {
    let mut rows = Vec::with_capacity(count);
    let mut addr = start;
    for _ in 0..count {
        let bytes = [
            read(addr),
            read(addr.wrapping_add(1)),
            read(addr.wrapping_add(2)),
        ];
        let insn = debug::decode(&bytes, addr);
        let hex: String = bytes[..insn.len as usize]
            .iter()
            .map(|b| format!("{b:02X} "))
            .collect();
        let text = format!(
            "{}:{addr:04X} {:<9}{:<20};{}",
            region_label(addr),
            hex.trim_end(),
            insn.text,
            insn.cycles
        );
        rows.push(DisasmRow { addr, text });
        addr = addr.wrapping_add(u16::from(insn.len.max(1)));
    }
    rows
}

/// Render the disasm pane: decode from `start` to fill `rect`, draw it with the
/// row at `pc` highlighted (the blue current-PC bar). Returns the rows so the
/// window can hit-test clicks (breakpoint toggling / run-to-cursor).
pub fn render_disasm(
    c: &mut Canvas,
    rect: Rect,
    read: impl Fn(u16) -> u8,
    start: u16,
    pc: u16,
    theme: &Theme,
) -> Vec<DisasmRow> {
    let count = (rect.h / line_height()).max(0) as usize + 1;
    let rows = disasm_rows(read, start, count);
    let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
    let highlight = rows.iter().position(|r| r.addr == pc);
    scroll_list(c, rect, &texts, 0, highlight, theme);
    rows
}

#[cfg(test)]
#[path = "debugger_tests.rs"]
mod tests;
