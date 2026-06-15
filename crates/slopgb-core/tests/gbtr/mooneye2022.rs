//! mooneye-test-suite (2022 build) harness — the copy of Gekkio's mooneye
//! test suite bundled in the c-sp collection (`mooneye-test-suite/`).
//!
//! This is **not** a duplicate of the pinned mts-20240926 bundle that
//! `tests/mooneye.rs` runs: v7.0 ships a 2022-03 build (upstream commit
//! 8d742b9d55, per the collection's CHANGELOG) and all 115 binaries differ
//! byte-wise from the 2024 bundle, so its expectations can genuinely
//! diverge — the suite runs here in full instead of being exempted as
//! foreign-covered.
//!
//! Protocol: the standard mooneye breakpoint protocol (`LD B,B` exit +
//! Fibonacci registers, 120-emulated-second timeout) for everything except:
//!
//! * `manual-only/sprite_priority.gb` never signals completion — render 15
//!   frame periods (the ROM keeps the LCD off through ~10 of them and
//!   hardware presents the first frame after the re-enable blank, so the
//!   image is stable from period 11) and compare against the c-sp
//!   replacement references shipped next to it (`sprite_priority-dmg.png` /
//!   `-cgb.png`, common palette ⇒ [`CgbColorMap::Identity`]), one leg each
//!   on [`Model::Dmg`] and [`Model::Cgb`] — mirroring
//!   `tests/common/mod.rs::run_sprite_priority` and the wilbertpol suite's
//!   handling.
//! * `madness/mgb_oam_dma_halt_sprites.gb` halts forever with no interrupt
//!   enabled and never executes `LD B,B`. This build differs byte-wise from
//!   the mts copy (sha256 4040ef40… here vs 861e670e… there), so it is
//!   *claimed* here rather than exempted as a duplicate: render 10 frames
//!   on [`Model::Mgb`] — the only model the test's asm documents — and
//!   compare against the vendored
//!   [`common::MGB_OAM_DMA_HALT_SPRITES_SHADES`] shade classes via the
//!   exact-DMG comparator, mirroring wilbertpol's `run_madness_case` (whose
//!   bundled reference decodes to the same classes).
//! * `utils/` (bootrom_dumper, dump_boot_hwio) are dump tools with no
//!   pass/fail protocol — documented exemptions.
//!
//! Model routing reuses [`common::models_for`] — this build already uses
//! the modern suffix convention (`-dmg0`, `-dmgABCmgb`, `-cgbABCDE`, group
//! letters) and the directory layout matches the mts bundle's
//! (`acceptance/`, `emulator-only/`, `misc/`, …), so suite-relative paths
//! feed it directly. A ROM whose suffix maps to no modeled revision
//! (`misc/boot_div-cgb0.gb`) is a revision-skip, announced loudly.

use std::path::Path;

use slopgb_core::Model;

use crate::common;
use crate::common::framecmp::CgbColorMap;
use crate::harness::{self, CaseResult};

/// Collection-relative suite directory.
const SUITE: &str = "mooneye-test-suite";

/// Known-failure baseline (see `harness::assert_against_baseline`),
/// derived by running the full matrix against the pinned v7.0 checkout:
/// 438/439 pass — every breakpoint-protocol case scores exactly like the
/// 439/439 the 2024 mts bundle posts in `tests/mooneye.rs`; only one
/// frame-compare leg fails, with direct precedent in the wilbertpol
/// suite's baseline:
///
/// * madness [Mgb]: this 2022 build renders an all-white frame under our
///   MGB HALT-frozen OAM-scan model (all 11 523 non-white reference pixels
///   differ), where the byte-different mts-20240926 build passes the
///   identical reference in `tests/mooneye.rs` — the same build-sensitive
///   divergence wilbertpol documents for its 2016 build (floor class F
///   per the index in `baselines/gambatte.txt`: build defect, the glitch
///   sprite's controlling bytes are undocumented DMA-driver residue —
///   don't bend the MGB scan model at the one verified build's expense).
const BASELINE: &[&str] = &["mooneye-test-suite/madness/mgb_oam_dma_halt_sprites.gb [Mgb]"];

/// How one suite ROM is verified — or why it never runs.
#[derive(Debug, PartialEq, Eq)]
enum Disposition {
    /// Breakpoint protocol + Fibonacci check, once per model.
    Protocol(Vec<Model>),
    /// `manual-only/sprite_priority.gb`: 15 frame periods + reference PNGs.
    SpritePriority,
    /// `madness/mgb_oam_dma_halt_sprites.gb`: 10 frames on MGB + the
    /// vendored shade-class reference.
    Madness,
    /// Documented never-run, with the citable reason.
    Exempt(&'static str),
}

/// Classify one suite-relative forward-slash ROM path.
fn classify(rel: &str) -> Disposition {
    match rel.split('/').next().unwrap_or("") {
        // bootrom_dumper.gb / dump_boot_hwio.gb dump hardware state for
        // manual transcription; neither the suite README nor the howto
        // define a machine-checkable pass criterion.
        "utils" => return Disposition::Exempt("dump tool, no pass/fail protocol"),
        "manual-only" => return Disposition::SpritePriority,
        "madness" => return Disposition::Madness,
        _ => {}
    }
    let models = common::models_for(Path::new(rel));
    if models.is_empty() {
        // misc/boot_div-cgb0.gb: documented to pass only on CGB revision 0,
        // which is not modeled (common::models_for suffix rules).
        Disposition::Exempt("no modeled hardware revision (-cgb0 suffix)")
    } else {
        Disposition::Protocol(models)
    }
}

/// Models a disposition produces cases for (used to attribute a ROM read
/// failure to every case it would have run).
fn case_models(disposition: &Disposition) -> Vec<Model> {
    match disposition {
        Disposition::Protocol(models) => models.clone(),
        Disposition::SpritePriority => vec![Model::Dmg, Model::Cgb],
        Disposition::Madness => vec![Model::Mgb],
        Disposition::Exempt(_) => vec![],
    }
}

/// One breakpoint-protocol case: run to `LD B,B` (or the mooneye
/// 120-emulated-second timeout), then check the Fibonacci signature.
fn run_protocol_case(rom: &[u8], model: Model) -> Result<(), String> {
    let mut gb = harness::boot(rom, model);
    harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)?;
    harness::check_fib(&gb)
}

/// One `manual-only/sprite_priority.gb` case: render 15 frame periods and
/// compare against one of the suite's two c-sp reference PNGs — common
/// palette, hence [`CgbColorMap::Identity`]. 15 leaves margin: the ROM
/// keeps the LCD off for ~10 periods while drawing, and the first frame
/// after the re-enable is presented blank on hardware (Pan Docs "LCDC.7"),
/// so the image is only stable from period 11.
fn run_sprite_priority_case(rom: &[u8], model: Model, png_path: &Path) -> Result<(), String> {
    let mut gb = harness::boot(rom, model);
    harness::run_for_frames(&mut gb, 15);
    harness::expect_frame_png(&gb, png_path, CgbColorMap::Identity)
}

/// The `madness/mgb_oam_dma_halt_sprites.gb` case: render 10 frames on MGB
/// and compare against the vendored shade-class reference (exact DMG-family
/// colors — MGB renders the fixed FF/AA/55/00 greys). See the module docs
/// for why this byte-distinct build is claimed despite `tests/mooneye.rs`
/// frame-verifying the mts copy.
fn run_madness_case(rom: &[u8]) -> Result<(), String> {
    let mut gb = harness::boot(rom, Model::Mgb);
    harness::run_for_frames(&mut gb, 10);
    common::compare_frame_exact_dmg(gb.frame(), common::MGB_OAM_DMA_HALT_SPRITES_SHADES)
}

/// The full rom×model matrix, ratcheted against the known-failure baseline.
#[test]
fn mooneye2022_matrix() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "mooneye2022_matrix",
            &format!("test-roms/{}/{SUITE} not present", common::GBTR_DIR),
        );
        return;
    };
    let suite_dir = root.join(SUITE);
    let mut roms = Vec::new();
    common::collect_roms(&suite_dir, true, &mut roms)
        .unwrap_or_else(|e| panic!("cannot enumerate ROMs under {}: {e}", suite_dir.display()));
    assert!(
        !roms.is_empty(),
        "{} exists but contains no .gb/.gbc ROMs — corrupt checkout?",
        suite_dir.display()
    );
    // Each ROM's legs are independent — fan out across cores (order preserved).
    let results: Vec<CaseResult> = harness::par_flat_map(&roms, |rom_path| {
        let rel = harness::rel_unix(&suite_dir, rom_path);
        let collection_rel = format!("{SUITE}/{rel}");
        let disposition = classify(&rel);
        if let Disposition::Exempt(reason) = disposition {
            println!("note: {collection_rel} skipped ({reason})");
            return Vec::new();
        }
        let rom = match std::fs::read(rom_path) {
            Ok(rom) => rom,
            Err(e) => {
                return case_models(&disposition)
                    .into_iter()
                    .map(|model| CaseResult {
                        key: harness::case_key(&collection_rel, model),
                        result: Err(format!("read failed: {e}")),
                    })
                    .collect();
            }
        };
        match disposition {
            Disposition::Protocol(models) => models
                .into_iter()
                .map(|model| CaseResult {
                    key: harness::case_key(&collection_rel, model),
                    result: harness::catch_case(|| run_protocol_case(&rom, model)),
                })
                .collect(),
            Disposition::SpritePriority => [
                (Model::Dmg, "sprite_priority-dmg.png"),
                (Model::Cgb, "sprite_priority-cgb.png"),
            ]
            .into_iter()
            .map(|(model, png)| {
                let png_path = rom_path.with_file_name(png);
                CaseResult {
                    key: harness::case_key(&collection_rel, model),
                    result: harness::catch_case(|| {
                        run_sprite_priority_case(&rom, model, &png_path)
                    }),
                }
            })
            .collect(),
            Disposition::Madness => vec![CaseResult {
                key: harness::case_key(&collection_rel, Model::Mgb),
                result: harness::catch_case(|| run_madness_case(&rom)),
            }],
            Disposition::Exempt(_) => unreachable!("handled above"),
        }
    });
    // Routing pin for the v7.0 checkout: 112 claimed ROMs route to exactly
    // 439 rom×model cases (436 breakpoint + 2 sprite_priority legs +
    // 1 madness leg — the same 439 the 2024 mts bundle yields in
    // tests/mooneye.rs, as no test was added or removed in between); a
    // different count means the routing or the checkout changed.
    assert_eq!(results.len(), 439, "case-matrix drift");
    let passed = results.iter().filter(|c| c.result.is_ok()).count();
    println!("mooneye2022: {passed}/{} cases pass", results.len());
    harness::assert_against_baseline("mooneye2022", &results, BASELINE);
}

/// Phase B2 inventory hook: collection-relative forward-slash paths of
/// every `.gb`/`.gbc` under `mooneye-test-suite/`, split into (claimed,
/// exempted). Claimed ROMs produce at least one rom×model case in
/// [`mooneye2022_matrix`]; exempted ones are documented never-run (the
/// [`Disposition::Exempt`] arms of [`classify`] carry the reasons).
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

/// Self-check of the inventory hook ahead of the global Phase B2 guard:
/// claimed ∩ exempted = ∅, claimed ∪ exempted = the on-disk ROM set, and
/// the exemptions are exactly the documented ones.
#[test]
fn mooneye2022_inventory_is_disjoint_and_complete() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "mooneye2022_inventory_is_disjoint_and_complete",
            &format!("test-roms/{}/{SUITE} not present", common::GBTR_DIR),
        );
        return;
    };
    let (claimed, exempted) = inventory();
    assert!(
        claimed.iter().all(|c| !exempted.contains(c)),
        "claimed and exempted overlap"
    );
    // The documented never-run set (see classify): the -cgb0 revision-skip
    // and the two utils/ dump tools. madness/ and manual-only/ are
    // *claimed* (frame-compare protocols, see module docs).
    assert_eq!(
        exempted,
        [
            format!("{SUITE}/misc/boot_div-cgb0.gb"),
            format!("{SUITE}/utils/bootrom_dumper.gb"),
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
    assert_eq!(on_disk.len(), 115, "pinned v7.0 suite ships 115 ROMs");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mooneye2022_special_dirs() {
        assert!(matches!(
            classify("utils/dump_boot_hwio.gb"),
            Disposition::Exempt(_)
        ));
        assert!(matches!(
            classify("utils/bootrom_dumper.gb"),
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
    }

    #[test]
    fn mooneye2022_routes_through_modern_suffix_convention() {
        // Spot checks of the common::models_for plumbing on real v7.0 paths.
        assert_eq!(
            classify("acceptance/boot_div-dmg0.gb"),
            Disposition::Protocol(vec![Model::Dmg0])
        );
        assert_eq!(
            classify("acceptance/div_timing.gb"),
            Disposition::Protocol(vec![
                Model::Dmg,
                Model::Mgb,
                Model::Sgb,
                Model::Sgb2,
                Model::Cgb,
                Model::Agb,
            ])
        );
        assert_eq!(
            classify("emulator-only/mbc1/rom_512kb.gb"),
            Disposition::Protocol(vec![Model::Dmg, Model::Cgb])
        );
        assert_eq!(
            classify("misc/boot_regs-A.gb"),
            Disposition::Protocol(vec![Model::Agb])
        );
        // The one revision-skip: CGB rev 0 is not modeled.
        assert!(matches!(
            classify("misc/boot_div-cgb0.gb"),
            Disposition::Exempt(_)
        ));
    }
}
