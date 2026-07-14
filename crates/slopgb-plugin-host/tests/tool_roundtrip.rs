//! End-to-end proof of the tool-plugin request/response path: a real Rust tool
//! plugin (built with slopgb_tools!, compiled to wasm32) receives an argument
//! string, reads the machine through its view, and returns text the host reads
//! back. Skips if wasm32 is unavailable.

use std::path::PathBuf;
use std::process::Command;

use slopgb_core::{GameBoy, Model};
use slopgb_plugin_host::{LoadedTool, ToolContext, ToolResult};

/// A minimal host context: the echo fixture only reads `pc`, so the formatted /
/// rendered methods are stubs.
struct TestCtx {
    gb: GameBoy,
}

impl ToolContext for TestCtx {
    fn gb(&self) -> &GameBoy {
        &self.gb
    }
    fn set_breakpoint(&mut self, _addr: u16) {}
    fn registers(&self) -> String {
        String::new()
    }
    fn cdl_ranges(&self) -> String {
        String::new()
    }
    fn disassemble(&self, _bank: u16, _from: u16, _to: u16) -> String {
        String::new()
    }
    fn vram_png(&self, _view: &str, _scale: u32) -> Vec<u8> {
        Vec::new()
    }
    fn screencap_png(&self, _scale: u32) -> Vec<u8> {
        Vec::new()
    }
    fn eval_expr(&self, _expr: &str) -> String {
        String::new()
    }
}

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

    let tool = LoadedTool::load(&bytes).unwrap();
    assert_eq!(tool.tools().len(), 1);
    assert_eq!(tool.tools()[0].name, "echo");
    let idx = tool.index_of("echo").expect("echo tool present");

    let mut ctx = TestCtx { gb };
    match tool.call(idx, "hello", &mut ctx).unwrap() {
        ToolResult::Text(s) => assert_eq!(s, format!("hello|pc={pc:04X}")),
        ToolResult::Image(_) => panic!("expected text"),
    }
}
