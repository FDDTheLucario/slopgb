//! mooneye-test-suite-wilbertpol harness — the 2016-era mooneye fork
//! (`mooneye-test-suite-wilbertpol/` in the c-sp collection).
//!
//! # Protocol (suite `game-boy-test-roms-howto.md`)
//!
//! Tests signal completion by executing the **undefined opcode 0xED** ("this
//! is how the Mooneye Test Suite worked back in 2016"); the core hard-locks
//! and raises [`slopgb_core::GameBoy::debug_undefined_hit`]. A test passed
//! iff B,C,D,E,H,L then hold Fibonacci 3,5,8,13,21,34 (howto "Test
//! Success/Failure"). 120 emulated seconds without the exit opcode means the
//! test hung; the timeout below adds ~30% margin on top.
//!
//! # Model routing (suite README.markdown "Test naming" — 2016 convention)
//!
//! This fork predates the modern suffix scheme in
//! `tests/common/mod.rs::models_for` (no `dmg0`/`dmgABC`/`cgbABCDE`); its
//! single-model suffixes are plain device names and its group letters are
//! `G` = dmg+mgb, `S` = sgb+sgb2, `C` = cgb+agb+ags, `A` = agb+ags. AGS is
//! not modeled (docs/ARCHITECTURE.md), so groups containing AGS drop that
//! member (`C` → Cgb+Agb, `A` → Agb) and a plain `ags` suffix would map to
//! no machine at all. Unsuffixed ROMs are expected to pass on all hardware;
//! they run on the modern harness's default set minus `Dmg0` — this fork's
//! expectations were verified on DMG CPU A/B devices only (README device
//! table) and it ships no `dmg0` variants, exactly the reasoning of
//! `models_for`'s default arm.

use std::path::Path;

use slopgb_core::{CLOCK_HZ, Model, SCREEN_H, SCREEN_W};

use crate::common;
use crate::common::framecmp::CgbColorMap;
use crate::harness::{self, CaseResult};

/// Collection-relative suite directory.
const SUITE: &str = "mooneye-test-suite-wilbertpol";

/// Howto timeout (120 emulated seconds) plus ~30% margin;
/// protocol-terminated ROMs finish far earlier.
const TIMEOUT_TCYCLES: u64 = 156 * CLOCK_HZ as u64;

/// Models an unsuffixed ROM runs on (see module docs: the modern default
/// set minus `Dmg0`).
const DEFAULT_MODELS: [Model; 6] = [
    Model::Dmg,
    Model::Mgb,
    Model::Sgb,
    Model::Sgb2,
    Model::Cgb,
    Model::Agb,
];

/// Known-failure baseline (`baselines/wilbertpol.txt`), one case key per
/// line, `#` comments allowed (`harness::parse_baseline`); shrinking it is
/// progress, growing it a regression (`harness::assert_against_baseline`).
const BASELINE_TXT: &str = include_str!("baselines/wilbertpol.txt");

/// How one suite ROM is verified — or why it never runs.
#[derive(Debug, PartialEq, Eq)]
enum Disposition {
    /// Undefined-opcode exit + Fibonacci check, once per model.
    Protocol(Vec<Model>),
    /// `manual-only/`: `sprite_priority.gb` never signals completion (howto
    /// "Screenshot based tests") — render 15 frame periods and compare
    /// against the suite's own common-palette references, mirroring
    /// `tests/common/mod.rs::run_sprite_priority`.
    SpritePriority,
    /// `madness/`: `mgb_oam_dma_halt_sprites.gb` halts forever with no
    /// interrupt enabled and never reaches 0xED — frame-compare the
    /// HALT-frozen OAM scan against the bundled reference on MGB (the only
    /// model whose behaviour the test's asm documents), mirroring
    /// `tests/common/mod.rs::run_madness`.
    Madness,
    /// Documented never-run, with the citable reason.
    Exempt(&'static str),
}

/// Classify one suite-relative forward-slash ROM path.
fn classify(rel: &str) -> Disposition {
    match rel.split('/').next().unwrap_or("") {
        // utils/dump_boot_hwio.gb dumps the boot-time hardware-register
        // state for manual transcription; neither the suite README nor the
        // howto define a machine-checkable pass criterion.
        "utils" => return Disposition::Exempt("dump tool, no pass/fail protocol"),
        // logic-analysis/ ROMs generate bus/PPU signal patterns meant to be
        // captured with a logic analyzer on real hardware's external pins;
        // nothing software-observable signals pass or fail.
        "logic-analysis" => {
            return Disposition::Exempt(
                "hardware logic-analyzer ROM, no software-observable pass criterion",
            );
        }
        "manual-only" => return Disposition::SpritePriority,
        "madness" => return Disposition::Madness,
        // Mirrors tests/common/mod.rs::models_for's emulator-only arm:
        // mapper tests probe the cartridge only, so one plain and one CGB
        // machine give double-speed-free coverage.
        "emulator-only" => return Disposition::Protocol(vec![Model::Dmg, Model::Cgb]),
        _ => {}
    }
    let stem = Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let models = stem
        .rsplit_once('-')
        .and_then(|(_, sfx)| suffix_models(sfx));
    match models {
        // Only a plain `ags` suffix maps to no machine (see module docs).
        Some(models) if models.is_empty() => {
            Disposition::Exempt("AGS-only ROM; AGS hardware is not modeled")
        }
        Some(models) => Disposition::Protocol(models),
        None => Disposition::Protocol(DEFAULT_MODELS.to_vec()),
    }
}

/// Map one 2016-convention filename suffix (the part after the last `-`)
/// to the models the ROM is documented to pass on (suite README "Test
/// naming"), or `None` when the suffix is not a model hint.
fn suffix_models(sfx: &str) -> Option<Vec<Model>> {
    let models = match sfx {
        "dmg" => vec![Model::Dmg],
        "mgb" => vec![Model::Mgb],
        "sgb" => vec![Model::Sgb],
        "sgb2" => vec![Model::Sgb2],
        "cgb" => vec![Model::Cgb],
        "agb" => vec![Model::Agb],
        // AGS is not modeled; an `ags`-only ROM runs nowhere.
        "ags" => vec![],
        _ => return group_letter_models(sfx),
    };
    Some(models)
}

/// Combined 2016 group letters, e.g. `GS` = dmg+mgb+sgb+sgb2. Groups whose
/// AGS member is dropped (`C`, `A`) overlap on AGB, so accumulation
/// deduplicates.
fn group_letter_models(sfx: &str) -> Option<Vec<Model>> {
    if sfx.is_empty() || !sfx.chars().all(|c| matches!(c, 'G' | 'S' | 'C' | 'A')) {
        return None;
    }
    let mut models = Vec::new();
    for c in sfx.chars() {
        let group: &[Model] = match c {
            'G' => &[Model::Dmg, Model::Mgb],
            'S' => &[Model::Sgb, Model::Sgb2],
            'C' => &[Model::Cgb, Model::Agb],
            'A' => &[Model::Agb],
            _ => unreachable!(),
        };
        for &m in group {
            if !models.contains(&m) {
                models.push(m);
            }
        }
    }
    Some(models)
}

/// Map one pixel of the suite's greyscale `madness/` reference image to a
/// DMG shade class. The bundled `mgb_oam_dma_halt_sprites_expected.png` is
/// 8-bit greyscale using exactly the three levels 255/176/104 — *not* the
/// c-sp common palette (FF/AA/55/00) — which map, in descending brightness,
/// to shades 0/1/2: the ROM's BGP $54 draws its checkerboard with shades 0
/// and 1 and OBP1 $AA maps the glitch sprite to shade 2 (see
/// test-roms-src/madness/mgb_oam_dma_halt_sprites.s and the provenance note
/// on `common::MGB_OAM_DMA_HALT_SPRITES_SHADES`).
fn madness_shade(rgb: [u8; 3]) -> Result<u8, String> {
    if rgb[0] != rgb[1] || rgb[1] != rgb[2] {
        return Err(format!("non-grey reference pixel {rgb:?}"));
    }
    match rgb[0] {
        255 => Ok(0),
        176 => Ok(1),
        104 => Ok(2),
        v => Err(format!("unexpected grey level {v} (want 255/176/104)")),
    }
}

/// One undefined-opcode protocol case: run to the 0xED exit (howto "Exit
/// Condition"), then check the Fibonacci signature.
/// Run an explicit list of wilbertpol ROMs on the **flag-on** reclock and
/// report the fib pass/fail — the classifier for the DMG line-153/timer engine
/// rows. `#[ignore]`'d. Rowlist lines: `<suite>/<rel>.gb [Model]`;
/// `SLOPGB_PROBE_OFF=1` A/Bs production.
#[test]
#[ignore = "session-local Phase-2 measurement aid; needs SLOPGB_ROWLIST"]
fn wilbertpol_flagon_probe() {
    let Ok(list_path) = std::env::var("SLOPGB_ROWLIST") else {
        eprintln!("SLOPGB_ROWLIST unset");
        return;
    };
    let Some(root) = common::gbtr_root() else {
        panic!("gbtr collection not present");
    };
    let off = std::env::var("SLOPGB_PROBE_OFF").is_ok();
    let body = std::fs::read_to_string(&list_path).expect("read rowlist");
    let (mut pass, mut fail) = (0u32, 0u32);
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let rel = it.next().unwrap_or("");
        let model = match it.next() {
            Some("[Dmg]") => Model::Dmg,
            Some("[Cgb]") => Model::Cgb,
            Some("[Mgb]") => Model::Mgb,
            Some("[Sgb]") | Some("[Sgb2]") => Model::Sgb,
            Some("[Agb]") => Model::Agb,
            _ => Model::Dmg,
        };
        let Ok(rom) = std::fs::read(root.join(rel)) else {
            continue;
        };
        let mut gb = if off {
            harness::boot(&rom, model)
        } else {
            harness::boot_with_reclock(&rom, model)
        };
        let r = harness::run_until_undefined(&mut gb, TIMEOUT_TCYCLES)
            .and_then(|()| harness::check_fib(&gb));
        match r {
            Ok(()) => pass += 1,
            Err(e) => {
                fail += 1;
                println!("FAIL {rel} [{model:?}] {e}");
            }
        }
    }
    println!(
        "wilbertpol_flagon_probe[{}] pass={pass} fail={fail}",
        if off { "OFF" } else { "ON" }
    );
}

fn run_protocol_case(rom: &[u8], model: Model) -> Result<(), String> {
    let mut gb = harness::boot(rom, model);
    harness::run_until_undefined(&mut gb, TIMEOUT_TCYCLES)?;
    harness::check_fib(&gb)
}

/// One `manual-only/sprite_priority.gb` case: render 15 frame periods
/// (mirrors `tests/common/mod.rs::run_sprite_priority`) and compare against
/// one of the suite's two reference PNGs. The ROM keeps the LCD off for ~9
/// periods while drawing and hardware presents the first frame after the
/// re-enable blank (Pan Docs "LCDC.7"), so the image is only stable from
/// period 10 — 15 leaves margin. The references are the c-sp replacements
/// for upstream's incompatible greyscale image (howto "Screenshot based
/// tests") and use the collection's common palette — exactly the core's
/// output, hence [`CgbColorMap::Identity`].
fn run_sprite_priority_case(rom: &[u8], model: Model, png_path: &Path) -> Result<(), String> {
    let mut gb = harness::boot(rom, model);
    harness::run_for_frames(&mut gb, 15);
    harness::expect_frame_png(&gb, png_path, CgbColorMap::Identity)
}

/// The `madness/mgb_oam_dma_halt_sprites.gb` case: render 10 frames on MGB
/// and compare against the bundled `*_expected.png`, decoded through
/// [`madness_shade`] into shade classes for
/// [`common::compare_frame_exact_dmg`].
///
/// This ROM differs byte-wise from the mts-20240926 build that
/// `tests/mooneye.rs` already frame-verifies (sha256 c15f602d… here vs
/// 861e670e… there), so it is claimed here rather than exempted as a
/// duplicate; its bundled reference happens to decode to the very same
/// shade classes as the vendored `common::MGB_OAM_DMA_HALT_SPRITES_SHADES`.
fn run_madness_case(rom: &[u8], png_path: &Path) -> Result<(), String> {
    let img = common::png::load_png(png_path)?;
    if (img.w, img.h) != (SCREEN_W, SCREEN_H) {
        return Err(format!(
            "{}: reference image is {}x{}, want {SCREEN_W}x{SCREEN_H}",
            png_path.display(),
            img.w,
            img.h
        ));
    }
    let shades = img
        .rgb
        .iter()
        .map(|&rgb| madness_shade(rgb))
        .collect::<Result<Vec<u8>, String>>()
        .map_err(|e| format!("{}: {e}", png_path.display()))?;
    let mut gb = harness::boot(rom, Model::Mgb);
    harness::run_for_frames(&mut gb, 10);
    common::compare_frame_exact_dmg(gb.frame(), &shades)
}

/// Models a disposition produces cases for (used to attribute a ROM read
/// failure — or a caught per-ROM panic — to every case it would have run).
fn case_models(disposition: &Disposition) -> Vec<Model> {
    match disposition {
        Disposition::Protocol(models) => models.clone(),
        Disposition::SpritePriority => vec![Model::Dmg, Model::Cgb],
        Disposition::Madness => vec![Model::Mgb],
        Disposition::Exempt(_) => vec![],
    }
}

/// Run every case of one ROM; empty for exempt ROMs (with a loud note, so
/// a skip can never be silent).
fn run_rom(rom_path: &Path, rel: &str) -> Vec<CaseResult> {
    let collection_rel = format!("{SUITE}/{rel}");
    let disposition = classify(rel);
    if let Disposition::Exempt(reason) = disposition {
        println!("note: {collection_rel} skipped ({reason})");
        return Vec::new();
    }
    let models = case_models(&disposition);
    let rom = match std::fs::read(rom_path) {
        Ok(rom) => rom,
        Err(e) => {
            return models
                .into_iter()
                .map(|model| CaseResult {
                    key: harness::case_key(&collection_rel, model),
                    result: Err(format!("read failed: {e}")),
                })
                .collect();
        }
    };
    match disposition {
        Disposition::Protocol(_) => models
            .into_iter()
            .map(|model| CaseResult {
                key: harness::case_key(&collection_rel, model),
                result: run_protocol_case(&rom, model),
            })
            .collect(),
        // Only DMG and CGB(-compat) references exist: the howto replaced
        // upstream's single greyscale image with "two images containing the
        // expected result for DMG and CGB".
        Disposition::SpritePriority => [
            (Model::Dmg, "sprite_priority-dmg.png"),
            (Model::Cgb, "sprite_priority-cgb.png"),
        ]
        .into_iter()
        .map(|(model, png)| CaseResult {
            key: harness::case_key(&collection_rel, model),
            result: run_sprite_priority_case(&rom, model, &rom_path.with_file_name(png)),
        })
        .collect(),
        Disposition::Madness => vec![CaseResult {
            key: harness::case_key(&collection_rel, Model::Mgb),
            result: run_madness_case(
                &rom,
                &rom_path.with_file_name("mgb_oam_dma_halt_sprites_expected.png"),
            ),
        }],
        Disposition::Exempt(_) => unreachable!("handled above"),
    }
}

/// Inventory hook: collection-relative forward-slash paths of
/// every `.gb`/`.gbc` file under the suite directory, split into (claimed,
/// exempted). Claimed ROMs produce at least one rom×model case in
/// [`wilbertpol_matrix`]; exempted ones are documented never-run (the
/// [`Disposition::Exempt`] arms of [`classify`] carry the reasons). Both
/// empty when the collection is not checked out.
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let Some(root) = common::gbtr_root() else {
        return (Vec::new(), Vec::new());
    };
    let suite_dir = root.join(SUITE);
    let mut roms = Vec::new();
    common::collect_roms(&suite_dir, true, &mut roms)
        .unwrap_or_else(|e| panic!("cannot enumerate ROMs under {}: {e}", suite_dir.display()));
    let (mut claimed, mut exempted) = (Vec::new(), Vec::new());
    for rom_path in &roms {
        let rel = harness::rel_unix(&suite_dir, rom_path);
        let bucket = match classify(&rel) {
            Disposition::Exempt(_) => &mut exempted,
            _ => &mut claimed,
        };
        bucket.push(format!("{SUITE}/{rel}"));
    }
    (claimed, exempted)
}

/// The full rom×model matrix, ratcheted against the known-failure baseline.
#[test]
fn wilbertpol_matrix() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "wilbertpol_matrix",
            &format!("test-roms/{}/{SUITE} not present", common::GBTR_DIR),
        );
        return;
    };
    let suite_dir = root.join(SUITE);
    assert!(
        suite_dir.is_dir(),
        "collection present but {} is missing — corrupt checkout",
        suite_dir.display()
    );
    let mut roms = Vec::new();
    common::collect_roms(&suite_dir, true, &mut roms)
        .unwrap_or_else(|e| panic!("cannot enumerate ROMs under {}: {e}", suite_dir.display()));
    assert!(
        !roms.is_empty(),
        "{} exists but contains no .gb/.gbc ROMs — corrupt checkout?",
        suite_dir.display()
    );
    // Hundreds of rom×model cases: spread the ROMs over worker threads.
    // Case results are keyed, so collection order does not matter; they are
    // sorted afterwards for deterministic failure listings.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(roms.len());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(rom_path) = roms.get(i) else { break };
                    let rel = harness::rel_unix(&suite_dir, rom_path);
                    // Catch per-ROM panics *inside* the worker loop: an
                    // uncaught one would tear down thread::scope, poison
                    // the results mutex and abort the whole matrix. The
                    // panic is attributed to every rom×model case the ROM
                    // classifies to, as keyed Err results the baseline
                    // ratchet can report.
                    let cases =
                        harness::catch_panic(|| run_rom(rom_path, &rel)).unwrap_or_else(|msg| {
                            let collection_rel = format!("{SUITE}/{rel}");
                            case_models(&classify(&rel))
                                .into_iter()
                                .map(|model| CaseResult {
                                    key: harness::case_key(&collection_rel, model),
                                    result: Err(format!("panicked: {msg}")),
                                })
                                .collect()
                        });
                    results.lock().unwrap().extend(cases);
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.key.cmp(&b.key));
    println!("wilbertpol: {} rom×model cases executed", results.len());
    harness::assert_against_baseline(
        "wilbertpol",
        &results,
        &harness::parse_baseline(BASELINE_TXT),
    );
}

/// Self-check of the inventory hook ahead of the global coverage guard:
/// claimed ∩ exempted = ∅, claimed ∪ exempted = the on-disk ROM set, and
/// the exemptions are exactly the documented ones.
#[test]
fn wilbertpol_inventory_is_disjoint_and_complete() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "wilbertpol_inventory_is_disjoint_and_complete",
            &format!("test-roms/{}/{SUITE} not present", common::GBTR_DIR),
        );
        return;
    };
    let (claimed, exempted) = inventory();
    assert!(
        claimed.iter().all(|c| !exempted.contains(c)),
        "claimed and exempted overlap"
    );
    // The documented never-run set (see classify): the boot-hwio dump tool
    // and the three logic-analyzer ROMs. Notably madness/ is *claimed*
    // here, unlike in tests/mooneye.rs: this build differs byte-wise from
    // the mts copy (see run_madness_case).
    assert_eq!(
        exempted,
        [
            format!("{SUITE}/logic-analysis/external-bus/read_timing/read_timing.gb"),
            format!("{SUITE}/logic-analysis/external-bus/write_timing/write_timing.gb"),
            format!("{SUITE}/logic-analysis/ppu/simple_scx/simple_scx.gb"),
            format!("{SUITE}/utils/dump_boot_hwio.gb"),
        ]
    );
    let suite_dir = root.join(SUITE);
    let mut roms = Vec::new();
    common::collect_roms(&suite_dir, true, &mut roms)
        .unwrap_or_else(|e| panic!("cannot enumerate ROMs under {}: {e}", suite_dir.display()));
    let mut on_disk: Vec<String> = roms
        .iter()
        .map(|p| format!("{SUITE}/{}", harness::rel_unix(&suite_dir, p)))
        .collect();
    on_disk.sort();
    let mut union: Vec<String> = claimed.iter().chain(&exempted).cloned().collect();
    union.sort();
    assert_eq!(union, on_disk, "inventory does not cover the suite exactly");
    assert_eq!(on_disk.len(), 121, "pinned v7.0 suite ships 121 ROMs");
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- 2016 suffix parser (suite README "Test naming") ---

    fn models(rel: &str) -> Vec<Model> {
        match classify(rel) {
            Disposition::Protocol(models) => models,
            other => panic!("{rel}: expected Protocol(..), got {other:?}"),
        }
    }

    #[test]
    fn wilbertpol_suffix_single_models() {
        assert_eq!(models("acceptance/boot_regs-dmg.gb"), [Model::Dmg]);
        assert_eq!(models("misc/boot_regs-mgb.gb"), [Model::Mgb]);
        assert_eq!(models("misc/boot_regs-sgb.gb"), [Model::Sgb]);
        assert_eq!(models("misc/boot_regs-sgb2.gb"), [Model::Sgb2]);
        assert_eq!(models("misc/boot_regs-cgb.gb"), [Model::Cgb]);
        assert_eq!(suffix_models("agb"), Some(vec![Model::Agb]));
    }

    #[test]
    fn wilbertpol_suffix_ags_is_exempt() {
        // AGS is not modeled; a plain `ags` ROM maps to no machine at all
        // (none ship in v7.0, but the rule is part of the convention).
        assert_eq!(suffix_models("ags"), Some(vec![]));
        assert!(matches!(
            classify("misc/boot_regs-ags.gb"),
            Disposition::Exempt(_)
        ));
    }

    #[test]
    fn wilbertpol_suffix_group_letters() {
        assert_eq!(
            models("acceptance/di_timing-GS.gb"),
            [Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2]
        );
        assert_eq!(
            models("acceptance/gpu/ly_lyc_0-GS.gb"),
            [Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2]
        );
        // C = cgb+agb+ags in this fork; AGS drops out.
        assert_eq!(
            models("acceptance/gpu/hblank_ly_scx_timing-C.gb"),
            [Model::Cgb, Model::Agb]
        );
        assert_eq!(models("misc/boot_regs-A.gb"), [Model::Agb]);
        assert_eq!(
            models("acceptance/boot_hwio-G.gb"),
            [Model::Dmg, Model::Mgb]
        );
        assert_eq!(models("misc/boot_hwio-S.gb"), [Model::Sgb, Model::Sgb2]);
        // Combined letters accumulate without duplicates (C and A overlap
        // on Agb).
        assert_eq!(suffix_models("CA"), Some(vec![Model::Cgb, Model::Agb]));
    }

    #[test]
    fn wilbertpol_no_suffix_runs_default_set() {
        assert_eq!(models("acceptance/div_timing.gb"), DEFAULT_MODELS);
        // Underscores and trailing digits are not suffix separators.
        assert_eq!(models("acceptance/timer/tim00_div_trigger.gb").len(), 6);
        assert_eq!(models("acceptance/call_cc_timing2.gb").len(), 6);
        assert_eq!(
            models("acceptance/gpu/intr_2_mode0_scx1_timing_nops.gb").len(),
            6
        );
    }

    #[test]
    fn wilbertpol_unrecognized_suffix_falls_back_to_default() {
        assert_eq!(suffix_models("expected"), None);
        assert_eq!(suffix_models(""), None);
        assert_eq!(suffix_models("X"), None);
        assert_eq!(models("acceptance/foo-bar.gb"), DEFAULT_MODELS);
    }

    // --- directory dispositions ---

    #[test]
    fn wilbertpol_special_dirs() {
        assert!(matches!(
            classify("utils/dump_boot_hwio.gb"),
            Disposition::Exempt(_)
        ));
        assert!(matches!(
            classify("logic-analysis/ppu/simple_scx/simple_scx.gb"),
            Disposition::Exempt(_)
        ));
        assert_eq!(
            classify("manual-only/sprite_priority.gb"),
            Disposition::SpritePriority
        );
        assert_eq!(
            classify("madness/mgb_oam_dma_halt_sprites.gb"),
            Disposition::Madness
        );
        // Mirrors models_for's emulator-only arm: mapper tests are
        // model-agnostic.
        assert_eq!(
            classify("emulator-only/mbc1_rom_4banks.gb"),
            Disposition::Protocol(vec![Model::Dmg, Model::Cgb])
        );
    }

    // --- madness reference grey levels ---

    #[test]
    fn wilbertpol_madness_shade_maps_the_three_levels() {
        assert_eq!(madness_shade([255, 255, 255]), Ok(0));
        assert_eq!(madness_shade([176, 176, 176]), Ok(1));
        assert_eq!(madness_shade([104, 104, 104]), Ok(2));
    }

    #[test]
    fn wilbertpol_madness_shade_rejects_other_pixels() {
        // A regenerated/foreign reference must be rejected, not misclassed.
        let err = madness_shade([0, 0, 0]).unwrap_err();
        assert!(err.contains("grey level 0"), "{err}");
        let err = madness_shade([255, 255, 0]).unwrap_err();
        assert!(err.contains("non-grey"), "{err}");
    }
}
