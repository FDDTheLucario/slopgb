//! Parity: each reference tool plugin's output equals the built-in
//! `mcp::tools` output for a fixed machine, and the MCP server lists + dispatches
//! a loaded plugin tool identically to a built-in. Both build the reference-tools
//! wasm crate on the fly and skip if wasm32 is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_core::{GameBoy, Model};
use slopgb_plugin_host::{LoadedTool, ToolResult as PluginToolResult};

use super::*;
use crate::dbg::Breakpoints;
use crate::mcp::Mcp;
use crate::mcp::tools::{self, Call, ToolResult};
use crate::symbols::SymbolTable;

/// Build the reference-tools crate to wasm; `None` if wasm32 is unavailable.
fn build_reference() -> Option<Vec<u8>> {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/reference-tools/Cargo.toml");
    let target = std::env::temp_dir().join("slopgb-reference-tools-target");
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            manifest,
        ])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let wasm = target.join("wasm32-unknown-unknown/release/slopgb_reference_tools.wasm");
    std::fs::read(wasm).ok()
}

/// A DMG machine with a couple of instructions at `0x0100` and a CDL log with a
/// few flags set, so disassembly / peek / cdl / cdl-ranges are all non-trivial.
fn machine() -> GameBoy {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100] = 0x18; // jr $0150
    rom[0x0101] = 0x4E;
    rom[0x0102] = 0xAF; // xor a
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    gb.set_cdl(true);
    let mut fx = vec![0u8; gb.cdl_flags().unwrap().len()];
    fx[0x0100] = 4; // executed
    fx[0x0101] = 1 | 2; // read + write
    assert!(gb.load_cdl(&fx));
    gb
}

fn symbols() -> SymbolTable {
    SymbolTable::parse("00:0100 Entry\n00:0150 Start\n")
}

fn native_bytes(r: ToolResult) -> (bool, Vec<u8>) {
    match r {
        ToolResult::Text(s) => (false, s.into_bytes()),
        ToolResult::Image(b) => (true, b),
    }
}

fn plugin_bytes(r: PluginToolResult) -> (bool, Vec<u8>) {
    match r {
        PluginToolResult::Text(s) => (false, s.into_bytes()),
        PluginToolResult::Image(b) => (true, b),
    }
}

#[test]
fn reference_plugins_match_builtins_byte_for_byte() {
    let Some(bytes) = build_reference() else {
        eprintln!("skipping parity test: wasm32 build unavailable");
        return;
    };
    let tool = LoadedTool::load(&bytes).expect("reference tools load");
    assert_eq!(tool.tools().len(), 9, "all nine tools exposed");

    // (plugin tool name, MCP arguments JSON, equivalent built-in Call).
    let cases: Vec<(&str, String, Call)> = vec![
        ("registers", "{}".into(), Call::Registers),
        (
            "peek",
            r#"{"from":"0100","to":"0110"}"#.into(),
            Call::Peek {
                from: "0100".into(),
                to: "0110".into(),
            },
        ),
        (
            "cdl",
            r#"{"from":"0100","to":"0110"}"#.into(),
            Call::Cdl {
                from: "0100".into(),
                to: "0110".into(),
            },
        ),
        ("cdl-ranges", "{}".into(), Call::CdlRanges),
        (
            "disassemble",
            r#"{"from":"0100","to":"0102"}"#.into(),
            Call::Disassemble {
                from: "0100".into(),
                to: "0102".into(),
            },
        ),
        (
            "vram",
            r#"{"view":"bg"}"#.into(),
            Call::Vram {
                view: "bg".into(),
                scale: 1,
            },
        ),
        (
            "vram",
            r#"{"view":"palette","scale":"3x"}"#.into(),
            Call::Vram {
                view: "palette".into(),
                scale: 3,
            },
        ),
        ("screencap", "{}".into(), Call::Screencap { scale: 1 }),
        (
            "screencap",
            r#"{"scale":"2"}"#.into(),
            Call::Screencap { scale: 2 },
        ),
        (
            "breakpoint",
            r#"{"address":"0150"}"#.into(),
            Call::Breakpoint {
                addr: "0150".into(),
            },
        ),
        (
            "expr",
            r#"{"expression":"bc+1"}"#.into(),
            Call::Expr {
                expr: "bc+1".into(),
            },
        ),
    ];

    for (name, args, call) in cases {
        let gb = machine();
        let syms = symbols();

        let mut native_bps = Breakpoints::default();
        let want = native_bytes(tools::dispatch(&call, &gb, &mut native_bps, &syms).unwrap());

        let idx = tool
            .index_of(name)
            .unwrap_or_else(|| panic!("plugin has {name}"));
        let mut plugin_bps = Breakpoints::default();
        let mut ctx = FrontendToolContext {
            gb: &gb,
            breakpoints: &mut plugin_bps,
            symbols: &syms,
        };
        let got = plugin_bytes(tool.call(idx, &args, &mut ctx).unwrap());

        assert_eq!(got.0, want.0, "{name}: text/image kind matches ({args})");
        assert_eq!(
            got.1, want.1,
            "{name}: byte-identical output for args {args}"
        );
        // The mutating tool's effect matches too.
        if name == "breakpoint" {
            assert!(plugin_bps.dot_at(0x0150, 0) && native_bps.dot_at(0x0150, 0));
        }
    }
}

/// Load the reference wasm into a `ToolPlugins` (via a temp dir, the real path).
fn load_plugins_from(bytes: &[u8]) -> (ToolPlugins, PathBuf) {
    let dir = std::env::temp_dir().join(format!("slopgb-ref-plugins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("reference.wasm"), bytes).unwrap();
    let plugins = ToolPlugins::load(Some(dir.as_path()));
    (plugins, dir)
}

#[test]
fn server_lists_and_dispatches_a_plugin_tool() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let Some(bytes) = build_reference() else {
        eprintln!("skipping server plugin test: wasm32 build unavailable");
        return;
    };
    let (plugins, dir) = load_plugins_from(&bytes);
    assert!(
        plugins.metadata().len() >= 9,
        "reference tools loaded from dir"
    );

    let mut mcp = Mcp::with_tool_plugins(plugins);
    mcp.start(0).unwrap();
    let port = mcp.port().unwrap();

    let gb = machine();
    let mut dbg = crate::dbg::Debugger::default();
    let syms = symbols();

    // Client thread issues tools/list then tools/call peek; this thread pumps.
    let client = std::thread::spawn(move || {
        let list = http_post(port, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        let peek = http_post(
            port,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"peek","arguments":{"from":"0100","to":"0110"}}}"#,
        );
        (list, peek)
    });

    let start = Instant::now();
    while !client.is_finished() && start.elapsed() < Duration::from_secs(10) {
        mcp.pump(&gb, &mut dbg, &syms);
        std::thread::sleep(Duration::from_millis(2));
    }
    let (list, peek) = client.join().unwrap();

    // tools/list advertises the plugin tools (peek among them, once).
    assert!(list.contains("\"peek\""), "peek listed: {list}");
    assert_eq!(list.matches("\"name\":\"peek\"").count(), 1, "peek once");

    // tools/call peek returns exactly what the built-in peek produces.
    let mut bps = Breakpoints::default();
    let want = match tools::dispatch(
        &Call::Peek {
            from: "0100".into(),
            to: "0110".into(),
        },
        &gb,
        &mut bps,
        &syms,
    )
    .unwrap()
    {
        ToolResult::Text(s) => s,
        ToolResult::Image(_) => unreachable!(),
    };
    assert!(peek.contains("\"isError\":false"), "peek ok: {peek}");
    // The dump text is JSON-embedded (tabs escaped); check a row fragment appears.
    let want_row = want.lines().next().unwrap().replace('\t', "\\t");
    assert!(
        peek.contains(&want_row),
        "peek text matches builtin: {peek}"
    );

    mcp.stop();
    let _ = std::fs::remove_dir_all(dir);

    // Local HTTP POST helper (mirrors the mcp_tests one).
    fn http_post(port: u16, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        s.write_all(req.as_bytes()).unwrap();
        s.flush().unwrap();
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            assert_ne!(s.read(&mut byte).unwrap(), 0, "server closed early");
            buf.push(byte[0]);
            if buf.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let headers = String::from_utf8_lossy(&buf).to_lowercase();
        let len: usize = headers
            .split("content-length:")
            .nth(1)
            .and_then(|s| s.split("\r\n").next())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let mut body_buf = vec![0u8; len];
        s.read_exact(&mut body_buf).unwrap();
        String::from_utf8_lossy(&body_buf).into_owned()
    }
}
