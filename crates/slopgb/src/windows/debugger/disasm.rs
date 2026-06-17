//! Disassembly pane: decode + bgb-format (`disasm_rows`) and rendering
//! (`render_disasm` + the profiler-count overlay), split out of `debugger.rs`
//! to keep each file under the size cap. Re-exported from the parent so the
//! existing `debugger::disasm_rows` / `debugger::DisasmFmt` paths are unchanged.

use std::collections::BTreeSet;

use slopgb_core::debug;

use super::region_label;
use crate::dbg::Breakpoints;
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height, measure};
use crate::ui::widgets::scroll_list;

/// One decoded disassembly line: its address and the formatted bgb text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisasmRow {
    pub addr: u16,
    pub text: String,
}

/// Display options for the disasm pane (Options → Debug). Defaults: RGBDS
/// syntax, uppercase hex, the counted-clocks column shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisasmFmt {
    /// Lowercase hex digits (addresses, byte column, and operand hex). The
    /// mnemonics are already lowercase, so lowercasing the decoded text only
    /// touches its `A-F` hex digits.
    pub lowercase_hex: bool,
    /// Show the trailing `;m-cycles` counted-clocks column.
    pub show_clocks: bool,
    /// Disassemble in RGBDS syntax (`$`-hex, `[mem]`, `ldh`, `db $xx`); when off,
    /// bgb / no$gmb syntax.
    pub rgbds: bool,
}

impl Default for DisasmFmt {
    fn default() -> Self {
        Self {
            lowercase_hex: false,
            show_clocks: true,
            rgbds: true,
        }
    }
}

/// Disassemble `count` instructions from `start`, each formatted as a bgb
/// disasm line `LABEL:ADDR  bytes  mnemonic  ;m-cycles`. `read(addr)` yields the
/// byte at `addr` (use `GameBoy::debug_read`). `fmt` applies the Debug-tab
/// display options. Exact column widths are tuned in the C8 visual diff; the
/// content (addr/bytes/mnemonic/cycles) is final.
pub fn disasm_rows(
    read: impl Fn(u16) -> u8,
    start: u16,
    count: usize,
    data_hints: &BTreeSet<u16>,
    fmt: DisasmFmt,
) -> Vec<DisasmRow> {
    let hx2 = |b: u8| {
        if fmt.lowercase_hex {
            format!("{b:02x}")
        } else {
            format!("{b:02X}")
        }
    };
    let addr_s = |a: u16| {
        if fmt.lowercase_hex {
            format!("{a:04x}")
        } else {
            format!("{a:04X}")
        }
    };
    let clk = |s: &str| {
        if fmt.show_clocks {
            format!(";{s}")
        } else {
            String::new()
        }
    };
    let mut rows = Vec::with_capacity(count);
    let mut addr = start;
    for _ in 0..count {
        // An address marked data renders as a single `db XX` byte (RM9), so the
        // disassembler doesn't mis-decode an embedded data table as code.
        if data_hints.contains(&addr) {
            let b = read(addr);
            let prefix = if fmt.rgbds { "$" } else { "" };
            let text = format!(
                "{}:{} {:<9}{:<20}{}",
                region_label(addr),
                addr_s(addr),
                hx2(b),
                format!("db {prefix}{}", hx2(b)),
                clk("")
            );
            rows.push(DisasmRow { addr, text });
            addr = addr.wrapping_add(1);
            continue;
        }
        let bytes = [
            read(addr),
            read(addr.wrapping_add(1)),
            read(addr.wrapping_add(2)),
        ];
        let syntax = if fmt.rgbds {
            debug::Syntax::Rgbds
        } else {
            debug::Syntax::Bgb
        };
        let insn = debug::decode_with(&bytes, addr, syntax);
        let hex: String = bytes[..insn.len as usize]
            .iter()
            .map(|b| format!("{} ", hx2(*b)))
            .collect();
        let mnem = if fmt.lowercase_hex {
            insn.text.to_ascii_lowercase()
        } else {
            insn.text
        };
        let text = format!(
            "{}:{} {:<9}{:<20}{}",
            region_label(addr),
            addr_s(addr),
            hex.trim_end(),
            mnem,
            clk(&insn.cycles.to_string())
        );
        rows.push(DisasmRow { addr, text });
        addr = addr.wrapping_add(u16::from(insn.len.max(1)));
    }
    rows
}

/// Width of the disasm pane's left gutter — holds the red breakpoint dot, and
/// the current-PC highlight bar extends across it. Module-private: only
/// [`render_disasm`] uses it.
const DISASM_GUTTER: i32 = 7;

/// Render the disasm pane: decode from `start` to fill `rect`, draw the rows
/// past a left gutter with the row at `pc` highlighted (the blue current-PC
/// bar), and a red dot in the gutter on every row carrying a breakpoint.
/// Returns the rows so the window can hit-test clicks.
#[allow(clippy::too_many_arguments)]
pub fn render_disasm(
    c: &mut Canvas,
    rect: Rect,
    read: impl Fn(u16) -> u8,
    start: u16,
    pc: u16,
    bps: &Breakpoints,
    data_hints: &BTreeSet<u16>,
    fmt: DisasmFmt,
    theme: &Theme,
) -> Vec<DisasmRow> {
    let lh = line_height();
    let count = (rect.h / lh).max(0) as usize + 1;
    let rows = disasm_rows(read, start, count, data_hints, fmt);
    let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
    let highlight = rows.iter().position(|r| r.addr == pc);
    let body = Rect::new(
        rect.x + DISASM_GUTTER,
        rect.y,
        (rect.w - DISASM_GUTTER).max(0),
        rect.h,
    );
    let drawn = scroll_list(c, body, &texts, 0, highlight, theme);
    // Extend the PC bar across the gutter and stamp breakpoint dots in it.
    for (i, row) in rows.iter().enumerate().take(drawn) {
        let y = rect.y + i as i32 * lh;
        if Some(i) == highlight {
            c.fill_rect(Rect::new(rect.x, y, DISASM_GUTTER, lh), theme.current);
        }
        if bps.contains(row.addr) {
            let cy = y + lh / 2;
            c.fill_rect(Rect::new(rect.x + 1, cy - 2, 4, 4), theme.breakpoint);
        }
    }
    rows
}

/// Overlay per-row execution counts (MB5 profiler) at the right edge of the
/// disasm pane: each row whose address has a nonzero tally shows `xN`. Only
/// called while the profiler is logging; the row layout matches
/// [`render_disasm`] (same `rect`, same `line_height`), so the counts line up.
pub fn render_profile_counts(
    c: &mut Canvas,
    rect: Rect,
    rows: &[DisasmRow],
    count: impl Fn(u16) -> u64,
    theme: &Theme,
) {
    let lh = line_height();
    let visible = (rect.h / lh).max(0) as usize;
    for (i, row) in rows.iter().enumerate().take(visible) {
        let n = count(row.addr);
        if n == 0 {
            continue;
        }
        let label = format!("x{n}");
        let x = (rect.right() - measure(&label) - 2).max(rect.x);
        // A muted tone, readable on both the white rows and the blue PC-row
        // highlight (the standard `current` ink would vanish on the PC row).
        draw_text(c, x, rect.y + i as i32 * lh, &label, theme.hilight);
    }
}

#[cfg(test)]
#[path = "disasm_tests.rs"]
mod tests;
