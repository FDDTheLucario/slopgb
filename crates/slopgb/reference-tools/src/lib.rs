//! Reference tool plugins: the built-in MCP debug tools, reimplemented on the
//! plugin ABI as the dogfood/proof set. Each one produces byte-identical output
//! to the matching `slopgb::mcp::tools` built-in for a fixed machine (the parity
//! test in the frontend pins this).
//!
//! Two styles are shown: `peek` / `cdl` / `breakpoint` do their own argument
//! parsing + row formatting on top of the low-level view primitives
//! (`read_banked` / `cdl_flag` / `set_breakpoint`), the way a third-party tool
//! would; the rest forward to a bulk-result host import (`registers` / `expr` /
//! disassembly with symbols / VRAM + screen PNGs / the CDL range list) whose
//! output is inherently the frontend's to produce.
//!
//! On the *happy path* (well-formed arguments in a single region/bank) the output
//! matches the built-ins; the built-ins' exact region-validation error messages
//! are not reproduced (a reference plugin, not a drop-in error surface).

use slopgb_plugin_api::{Capabilities, GameBoyView, ToolPlugin, ToolResult, args, slopgb_tools};

/// `registers`: the one-line CPU + LCD readout.
struct Registers;
impl ToolPlugin for Registers {
    fn new() -> Self {
        Registers
    }
    fn name(&self) -> &str {
        "registers"
    }
    fn description(&self) -> &str {
        "Read the CPU + LCD register state."
    }
    fn call(&mut self, _args: &str, gb: &GameBoyView) -> ToolResult {
        ToolResult::Text(gb.registers_text())
    }
}

/// `peek`: dump memory bytes, 16 per row.
struct Peek;
impl ToolPlugin for Peek {
    fn new() -> Self {
        Peek
    }
    fn name(&self) -> &str {
        "peek"
    }
    fn description(&self) -> &str {
        "Dump memory bytes, 16 per row."
    }
    fn input_schema(&self) -> &str {
        RANGE_SCHEMA
    }
    fn call(&mut self, a: &str, gb: &GameBoyView) -> ToolResult {
        match range(a) {
            Some((bank, from, to)) => ToolResult::Text(dump_rows(bank, from, to, |addr| {
                format!("{:02X}", gb.read_banked(bank, addr))
            })),
            None => err("peek: want from/to as AAAA or BB:AAAA hex"),
        }
    }
}

/// `cdl`: dump code/data-log access words, 16 per row.
struct Cdl;
impl ToolPlugin for Cdl {
    fn new() -> Self {
        Cdl
    }
    fn name(&self) -> &str {
        "cdl"
    }
    fn description(&self) -> &str {
        "Dump code/data-log access (r/w/x per byte, `.` if none), 16 per row."
    }
    fn input_schema(&self) -> &str {
        RANGE_SCHEMA
    }
    fn call(&mut self, a: &str, gb: &GameBoyView) -> ToolResult {
        match range(a) {
            Some((bank, from, to)) => ToolResult::Text(dump_rows(bank, from, to, |addr| {
                cdl_word(gb.cdl_flag(bank, addr))
            })),
            None => err("cdl: want from/to as AAAA or BB:AAAA hex"),
        }
    }
}

/// `cdl-ranges`: the continuous logged ranges.
struct CdlRanges;
impl ToolPlugin for CdlRanges {
    fn new() -> Self {
        CdlRanges
    }
    fn name(&self) -> &str {
        "cdl-ranges"
    }
    fn description(&self) -> &str {
        "List the continuous address ranges the code/data log has recorded so \
         far (non-`.`), one `AAAA-AAAA` / `BB:AAAA-BB:AAAA` range per line."
    }
    fn call(&mut self, _args: &str, gb: &GameBoyView) -> ToolResult {
        ToolResult::Text(gb.cdl_ranges())
    }
}

/// `disassemble`: a range, with symbol substitution (frontend-formatted).
struct Disassemble;
impl ToolPlugin for Disassemble {
    fn new() -> Self {
        Disassemble
    }
    fn name(&self) -> &str {
        "disassemble"
    }
    fn description(&self) -> &str {
        "Disassemble a range. Rows: `BB:AAAA<tab>label<tab>instruction<tab>cycles`."
    }
    fn input_schema(&self) -> &str {
        RANGE_SCHEMA
    }
    fn call(&mut self, a: &str, gb: &GameBoyView) -> ToolResult {
        match range(a) {
            Some((bank, from, to)) => ToolResult::Text(gb.disassemble(bank, from, to)),
            None => err("disassemble: want from/to as AAAA or BB:AAAA hex"),
        }
    }
}

/// `vram`: capture a VRAM view as a PNG.
struct Vram;
impl ToolPlugin for Vram {
    fn new() -> Self {
        Vram
    }
    fn name(&self) -> &str {
        "vram"
    }
    fn description(&self) -> &str {
        "Capture a VRAM view as a PNG."
    }
    fn input_schema(&self) -> &str {
        r#"{"type":"object","properties":{"view":{"type":"string","description":"one of: bg, win, tile0, tile1, oam, palette"},"scale":{"type":"string","description":"optional PNG magnification: 2x-6x (omit for native size)"}},"required":["view"]}"#
    }
    fn call(&mut self, a: &str, gb: &GameBoyView) -> ToolResult {
        let view = args::field(a, "view").unwrap_or_default();
        ToolResult::Image(gb.vram(&view, scale(a)))
    }
}

/// `screencap`: the current screen as a PNG.
struct Screencap;
impl ToolPlugin for Screencap {
    fn new() -> Self {
        Screencap
    }
    fn name(&self) -> &str {
        "screencap"
    }
    fn description(&self) -> &str {
        "Capture the current Game Boy (Color) screen (160x144) as a PNG."
    }
    fn input_schema(&self) -> &str {
        r#"{"type":"object","properties":{"scale":{"type":"string","description":"optional PNG magnification: 2x-6x (omit for native size)"}},"required":[]}"#
    }
    fn call(&mut self, a: &str, gb: &GameBoyView) -> ToolResult {
        ToolResult::Image(gb.screencap(scale(a)))
    }
}

/// `breakpoint`: set a PC breakpoint (the one mutating tool).
struct Breakpoint;
impl ToolPlugin for Breakpoint {
    fn new() -> Self {
        Breakpoint
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::INTROSPECTION.union(Capabilities::MUTATE)
    }
    fn name(&self) -> &str {
        "breakpoint"
    }
    fn description(&self) -> &str {
        "Set a PC breakpoint."
    }
    fn input_schema(&self) -> &str {
        r#"{"type":"object","properties":{"address":{"type":"string","description":"AAAA or BB:AAAA hex"}},"required":["address"]}"#
    }
    fn call(&mut self, a: &str, gb: &GameBoyView) -> ToolResult {
        match args::field(a, "address").and_then(|s| parse_addr(&s)) {
            Some((bank, addr)) => {
                gb.set_breakpoint(addr);
                ToolResult::Text(format!("breakpoint set at {bank:02X}:{addr:04X}"))
            }
            None => err("breakpoint: want an address AAAA or BB:AAAA hex"),
        }
    }
}

/// `expr`: evaluate a bgb-style debugger expression (frontend evaluator).
struct Expr;
impl ToolPlugin for Expr {
    fn new() -> Self {
        Expr
    }
    fn name(&self) -> &str {
        "expr"
    }
    fn description(&self) -> &str {
        "Evaluate a bgb-style debugger expression (hex default, registers, `[addr]`)."
    }
    fn input_schema(&self) -> &str {
        r#"{"type":"object","properties":{"expression":{"type":"string","description":"e.g. `bc+1`, `[ff80]`, `pc`"}},"required":["expression"]}"#
    }
    fn call(&mut self, a: &str, gb: &GameBoyView) -> ToolResult {
        match args::field(a, "expression") {
            Some(e) => ToolResult::Text(gb.expr(&e)),
            None => err("expr: want an `expression` string"),
        }
    }
}

slopgb_tools!(
    Registers,
    Peek,
    Cdl,
    CdlRanges,
    Disassemble,
    Vram,
    Screencap,
    Breakpoint,
    Expr,
);

/// The shared input schema for the `from`/`to` range tools.
const RANGE_SCHEMA: &str = r#"{"type":"object","properties":{"from":{"type":"string","description":"start address, AAAA or BB:AAAA hex (BB = bank)"},"to":{"type":"string","description":"end address (inclusive), same region/bank as `from`"}},"required":["from","to"]}"#;

fn err(msg: &str) -> ToolResult {
    ToolResult::Text(msg.to_owned())
}

/// Parse a `BB:AAAA` / `AAAA` address into `(bank, addr)` (bank 0 for the bare
/// form), matching the built-in `addr::parse_one` on well-formed input.
fn parse_addr(s: &str) -> Option<(u16, u16)> {
    let s = s.trim();
    match s.split_once(':') {
        Some((b, a)) => Some((
            u16::from_str_radix(b.trim(), 16).ok()?,
            u16::from_str_radix(a.trim(), 16).ok()?,
        )),
        None => Some((0, u16::from_str_radix(s, 16).ok()?)),
    }
}

/// The `from`/`to` pair of a range tool as `(bank, from_addr, to_addr)`.
fn range(a: &str) -> Option<(u16, u16, u16)> {
    let (bank, from) = parse_addr(&args::field(a, "from")?)?;
    let (_, to) = parse_addr(&args::field(a, "to")?)?;
    Some((bank, from, to))
}

/// The optional `scale` argument (`2x`-`6x` or a bare `2`-`6`; absent → 1),
/// matching the built-in `tools::parse_scale` on valid input.
fn scale(a: &str) -> u32 {
    let s = args::field(a, "scale").unwrap_or_default();
    let s = s.trim();
    let digits = s.strip_suffix(['x', 'X']).unwrap_or(s);
    match digits.parse::<u32>() {
        Ok(n) if (1..=6).contains(&n) => n,
        _ => 1,
    }
}

/// A memory/CDL dump: 16 cells per row, `BB:AAAA\t` then the space-joined cells,
/// trailing space trimmed — the built-in `tools::dump_rows` layout.
fn dump_rows(bank: u16, from: u16, to: u16, cell: impl Fn(u16) -> String) -> String {
    let mut out = String::new();
    let mut row = from;
    loop {
        out.push_str(&format!("{bank:02X}:{row:04X}\t"));
        for i in 0..16u16 {
            let a = row.wrapping_add(i);
            if a > to || a < row {
                break;
            }
            out.push_str(&cell(a));
            out.push(' ');
        }
        while out.ends_with(' ') {
            out.pop();
        }
        out.push('\n');
        let last = row.wrapping_add(15);
        if last >= to || last < row {
            break;
        }
        row = row.wrapping_add(16);
    }
    out
}

/// A CDL access word: any of `r`/`w`/`x` present, else `.` (matches the built-in).
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
