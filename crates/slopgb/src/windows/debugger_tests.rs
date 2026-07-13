//! Debugger window tests, split into category submodules to keep every file
//! under the size cap. Shared fixtures (`regs0`, `AREA`, `NOPS`) live here;
//! each category module pulls them via `use super::{...}` and reaches the code
//! under test via `use super::super::*`.

use super::*;

/// Register snapshot for `on_left_click`: the common PC=0x0100 / SP=0xFFFE the
/// pane tests use (other fields zero — only PC/SP drive disasm/stack clicks).
fn regs0() -> Registers {
    let mut r = Registers::default();
    r.pc = 0x0100;
    r.sp = 0xFFFE;
    r
}

/// The default debugger window size, partitioned the way `render_debugger` does.
const AREA: Rect = Rect::new(0, 0, 760, 560);
const NOPS: fn(u16) -> u8 = |_| 0x00; // every line a 1-byte nop

#[path = "debugger_tests/interaction.rs"]
mod interaction;
#[path = "debugger_tests/layout.rs"]
mod layout;
#[path = "debugger_tests/menubar.rs"]
mod menubar;
