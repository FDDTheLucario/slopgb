//! Session-local measurement aid (port Stage S5 / C-stage): run an explicit
//! list of gambatte rows on the **flag-on** deferred reclock and compare the
//! OCR'd hex screen against the filename expectation — the fast iteration loop
//! for the StatUpdate-engine port (the full suite flip-delta capture is ~4 min;
//! this runs a category in seconds). `#[ignore]`'d, so it never runs in the
//! gate. Reuses the real suite OCR (`read_hex_screen`) + expectation parse
//! (`plan_rom_on_disk`), so its verdict matches `run_case`'s exactly.
//!
//! Usage: `SLOPGB_ROWLIST=/tmp/rows.txt cargo test -p slopgb-core --test gbtr
//! --release -- --ignored flagon_probe --nocapture`. Each `rows.txt` line must
//! begin `gambatte/<rel>.gbc [Model]` (the c2_bug.txt / gbflip_full.txt format;
//! trailing `want=…/sameboy=…` columns are ignored). Set `SLOPGB_PROBE_OFF=1`
//! to A/B against the flag-off (production) frame.

// `super` is the `gambatte` module; the glob pulls in its private helpers
// (`read_hex_screen`, `plan_rom_on_disk`, `Check`, `RUN_DOTS`) plus the
// `harness`/`common`/`Model`/`CYCLES_PER_FRAME` use-imports it already holds.
use super::*;

/// Parse a rowlist line's leading `gambatte/<rel> [Model]` tokens.
fn parse_row(line: &str) -> Option<(String, Model)> {
    let mut it = line.split_whitespace();
    let rel = it.next()?.to_string();
    if !rel.starts_with("gambatte/") {
        return None;
    }
    let model = match it.next()? {
        "[Dmg]" => Model::Dmg,
        "[Cgb]" => Model::Cgb,
        "[Mgb]" => Model::Mgb,
        "[Sgb]" => Model::Sgb,
        "[Agb]" => Model::Agb,
        _ => return None,
    };
    Some((rel, model))
}

/// The filename-tag hex expectation for one (rom, model), or `None` if that
/// side has no `_out<hex>` (audio/PNG/blank rows are skipped by the probe).
fn expected_hex(rom_path: &std::path::Path, model: Model) -> Option<String> {
    let (dmg, cgb) = plan_rom_on_disk(rom_path);
    let side = if model.is_cgb() { cgb } else { dmg };
    match side {
        Some(Check::Hex(h)) => Some(h),
        _ => None,
    }
}

#[test]
#[ignore = "session-local S5 measurement aid; needs SLOPGB_ROWLIST"]
fn flagon_probe() {
    let Ok(list_path) = std::env::var("SLOPGB_ROWLIST") else {
        eprintln!("SLOPGB_ROWLIST unset — nothing to do");
        return;
    };
    let Some(root) = common::gbtr_root() else {
        panic!("game-boy-test-roms collection not present");
    };
    let off = std::env::var("SLOPGB_PROBE_OFF").is_ok();
    let body = std::fs::read_to_string(&list_path).expect("read rowlist");

    let (mut pass, mut fail, mut skip) = (0u32, 0u32, 0u32);
    let mut fails: Vec<String> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((rel, model)) = parse_row(line) else {
            continue;
        };
        let path = root.join(&rel);
        let Ok(rom) = std::fs::read(&path) else {
            eprintln!("MISSING {rel}");
            skip += 1;
            continue;
        };
        let Some(want) = expected_hex(&path, model) else {
            skip += 1;
            continue;
        };
        let mut gb = if off {
            harness::boot(&rom, model)
        } else if std::env::var("SLOPGB_PROBE_LE").is_ok() {
            // Leading-edge-only (stat_update_tick engine, but NOT the tier2
            // render-frame recalibration) — isolates engine vs render bugs.
            let mut gb = harness::boot(&rom, model);
            gb.set_leading_edge_reads(true);
            gb
        } else {
            harness::boot_with_reclock(&rom, model)
        };
        // Frame-alignment probe: SLOPGB_FRAME_DELTA shifts the OCR capture
        // point by N frames (signed) to test whether a regression is an
        // OCR-capture-frame mis-alignment (cheap global fix) vs a genuine
        // render/engine bug.
        let fdelta: i64 = std::env::var("SLOPGB_FRAME_DELTA")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let target =
            (RUN_DOTS as i64 + i64::from(CYCLES_PER_FRAME) + fdelta * i64::from(CYCLES_PER_FRAME))
                .max(0) as u64;
        while gb.cycles() < target {
            gb.step();
        }
        let got = read_hex_screen(gb.frame(), model.is_cgb());
        // Mirror `check_hex_screen`: only the first `want.len()` tiles matter.
        let got_pref: String = got.chars().take(want.len()).collect();
        if got_pref == want {
            pass += 1;
        } else {
            fail += 1;
            fails.push(format!(
                "FAIL {rel} [{model:?}] want={want} got={got_pref} (full={got})"
            ));
        }
    }
    for f in &fails {
        println!("{f}");
    }
    println!(
        "flagon_probe[{}] pass={pass} fail={fail} skip={skip}",
        if off { "OFF" } else { "ON" }
    );
}
