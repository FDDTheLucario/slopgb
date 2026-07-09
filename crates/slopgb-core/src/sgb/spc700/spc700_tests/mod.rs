//! SPC700 test suite (compiled as `super::tests`).
//!
//! - [`ops`]     — per-opcode / per-flag unit tests, incl. the documented
//!   quirks (`DIV` overflow, `MUL`, `DAA`/`DAS`, `ADDW`/`SUBW` half-carry,
//!   `MOV1`/`NOT1` bit addressing).
//! - [`timers`]  — timer divider + read-and-clear behaviour.
//! - [`ipl`]     — reset, IPL ROM visibility, the `$AA`/`$BB` handshake, and
//!   the comm-port latch model.
//! - [`harness`] — an always-run 256-opcode smoke test plus the `#[ignore]`d
//!   `SingleStepTests/spc700` conformance harness (10⁶ hardware-traced cases).

use super::*;

mod harness;
mod ipl;
mod ops;
mod timers;

/// A CPU in flat-RAM mode (no I/O / IPL decode) with a clean 64 KB RAM — the
/// setup the unit tests and the `SingleStepTests` harness share.
fn cpu_flat() -> Spc700 {
    let mut s = Spc700::new();
    s.flat_mem = true;
    s.pc = 0x0200;
    s
}

/// Load `prog` at `$0200`, apply `setup`, execute one instruction, return the
/// CPU and the cycles the instruction consumed.
fn run1(prog: &[u8], setup: impl FnOnce(&mut Spc700)) -> (Spc700, u32) {
    let mut s = cpu_flat();
    for (i, b) in prog.iter().enumerate() {
        s.ram[0x0200 + i] = *b;
    }
    setup(&mut s);
    let cyc = s.step();
    (s, cyc)
}
