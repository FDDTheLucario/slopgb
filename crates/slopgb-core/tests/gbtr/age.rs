//! age-test-roms suite harness (c-sp/game-boy-test-roms v7.0,
//! `age-test-roms/`).
//!
//! Protocol (age-test-roms/game-boy-test-roms-howto.md): every ROM signals
//! completion by executing `LD B,B`; tests that can self-verify then hold
//! the Fibonacci register signature. The three `m3-bg-*` directories cannot
//! self-verify ("Screenshot based tests" in the howto) and are compared
//! against the reference PNGs shipped next to each ROM instead — still
//! after the `LD B,B` exit, with no Fibonacci check. The reference colors
//! are the collection's common palette — straight `(X << 3) | (X >> 2)`
//! CGB expansion plus the FF/AA/55/00 DMG greys (howto, "Screenshot based
//! tests") — which is exactly the core's output, so the comparison map is
//! [`CgbColorMap::Identity`].
//!
//! Model routing comes from the device segments in each file name
//! (age-test-roms/README.md "Test naming") filtered through
//! docs/ARCHITECTURE.md §CGB revision policy: `Model::Cgb` models CPU CGB C
//! only, so `cgbE`-only and `ncmE`-only variants are revision-skips.
//!
//! The howto gives no per-ROM runtime figures, only the `LD B,B` exit
//! condition, so the mooneye 120-emulated-second hang timeout
//! ([`common::TIMEOUT_TCYCLES`]) is reused; every ROM is
//! protocol-terminated and finishes far earlier in practice.

use std::path::{Path, PathBuf};

use slopgb_core::Model;

use crate::common;
use crate::common::framecmp::CgbColorMap;
use crate::harness::{self, CaseResult};

/// Suite directory under the collection root.
const SUITE_DIR: &str = "age-test-roms";

/// Known-failure baseline (see `harness::assert_against_baseline`): the
/// rom×model cases the emulator does not pass yet. Shrinking this list is
/// progress; growing it is a regression.
const BASELINE: &[&str] = &[
    "age-test-roms/halt/ei-halt-dmgC-cgbBCE.gb [Cgb]",
    "age-test-roms/halt/ei-halt-dmgC-cgbBCE.gb [Dmg]",
    "age-test-roms/halt/halt-m0-interrupt-dmgC-cgbBCE.gb [Cgb]",
    "age-test-roms/halt/halt-m0-interrupt-dmgC-cgbBCE.gb [Dmg]",
    "age-test-roms/lcd-align-ly/lcd-align-ly-cgbBC.gb [Cgb]",
    "age-test-roms/ly/ly-dmgC-cgbBC.gb [Cgb]",
    "age-test-roms/m3-bg-bgp/m3-bg-bgp.gb [Dmg]",
    "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-nocgb.gb [Cgb]",
    "age-test-roms/m3-bg-lcdc/m3-bg-lcdc.gb [Cgb]",
    "age-test-roms/m3-bg-scx/m3-bg-scx-ds.gb [Cgb]",
    "age-test-roms/m3-bg-scx/m3-bg-scx-nocgb.gb [Cgb]",
    "age-test-roms/m3-bg-scx/m3-bg-scx.gb [Cgb]",
    "age-test-roms/m3-bg-scx/m3-bg-scx.gb [Dmg]",
    "age-test-roms/oam/oam-read-dmgC-cgbBC.gb [Cgb]",
    "age-test-roms/oam/oam-read-dmgC-cgbBC.gb [Dmg]",
    "age-test-roms/oam/oam-read-ncmBC.gb [Cgb]",
    "age-test-roms/oam/oam-write-cgbBCE.gb [Cgb]",
    "age-test-roms/oam/oam-write-dmgC.gb [Dmg]",
    "age-test-roms/oam/oam-write-ncmBCE.gb [Cgb]",
    "age-test-roms/speed-switch/caution/spsw-interrupts-cgbBC.gb [Cgb]",
    "age-test-roms/speed-switch/spsw-ch2-lc-delay-cgbBCE.gb [Cgb]",
    "age-test-roms/speed-switch/spsw-mode0-cgbBCE.gb [Cgb]",
    "age-test-roms/speed-switch/spsw-stop-prefetch-cgbBCE.gb [Cgb]",
    "age-test-roms/speed-switch/spsw-tima-cgbBC.gb [Cgb]",
    "age-test-roms/stat-interrupt/stat-int-dmgC-cgbBCE.gb [Cgb]",
    "age-test-roms/stat-interrupt/stat-int-dmgC-cgbBCE.gb [Dmg]",
    "age-test-roms/stat-interrupt/stat-int-ncmBCE.gb [Cgb]",
    "age-test-roms/stat-mode-sprites/stat-mode-sprites-dmgC-cgbBCE.gb [Cgb]",
    "age-test-roms/stat-mode-sprites/stat-mode-sprites-dmgC-cgbBCE.gb [Dmg]",
    "age-test-roms/stat-mode-sprites/stat-mode-sprites-ds-cgbBCE.gb [Cgb]",
    "age-test-roms/stat-mode-window/stat-mode-window-cgbBCE.gb [Cgb]",
    "age-test-roms/stat-mode-window/stat-mode-window-dmgC.gb [Dmg]",
    "age-test-roms/stat-mode-window/stat-mode-window-ds-cgbBCE.gb [Cgb]",
    "age-test-roms/stat-mode-window/stat-mode-window-ncmBCE.gb [Cgb]",
    "age-test-roms/stat-mode/stat-mode-dmgC-cgbBC.gb [Cgb]",
    "age-test-roms/stat-mode/stat-mode-dmgC-cgbBC.gb [Dmg]",
    "age-test-roms/stat-mode/stat-mode-ds-cgbBCE.gb [Cgb]",
    "age-test-roms/stat-mode/stat-mode-ncmBC.gb [Cgb]",
    "age-test-roms/vram/vram-read-cgbBCE.gb [Cgb]",
    "age-test-roms/vram/vram-read-dmgC.gb [Dmg]",
    "age-test-roms/vram/vram-read-ncmBCE.gb [Cgb]",
];

/// One device segment of an age file stem (age-test-roms/README.md "Test
/// naming"). Revision letters are kept verbatim for the policy filter in
/// [`segment_models`].
#[derive(Debug, PartialEq, Eq)]
enum Segment {
    /// `dmgC`: verified on DMG-CPU C.
    DmgC,
    /// `cgb<REVS>`: verified on these CGB SoC revisions, CGB mode.
    Cgb(String),
    /// `ncm<REVS>`: these CGB SoC revisions in non-CGB compatibility mode
    /// (the cart is DMG-flagged; the CGB boot ROM drops into compat mode).
    Ncm(String),
    /// `nocgb`: built without the CGB header flag.
    NoCgb,
    /// `ds`: double-speed variant — inherently CGB-only.
    Ds,
}

/// Parse one `-`-separated token as a device segment. Unknown tokens are
/// `None`: they belong to the test's name proper (the `ly` in
/// `lcd-align-ly-cgbBC`), and a multi-token remainder such as `ds-cgbBCE`
/// is rejected too (load-bearing for [`m3_legs`] reference matching).
fn parse_segment(token: &str) -> Option<Segment> {
    /// CGB SoC revision list: one or more of the revisions A–E that exist
    /// on real silicon (gbhwdb; the collection ships B/C/E variants).
    fn revs(s: &str) -> Option<String> {
        (!s.is_empty() && s.chars().all(|c| matches!(c, 'A'..='E'))).then(|| s.to_string())
    }
    match token {
        "dmgC" => Some(Segment::DmgC),
        "nocgb" => Some(Segment::NoCgb),
        "ds" => Some(Segment::Ds),
        _ => {
            if let Some(r) = token.strip_prefix("cgb") {
                revs(r).map(Segment::Cgb)
            } else if let Some(r) = token.strip_prefix("ncm") {
                revs(r).map(Segment::Ncm)
            } else {
                None
            }
        }
    }
}

/// The models one device segment maps to, per docs/ARCHITECTURE.md §CGB
/// revision policy (`Model::Cgb` ≡ CPU CGB C):
///
/// * `dmgC`/`nocgb` → [`Model::Dmg`];
/// * `cgb<REVS>` → [`Model::Cgb`] iff `C ∈ REVS`, else revision-skip;
/// * `ncm<REVS>` → [`Model::Cgb`] iff `B ∈ REVS` or `C ∈ REVS` — the
///   policy's "`-ncmBC(E)`" run row: non-CGB compat mode does not diverge
///   between B and C silicon — else revision-skip;
/// * `ds` → [`Model::Cgb`] (double speed exists on CGB only).
///
/// Empty slice = the segment names only unmodeled silicon (revision-skip).
fn segment_models(seg: &Segment) -> &'static [Model] {
    match seg {
        Segment::DmgC | Segment::NoCgb => &[Model::Dmg],
        Segment::Ds => &[Model::Cgb],
        Segment::Cgb(revs) if revs.contains('C') => &[Model::Cgb],
        Segment::Ncm(revs) if revs.contains('B') || revs.contains('C') => &[Model::Cgb],
        Segment::Cgb(_) | Segment::Ncm(_) => &[],
    }
}

/// Routing verdict for one ROM stem, from its trailing device segments.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// Run on these models (deduplicated, filename order).
    Run(Vec<Model>),
    /// Every device segment names unmodeled silicon — skip per policy.
    RevisionSkip,
    /// No device segments at all (the unsuffixed `m3-bg-*` ROMs; those are
    /// routed by their reference PNGs instead, see [`m3_legs`]).
    Unsuffixed,
}

/// Split a stem's trailing device segments and fold them into models. The
/// device suffix is the longest all-recognized token tail (so the `ly` in
/// `lcd-align-ly-cgbBC` stays part of the test name).
fn route_by_suffix(stem: &str) -> Route {
    let tokens: Vec<&str> = stem.split('-').collect();
    let mut start = tokens.len();
    while start > 0 && parse_segment(tokens[start - 1]).is_some() {
        start -= 1;
    }
    if start == tokens.len() {
        return Route::Unsuffixed;
    }
    let mut models: Vec<Model> = Vec::new();
    for token in &tokens[start..] {
        let seg = parse_segment(token).expect("tail tokens are device segments");
        for &m in segment_models(&seg) {
            if !models.contains(&m) {
                models.push(m);
            }
        }
    }
    if models.is_empty() {
        Route::RevisionSkip
    } else {
        Route::Run(models)
    }
}

/// Reference-PNG legs for one `m3-bg-*` ROM: a PNG belongs to the ROM iff
/// its stem is `<rom stem>-<single device segment>` (a multi-token
/// remainder like `ds-cgbBCE` belongs to the `-ds` sibling ROM instead).
/// The segment picks the model per [`segment_models`]; references for
/// unmodeled silicon are parked (e.g. `m3-bg-bgp-ncmE.png` — policy row
/// "`-ncmE` ×3").
fn m3_legs<'a>(rom_stem: &str, png_stems: &[&'a str]) -> Vec<(Model, &'a str)> {
    let mut legs = Vec::new();
    for &png in png_stems {
        let Some(rest) = png.strip_prefix(rom_stem).and_then(|r| r.strip_prefix('-')) else {
            continue;
        };
        let Some(seg) = parse_segment(rest) else {
            continue;
        };
        for &m in segment_models(&seg) {
            legs.push((m, png));
        }
    }
    legs
}

/// How one rom×model case is verified.
enum Proto {
    /// `LD B,B` exit + Fibonacci registers (howto, "Test Success/Failure").
    Fib,
    /// `LD B,B` exit + frame comparison against this reference PNG (howto,
    /// "Screenshot based tests"); no Fibonacci check.
    Frame(PathBuf),
}

struct Case {
    model: Model,
    proto: Proto,
}

/// All cases for one ROM file. Empty = revision-skip (exempt, per the
/// [`segment_models`] policy filter — the only documented never-run rule in
/// this suite).
fn cases_for(rom_path: &Path) -> Vec<Case> {
    let stem = rom_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_else(|| panic!("non-UTF-8 rom name: {}", rom_path.display()));
    let dir = rom_path.parent().expect("rom has a parent directory");
    let in_m3_dir = dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("m3-bg"));
    if in_m3_dir {
        // Screenshot-compared directory: route by which reference PNGs
        // exist next to the ROM (each PNG names the device it was captured
        // on, howto "Screenshot based tests").
        let stems = png_stems(dir);
        let stem_refs: Vec<&str> = stems.iter().map(String::as_str).collect();
        let legs = m3_legs(stem, &stem_refs);
        assert!(
            !legs.is_empty(),
            "{}: no usable reference PNG — collection layout changed?",
            rom_path.display()
        );
        return legs
            .into_iter()
            .map(|(model, png)| Case {
                model,
                proto: Proto::Frame(dir.join(format!("{png}.png"))),
            })
            .collect();
    }
    match route_by_suffix(stem) {
        Route::Run(models) => models
            .into_iter()
            .map(|model| Case {
                model,
                proto: Proto::Fib,
            })
            .collect(),
        Route::RevisionSkip => Vec::new(),
        Route::Unsuffixed => panic!(
            "{}: no device segments outside the m3-bg directories — \
             collection layout changed?",
            rom_path.display()
        ),
    }
}

/// Sorted stems of the `.png` reference images in `dir`.
fn png_stems(dir: &Path) -> Vec<String> {
    let mut stems: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| {
            let p = e.expect("readable directory entry").path();
            (p.extension().and_then(|x| x.to_str()) == Some("png"))
                .then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    stems.sort();
    stems
}

/// Run one fib-protocol case: `LD B,B` then the Fibonacci signature.
fn run_fib_case(rom: &[u8], model: Model) -> Result<(), String> {
    let mut gb = harness::boot(rom, model);
    harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)?;
    harness::check_fib(&gb)
}

/// Run one screenshot case: `LD B,B`, then advance to the next completed
/// frame boundary before comparing — the breakpoint fires mid-frame and the
/// test image is stable by the time the following frame completes.
fn run_frame_case(rom: &[u8], model: Model, png: &Path) -> Result<(), String> {
    let mut gb = harness::boot(rom, model);
    harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)?;
    harness::run_for_frames(&mut gb, 1);
    harness::expect_frame_png(&gb, png, CgbColorMap::Identity)
}

/// Enumerate the suite's ROMs, or `None` with the skip/fail notice already
/// emitted when the collection is not checked out.
fn suite_roms(test: &str) -> Option<(PathBuf, Vec<PathBuf>)> {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(test, "test-roms/game-boy-test-roms-v7.0 not present");
        return None;
    };
    let suite = root.join(SUITE_DIR);
    if !suite.is_dir() {
        common::skip_or_fail_gbtr(test, &format!("{} not present", suite.display()));
        return None;
    }
    let mut roms = Vec::new();
    common::collect_roms(&suite, true, &mut roms).unwrap_or_else(|e| {
        panic!(
            "{test}: cannot enumerate ROMs under {}: {e}",
            suite.display()
        )
    });
    assert!(
        !roms.is_empty(),
        "{test}: {} exists but holds no ROMs — corrupt checkout?",
        suite.display()
    );
    Some((root, roms))
}

/// Inventory hook for the Phase B2 coverage guard: collection-relative
/// forward-slash paths of every `.gb`/`.gbc` under `age-test-roms/`,
/// partitioned into (claimed = produces at least one rom×model case,
/// exempted = the [`segment_models`] revision-skips — `cgbE`-only and
/// `ncmE`-only variants, ARCHITECTURE.md §CGB revision policy row
/// "`-cgbE` ×6, `-ncmE` ×3").
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let Some((root, roms)) = suite_roms("age inventory") else {
        return (Vec::new(), Vec::new());
    };
    let mut claimed = Vec::new();
    let mut exempted = Vec::new();
    for rom_path in &roms {
        let rel = harness::rel_unix(&root, rom_path);
        if cases_for(rom_path).is_empty() {
            exempted.push(rel);
        } else {
            claimed.push(rel);
        }
    }
    (claimed, exempted)
}

/// Full age matrix: every claimed rom×model case through its protocol,
/// ratcheted against [`BASELINE`].
#[test]
fn age_matrix() {
    let Some((root, roms)) = suite_roms("age") else {
        return;
    };
    let mut results: Vec<CaseResult> = Vec::new();
    for rom_path in &roms {
        let cases = cases_for(rom_path);
        let rel = harness::rel_unix(&root, rom_path);
        if cases.is_empty() {
            println!(
                "note: {rel} skipped (revision not modeled; ARCHITECTURE.md §CGB revision policy)"
            );
            continue;
        }
        let rom = std::fs::read(rom_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", rom_path.display()));
        for case in cases {
            let result = harness::catch_case(|| match &case.proto {
                Proto::Fib => run_fib_case(&rom, case.model),
                Proto::Frame(png) => run_frame_case(&rom, case.model, png),
            });
            results.push(CaseResult {
                key: harness::case_key(&rel, case.model),
                result,
            });
        }
    }
    // The collection is pinned (v7.0): 47 ROMs route to exactly 49 cases
    // (39 fib + 10 screenshot legs); a different count means the routing
    // or the checkout changed.
    assert_eq!(results.len(), 49, "age case matrix changed size");
    harness::assert_against_baseline("age", &results, BASELINE);
}

/// Self-check for the inventory hook: claimed and exempted are disjoint and
/// together cover exactly the on-disk `.gb`/`.gbc` set, and the exempted
/// set is exactly the nine documented revision-skips.
#[test]
fn age_inventory_partitions_suite() {
    let Some((root, roms)) = suite_roms("age inventory") else {
        return;
    };
    let (claimed, exempted) = inventory();
    for c in &claimed {
        assert!(!exempted.contains(c), "{c} both claimed and exempted");
    }
    let mut combined: Vec<String> = claimed.iter().chain(&exempted).cloned().collect();
    combined.sort();
    let mut on_disk: Vec<String> = roms.iter().map(|p| harness::rel_unix(&root, p)).collect();
    on_disk.sort();
    assert_eq!(combined, on_disk, "inventory does not cover age-test-roms/");
    // ARCHITECTURE.md §CGB revision policy, age row: "-cgbE ×6, -ncmE ×3".
    let mut exempted = exempted;
    exempted.sort();
    assert_eq!(
        exempted,
        [
            "age-test-roms/lcd-align-ly/lcd-align-ly-cgbE.gb",
            "age-test-roms/ly/ly-cgbE.gb",
            "age-test-roms/ly/ly-ncmE.gb",
            "age-test-roms/oam/oam-read-cgbE.gb",
            "age-test-roms/oam/oam-read-ncmE.gb",
            "age-test-roms/speed-switch/caution/spsw-interrupts-cgbE.gb",
            "age-test-roms/speed-switch/spsw-tima-cgbE.gb",
            "age-test-roms/stat-mode/stat-mode-cgbE.gb",
            "age-test-roms/stat-mode/stat-mode-ncmE.gb",
        ],
        "exempt set is not the nine documented revision-skips"
    );
    // speed-switch/caution/ is *claimed*, not exempted: its WARNING.md
    // documents real-hardware oscillator instability after premature
    // speed-switch HALT wakeup — a physical-silicon concern with no
    // equivalent in an emulator.
    assert!(
        claimed
            .iter()
            .any(|c| c == "age-test-roms/speed-switch/caution/spsw-interrupts-cgbBC.gb"),
        "caution/ ROM missing from the claimed set"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- device segment parser (real tokens from the shipped file names) ---

    #[test]
    fn segment_parser_recognizes_device_tokens() {
        assert_eq!(parse_segment("dmgC"), Some(Segment::DmgC));
        assert_eq!(parse_segment("nocgb"), Some(Segment::NoCgb));
        assert_eq!(parse_segment("ds"), Some(Segment::Ds));
        assert_eq!(parse_segment("cgbBC"), Some(Segment::Cgb("BC".into())));
        assert_eq!(parse_segment("cgbBCE"), Some(Segment::Cgb("BCE".into())));
        assert_eq!(parse_segment("cgbE"), Some(Segment::Cgb("E".into())));
        assert_eq!(parse_segment("ncmBC"), Some(Segment::Ncm("BC".into())));
        assert_eq!(parse_segment("ncmBCE"), Some(Segment::Ncm("BCE".into())));
        assert_eq!(parse_segment("ncmE"), Some(Segment::Ncm("E".into())));
    }

    #[test]
    fn segment_parser_rejects_name_tokens() {
        for token in ["ly", "halt", "interrupt", "m0", "read", "bg", "m3"] {
            assert_eq!(parse_segment(token), None, "{token}");
        }
        // Bare prefixes and non-revision tails are not device segments.
        assert_eq!(parse_segment("cgb"), None);
        assert_eq!(parse_segment("ncm"), None);
        assert_eq!(parse_segment("cgbX"), None);
        // Multi-token remainders must not parse as one segment (m3-bg ref
        // matching relies on this to keep `-ds-cgbBCE.png` away from the
        // base ROM).
        assert_eq!(parse_segment("ds-cgbBCE"), None);
        assert_eq!(parse_segment("nocgb-ncmBCE"), None);
    }

    // --- suffix routing (real stems from the shipped collection) ---

    #[test]
    fn suffix_routing_dual_device_names() {
        assert_eq!(
            route_by_suffix("ly-dmgC-cgbBC"),
            Route::Run(vec![Model::Dmg, Model::Cgb])
        );
        assert_eq!(
            route_by_suffix("ei-halt-dmgC-cgbBCE"),
            Route::Run(vec![Model::Dmg, Model::Cgb])
        );
        assert_eq!(
            route_by_suffix("halt-m0-interrupt-dmgC-cgbBCE"),
            Route::Run(vec![Model::Dmg, Model::Cgb])
        );
    }

    #[test]
    fn suffix_routing_single_device_names() {
        assert_eq!(
            route_by_suffix("lcd-align-ly-cgbBC"),
            Route::Run(vec![Model::Cgb])
        );
        assert_eq!(
            route_by_suffix("oam-write-dmgC"),
            Route::Run(vec![Model::Dmg])
        );
        assert_eq!(
            route_by_suffix("vram-read-nocgb"),
            Route::Run(vec![Model::Dmg])
        );
        assert_eq!(route_by_suffix("ly-ncmBC"), Route::Run(vec![Model::Cgb]));
        assert_eq!(
            route_by_suffix("stat-int-ncmBCE"),
            Route::Run(vec![Model::Cgb])
        );
        assert_eq!(
            route_by_suffix("spsw-div-cgbBCE"),
            Route::Run(vec![Model::Cgb])
        );
    }

    #[test]
    fn suffix_routing_dedups_ds_with_cgb_revisions() {
        // `ds` and `cgbBCE` both map to Model::Cgb — one leg, not two.
        assert_eq!(
            route_by_suffix("stat-mode-ds-cgbBCE"),
            Route::Run(vec![Model::Cgb])
        );
    }

    #[test]
    fn suffix_routing_revision_skips() {
        // ARCHITECTURE.md §CGB revision policy: Model::Cgb is CPU CGB C; E-only
        // variants have no modeled hardware ("-cgbE ×6, -ncmE ×3" skips).
        assert_eq!(route_by_suffix("spsw-tima-cgbE"), Route::RevisionSkip);
        assert_eq!(route_by_suffix("lcd-align-ly-cgbE"), Route::RevisionSkip);
        assert_eq!(route_by_suffix("ly-ncmE"), Route::RevisionSkip);
    }

    #[test]
    fn suffix_routing_unsuffixed_m3_stems() {
        assert_eq!(route_by_suffix("m3-bg-bgp"), Route::Unsuffixed);
        assert_eq!(route_by_suffix("m3-bg-lcdc"), Route::Unsuffixed);
        // The `-ds` screenshot variant still parses (Cgb), consistent with
        // its `-cgbBCE` reference; m3 routing goes by references anyway.
        assert_eq!(
            route_by_suffix("m3-bg-lcdc-ds"),
            Route::Run(vec![Model::Cgb])
        );
    }

    // --- m3-bg reference routing (real PNG sets from the collection) ---

    #[test]
    fn m3_refs_route_dmg_and_compat_legs() {
        // m3-bg-bgp: DMG-flagged cart; ncmE reference parked per policy.
        let pngs = ["m3-bg-bgp-dmgC", "m3-bg-bgp-ncmBC", "m3-bg-bgp-ncmE"];
        assert_eq!(
            m3_legs("m3-bg-bgp", &pngs),
            vec![
                (Model::Dmg, "m3-bg-bgp-dmgC"),
                (Model::Cgb, "m3-bg-bgp-ncmBC")
            ]
        );
    }

    #[test]
    fn m3_refs_do_not_leak_across_sibling_roms() {
        let pngs = [
            "m3-bg-lcdc-cgbBCE",
            "m3-bg-lcdc-dmgC",
            "m3-bg-lcdc-ds-cgbBCE",
            "m3-bg-lcdc-nocgb-ncmBCE",
        ];
        assert_eq!(
            m3_legs("m3-bg-lcdc", &pngs),
            vec![
                (Model::Cgb, "m3-bg-lcdc-cgbBCE"),
                (Model::Dmg, "m3-bg-lcdc-dmgC"),
            ]
        );
        assert_eq!(
            m3_legs("m3-bg-lcdc-ds", &pngs),
            vec![(Model::Cgb, "m3-bg-lcdc-ds-cgbBCE")]
        );
        assert_eq!(
            m3_legs("m3-bg-lcdc-nocgb", &pngs),
            vec![(Model::Cgb, "m3-bg-lcdc-nocgb-ncmBCE")]
        );
    }
}
