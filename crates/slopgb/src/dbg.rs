//! Frontend debugger execution control: break / resume / step / step-over over
//! the live machine, driving the emulation loop. The breakpoint-by-click UI
//! lands in a later increment; this is the keyboard-driven core (plan C6/C7).
//!
//! Stepping uses the core's own `step` (one instruction) and
//! `run_until_breakpoint` (run to a return address) — no test-only paths, so the
//! golden gate is untouched (it never breaks).

use slopgb_core::{GameBoy, debug};

/// Debugger run-state owned by the event loop. When `broken`, the paced loop
/// emulates zero frames so the LCD holds its last frame (bgb's "(debugging)").
#[derive(Default)]
pub struct Debugger {
    broken: bool,
}

/// Upper bound on instructions a single step-over runs before giving up, so a
/// runaway / never-returning subroutine can't hang the UI thread.
const STEP_OVER_CAP: u64 = 10_000_000;

impl Debugger {
    /// Whether emulation is currently frozen at a break.
    #[must_use]
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// Toggle between running and broken; returns the new broken state.
    pub fn toggle_break(&mut self) -> bool {
        self.broken = !self.broken;
        self.broken
    }

    /// Execute exactly one instruction (F7, "step into").
    pub fn step(&self, gb: &mut GameBoy) {
        gb.step();
    }

    /// Step over a `call`/`rst` by running to the instruction after it (F8);
    /// any other instruction is a single step. A not-taken conditional call
    /// falls through to that same address, so this is correct either way.
    pub fn step_over(&self, gb: &mut GameBoy) {
        let pc = gb.cpu_regs().pc;
        let op = gb.debug_read(pc);
        if is_subroutine_call(op) {
            let bytes = [
                op,
                gb.debug_read(pc.wrapping_add(1)),
                gb.debug_read(pc.wrapping_add(2)),
            ];
            let len = debug::decode(&bytes, pc).len.max(1);
            let ret = pc.wrapping_add(u16::from(len));
            gb.run_until_breakpoint(&[ret], STEP_OVER_CAP);
        } else {
            gb.step();
        }
    }
}

/// Whether `op` pushes a return address — the `call`/`rst` family step-over runs
/// through. CALL: `CD` and the four conditionals `C4/CC/D4/DC`; RST: the eight
/// `11_xxx_111` opcodes (`(op & 0xC7) == 0xC7`).
#[must_use]
pub fn is_subroutine_call(op: u8) -> bool {
    matches!(op, 0xCD | 0xC4 | 0xCC | 0xD4 | 0xDC) || (op & 0xC7) == 0xC7
}

#[cfg(test)]
#[path = "dbg_tests.rs"]
mod tests;
