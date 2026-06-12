//! same-suite — SameBoy's SameSuite ROMs (`same-suite/` in the collection).
//!
//! Protocol (same-suite/game-boy-test-roms-howto.md): every ROM executes
//! `LD B,B` when finished; pass ⇔ B,C,D,E,H,L = 3,5,8,13,21,34 — the
//! mooneye Fibonacci signature. Standard mooneye timeout applies
//! (120 emulated seconds, `common::TIMEOUT_TCYCLES`).
//!
//! Model routing is fixed by docs/ARCHITECTURE.md §CGB revision policy
//! ("same-suite" row, binding):
//!
//! * unsuffixed ROMs → [`Model::Cgb`] — the APU tests are CGB-E-verified
//!   (same-suite/apu/README.md "Results") and expected to pass on our
//!   glitch-free CGB-C core via the policy's no-PCM-glitch companion rule;
//! * `-cgb<revs>` names the CGB revision set a ROM passes on; `Model::Cgb`
//!   models exactly CPU CGB C, so the ROM runs iff `C` is in the set
//!   (`-cgb0BC` runs; `-cgb0`, `-cgb0B`, `-cgbB`, `-cgbDE` are
//!   revision-skips with a loud note — the policy's documented
//!   extra_length_clocking hole);
//! * `-A` → [`Model::Agb`];
//! * `sgb/` (the MLT_REQ command tests — MLT_REQ is a Super Game Boy
//!   command, Pan Docs "SGB Command MLT_REQ") → [`Model::Sgb`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use slopgb_core::Model;

use crate::common;
use crate::harness::{self, CaseResult};

/// Collection-relative directory of this suite.
const SUITE_DIR: &str = "same-suite";

/// Where one ROM runs, decided purely from its collection-relative path.
#[derive(Debug, PartialEq, Eq)]
enum Route<'a> {
    /// Run on this model (every same-suite ROM maps to exactly one model).
    Run(Model),
    /// ARCHITECTURE.md §CGB revision policy skip: the ROM's CGB revision
    /// set (carried here, e.g. `"0B"`) excludes C, the one revision
    /// `Model::Cgb` models.
    RevisionSkip(&'a str),
}

/// Parse a SameSuite CGB revision-set suffix: `cgb` followed by one or more
/// revision letters out of 0/A/B/C/D/E (the silicon revisions that exist,
/// gbhwdb). Returns the letter set, or `None` for non-revision tokens.
fn cgb_revision_set(sfx: &str) -> Option<&str> {
    let revs = sfx.strip_prefix("cgb")?;
    let valid = !revs.is_empty()
        && revs
            .chars()
            .all(|c| matches!(c, '0' | 'A' | 'B' | 'C' | 'D' | 'E'));
    valid.then_some(revs)
}

/// Model routing for one same-suite ROM (collection-relative, forward
/// slashes) per the policy table quoted in the module docs.
fn route(rel: &str) -> Route<'_> {
    let in_sgb_dir = rel
        .strip_prefix(SUITE_DIR)
        .is_some_and(|r| r.starts_with("/sgb/"));
    if in_sgb_dir {
        return Route::Run(Model::Sgb);
    }
    let stem = Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if let Some((_, sfx)) = stem.rsplit_once('-') {
        if sfx == "A" {
            return Route::Run(Model::Agb);
        }
        if let Some(revs) = cgb_revision_set(sfx) {
            // Model::Cgb models exactly CPU CGB C, so membership of C in
            // the suffix's revision set decides run-vs-skip.
            return if revs.contains('C') {
                Route::Run(Model::Cgb)
            } else {
                Route::RevisionSkip(revs)
            };
        }
    }
    Route::Run(Model::Cgb)
}

/// Every ROM of the suite as (absolute path, collection-relative key),
/// sorted (collect_roms sorts). An empty or unreadable directory panics:
/// that is a corrupt checkout, not a green suite.
fn suite_roms(root: &Path) -> Vec<(PathBuf, String)> {
    let dir = root.join(SUITE_DIR);
    let mut paths = Vec::new();
    common::collect_roms(&dir, true, &mut paths)
        .unwrap_or_else(|e| panic!("same-suite: cannot enumerate {}: {e}", dir.display()));
    assert!(
        !paths.is_empty(),
        "{} exists but contains no .gb/.gbc ROMs — corrupt checkout?",
        dir.display()
    );
    paths
        .into_iter()
        .map(|p| {
            let rel = harness::rel_unix(root, &p);
            (p, rel)
        })
        .collect()
}

/// Known-failure baseline (see `harness::assert_against_baseline`),
/// discovered by running the full matrix; shrinking it is progress,
/// growing it a regression. Every entry reaches `LD B,B` with the all-$42
/// fail registers (no timeouts). Of note: per ARCHITECTURE.md §CGB
/// revision policy, `channel_1_sweep_restart_2` is the first candidate for
/// the *permanent* documented expected-fail list — it passes only on real
/// CGB-E silicon; even SameBoy emulating CGB-E fails it (apu/README.md).
const BASELINE: &[&str] = &[
    // apu/channel_1: 4 of 19 claimed cases. The trigger/duty/alignment/
    // envelope/zombie/freq-change families pass via the SameBoy-style
    // countdown model (src/apu/; the -A variant passes through the same
    // `just_reloaded` + NRx4 freq-high glitch path); what remains is the
    // sweep micro-timing machinery (square_sweep_calculate_countdown /
    // restart-hold windows, SameBoy apu.c) and the CGB-C-suffixed
    // freq_change_timing variant.
    "same-suite/apu/channel_1/channel_1_freq_change_timing-cgb0BC.gb [Cgb]",
    "same-suite/apu/channel_1/channel_1_sweep.gb [Cgb]",
    "same-suite/apu/channel_1/channel_1_sweep_restart.gb [Cgb]",
    // Likely permanent (CGB-E-silicon-only pass; see doc comment above).
    "same-suite/apu/channel_1/channel_1_sweep_restart_2.gb [Cgb]",
    // apu/channel_2: none — all 14 claimed cases pass.
    // apu/channel_3: none — all 14 claimed cases pass, matching
    // apu/README.md ("CPU-CGB-C passes the channel 3 tests").
    // apu/channel_4: 2 of 12 claimed cases. The free-running-counter noise
    // model (src/apu/noise.rs) covers the alignment/restart family; what
    // remains needs SameBoy's NR43-write LFSR-corruption tables, which
    // upstream documents as revision- and unit-specific with
    // non-deterministic variants (apu.c nr43_write) — only the
    // deterministic paths are modelled.
    "same-suite/apu/channel_4/channel_4_align.gb [Cgb]",
    "same-suite/apu/channel_4/channel_4_freq_change.gb [Cgb]",
    // apu top level: none — the plain DIV-event tests and the *_10
    // variants (which phase-lock DIV == $10, i.e. the DIV-APU bit HIGH,
    // before each NR52 power-on / DIV write — they never touch KEY1) all
    // pass via the power-on DIV-event skip glitch (SameBoy GB_apu_init).
    // dma: 3 of 4 (gbc_dma_cont passes).
    "same-suite/dma/gdma_addr_mask.gb [Cgb]",
    "same-suite/dma/hdma_lcd_off.gb [Cgb]",
    "same-suite/dma/hdma_mode0.gb [Cgb]",
    // interrupt: 1 of 1.
    "same-suite/interrupt/ei_delay_halt.gb [Cgb]",
    // sgb: both MLT_REQ command tests (SGB packet/multiplayer protocol).
    "same-suite/sgb/command_mlt_req.gb [Sgb]",
    "same-suite/sgb/command_mlt_req_1_incrementing.gb [Sgb]",
];

/// Full breakpoint-protocol matrix: every routed rom×model case runs to
/// `LD B,B` (or the 120-emulated-second mooneye timeout) and is checked for
/// the Fibonacci signature; results are ratcheted against the baseline.
/// Revision-skips are announced loudly, never silently dropped.
#[test]
fn same_suite_breakpoint_matrix() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "same_suite_breakpoint_matrix",
            "test-roms/game-boy-test-roms-v7.0 not present",
        );
        return;
    };
    let mut results: Vec<CaseResult> = Vec::new();
    for (path, rel) in suite_roms(&root) {
        let model = match route(&rel) {
            Route::Run(model) => model,
            Route::RevisionSkip(revs) => {
                println!(
                    "note: {rel} skipped — passes only on CGB revision(s) {revs}, and \
                     Model::Cgb models CPU CGB C (ARCHITECTURE.md §CGB revision policy)"
                );
                continue;
            }
        };
        let rom = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("same-suite: cannot read {}: {e}", path.display()));
        // Catch per-case panics (a core regression mid-suite) so one broken
        // ROM cannot mask the other cases' results — same rationale as the
        // mooneye harness's run_group.
        let result = harness::catch_case(|| {
            let mut gb = harness::boot(&rom, model);
            harness::run_until_breakpoint(&mut gb, common::TIMEOUT_TCYCLES)
                .and_then(|()| harness::check_fib(&gb))
        });
        results.push(CaseResult {
            key: harness::case_key(&rel, model),
            result,
        });
    }
    harness::assert_against_baseline("same-suite", &results, BASELINE);
}

/// Phase B2 inventory: (claimed, exempted) collection-relative paths of
/// every `.gb`/`.gbc` under `same-suite/`. Claimed ROMs produce exactly one
/// rom×model case in `same_suite_breakpoint_matrix`; exempted ROMs are the
/// documented §CGB-revision-policy skips (their suffix revision set
/// excludes C — the policy's extra_length_clocking hole plus
/// `channel_1_freq_change_timing-cgbDE`).
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let Some(root) = common::gbtr_root() else {
        return (Vec::new(), Vec::new());
    };
    let mut claimed = Vec::new();
    let mut exempted = Vec::new();
    for (_, rel) in suite_roms(&root) {
        match route(&rel) {
            Route::Run(_) => claimed.push(rel),
            Route::RevisionSkip(_) => exempted.push(rel),
        }
    }
    (claimed, exempted)
}

/// Self-check ahead of the global Phase B2 guard: the inventory partitions
/// the on-disk ROM set exactly (claimed ∩ exempted = ∅, claimed ∪ exempted
/// = every `.gb`/`.gbc` under `same-suite/`), the exempt set is pinned to
/// the six documented revision-skips and the partition sizes to the v7.0
/// checkout, so a re-pinned collection or a routing change fails loudly.
#[test]
fn same_suite_inventory_partitions_disk_exactly() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "same_suite_inventory_partitions_disk_exactly",
            "test-roms/game-boy-test-roms-v7.0 not present",
        );
        return;
    };
    let (claimed, exempted) = inventory();
    let claimed_set: BTreeSet<&str> = claimed.iter().map(String::as_str).collect();
    let exempted_set: BTreeSet<&str> = exempted.iter().map(String::as_str).collect();
    assert_eq!(
        claimed_set.len(),
        claimed.len(),
        "duplicate claimed entries"
    );
    assert_eq!(
        exempted_set.len(),
        exempted.len(),
        "duplicate exempted entries"
    );
    let overlap: Vec<&&str> = claimed_set.intersection(&exempted_set).collect();
    assert!(overlap.is_empty(), "claimed ∩ exempted ≠ ∅: {overlap:?}");
    let on_disk: BTreeSet<String> = suite_roms(&root).into_iter().map(|(_, rel)| rel).collect();
    let union: BTreeSet<&str> = claimed_set.union(&exempted_set).copied().collect();
    let on_disk_refs: BTreeSet<&str> = on_disk.iter().map(String::as_str).collect();
    assert_eq!(
        union, on_disk_refs,
        "inventory does not cover the on-disk ROM set exactly"
    );
    // The exempt set is exactly the six on-disk revision-skips of the
    // module docs (no-C suffix sets; ARCHITECTURE.md §CGB revision policy,
    // same-suite row), in collect_roms walk order.
    assert_eq!(
        exempted,
        [
            "same-suite/apu/channel_1/channel_1_extra_length_clocking-cgb0B.gb",
            "same-suite/apu/channel_1/channel_1_freq_change_timing-cgbDE.gb",
            "same-suite/apu/channel_2/channel_2_extra_length_clocking-cgb0B.gb",
            "same-suite/apu/channel_3/channel_3_extra_length_clocking-cgb0.gb",
            "same-suite/apu/channel_3/channel_3_extra_length_clocking-cgbB.gb",
            "same-suite/apu/channel_4/channel_4_extra_length_clocking-cgb0B.gb",
        ],
        "exempt set drifted from the six documented revision-skips"
    );
    assert_eq!(on_disk.len(), 78, "same-suite ships 78 ROMs in v7.0");
    assert_eq!(claimed.len(), 72, "claimed-ROM count drift");
}

#[test]
fn same_suite_route_unsuffixed_is_cgb() {
    assert_eq!(
        route("same-suite/apu/channel_1/channel_1_align.gb"),
        Route::Run(Model::Cgb)
    );
    assert_eq!(
        route("same-suite/dma/hdma_mode0.gb"),
        Route::Run(Model::Cgb)
    );
}

#[test]
fn same_suite_route_underscored_names_are_not_suffixes() {
    // Underscore-separated numerals must not be mistaken for suffixes.
    assert_eq!(
        route("same-suite/apu/channel_4/channel_4_lfsr_15_7.gb"),
        Route::Run(Model::Cgb)
    );
    assert_eq!(
        route("same-suite/apu/div_write_trigger_10.gb"),
        Route::Run(Model::Cgb)
    );
}

#[test]
fn same_suite_route_revision_set_with_c_runs_on_cgb() {
    assert_eq!(
        route("same-suite/apu/channel_1/channel_1_freq_change_timing-cgb0BC.gb"),
        Route::Run(Model::Cgb)
    );
}

#[test]
fn same_suite_route_revision_sets_without_c_are_skips() {
    // The on-disk no-C suffix set; each carries its revision letters for the
    // skip note.
    assert_eq!(
        route("same-suite/apu/channel_3/channel_3_extra_length_clocking-cgb0.gb"),
        Route::RevisionSkip("0")
    );
    assert_eq!(
        route("same-suite/apu/channel_1/channel_1_extra_length_clocking-cgb0B.gb"),
        Route::RevisionSkip("0B")
    );
    assert_eq!(
        route("same-suite/apu/channel_3/channel_3_extra_length_clocking-cgbB.gb"),
        Route::RevisionSkip("B")
    );
    assert_eq!(
        route("same-suite/apu/channel_1/channel_1_freq_change_timing-cgbDE.gb"),
        Route::RevisionSkip("DE")
    );
}

#[test]
fn same_suite_route_a_suffix_is_agb() {
    assert_eq!(
        route("same-suite/apu/channel_1/channel_1_freq_change_timing-A.gb"),
        Route::Run(Model::Agb)
    );
}

#[test]
fn same_suite_route_sgb_dir_is_sgb() {
    assert_eq!(
        route("same-suite/sgb/command_mlt_req.gb"),
        Route::Run(Model::Sgb)
    );
    assert_eq!(
        route("same-suite/sgb/command_mlt_req_1_incrementing.gb"),
        Route::Run(Model::Sgb)
    );
}

#[test]
fn same_suite_route_non_revision_dash_token_defaults_to_cgb() {
    // A dash token that is not a revision suffix (hypothetical) is part of
    // the name, not routing information.
    assert_eq!(route("same-suite/apu/some-test.gb"), Route::Run(Model::Cgb));
    // "cgb" with no letters or with letters outside 0/A/B/C/D/E is not a
    // revision set either.
    assert_eq!(cgb_revision_set("cgb"), None);
    assert_eq!(cgb_revision_set("cgbX"), None);
    assert_eq!(cgb_revision_set("agb"), None);
}
