//! Mooneye pass/fail protocol constants, defined once and shared between the
//! integration harness (`tests/common/mod.rs`, via `mod protocol`) and the
//! `run_mooneye` dev example (`examples/run_mooneye.rs`, via a `#[path]`
//! include) so the two cannot drift apart.

/// The Fibonacci register signature of a passing test (B,C,D,E,H,L);
/// test-roms-src/README.markdown, "Pass/fail reporting".
pub const FIB: [u8; 6] = [3, 5, 8, 13, 21, 34];

/// 120 emulated seconds; a test that has not hit `LD B,B` by then has hung.
pub const TIMEOUT_TCYCLES: u64 = 120 * 4_194_304;
