//! End-to-end proof of the tool-plugin request/response path: a real Rust tool
//! plugin (built with slopgb_tool_plugin!, compiled to wasm32) receives an
//! argument string, reads the machine through its view, and returns text the
//! host reads back. Skips if wasm32 is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_core::{GameBoy, Model};
use slopgb_plugin_host::{LoadedTool, ToolResult};

fn build_fixture() -> Option<Vec<u8>> {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/echo-tool/Cargo.toml"
    );
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("echo-tool-target");
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            manifest,
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let wasm = target_dir.join("wasm32-unknown-unknown/release/echo_tool.wasm");
    std::fs::read(wasm).ok()
}

#[test]
fn tool_plugin_round_trip() {
    let Some(bytes) = build_fixture() else {
        eprintln!("skipping tool_plugin_round_trip: wasm32 build unavailable");
        return;
    };

    let gb = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    let pc = gb.cpu_regs().pc;

    let mut tool = LoadedTool::load(&bytes).unwrap();
    assert_eq!(tool.name(), "echo");

    match tool.call("hello", &gb).unwrap() {
        ToolResult::Text(s) => assert_eq!(s, format!("hello|pc={pc:04X}")),
        ToolResult::Image(_) => panic!("expected text"),
    }
}
