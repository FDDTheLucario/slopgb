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
//! breakpoint protocol.

mod common;

#[test]
fn acceptance_root() {
    common::run_group("acceptance", false);
}

#[test]
fn acceptance_bits() {
    common::run_group("acceptance/bits", false);
}

#[test]
fn acceptance_instr() {
    common::run_group("acceptance/instr", false);
}

#[test]
fn acceptance_interrupts() {
    common::run_group("acceptance/interrupts", false);
}

#[test]
fn acceptance_oam_dma() {
    common::run_group("acceptance/oam_dma", false);
}

#[test]
fn acceptance_ppu() {
    common::run_group("acceptance/ppu", false);
}

#[test]
fn acceptance_serial() {
    common::run_group("acceptance/serial", false);
}

#[test]
fn acceptance_timer() {
    common::run_group("acceptance/timer", false);
}

#[test]
fn emulator_only_mbc1() {
    common::run_group("emulator-only/mbc1", false);
}

#[test]
fn emulator_only_mbc2() {
    common::run_group("emulator-only/mbc2", false);
}

#[test]
fn emulator_only_mbc5() {
    common::run_group("emulator-only/mbc5", false);
}

#[test]
fn misc() {
    common::run_group("misc", true);
}

#[test]
fn madness() {
    common::run_group("madness", false);
}

#[test]
fn sprite_priority() {
    common::run_sprite_priority();
}
