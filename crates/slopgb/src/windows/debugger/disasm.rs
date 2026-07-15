//! Disassembly pane: decode + bgb-format (`disasm_rows`) and rendering
//! (`render_disasm` + the profiler-count overlay), split out of `debugger.rs`
//! to keep each file under the size cap. Re-exported from the parent so the
//! existing `debugger::disasm_rows` / `debugger::DisasmFmt` paths are unchanged.

use std::collections::BTreeSet;

use slopgb_core::debug;

use super::region_label;
use crate::dbg::Breakpoints;
use crate::symbols::SymbolTable;
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height, measure};
use crate::ui::widgets::scroll_list;

/// One disassembly line: its address, the formatted text, the absolute address
/// it references (for symbol substitution), and whether it is a synthetic symbol
/// label line (`name:`) rather than a real instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisasmRow {
    pub addr: u16,
    pub text: String,
    pub target: Option<u16>,
    pub is_label: bool,
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
    /// Lowercase mnemonics + register names (bgb's "lowercase disassembler").
    /// Off uppercases them; the hex digits follow [`Self::lowercase_hex`]
    /// independently.
    pub lowercase_disasm: bool,
}

impl Default for DisasmFmt {
    fn default() -> Self {
        Self {
            lowercase_hex: false,
            show_clocks: true,
            rgbds: true,
            lowercase_disasm: true,
        }
    }
}

/// Case a decoded instruction's text. The core decoder emits mnemonics +
/// register names lowercase and hex digits UPPERCASE, so the character's own
/// case disambiguates them regardless of syntax: an `A-F` letter is a hex digit
/// (cased by `lower_hex`), any other letter is mnemonic/register (uppercased
/// unless `lower_mnemonic`). Digits and punctuation pass through. Runs before
/// symbol substitution, so no symbol-name casing is at risk.
#[must_use]
pub fn case_disasm(text: &str, lower_mnemonic: bool, lower_hex: bool) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                // A hex digit (the decoder's only uppercase output).
                if lower_hex { c.to_ascii_lowercase() } else { c }
            } else if c.is_ascii_lowercase() {
                if lower_mnemonic {
                    c
                } else {
                    c.to_ascii_uppercase()
                }
            } else {
                c
            }
        })
        .collect()
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
            let db_kw = if fmt.lowercase_disasm { "db" } else { "DB" };
            let text = format!(
                "{}:{} {:<9}{:<20}{}",
                region_label(addr),
                addr_s(addr),
                hx2(b),
                format!("{db_kw} {prefix}{}", hx2(b)),
                clk("")
            );
            rows.push(DisasmRow {
                addr,
                text,
                target: None,
                is_label: false,
            });
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
        let mnem = case_disasm(&insn.text, fmt.lowercase_disasm, fmt.lowercase_hex);
        let text = format!(
            "{}:{} {:<9}{:<20}{}",
            region_label(addr),
            addr_s(addr),
            hex.trim_end(),
            mnem,
            clk(&insn.cycles.to_string())
        );
        rows.push(DisasmRow {
            addr,
            text,
            target: insn.target,
            is_label: false,
        });
        addr = addr.wrapping_add(u16::from(insn.len.max(1)));
    }
    rows
}

/// Annotate disassembly `rows` with a loaded symbol table: insert a `name:` label
/// line above each row whose address is an exact symbol, and replace a row's
/// operand target hex (`$0150`/`0150`) with the symbol name when its `target` is a
/// known symbol. `fmt` must match the dialect/case the rows were rendered with so
/// the hex token is found. Empty table → rows unchanged (fast path).
#[must_use]
pub fn annotate_symbols(
    rows: Vec<DisasmRow>,
    syms: &SymbolTable,
    fmt: DisasmFmt,
) -> Vec<DisasmRow> {
    if syms.is_empty() {
        return rows;
    }
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows {
        if !row.is_label {
            if let Some(name) = syms.name_at(row.addr) {
                // Blank spacer above the label for breathing room (bgb parity),
                // skipped when the label is the very top row of the pane.
                if !out.is_empty() {
                    out.push(DisasmRow {
                        addr: row.addr,
                        text: String::new(),
                        target: None,
                        is_label: true,
                    });
                }
                out.push(DisasmRow {
                    addr: row.addr,
                    text: format!("{name}:"),
                    target: None,
                    is_label: true,
                });
            }
            if let Some((t, n)) = row.target.and_then(|t| syms.name_at(t).map(|n| (t, n))) {
                // Replace only the *last* hex occurrence (the operand): the same
                // digits also appear in the row's leading address label.
                row.text = replace_last(&row.text, &target_hex(t, fmt), n);
            }
        }
        out.push(row);
    }
    out
}

/// Replace the **last** occurrence of `from` in `text` with `to` (the operand
/// hex is the last hex on a disasm line; the leading address label has the same
/// digits and must be left alone).
fn replace_last(text: &str, from: &str, to: &str) -> String {
    match text.rfind(from) {
        Some(i) => format!("{}{}{}", &text[..i], to, &text[i + from.len()..]),
        None => text.to_string(),
    }
}

/// The operand hex token for `addr` exactly as [`disasm_rows`] rendered it (case
/// + RGBDS `$` prefix), so [`annotate_symbols`] can find and replace it.
fn target_hex(addr: u16, fmt: DisasmFmt) -> String {
    let body = if fmt.lowercase_hex {
        format!("{addr:04x}")
    } else {
        format!("{addr:04X}")
    };
    if fmt.rgbds { format!("${body}") } else { body }
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
    symbols: &SymbolTable,
    theme: &Theme,
) -> Vec<DisasmRow> {
    let lh = line_height();
    let count = (rect.h / lh).max(0) as usize + 1;
    let rows = annotate_symbols(
        disasm_rows(read, start, count, data_hints, fmt),
        symbols,
        fmt,
    );
    let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
    // Highlight the *instruction* row at PC, not a label line sharing its address.
    let highlight = rows.iter().position(|r| r.addr == pc && !r.is_label);
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
        if !row.is_label && bps.contains(row.addr) {
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
        if row.is_label {
            continue;
        }
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
