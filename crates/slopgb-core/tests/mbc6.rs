//! MBC6 exerciser ROM integration tests (breakpoint protocol).
//!
//! The ROM sources (RGBDS and wla-dx twins, assembling to the same test
//! program) and their prebuilt binaries live in `<repo>/roms/mbc6/`. Unlike
//! the downloaded suites they are committed, so these tests never skip. On
//! failure the ROM reports the failing test number in C (see the source's
//! test map).

#[allow(dead_code)]
mod common;

use slopgb_core::Model;
use std::path::Path;

/// Run one prebuilt exerciser ROM on both hardware families (mirrors the
/// mooneye `emulator-only/` matrix: mapper tests are model-agnostic).
fn run(name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../roms/mbc6")
        .join(name);
    let rom = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {} — rebuild it per roms/mbc6/README.md: {e}",
            path.display()
        )
    });
    let mut fails = Vec::new();
    for model in [Model::Dmg, Model::Cgb] {
        if let Err(e) = common::run_breakpoint_rom(&rom, model) {
            fails.push(format!("{name} [{model:?}]: {e}"));
        }
    }
    assert!(
        fails.is_empty(),
        "MBC6 exerciser failed:\n  {}",
        fails.join("\n  ")
    );
}

#[test]
fn mbc6_exerciser_rgbds() {
    run("mbc6test.gb");
}

#[test]
fn mbc6_exerciser_wla() {
    run("mbc6test-wla.gb");
}
