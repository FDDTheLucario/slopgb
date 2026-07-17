//! The built-in MCP tools' logic, run on the UI thread against the live machine.
//! Each is read-only `&self` introspection except `breakpoint`, which toggles
//! the App-owned breakpoint set. Formatting reuses the core disassembler and
//! decoders, the debugger's expression evaluator, and the symbol table; nothing
//! here advances a cycle, so the whole surface is golden-safe. The `pub(crate)`
//! formatters here are also what the reference tool plugins delegate to (via
//! `plugin_host::FrontendToolContext`), so a ported tool matches byte-for-byte.

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
    CdlRanges,
    Vram { view: String, scale: u32 },
    Screencap { scale: u32 },
    Breakpoint { addr: String },
    Registers,
    Coprocessor,
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
        Call::CdlRanges => Ok(ToolResult::Text(cdl_ranges(gb))),
        Call::Vram { view, scale } => {
            let bmp = vram::capture(gb, view)?;
            Ok(ToolResult::Image(encode_scaled(
                &bmp.px, bmp.w, bmp.h, *scale,
            )))
        }
        Call::Screencap { scale } => Ok(ToolResult::Image(encode_scaled(
            gb.frame(),
            slopgb_core::SCREEN_W,
            slopgb_core::SCREEN_H,
            *scale,
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
        Call::Coprocessor => Ok(ToolResult::Text(coprocessor_status(gb))),
        Call::Expr { expr } => Ok(ToolResult::Text(expr_eval(gb, expr))),
    }
}

/// Parse the optional magnification argument the two image tools accept. Absent
/// or blank → `1` (native size). Otherwise `2`–`6`, written bare (`4`) or with
/// the advertised `x` suffix (`4x`); anything else is an error the agent sees.
pub fn parse_scale(s: Option<&str>) -> Result<u32, String> {
    let s = s.map(str::trim).unwrap_or("");
    if s.is_empty() {
        return Ok(1);
    }
    let digits = s.strip_suffix(|c: char| c == 'x' || c == 'X').unwrap_or(s);
    match digits.parse::<u32>() {
        Ok(n @ 1..=6) => Ok(n),
        _ => Err(format!(
            "scale must be one of 2x, 3x, 4x, 5x, 6x (got {s:?})"
        )),
    }
}

/// Encode `px` (XRGB8888, `w×h`) as a PNG, nearest-neighbor magnified by `scale`
/// (`1` = native). Pixel-art screens are tiny, so integer upscaling is enough for
/// a model that struggles to read them at native size.
pub(crate) fn encode_scaled(px: &[u32], w: usize, h: usize, scale: u32) -> Vec<u8> {
    if scale <= 1 {
        return png::encode(px, w, h);
    }
    let f = scale as usize;
    let (nw, nh) = (w * f, h * f);
    let mut out = vec![0u32; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            out[y * nw + x] = px.get((y / f) * w + (x / f)).copied().unwrap_or(0);
        }
    }
    png::encode(&out, nw, nh)
}

/// Disassemble `[from, to]`, one instruction per line:
/// `BB:AAAA\tLABEL\tinstruction\tcycles` (empty label → two tabs). Reads through
/// the requested bank, and substitutes a known symbol name for a branch/call
/// operand (bgb's inline label).
pub(crate) fn disassemble(gb: &GameBoy, symbols: &SymbolTable, from: Addr, to: Addr) -> String {
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
        let _ = writeln!(
            out,
            "{:02X}:{a:04X}\t{label}\t{text}\t{}",
            from.bank, insn.cycles
        );
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

/// List every continuous span the CDL has logged, one range per line in the
/// tools' address form: `BB:AAAA-BB:AAAA` for the banked regions (bank on both
/// ends), bare `AAAA-AAAA` elsewhere. Empty when the log is off / nothing logged.
pub(crate) fn cdl_ranges(gb: &GameBoy) -> String {
    let mut out = String::new();
    for r in gb.cdl_logged_ranges() {
        if addr::Region::of(r.start).banked() {
            let _ = writeln!(
                out,
                "{:02x}:{:04x}-{:02x}:{:04x}",
                r.bank, r.start, r.bank, r.end
            );
        } else {
            let _ = writeln!(out, "{:04x}-{:04x}", r.start, r.end);
        }
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
pub(crate) fn registers(gb: &GameBoy) -> String {
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

/// The SGB audio coprocessor status (the `coprocessor` tool): whether this
/// machine has an SGB coprocessor and, if so, whether the built-in HLE APU or
/// the wasm SPC700 + 65C816 plugins are engaged and actually running. A machine
/// that is not in SGB mode says so — the SPC700/65C816 only run on SGB.
pub(crate) fn coprocessor_status(gb: &GameBoy) -> String {
    gb.sgb_coprocessor_status().unwrap_or_else(|| {
        "no SGB coprocessor: this machine is NOT in Super Game Boy mode, so the SPC700 / \
         65C816 never run. Set System -> Super Gameboy (or --model sgb); the coprocessor \
         chips exist only on Model::Sgb / Sgb2."
            .to_string()
    })
}

/// Evaluate a bgb-style debugger expression against the live regs + memory.
pub(crate) fn expr_eval(gb: &GameBoy, s: &str) -> String {
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
