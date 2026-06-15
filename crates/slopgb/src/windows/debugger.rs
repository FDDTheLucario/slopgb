//! The bgb debugger window (Layer C): composes the `ui` widgets over
//! `slopgb_core::debug` introspection. This module is the window *content* —
//! pure rendering into a [`Canvas`], unit-tested headless; the winit surface
//! wiring (B12b) feeds it a real buffer later.

use slopgb_core::debug;

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, hex_row, line_height};
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

/// The values the registers panel shows, gathered from the machine
/// (`cpu_regs` + `ime`/`ime_pending`/`double_speed` + `debug_read` of the PPU /
/// interrupt registers). Built by the window layer so the renderer stays pure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegsView {
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub sp: u16,
    pub pc: u16,
    pub ime: bool,
    pub ima: bool,
    pub lcdc: u8,
    pub stat: u8,
    pub ly: u8,
    pub ie: u8,
    pub iflag: u8,
    pub double_speed: bool,
}

/// The two-column register lines bgb shows (`af= …  lcdc=…`, …). `cnt` and the
/// ROM-bank field are omitted pending their deferred accessors.
#[must_use]
pub fn regs_lines(r: &RegsView) -> Vec<String> {
    let flag = |b: bool| if b { '1' } else { '.' };
    vec![
        format!("af= {:04X}   lcdc={:02X}", r.af, r.lcdc),
        format!("bc= {:04X}   stat={:02X}", r.bc, r.stat),
        format!("de= {:04X}   ly= {:02X}", r.de, r.ly),
        format!("hl= {:04X}", r.hl),
        format!("sp= {:04X}   ie= {:02X}", r.sp, r.ie),
        format!("pc= {:04X}   if= {:02X}", r.pc, r.iflag),
        format!("ime={}   spd= {}", flag(r.ime), u8::from(r.double_speed)),
        format!("ima={}", flag(r.ima)),
    ]
}

/// Draw the registers panel into `rect`.
pub fn render_regs(c: &mut Canvas, rect: Rect, r: &RegsView, theme: &Theme) {
    let saved = c.push_clip(rect);
    for (i, line) in regs_lines(r).iter().enumerate() {
        draw_text(
            c,
            rect.x + 1,
            rect.y + i as i32 * line_height(),
            line,
            theme.text,
        );
    }
    c.set_clip(saved);
}

/// Stack-pane lines from [`slopgb_core::GameBoy::stack`] output: `LABEL:ADDR WORD`,
/// descending from SP.
#[must_use]
pub fn stack_lines(stack: &[(u16, u16)]) -> Vec<String> {
    stack
        .iter()
        .map(|&(a, w)| format!("{}:{a:04X} {w:04X}", region_label(a)))
        .collect()
}

/// Draw the stack pane; the top (SP) row gets the highlight bar, as in bgb.
pub fn render_stack(c: &mut Canvas, rect: Rect, stack: &[(u16, u16)], theme: &Theme) {
    let lines = stack_lines(stack);
    let texts: Vec<&str> = lines.iter().map(String::as_str).collect();
    let highlight = (!texts.is_empty()).then_some(0);
    scroll_list(c, rect, &texts, 0, highlight, theme);
}

/// Memory-pane rows: `count` hex-dump lines of 16 bytes each from `start`,
/// via [`hex_row`] over `read` (use `GameBoy::debug_read`).
#[must_use]
pub fn memory_rows(read: impl Fn(u16) -> u8, start: u16, count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            let base = start.wrapping_add((i * 16) as u16);
            let bytes: Vec<u8> = (0..16).map(|j| read(base.wrapping_add(j))).collect();
            hex_row(&format!("{}:{base:04X}", region_label(base)), &bytes)
        })
        .collect()
}

/// Draw the memory hex-dump pane.
pub fn render_memory(
    c: &mut Canvas,
    rect: Rect,
    read: impl Fn(u16) -> u8,
    start: u16,
    theme: &Theme,
) {
    let count = (rect.h / line_height()).max(0) as usize + 1;
    let rows = memory_rows(read, start, count);
    let texts: Vec<&str> = rows.iter().map(String::as_str).collect();
    scroll_list(c, rect, &texts, 0, None, theme);
}

#[cfg(test)]
#[path = "debugger_tests.rs"]
mod tests;
