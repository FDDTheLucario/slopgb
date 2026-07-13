//! Window / render pixel-leg pinned-behavior tests.
//!
//! Split by clock family into cohesive submodules (no-god-files cap):
//! `tier2` (the `tier2_*` reclock window tests) + `eager` (the `eager_*`
//! clock re-host window tests).

#[path = "window/eager.rs"]
mod eager;
#[path = "window/tier2.rs"]
mod tier2;

// Flag-on read-trace probe module; `#[ignore]`'d so it never runs in the gate.
