//! Frontend debugger execution control: break / resume / step / step-over over
//! the live machine, driving the emulation loop. The breakpoint-by-click UI
//! lands in a later increment; this is the keyboard-driven core (plan C6/C7).
//!
//! Stepping uses the core's own `step` (one instruction) and
//! `run_until_breakpoint` (run to a return address) — no test-only paths, so the
//! golden gate is untouched (it never breaks).

use std::collections::BTreeSet;

use slopgb_core::{DebugReg, GameBoy, Watchpoint, debug};

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

    /// Remove the breakpoint at `addr` if present (the manager's clear — an
    /// idempotent remove, so a stale list row can never re-add one).
    pub fn remove(&mut self, addr: u16) {
        self.pc.remove(&addr);
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

/// The debugger's memory watchpoints (RM8), App-owned beside [`Breakpoints`]
/// because the free-run loop consults them. Each "Set watchpoint" toggles a
/// **write** watchpoint at the cursor (bgb's common "break when this changes").
#[derive(Default, Clone, Debug)]
pub struct Watchpoints {
    items: Vec<Watchpoint>,
}

impl Watchpoints {
    /// Toggle a write watchpoint at `addr`; returns whether it is now set.
    pub fn toggle_write(&mut self, addr: u16) -> bool {
        if let Some(i) = self.items.iter().position(|w| w.addr == addr) {
            self.items.remove(i);
            false
        } else {
            self.items.push(Watchpoint {
                addr,
                read: false,
                write: true,
            });
            true
        }
    }

    /// The watchpoints, for [`GameBoy::set_watchpoints`] and the manager dialog.
    #[must_use]
    pub fn list(&self) -> &[Watchpoint] {
        &self.items
    }

    /// Remove the watchpoint at `addr` if present (the manager's idempotent clear).
    pub fn remove(&mut self, addr: u16) {
        self.items.retain(|w| w.addr != addr);
    }

    /// Whether no watchpoint is set (the free-run loop stays a plain `run_frame`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// A 16-bit CPU register pair the registers pane can edit ("edit register",
/// rc-registers.png). Maps to the core's [`DebugReg`]; the frontend keeps its
/// own copy so `windows`/`dbg` don't leak the core enum into hit-test results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegField {
    Af,
    Bc,
    De,
    Hl,
    Sp,
    Pc,
}

impl RegField {
    /// The core register this edits.
    #[must_use]
    pub fn to_core(self) -> DebugReg {
        match self {
            RegField::Af => DebugReg::Af,
            RegField::Bc => DebugReg::Bc,
            RegField::De => DebugReg::De,
            RegField::Hl => DebugReg::Hl,
            RegField::Sp => DebugReg::Sp,
            RegField::Pc => DebugReg::Pc,
        }
    }
}

/// An execution/state change a debugger click (menu item or pane click) asks
/// `main` to apply against the live machine — kept out of the windows layer so
/// every `&mut gb` mutation stays in `main`, where the golden gate is honored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugAction {
    /// Toggle a breakpoint at the address (F2 / `Set break`).
    ToggleBreakpoint(u16),
    /// Run until PC reaches the address (`Run to cursor`, F4).
    RunToCursor(u16),
    /// Redirect PC to the address without running (`Jump to cursor`, F6).
    SetPc(u16),
    /// Push the current PC and jump to the address (`Call cursor`).
    Call(u16),
    /// Write a register pair (`edit register`, RM11).
    SetReg(RegField, u16),
    /// Toggle a write watchpoint at the address (`Set watchpoint`, RM8).
    ToggleWatchpoint(u16),
    /// Remove the breakpoint at the address (the manager's clear, RM15).
    ClearBreakpoint(u16),
    /// Remove the watchpoint at the address (the manager's clear, RM15).
    ClearWatchpoint(u16),
}

/// Debugger run-state owned by the event loop. When `broken`, the paced loop
/// emulates zero frames so the LCD holds its last frame (bgb's "(debugging)").
#[derive(Default)]
pub struct Debugger {
    broken: bool,
    bps: Breakpoints,
    wps: Watchpoints,
}

/// Upper bound on instructions a single step-over runs before giving up, so a
/// runaway / never-returning subroutine can't hang the UI thread.
const STEP_OVER_CAP: u64 = 10_000_000;

/// Upper bound on instructions a `Run to cursor` runs before giving up, so a
/// cursor the PC never reaches can't hang the UI thread (~tens of seconds of
/// emulated time).
const RUN_TO_CURSOR_CAP: u64 = 100_000_000;

/// Upper bound on instructions a step-out runs before giving up, so a routine
/// that never returns (a main loop) can't hang the UI thread.
const STEP_OUT_CAP: u64 = 10_000_000;

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

    /// The watchpoint set (read — for the free-run arm check + the manager).
    #[must_use]
    pub fn watchpoints(&self) -> &Watchpoints {
        &self.wps
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
            DebugAction::SetPc(addr) => gb.debug_set_pc(addr),
            DebugAction::Call(addr) => gb.debug_call(addr),
            DebugAction::SetReg(field, value) => gb.debug_set_reg(field.to_core(), value),
            DebugAction::ToggleWatchpoint(addr) => {
                self.wps.toggle_write(addr);
                gb.set_watchpoints(self.wps.list());
            }
            DebugAction::ClearBreakpoint(addr) => self.bps.remove(addr),
            DebugAction::ClearWatchpoint(addr) => {
                self.wps.remove(addr);
                gb.set_watchpoints(self.wps.list());
            }
        }
    }

    /// Execute exactly one instruction (F7, "Trace" / step into).
    pub fn step(&self, gb: &mut GameBoy) {
        gb.step();
    }

    /// Step out of the current subroutine (F8): single-step until SP rises
    /// above its entry value — a `ret`/`reti` has popped the frame's return
    /// address past where we started — or the cap is hit (a routine that never
    /// returns, like a main loop, can't hang the UI thread). bgb's "Step out".
    pub fn step_out(&self, gb: &mut GameBoy) {
        let entry_sp = gb.cpu_regs().sp;
        for _ in 0..STEP_OUT_CAP {
            gb.step();
            // Wrap-safe "SP rose above entry": the signed 16-bit difference is
            // positive once a `ret`/`reti` has popped the frame past where we
            // started, even when the pop wraps the 0xFFFF→0x0000 boundary (a
            // plain `sp > entry_sp` would miss that return).
            if (gb.cpu_regs().sp.wrapping_sub(entry_sp) as i16) > 0 {
                break;
            }
        }
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
