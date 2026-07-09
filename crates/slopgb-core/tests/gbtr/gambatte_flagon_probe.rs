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

/// Which clock the probe boots under — read once from the env, then handed to
/// every worker thread (avoids a per-row `env::var` syscall storm).
#[derive(Clone, Copy)]
enum ProbeMode {
    /// Production (flag-off) frame.
    Off,
    /// Leading-edge-only (stat_update_tick engine, no tier2 render recalibration)
    /// — isolates engine vs render bugs.
    Le,
    /// Eager-value: the eager clock + tier2 read/render laws as cc+0 value peeks,
    /// dispatch staying cc+4 (does NOT set tier2_reclock).
    Ev,
    /// Tier-2 deferred reclock (the default flag-on frame).
    Reclock,
}

impl ProbeMode {
    fn boot(self, rom: &[u8], model: Model) -> GameBoy {
        match self {
            ProbeMode::Off => harness::boot(rom, model),
            ProbeMode::Le => {
                let mut gb = harness::boot(rom, model);
                gb.set_leading_edge_reads(true);
                gb
            }
            ProbeMode::Ev => {
                let mut gb = harness::boot(rom, model);
                gb.set_eager_value(true);
                gb
            }
            ProbeMode::Reclock => harness::boot_with_reclock(rom, model),
        }
    }
}

/// One row's verdict.
enum RowResult {
    Pass,
    Fail(String),
    Skip,
}

/// Run one row (fresh GB instance — no shared state, so this is the parallel
/// unit). `root`/`mode`/`fdelta` are read-only across threads.
fn run_probe_row(root: &std::path::Path, rel: &str, model: Model, mode: ProbeMode, fdelta: i64) -> RowResult {
    let path = root.join(rel);
    let Ok(rom) = std::fs::read(&path) else {
        eprintln!("MISSING {rel}");
        return RowResult::Skip;
    };
    let Some(want) = expected_hex(&path, model) else {
        return RowResult::Skip;
    };
    let mut gb = mode.boot(&rom, model);
    let target = (RUN_DOTS as i64
        + i64::from(CYCLES_PER_FRAME)
        + fdelta * i64::from(CYCLES_PER_FRAME))
    .max(0) as u64;
    while gb.cycles() < target {
        gb.step();
    }
    let got = read_hex_screen(gb.frame(), model.is_cgb());
    // Mirror `check_hex_screen`: only the first `want.len()` tiles matter.
    let got_pref: String = got.chars().take(want.len()).collect();
    if got_pref == want {
        RowResult::Pass
    } else {
        RowResult::Fail(format!(
            "FAIL {rel} [{model:?}] want={want} got={got_pref} (full={got})"
        ))
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
    // Mode + frame-alignment read ONCE (not per row). `SLOPGB_FRAME_DELTA` shifts
    // the OCR capture point by N frames (signed) to test whether a regression is
    // an OCR-capture-frame mis-alignment vs a genuine render/engine bug.
    let mode = if off {
        ProbeMode::Off
    } else if std::env::var("SLOPGB_PROBE_LE").is_ok() {
        ProbeMode::Le
    } else if std::env::var("SLOPGB_PROBE_EV").is_ok() {
        ProbeMode::Ev
    } else {
        ProbeMode::Reclock
    };
    let fdelta: i64 = std::env::var("SLOPGB_FRAME_DELTA")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = std::fs::read_to_string(&list_path).expect("read rowlist");

    // Each ROM is an independent fresh-boot run, so fan the rows across the
    // machine's cores (std threads only — no core dep). A 3422-row full-CGB
    // two-bin drops from ~4 min to seconds. Output is order-stable: fails are
    // sorted after the join.
    let rows: Vec<(String, Model)> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(parse_row)
        .collect();
    let nthreads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(rows.len().max(1));
    let chunk = rows.len().div_ceil(nthreads.max(1));

    let (mut pass, mut fail, mut skip) = (0u32, 0u32, 0u32);
    let mut fails: Vec<String> = Vec::new();
    std::thread::scope(|s| {
        let root = &root;
        let handles: Vec<_> = rows
            .chunks(chunk.max(1))
            .map(|slice| {
                s.spawn(move || {
                    let (mut p, mut f, mut sk) = (0u32, 0u32, 0u32);
                    let mut fl: Vec<String> = Vec::new();
                    for (rel, model) in slice {
                        match run_probe_row(root, rel, *model, mode, fdelta) {
                            RowResult::Pass => p += 1,
                            RowResult::Fail(msg) => {
                                f += 1;
                                fl.push(msg);
                            }
                            RowResult::Skip => sk += 1,
                        }
                    }
                    (p, f, sk, fl)
                })
            })
            .collect();
        for h in handles {
            let (p, f, sk, fl) = h.join().expect("probe worker panicked");
            pass += p;
            fail += f;
            skip += sk;
            fails.extend(fl);
        }
    });
    fails.sort();

    for f in &fails {
        println!("{f}");
    }
    println!(
        "flagon_probe[{}] pass={pass} fail={fail} skip={skip}",
        if off { "OFF" } else { "ON" }
    );
}
