//! acid suite — Matt Currie's three PPU acid tests from the c-sp collection:
//! `dmg-acid2/`, `cgb-acid2/`, `cgb-acid-hell/`.
//!
//! Protocol (each suite's `game-boy-test-roms-howto.md`): the ROM executes
//! `LD B,B` when finished ("Exit Condition"), and the pass criterion is the
//! screen matching the suite's reference screenshot ("Test Success/Failure").
//! There is no register signature — these predate/ignore the mooneye
//! Fibonacci convention — so the verdict is frame comparison only.
//!
//! Color rule: the howtos require DMG shades #FFFFFF/#AAAAAA/#555555/#000000
//! and 5-bit CGB channels expanded as `(X << 3) | (X >> 2)`. That is exactly
//! the core's output (default DMG palette + `Ppu::cgb_color`), so the
//! references are compared through [`CgbColorMap::Identity`].
//!
//! Model matrix (4 cases): each shipped reference image is one rom×model
//! case, per docs/ARCHITECTURE.md §CGB revision policy ("cgb-acid2 /
//! acid-hell: single upstream reference (revision-agnostic); no skips"):
//!
//! * `dmg-acid2.gb` on [`Model::Dmg`] vs `dmg-acid2-dmg.png`, and on
//!   [`Model::Cgb`] vs `dmg-acid2-cgb.png` — the cart is DMG-flagged, so the
//!   CGB boot ROM drops into compatibility mode and the howto's compat-mode
//!   palette screenshot applies.
//! * `cgb-acid2.gbc` on [`Model::Cgb`] vs `cgb-acid2.png`.
//! * `cgb-acid-hell.gbc` on [`Model::Cgb`] vs `cgb-acid-hell.png`.

use std::path::Path;

use slopgb_core::Model;

use crate::common;
use crate::common::framecmp::CgbColorMap;
use crate::harness::{self, CaseResult, assert_against_baseline, case_key};

/// The three suite directories this module owns, collection-relative.
const SUITE_DIRS: [&str; 3] = ["dmg-acid2", "cgb-acid2", "cgb-acid-hell"];

/// One rom×model case with its reference screenshot (paths
/// collection-relative, forward slashes).
struct Case {
    rom: &'static str,
    model: Model,
    reference: &'static str,
}

const CASES: [Case; 4] = [
    Case {
        rom: "dmg-acid2/dmg-acid2.gb",
        model: Model::Dmg,
        reference: "dmg-acid2/dmg-acid2-dmg.png",
    },
    Case {
        rom: "dmg-acid2/dmg-acid2.gb",
        model: Model::Cgb,
        reference: "dmg-acid2/dmg-acid2-cgb.png",
    },
    Case {
        rom: "cgb-acid2/cgb-acid2.gbc",
        model: Model::Cgb,
        reference: "cgb-acid2/cgb-acid2.png",
    },
    Case {
        rom: "cgb-acid-hell/cgb-acid-hell.gbc",
        model: Model::Cgb,
        reference: "cgb-acid-hell/cgb-acid-hell.png",
    },
];

/// Known-failure baseline (see `harness::assert_against_baseline`).
///
/// * `dmg-acid2 [Cgb]`: the frame renders in plain greys (e.g. #A5A5A5,
///   6442 pixels off) instead of the CGB boot ROM's compat-mode palettes —
///   the howto requires BG #000000/#0063C6/#7BFF31/#FFFFFF and OBJ
///   #000000/#943939/#FF8484/#FFFFFF, i.e. boot-assigned palette RAM is
///   not yet emulated for DMG-flagged carts.
/// * `cgb-acid-hell [Cgb]`: 2 pixels swapped at (80,68)/(80,69)
///   (#FFFF00 vs #000000), a one-line PPU divergence.
const BASELINE: &[&str] = &[
    "dmg-acid2/dmg-acid2.gb [Cgb]",
    "cgb-acid-hell/cgb-acid-hell.gbc [Cgb]",
];

/// Run one acid case: to the `LD B,B` exit (the howtos give no run-time
/// figure, so the mooneye-style 120-emulated-second budget applies), then
/// compare the *next* completed frame against the reference screenshot.
fn run_case(root: &Path, case: &Case) -> Result<(), String> {
    let rom_path = root.join(case.rom);
    let rom = std::fs::read(&rom_path).map_err(|e| format!("read {}: {e}", rom_path.display()))?;
    let mut gb = harness::boot(&rom, case.model);
    harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)?;
    // The breakpoint fires mid-frame, while `gb.frame()` still holds the
    // previous completed frame; advancing one frame boundary renders the
    // finished test screen, which is stable from then on (the ROMs idle
    // after LD B,B).
    harness::run_for_frames(&mut gb, 1);
    harness::expect_frame_png(&gb, &root.join(case.reference), CgbColorMap::Identity)
}

/// The full 4-case acid matrix, ratcheted against [`BASELINE`].
#[test]
fn acid_frame_matrix() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "acid_frame_matrix",
            "test-roms/game-boy-test-roms-v7.0 not present",
        );
        return;
    };
    let results: Vec<CaseResult> = CASES
        .iter()
        .map(|case| CaseResult {
            key: case_key(case.rom, case.model),
            result: run_case(&root, case),
        })
        .collect();
    assert_against_baseline("acid", &results, BASELINE);
}

/// Inventory of every `.gb`/`.gbc` under this module's suite dirs:
/// `(claimed, exempted)` collection-relative forward-slash paths. All three
/// ROMs produce cases (see [`CASES`]); nothing is exempt.
#[allow(dead_code)] // consumed by the Phase B2 inventory guard
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let mut claimed: Vec<String> = CASES.iter().map(|c| c.rom.to_string()).collect();
    claimed.sort();
    claimed.dedup(); // dmg-acid2.gb appears in two cases
    (claimed, Vec::new())
}

/// `path` relative to `root` as a forward-slash string — the inventory key
/// format, independent of the host path separator.
fn rel_unix(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or_else(|_| panic!("{} not under {}", path.display(), root.display()))
        .components()
        .map(|c| c.as_os_str().to_str().expect("non-UTF-8 ROM path"))
        .collect::<Vec<_>>()
        .join("/")
}

/// Self-check ahead of the global Phase B2 guard: claimed and exempted are
/// disjoint, and together cover exactly the on-disk `.gb`/`.gbc` set of the
/// suite dirs.
#[test]
fn acid_inventory_is_disjoint_and_complete() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "acid_inventory_is_disjoint_and_complete",
            "test-roms/game-boy-test-roms-v7.0 not present",
        );
        return;
    };
    let (claimed, exempted) = inventory();
    for c in &claimed {
        assert!(!exempted.contains(c), "{c} both claimed and exempted");
    }
    let mut on_disk = Vec::new();
    for dir in SUITE_DIRS {
        let mut roms = Vec::new();
        common::collect_roms(&root.join(dir), true, &mut roms)
            .unwrap_or_else(|e| panic!("cannot enumerate {dir}: {e}"));
        on_disk.extend(roms.iter().map(|p| rel_unix(&root, p)));
    }
    on_disk.sort();
    let mut union: Vec<String> = claimed.iter().chain(&exempted).cloned().collect();
    union.sort();
    assert_eq!(
        union, on_disk,
        "inventory() does not cover the on-disk ROM set exactly"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_unix_joins_components_with_forward_slashes() {
        assert_eq!(
            rel_unix(Path::new("/a/b"), Path::new("/a/b/dmg-acid2/dmg-acid2.gb")),
            "dmg-acid2/dmg-acid2.gb"
        );
    }

    #[test]
    fn inventory_claims_three_roms_and_exempts_none() {
        let (claimed, exempted) = inventory();
        assert_eq!(
            claimed,
            [
                "cgb-acid-hell/cgb-acid-hell.gbc",
                "cgb-acid2/cgb-acid2.gbc",
                "dmg-acid2/dmg-acid2.gb",
            ]
        );
        assert!(exempted.is_empty());
    }
}
