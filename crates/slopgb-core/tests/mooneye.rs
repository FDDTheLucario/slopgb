//! Mooneye test-suite integration harness.
//!
//! Discovers ROMs under `<repo>/test-roms/mts-*/` at runtime (prints a skip
//! notice and passes when the ROMs are not checked out), maps each ROM to the
//! hardware models it must pass on (filename suffix rules — see
//! `common/mod.rs`), runs it through the `LD B,B`/Fibonacci breakpoint
//! protocol, and reports every failing `rom [model]` combination of a group
//! at once.
//!
//! One `#[test]` per directory group so the groups parallelize. `utils/` is
//! not a test category and `manual-only/sprite_priority` is verified by
//! frame comparison against the suite's reference image instead of the
//! breakpoint protocol. `every_release_rom_is_harnessed` guards the group
//! list against drift: `common::mts_root()` picks the *newest* release at
//! runtime, so a future release could otherwise add ROMs no group runs.

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
    ("madness", false),
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
    // Covered by `sprite_priority` below via frame comparison rather than a
    // breakpoint-protocol group.
    if rel == Path::new("manual-only/sprite_priority.gb") {
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
        println!("skipping coverage guard: no mooneye ROMs under <repo>/test-roms/mts-*");
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

#[test]
fn acceptance_root() {
    run_group("acceptance");
}

#[test]
fn acceptance_bits() {
    run_group("acceptance/bits");
}

#[test]
fn acceptance_instr() {
    run_group("acceptance/instr");
}

#[test]
fn acceptance_interrupts() {
    run_group("acceptance/interrupts");
}

#[test]
fn acceptance_oam_dma() {
    run_group("acceptance/oam_dma");
}

#[test]
fn acceptance_ppu() {
    run_group("acceptance/ppu");
}

#[test]
fn acceptance_serial() {
    run_group("acceptance/serial");
}

#[test]
fn acceptance_timer() {
    run_group("acceptance/timer");
}

#[test]
fn emulator_only_mbc1() {
    run_group("emulator-only/mbc1");
}

#[test]
fn emulator_only_mbc2() {
    run_group("emulator-only/mbc2");
}

#[test]
fn emulator_only_mbc5() {
    run_group("emulator-only/mbc5");
}

#[test]
fn misc() {
    run_group("misc");
}

#[test]
fn madness() {
    run_group("madness");
}

#[test]
fn sprite_priority() {
    common::run_sprite_priority();
}
