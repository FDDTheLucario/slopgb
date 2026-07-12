//! mealybug-tearoom-tests suite harness.
//!
//! Protocol (`mealybug-tearoom-tests/game-boy-test-roms-howto.md` + the
//! upstream README bundled in the same directory): every ROM signals
//! completion by executing `LD B,B` ("Exit Condition"); the verdict then
//! depends on the directory:
//!
//! * `ppu/` — "Test success/failure has to be determined by screenshot
//!   comparison" against the reference PNG shipped next to the ROM. The
//!   references use the collection's common palette, which is exactly the
//!   core's output, so [`CgbColorMap::Identity`] applies (see
//!   `common/framecmp.rs`).
//! * `dma/` — mooneye-style Fibonacci registers at the breakpoint; the two
//!   ROMs carry the mooneye `-C` model suffix and ship no reference PNGs.
//! * `mbc/` — Fibonacci as well; `mbc3_rtc.gb` probes the cartridge only and
//!   is model-agnostic, so it runs on one plain and one CGB machine exactly
//!   like the mooneye `emulator-only/` mapper tests.
//!
//! Reference selection per docs/ARCHITECTURE.md §CGB revision policy
//! (including its DMG-revision note): `Model::Dmg` pins late-DMG silicon —
//! the suite's "blob" (DMG-C-ish) capture series — consistent with age's
//! `-dmgC` routing and gambatte's `dmg08` expectations, so DMG legs compare
//! against `<stem>_dmg_blob.png`; `Model::Cgb` (≡ CPU CGB C) legs against
//! `<stem>_cgb_c.png`. The shipped `_cgb_d.png` references are PARKED for a
//! future CgbD/CgbE revision model, and the two `_dmg_b.png` ones stay
//! parked likewise: they capture early-DMG-B silicon whose output differs
//! from the blob series, and the policy picks blob for corpus consistency
//! (mooneye's `-dmgABC` ROMs pass on our `Model::Dmg`; B-vs-blob skew shows
//! up only in these two screenshots). Neither parked series creates a leg.
//! A rom-model leg with no shipped reference does not run, and
//! `ppu/win_without_bg.gb`, which ships no reference at all, is the suite's
//! only whole-ROM exemption.
//!
//! The howto states no per-ROM duration; every ROM is breakpoint-terminated
//! and finishes in well under a second, so the runner reuses the mooneye
//! 120-emulated-second hang timeout ([`common::TIMEOUT_TCYCLES`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use slopgb_core::Model;

use crate::common;
use crate::common::framecmp::CgbColorMap;
use crate::harness::{self, CaseResult};

/// Suite directory inside the collection.
const SUITE_DIR: &str = "mealybug-tearoom-tests";

/// How one rom×model case is judged after the `LD B,B` breakpoint.
enum Check {
    /// Mooneye Fibonacci register signature (`dma/`, `mbc/`).
    Fib,
    /// Frame compare against this reference PNG (`ppu/`).
    Frame(PathBuf),
}

/// One runnable rom×model leg.
struct Case {
    /// Collection-relative forward-slash ROM path.
    rel: String,
    rom: PathBuf,
    model: Model,
    check: Check,
}

/// Reference PNG filename for a `ppu/` ROM stem on a given model, per the
/// ARCHITECTURE.md reference-selection table: DMG legs use the `_dmg_blob`
/// screenshot series, CGB legs the `_cgb_c` (CPU CGB C) series.
fn ppu_ref_name(stem: &str, model: Model) -> String {
    match model {
        Model::Dmg => format!("{stem}_dmg_blob.png"),
        Model::Cgb => format!("{stem}_cgb_c.png"),
        other => panic!("no mealybug ppu reference series for {other:?}"),
    }
}

/// Mooneye-style model suffix carried by the two `dma/` ROMs: a trailing
/// `-C` names the CGB hardware group, which the revision policy runs on
/// `Model::Cgb` only (docs/ARCHITECTURE.md §CGB revision policy).
fn has_cgb_suffix(stem: &str) -> bool {
    stem.rsplit_once('-').is_some_and(|(_, sfx)| sfx == "C")
}

/// Walk the suite directory and split every on-disk ROM into its runnable
/// rom×model legs plus the documented whole-ROM exemptions (collection-
/// relative paths). Panics on enumeration errors or unrouted ROMs — a
/// changed collection layout must fail loudly, never shrink the matrix.
fn suite_cases(root: &Path) -> (Vec<Case>, Vec<String>) {
    let suite = root.join(SUITE_DIR);
    let mut roms = Vec::new();
    common::collect_roms(&suite, true, &mut roms)
        .unwrap_or_else(|e| panic!("cannot enumerate ROMs under {}: {e}", suite.display()));
    assert!(
        !roms.is_empty(),
        "{SUITE_DIR} exists but contains no .gb/.gbc ROMs — corrupt checkout?"
    );
    let mut cases = Vec::new();
    let mut exempted = Vec::new();
    for rom in roms {
        let rel = harness::rel_unix(root, &rom);
        let stem = rom
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_else(|| panic!("non-UTF-8 ROM name: {rel}"));
        let sub = rel.split('/').nth(1).unwrap_or("");
        match sub {
            "ppu" => {
                // A leg exists iff its policy reference ships next to the
                // ROM (27 _cgb_c + 24 _dmg_blob in the pinned v7.0 zip;
                // mealybug_leg_matrix_locked_to_v7_references locks the
                // counts so a corrupt checkout cannot silently shrink the
                // matrix). Zero legs = whole-ROM exemption: only
                // win_without_bg.gb, which ships no reference at all.
                let mut legs = 0;
                for model in [Model::Dmg, Model::Cgb] {
                    let png = rom.with_file_name(ppu_ref_name(stem, model));
                    if png.is_file() {
                        legs += 1;
                        cases.push(Case {
                            rel: rel.clone(),
                            rom: rom.clone(),
                            model,
                            check: Check::Frame(png),
                        });
                    }
                }
                if legs == 0 {
                    println!("note: {rel} exempt (no shipped reference image)");
                    exempted.push(rel);
                }
            }
            "dma" => {
                // hdma_during_halt-C / hdma_timing-C: mooneye protocol.
                assert!(has_cgb_suffix(stem), "unrouted mealybug dma ROM {rel}");
                cases.push(Case {
                    rel,
                    rom,
                    model: Model::Cgb,
                    check: Check::Fib,
                });
            }
            "mbc" => {
                // Model-agnostic cartridge probe: one plain and one CGB
                // machine, mirroring the mooneye emulator-only/ matrix.
                for model in [Model::Dmg, Model::Cgb] {
                    cases.push(Case {
                        rel: rel.clone(),
                        rom: rom.clone(),
                        model,
                        check: Check::Fib,
                    });
                }
            }
            other => panic!("unrouted mealybug ROM {rel} (subdirectory {other:?})"),
        }
    }
    (cases, exempted)
}

/// Run one rom×model leg through the suite protocol.
fn run_case(rom: &[u8], case: &Case) -> Result<(), String> {
    let mut gb = harness::boot(rom, case.model);
    harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)?;
    match &case.check {
        Check::Fib => harness::check_fib(&gb),
        Check::Frame(png) => {
            // The upstream README says to screenshot at the LD B,B
            // breakpoint, which fires mid-frame; advance to the next
            // completed frame boundary so the compared image is the stable,
            // fully rendered post-test screen (the ROM only spins after the
            // breakpoint, so that frame repeats forever).
            harness::run_for_frames(&mut gb, 1);
            harness::expect_frame_png(&gb, png, CgbColorMap::Identity)
        }
    }
}

/// Known-failure baseline (see `harness::assert_against_baseline`): one case
/// key per line in `baselines/mealybug.txt`, discovered by running the full
/// matrix. Shrinking it is progress; growing it is a regression.
fn baseline() -> Vec<&'static str> {
    harness::parse_baseline(include_str!("baselines/mealybug.txt"))
}

/// Phase B2 inventory guard hook: (claimed, exempted) collection-relative
/// forward-slash paths of every `.gb`/`.gbc` under the suite directory.
/// claimed = at least one rom×model leg runs; exempted = documented
/// never-run (`ppu/win_without_bg.gb`: no reference image shipped, see the
/// module docs and ARCHITECTURE.md §CGB revision policy).
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let Some(root) = common::gbtr_root() else {
        return (Vec::new(), Vec::new());
    };
    let (cases, exempted) = suite_cases(&root);
    let mut claimed: Vec<String> = cases.iter().map(|c| c.rel.clone()).collect();
    // Legs of one ROM are pushed consecutively from a sorted walk, so
    // dedup() collapses them to one claim per ROM.
    claimed.dedup();
    (claimed, exempted)
}

/// Full rom×model matrix, ratcheted against the known-failure baseline.
#[test]
fn mealybug_matrix() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "mealybug",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    let (cases, _) = suite_cases(&root);
    let mut results = Vec::new();
    for case in &cases {
        let rom =
            std::fs::read(&case.rom).unwrap_or_else(|e| panic!("read {}: {e}", case.rom.display()));
        results.push(CaseResult {
            key: harness::case_key(&case.rel, case.model),
            result: harness::catch_case(|| run_case(&rom, case)),
        });
    }
    harness::assert_against_baseline("mealybug", &results, &baseline());
}

/// Self-check that [`inventory`] covers the on-disk suite exactly:
/// claimed ∩ exempted = ∅ and claimed ∪ exempted = every `.gb`/`.gbc` file
/// under the suite directory (walked independently here).
#[test]
fn mealybug_inventory_covers_suite_exactly() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "mealybug_inventory",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    let (claimed, exempted) = inventory();
    let claimed: BTreeSet<String> = claimed.into_iter().collect();
    let exempted: BTreeSet<String> = exempted.into_iter().collect();
    let overlap: Vec<&String> = claimed.intersection(&exempted).collect();
    assert!(overlap.is_empty(), "claimed ∩ exempted ≠ ∅: {overlap:?}");
    let mut on_disk = Vec::new();
    common::collect_roms(&root.join(SUITE_DIR), true, &mut on_disk).unwrap();
    let on_disk: BTreeSet<String> = on_disk
        .iter()
        .map(|p| harness::rel_unix(&root, p))
        .collect();
    let union: BTreeSet<String> = claimed.union(&exempted).cloned().collect();
    assert_eq!(
        union, on_disk,
        "inventory does not cover the on-disk ROM set exactly"
    );
}

/// The leg matrix is derived from which reference PNGs ship next to the
/// ROMs, so a corrupt checkout (a missing PNG) would otherwise silently
/// shrink it. Lock the pinned v7.0 shipped set:
///
/// * `ppu/` (32 ROMs): 20 ROMs run both legs, the 7 `*2` CGB-behavior ROMs
///   ship only `_cgb_c` references (CGB-only legs — no DMG reference is
///   parked, none exists), and 4 ROMs (`m3_wx_4/5/6_change`,
///   `m3_lcdc_win_en_change_multiple_wx`) ship only DMG references (their
///   missing-CGB legs are reference holes, not whole-ROM exemptions) →
///   24 Dmg + 27 Cgb legs;
/// * `dma/`: 2 Cgb legs; `mbc/`: 1 ROM × {Dmg, Cgb} = 2 legs;
/// * exactly one whole-ROM exemption: `ppu/win_without_bg.gb`.
#[test]
fn mealybug_leg_matrix_locked_to_v7_references() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "mealybug_leg_matrix",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    let (cases, exempted) = suite_cases(&root);
    let count = |sub: &str, model: Model| {
        cases
            .iter()
            .filter(|c| c.rel.starts_with(&format!("{SUITE_DIR}/{sub}/")) && c.model == model)
            .count()
    };
    assert_eq!(count("ppu", Model::Dmg), 24, "ppu DMG-blob legs");
    assert_eq!(count("ppu", Model::Cgb), 27, "ppu CGB-C legs");
    assert_eq!(count("dma", Model::Cgb), 2, "dma CGB legs");
    assert_eq!(count("dma", Model::Dmg), 0, "dma must not run on DMG");
    assert_eq!(count("mbc", Model::Dmg), 1, "mbc DMG legs");
    assert_eq!(count("mbc", Model::Cgb), 1, "mbc CGB legs");
    assert_eq!(cases.len(), 55, "total rom×model legs");
    assert_eq!(
        exempted,
        vec![format!("{SUITE_DIR}/ppu/win_without_bg.gb")],
        "whole-ROM exemptions"
    );
}

#[test]
fn mealybug_ppu_reference_names_follow_revision_policy() {
    assert_eq!(
        ppu_ref_name("m3_bgp_change", Model::Dmg),
        "m3_bgp_change_dmg_blob.png"
    );
    assert_eq!(
        ppu_ref_name("m3_bgp_change", Model::Cgb),
        "m3_bgp_change_cgb_c.png"
    );
}

#[test]
#[should_panic(expected = "no mealybug ppu reference series")]
fn mealybug_ppu_reference_names_reject_unrouted_models() {
    ppu_ref_name("m3_bgp_change", Model::Mgb);
}

#[test]
fn mealybug_dma_suffix_detection() {
    assert!(has_cgb_suffix("hdma_during_halt-C"));
    assert!(has_cgb_suffix("hdma_timing-C"));
    assert!(!has_cgb_suffix("mbc3_rtc"));
    assert!(!has_cgb_suffix("m3_bgp_change"));
}

#[test]
fn mealybug_baseline_has_no_duplicate_keys() {
    let baseline = baseline();
    let unique: BTreeSet<&&str> = baseline.iter().collect();
    assert_eq!(unique.len(), baseline.len(), "duplicate baseline entries");
}

/// Red-before-green pin for the #11ej eager per-register CGB write-commit debt
/// (`Ppu::stage_write`, palette `6 + 2*parity`, WX `12`): these DMG-compat
/// mode-3 pixel legs pass tier2 (identical whole-dot render code) and fail the
/// eager clock ONLY on the cc+0 write-commit position. Fails with the CGB
/// per-register debt reverted (uniform 8 → wrong pixel column), passes with it.
#[test]
fn mealybug_eager_cgb_m3_writecommit_passes() {
    let Some(root) = common::gbtr_root() else {
        return;
    };
    for stem in [
        "m3_bgp_change",
        "m3_window_timing",
        "m3_window_timing_wx_0",
        "m3_wx_4_change_sprites",
    ] {
        let rom_path = root.join(SUITE_DIR).join("ppu").join(format!("{stem}.gb"));
        let png = root
            .join(SUITE_DIR)
            .join("ppu")
            .join(ppu_ref_name(stem, Model::Cgb));
        let rom =
            std::fs::read(&rom_path).unwrap_or_else(|e| panic!("read {}: {e}", rom_path.display()));
        let mut gb = harness::boot_eager(&rom, Model::Cgb);
        harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)
            .unwrap_or_else(|e| panic!("{stem} [Cgb] eager: {e}"));
        harness::run_for_frames(&mut gb, 1);
        harness::expect_frame_png(&gb, &png, CgbColorMap::Identity)
            .unwrap_or_else(|e| panic!("{stem} [Cgb] eager: {e}"));
    }
}

/// Red-before-green pin for the #11el eager DMG SCX POST-match write-commit debt
/// (`Ppu::stage_write`, FF43 `hunt_done && dot > hunt_match_dot => 6`). A mid-
/// mode-3 SCX write landing AFTER this line's fine-scroll comparator lock is a
/// pure coarse/pixel tile shift; its eager cc+0 commit lands the tile column 4
/// dots early without the render-frame debt. Fails with the FF43 post-match arm
/// reverted (159px off, wrong tile column), passes with it. `m3_scx_low_3_bits`
/// is NOT here: its write is PRE-match (feeds the emergent mode-3 length that the
/// gambatte m3stat/late_scx rows read) — genuine length coupling, kept zero-debt.
#[test]
fn mealybug_eager_dmg_m3_scx_high_writecommit_passes() {
    let Some(root) = common::gbtr_root() else {
        return;
    };
    let stem = "m3_scx_high_5_bits";
    let rom_path = root.join(SUITE_DIR).join("ppu").join(format!("{stem}.gb"));
    let png = root
        .join(SUITE_DIR)
        .join("ppu")
        .join(ppu_ref_name(stem, Model::Dmg));
    let rom =
        std::fs::read(&rom_path).unwrap_or_else(|e| panic!("read {}: {e}", rom_path.display()));
    let mut gb = harness::boot_eager(&rom, Model::Dmg);
    harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)
        .unwrap_or_else(|e| panic!("{stem} [Dmg] eager: {e}"));
    harness::run_for_frames(&mut gb, 1);
    harness::expect_frame_png(&gb, &png, CgbColorMap::Identity)
        .unwrap_or_else(|e| panic!("{stem} [Dmg] eager: {e}"));
}
