//! Mooneye test-suite integration harness.
//!
//! Discovers ROMs under `<repo>/test-roms/mts-*/` at runtime (prints a skip
//! notice and passes when the ROMs are not checked out — unless
//! `SLOPGB_REQUIRE_ROMS=1`, as in CI, which makes that a hard failure), maps
//! each ROM to the hardware models it must pass on (filename suffix rules —
//! see `common/mod.rs`), runs it through the `LD B,B`/Fibonacci breakpoint
//! protocol, and reports every failing `rom [model]` combination of a group
//! at once.
//!
//! One `#[test]` per directory group so the groups parallelize. `utils/` is
//! not a test category, and `manual-only/sprite_priority` plus
//! `madness/mgb_oam_dma_halt_sprites` (which halts forever and never
//! executes `LD B,B`) are verified by frame comparison against the suite's
//! reference images instead of the breakpoint protocol.
//! `every_release_rom_is_harnessed` guards the group list against drift:
//! `common::mts_root()` picks the *newest* release at runtime, so a future
//! release could otherwise add ROMs no group runs.

mod common;

use std::path::Path;

/// Every directory group harnessed below, as `(dir, recursive)`. Single
/// source of truth shared by the per-group `#[test]`s (via [`run_group`],
/// which refuses unlisted directories) and the coverage test, so the two
/// cannot drift apart.
const GROUPS: &[(&str, bool)] = &[
    ("acceptance", false),
    ("acceptance/bits", false),
    ("acceptance/instr", false),
    ("acceptance/interrupts", false),
    ("acceptance/oam_dma", false),
    ("acceptance/ppu", false),
    ("acceptance/serial", false),
    ("acceptance/timer", false),
    ("emulator-only/mbc1", false),
    ("emulator-only/mbc2", false),
    ("emulator-only/mbc5", false),
    ("misc", true),
];

/// Run one group from [`GROUPS`]; panics if `dir` is not listed there.
fn run_group(dir: &str) {
    let &(_, recursive) = GROUPS
        .iter()
        .find(|&&(d, _)| d == dir)
        .unwrap_or_else(|| panic!("{dir} is not listed in GROUPS"));
    common::run_group(dir, recursive);
}

/// Is this ROM (path relative to the mts root) executed by some test in this
/// binary, or explicitly exempt?
fn is_harnessed(rel: &Path) -> bool {
    // Exempt: utils/ holds helper ROMs (e.g. the boot-ROM dumper), not
    // pass/fail tests.
    if rel.starts_with("utils") {
        return true;
    }
    // Covered by `sprite_priority` / `madness` below via frame comparison
    // rather than a breakpoint-protocol group (the madness ROM halts
    // forever and never executes `LD B,B`).
    if rel == Path::new("manual-only/sprite_priority.gb")
        || rel == Path::new("madness/mgb_oam_dma_halt_sprites.gb")
    {
        return true;
    }
    GROUPS.iter().any(|&(dir, recursive)| {
        let dir = Path::new(dir);
        if recursive {
            rel.starts_with(dir)
        } else {
            rel.parent() == Some(dir)
        }
    })
}

/// Completeness guard: every `.gb`/`.gbc` in the discovered release must be
/// claimed by a harnessed group or an explicit exemption. Catches ROMs in
/// unharnessed places (e.g. directly under `emulator-only/`, where only the
/// mbc1/mbc2/mbc5 subdirectories have groups) and directories added by
/// future releases.
#[test]
fn every_release_rom_is_harnessed() {
    let Some(root) = common::mts_root() else {
        common::skip_or_fail(
            "coverage guard",
            "no mooneye ROMs under <repo>/test-roms/mts-*",
        );
        return;
    };
    let mut roms = Vec::new();
    common::collect_roms(&root, true, &mut roms)
        .unwrap_or_else(|e| panic!("cannot enumerate ROMs under {}: {e}", root.display()));
    assert!(
        !roms.is_empty(),
        "{} exists but contains no .gb/.gbc ROMs — corrupt checkout?",
        root.display()
    );
    let unclaimed: Vec<String> = roms
        .iter()
        .map(|p| p.strip_prefix(&root).expect("collected ROM under mts root"))
        .filter(|rel| !is_harnessed(rel))
        .map(|rel| rel.display().to_string())
        .collect();
    assert!(
        unclaimed.is_empty(),
        "ROMs in the release are not run by any test group (extend GROUPS or \
         the exemptions in is_harnessed):\n  {}",
        unclaimed.join("\n  ")
    );
}

/// Generate one `#[test]` wrapper per directory group. Each `$dir` must be
/// listed in [`GROUPS`] — [`run_group`] panics on unlisted directories.
macro_rules! group_tests {
    ($($name:ident => $dir:literal),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_group($dir);
            }
        )*
    };
}

group_tests! {
    acceptance_root => "acceptance",
    acceptance_bits => "acceptance/bits",
    acceptance_instr => "acceptance/instr",
    acceptance_interrupts => "acceptance/interrupts",
    acceptance_oam_dma => "acceptance/oam_dma",
    acceptance_ppu => "acceptance/ppu",
    acceptance_serial => "acceptance/serial",
    acceptance_timer => "acceptance/timer",
    emulator_only_mbc1 => "emulator-only/mbc1",
    emulator_only_mbc2 => "emulator-only/mbc2",
    emulator_only_mbc5 => "emulator-only/mbc5",
    misc => "misc",
}

#[test]
fn madness() {
    common::run_madness();
}

#[test]
fn sprite_priority() {
    common::run_sprite_priority();
}
