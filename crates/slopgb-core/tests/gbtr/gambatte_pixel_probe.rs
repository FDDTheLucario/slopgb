//! Session-local PIXEL two-bin (goal.md #11bo — the mode-3 render reclock).
//! Runs the §3b pixel-reference flip-blocker legs (the 100 SameBoy-PASS
//! `pixel-classify-2026-07-03.md` rows: scy/scx_during_m3 · bgtiledata ·
//! bgtilemap · dmgpalette · mealybug m3_* · bgen · window) on the **flag-on**
//! deferred render reclock and compares the rendered 160×144 framebuffer
//! against the sibling reference PNG with the suite's OWN comparator
//! (`harness::expect_frame_png` — the exact check `run_case` uses, so a probe
//! PASS is a real suite PASS). Production (OFF) passes all 100; the flip
//! (ON) breaks them; a mode-3 render slice is landed when its legs pass ON
//! with zero OFF-passing legs dropped.
//!
//! `#[ignore]`'d — never runs in the gate.
//!
//! Usage: `SLOPGB_ROWLIST=/tmp/pixel.txt cargo test -p slopgb-core --test gbtr
//! --release -- --ignored pixel_probe --nocapture`. Rowlist lines:
//! `gambatte/<rel>.gb[c] [Model]` or `mealybug-tearoom-tests/ppu/<rom>.gb
//! [Model]` (trailing columns ignored). `SLOPGB_PROBE_OFF=1` → the flag-off
//! (production) frame, the A/B baseline.

use super::*;

/// Parse a rowlist line's leading `<rel> [Model]` tokens (gambatte or mealybug).
fn pixel_parse_row(line: &str) -> Option<(String, Model)> {
    let mut it = line.split_whitespace();
    let rel = it.next()?.to_string();
    if !(rel.starts_with("gambatte/") || rel.starts_with("mealybug-tearoom-tests/")) {
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

/// Mealybug ppu policy reference (mirrors `mealybug::ppu_ref_name`).
fn pixel_mealybug_ref(stem: &str, model: Model) -> Option<String> {
    match model {
        Model::Dmg => Some(format!("{stem}_dmg_blob.png")),
        Model::Cgb => Some(format!("{stem}_cgb_c.png")),
        _ => None,
    }
}

/// Render one pixel leg flag-on (or off) and compare its frame to the
/// reference PNG. `Ok(())` = the leg renders correctly.
fn pixel_run_leg(rom: &[u8], model: Model, path: &std::path::Path, rel: &str, off: bool) -> Result<(), String> {
    let mut gb = if off {
        harness::boot(rom, model)
    } else {
        harness::boot_with_reclock(rom, model)
    };
    if rel.starts_with("mealybug-tearoom-tests/") {
        // Mealybug protocol: run to LD B,B, then one more frame to the stable
        // post-test screen; Identity colour map (the ROM renders the core's
        // own shades).
        harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)?;
        harness::run_for_frames(&mut gb, 1);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let name = pixel_mealybug_ref(stem, model).ok_or("no mealybug ref series")?;
        let png = path.with_file_name(name);
        harness::expect_frame_png(&gb, &png, CgbColorMap::Identity)
    } else {
        // Gambatte protocol: 15 frames warmup + 1 evaluated frame, Png ref via
        // `plan_rom_on_disk`; DMG Identity, CGB Gambatte colour map.
        let (dmg, cgb) = plan_rom_on_disk(path);
        let check = if model.is_cgb() { cgb } else { dmg };
        let Some(Check::Png(suffix)) = check else {
            return Err(format!("{rel} [{model:?}] is not a Png-reference leg"));
        };
        let target = RUN_DOTS + 2 * u64::from(CYCLES_PER_FRAME);
        while gb.cycles() < target {
            gb.step();
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let png = path
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .join(format!("{stem}{suffix}.png"));
        let map = if model.is_cgb() {
            CgbColorMap::Gambatte
        } else {
            CgbColorMap::Identity
        };
        harness::expect_frame_png(&gb, &png, map)
    }
}

#[test]
#[ignore = "session-local pixel two-bin; needs SLOPGB_ROWLIST"]
fn pixel_probe() {
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
        let Some((rel, model)) = pixel_parse_row(line) else {
            continue;
        };
        let path = root.join(&rel);
        let Ok(rom) = std::fs::read(&path) else {
            eprintln!("MISSING {rel}");
            skip += 1;
            continue;
        };
        match harness::catch_case(|| pixel_run_leg(&rom, model, &path, &rel, off)) {
            Ok(()) => pass += 1,
            Err(e) => {
                fail += 1;
                let first = e.lines().next().unwrap_or("");
                fails.push(format!("FAIL {rel} [{model:?}] {first}"));
            }
        }
    }
    for f in &fails {
        println!("{f}");
    }
    println!(
        "pixel_probe[{}] pass={pass} fail={fail} skip={skip}",
        if off { "OFF" } else { "ON" }
    );
}
