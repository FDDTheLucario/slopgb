//! Harness binary for the c-sp/game-boy-test-roms v7.0 collection
//! (`test-roms/game-boy-test-roms-v7.0/`, fetched by `test-roms/download.sh`).
//!
//! One module per suite; shared runners live in `gbtr/harness.rs`. Each
//! suite asserts its results against an explicit known-failure baseline
//! (`harness::assert_against_baseline`) so the build stays green while the
//! emulator is brought up to each suite's reference behavior — shrinking a
//! baseline is progress, growing one is a regression. Suites pick models
//! and reference images per docs/ARCHITECTURE.md §CGB revision policy.

// dead_code: `common` is shared with the mooneye binary, which consumes the
// parts this binary does not (run_group etc.).
#[allow(dead_code)]
#[path = "../common/mod.rs"]
mod common;

mod acid;
mod age;
mod blargg;
mod gambatte;
mod gbmicrotest;
mod harness;
mod inventory;
mod mealybug;
mod mooneye2022;
mod same_suite;
mod smallsuites;
mod wilbertpol;
