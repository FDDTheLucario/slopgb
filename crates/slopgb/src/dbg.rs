//! Frontend debugger execution control: break / resume / step / step-over over
//! the live machine, driving the emulation loop. The breakpoint-by-click UI
//! lands in a later increment; this is the keyboard-driven core (plan C6/C7).
//!
//! Stepping uses the core's own `step` (one instruction) and
//! `run_until_breakpoint` (run to a return address) — no test-only paths, so the
//! golden gate is untouched (it never breaks).

use std::collections::BTreeSet;

use slopgb_core::{GameBoy, debug};

/// The set of PC breakpoints the free-run loop halts on. Lives in the
/// App-owned [`Debugger`] (not the per-window view state) because both the key
/// handler and the run loop consult it, and a breakpoint is a property of the
/// machine, not of one debugger window. Watchpoints (RM8) extend this later.
#[derive(Default, Clone, Debug)]
pub struct Breakpoints {
    pc: BTreeSet<u16>,
}

impl Breakpoints {
    /// Toggle a breakpoint at `addr`; returns whether it is now set.
    pub fn toggle(&mut self, addr: u16) -> bool {
        if self.pc.remove(&addr) {
            false
        } else {
            self.pc.insert(addr);
            true
        }
    }

    /// Whether a breakpoint is set at `addr` (the disasm gutter dot).
    #[must_use]
    pub fn contains(&self, addr: u16) -> bool {
        self.pc.contains(&addr)
    }

    /// The breakpoint addresses, for [`GameBoy::run_frame_until_breakpoint`].
    #[must_use]
    pub fn pc_list(&self) -> Vec<u16> {
        self.pc.iter().copied().collect()
    }

    /// Whether no breakpoint is set (the free-run loop stays a plain `run_frame`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pc.is_empty()
    }
}

/// An execution/state change a debugger click (menu item or pane click) asks
/// `main` to apply against the live machine — kept out of the windows layer so
/// every `&mut gb` mutation stays in `main`, where the golden gate is honored.
/// `Jump to cursor` / `Call cursor` need a core PC/SP-write accessor (lands with
/// RM11) — those menu items are greyed until then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugAction {
    /// Toggle a breakpoint at the address (F2 / `Set break`).
    ToggleBreakpoint(u16),
    /// Run until PC reaches the address (`Run to cursor`, F4).
    RunToCursor(u16),
}

/// Debugger run-state owned by the event loop. When `broken`, the paced loop
/// emulates zero frames so the LCD holds its last frame (bgb's "(debugging)").
#[derive(Default)]
pub struct Debugger {
    broken: bool,
    bps: Breakpoints,
}

/// Upper bound on instructions a single step-over runs before giving up, so a
/// runaway / never-returning subroutine can't hang the UI thread.
const STEP_OVER_CAP: u64 = 10_000_000;

/// Upper bound on instructions a `Run to cursor` runs before giving up, so a
/// cursor the PC never reaches can't hang the UI thread (~tens of seconds of
/// emulated time).
const RUN_TO_CURSOR_CAP: u64 = 100_000_000;

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

    /// Force the broken flag (the free-run loop sets it on a breakpoint hit).
    pub fn set_broken(&mut self, broken: bool) {
        self.broken = broken;
    }

    /// The breakpoint set (read — for the gutter dots + the run list).
    #[must_use]
    pub fn breakpoints(&self) -> &Breakpoints {
        &self.bps
    }

    /// Apply a [`DebugAction`] from a menu item or pane click against the live
    /// machine. `Run to cursor` halts at the cursor afterward (bgb's behavior).
    pub fn apply(&mut self, gb: &mut GameBoy, action: DebugAction) {
        match action {
            DebugAction::ToggleBreakpoint(addr) => {
                self.bps.toggle(addr);
            }
            DebugAction::RunToCursor(addr) => {
                gb.run_until_breakpoint(&[addr], RUN_TO_CURSOR_CAP);
                self.broken = true;
            }
        }
    }

    /// Execute exactly one instruction (F7, "Trace" / step into).
    pub fn step(&self, gb: &mut GameBoy) {
        gb.step();
    }

    /// Step over a `call`/`rst` by running to the instruction after it (F3);
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
