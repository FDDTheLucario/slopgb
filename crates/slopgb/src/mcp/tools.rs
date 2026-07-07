//! The seven MCP tools' logic, run on the UI thread against the live machine.
//! Each is read-only `&self` introspection except `breakpoint`, which toggles
//! the App-owned breakpoint set. Formatting reuses the core disassembler and
//! decoders, the debugger's expression evaluator, and the symbol table; nothing
//! here advances a cycle, so the whole surface is golden-safe.

use std::fmt::Write as _;

use slopgb_core::{GameBoy, debug};

use crate::dbg::Breakpoints;
use crate::mcp::addr::{self, Addr};
use crate::mcp::{png, vram};
use crate::symbols::SymbolTable;

/// The result of a tool call: either text, or a PNG image (the `vram` capture).
pub enum ToolResult {
    Text(String),
    /// PNG bytes, surfaced as MCP `image` content (base64, `image/png`).
    Image(Vec<u8>),
}

/// A parsed tool invocation, produced by the transport and executed by
/// [`dispatch`] on the UI thread.
pub enum Call {
    Disassemble { from: String, to: String },
    Peek { from: String, to: String },
    Cdl { from: String, to: String },
    Vram { view: String },
    Screencap,
    Breakpoint { addr: String },
    Registers,
    Expr { expr: String },
}

/// Execute a tool call against the live machine. `breakpoints` is the only
/// mutable state any tool touches (the `breakpoint` tool); everything else is
/// read-only introspection.
pub fn dispatch(
    call: &Call,
    gb: &GameBoy,
    breakpoints: &mut Breakpoints,
    symbols: &SymbolTable,
) -> Result<ToolResult, String> {
    match call {
        Call::Disassemble { from, to } => {
            let (a, b) = addr::parse_range(from, to)?;
            Ok(ToolResult::Text(disassemble(gb, symbols, a, b)))
        }
        Call::Peek { from, to } => {
            let (a, b) = addr::parse_range(from, to)?;
            Ok(ToolResult::Text(dump_rows(a, b, |bank, addr| {
                format!("{:02X}", gb.debug_read_banked(bank, addr))
            })))
        }
        Call::Cdl { from, to } => {
            let (a, b) = addr::parse_range(from, to)?;
            Ok(ToolResult::Text(dump_rows(a, b, |bank, addr| {
                cdl_word(gb.cdl_flag_banked(bank, addr))
            })))
        }
        Call::Vram { view } => {
            let bmp = vram::capture(gb, view)?;
            Ok(ToolResult::Image(png::encode(&bmp.px, bmp.w, bmp.h)))
        }
        Call::Screencap => Ok(ToolResult::Image(png::encode(
            gb.frame(),
            slopgb_core::SCREEN_W,
            slopgb_core::SCREEN_H,
        ))),
        Call::Breakpoint { addr } => {
            let a = addr::parse_one(addr)?;
            breakpoints.set(a.addr);
            Ok(ToolResult::Text(format!(
                "breakpoint set at {:02X}:{:04X}",
                a.bank, a.addr
            )))
        }
        Call::Registers => Ok(ToolResult::Text(registers(gb))),
        Call::Expr { expr } => Ok(ToolResult::Text(expr_eval(gb, expr))),
    }
}

/// Disassemble `[from, to]`, one instruction per line:
/// `BB:AAAA\tLABEL\tinstruction\tcycles` (empty label → two tabs). Reads through
/// the requested bank, and substitutes a known symbol name for a branch/call
/// operand (bgb's inline label).
fn disassemble(gb: &GameBoy, symbols: &SymbolTable, from: Addr, to: Addr) -> String {
    let read = |a: u16| gb.debug_read_banked(from.bank, a);
    let mut out = String::new();
    let mut a = from.addr;
    loop {
        let bytes = [read(a), read(a.wrapping_add(1)), read(a.wrapping_add(2))];
        let insn = debug::decode_with(&bytes, a, debug::Syntax::Rgbds);
        let label = symbols.name_at(a).unwrap_or("");
        let mut text = insn.text;
        if let Some((t, name)) = insn.target.and_then(|t| symbols.name_at(t).map(|n| (t, n))) {
            text = replace_last(&text, &format!("${t:04X}"), name);
        }
        let _ = writeln!(out, "{:02X}:{a:04X}\t{label}\t{text}\t{}", from.bank, insn.cycles);
        let next = a.wrapping_add(u16::from(insn.len.max(1)));
        if a >= to.addr || next <= a {
            break; // reached the end, or the 16-bit address wrapped
        }
        a = next;
    }
    out
}

/// A memory/CDL dump: 16 cells per row, `BB:AAAA\t` then the space-joined cells.
/// Shared by `peek` (hex bytes) and `cdl` (access words).
fn dump_rows(from: Addr, to: Addr, cell: impl Fn(u16, u16) -> String) -> String {
    let mut out = String::new();
    let mut row = from.addr;
    loop {
        let _ = write!(out, "{:02X}:{row:04X}\t", from.bank);
        for i in 0..16u16 {
            let a = row.wrapping_add(i);
            if a > to.addr || a < row {
                break; // past the range, or wrapped
            }
            let _ = write!(out, "{} ", cell(from.bank, a));
        }
        while out.ends_with(' ') {
            out.pop();
        }
        out.push('\n');
        let last = row.wrapping_add(15);
        if last >= to.addr || last < row {
            break;
        }
        row = row.wrapping_add(16);
    }
    out
}

/// A CDL access word: any of `r`/`w`/`x` present, else `.`.
fn cdl_word(flag: u8) -> String {
    let mut s = String::new();
    if flag & 1 != 0 {
        s.push('r');
    }
    if flag & 2 != 0 {
        s.push('w');
    }
    if flag & 4 != 0 {
        s.push('x');
    }
    if s.is_empty() {
        s.push('.');
    }
    s
}

/// The one-line register readout the debugger window shows.
fn registers(gb: &GameBoy) -> String {
    let r = gb.cpu_regs();
    let rd = |a| gb.debug_read(a);
    let ram = gb
        .ram_bank()
        .map_or_else(|| "--".to_owned(), |b| format!("{b:02X}"));
    let wave: String = gb.wave_ram().iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "af={:04X} bc={:04X} de={:04X} hl={:04X} sp={:04X} pc={:04X} \
         lcdc={:02X} stat={:02X} ly={:02X} cnt={} ie={:02X} if={:02X} \
         ime={} ima={} spd={} rom={:03X} ram={ram} wave={wave}",
        r.af(),
        r.bc(),
        r.de(),
        r.hl(),
        r.sp,
        r.pc,
        rd(0xFF40),
        rd(0xFF41),
        rd(0xFF44),
        gb.cycles() as u32,
        rd(0xFFFF),
        rd(0xFF0F),
        u8::from(gb.ime()),
        if gb.ime_pending() { '1' } else { '.' },
        u8::from(gb.double_speed()),
        gb.rom_bank(),
    )
}

/// Evaluate a bgb-style debugger expression against the live regs + memory.
fn expr_eval(gb: &GameBoy, s: &str) -> String {
    let regs = gb.cpu_regs();
    match crate::windows::debugger::eval_expr(s, &regs, |a| gb.debug_read(a)) {
        Ok(v) => format!("{v:#06X} ({v})"),
        Err(e) => format!("error: {e}"),
    }
}

/// Replace the **last** occurrence of `from` with `to` (the operand hex is the
/// last hex token on a disasm line; a leading address would share the digits).
fn replace_last(text: &str, from: &str, to: &str) -> String {
    match text.rfind(from) {
        Some(i) => format!("{}{}{}", &text[..i], to, &text[i + from.len()..]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
