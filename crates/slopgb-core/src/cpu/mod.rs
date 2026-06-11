//! SM83 CPU core: registers, decode/execute, interrupt dispatch.
//!
//! The CPU is the machine's clock master. Every memory access it performs is
//! one M-cycle; internal cycles with no bus access call [`Bus::tick`]. The
//! `Bus` implementation advances every peripheral by one M-cycle at the
//! *start* of each of these calls, then performs the access. The CPU itself
//! never counts time beyond issuing these calls.

mod execute;
mod registers;

pub use registers::{Flags, Registers};

use crate::model::Model;

/// One M-cycle granular view of the rest of the machine, as seen by the CPU.
///
/// Contract (see docs/ARCHITECTURE.md §Timing):
/// * Each of [`read`](Bus::read), [`write`](Bus::write) and
///   [`tick`](Bus::tick) advances the machine by exactly one M-cycle, then
///   performs the access (if any).
/// * [`pending`](Bus::pending) and [`ack`](Bus::ack) take no time.
pub trait Bus {
    /// One M-cycle ending in a memory read.
    fn read(&mut self, addr: u16) -> u8;
    /// One M-cycle ending in a memory write.
    fn write(&mut self, addr: u16, value: u8);
    /// One M-cycle with no memory access.
    fn tick(&mut self);
    /// `IF & IE & 0x1F` right now. Takes no time.
    fn pending(&self) -> u8;
    /// Clear bit `bit` (0..=4) of IF. Takes no time.
    fn ack(&mut self, bit: u8);
    /// CPU executed STOP: if a speed switch is armed (CGB KEY1.0), perform
    /// it and return true; otherwise enter stop mode semantics as the bus
    /// sees fit. Takes no time.
    fn stop(&mut self) -> bool;
}

/// SM83 CPU. Owns architectural registers, IME, halt state.
pub struct Cpu {
    regs: Registers,
    /// Interrupt master enable.
    ime: bool,
    /// EI executed, IME turns on after the *next* instruction.
    ime_pending: bool,
    halted: bool,
    /// Halt bug armed: next opcode fetch does not increment PC.
    halt_bug: bool,
    /// Set once `LD B,B` (0x40) executes — mooneye "test done" breakpoint.
    debug_breakpoint: bool,
}

impl Cpu {
    /// CPU with the post-boot register values of `model`.
    pub fn new(model: Model) -> Self {
        Self {
            regs: Registers::post_boot(model),
            ime: false,
            ime_pending: false,
            halted: false,
            halt_bug: false,
            debug_breakpoint: false,
        }
    }

    /// Run one instruction (including any interrupt dispatch that precedes
    /// it), or one M-cycle of halt.
    pub fn step(&mut self, bus: &mut impl Bus) {
        execute::step(self, bus);
    }

    pub fn regs(&self) -> Registers {
        self.regs
    }

    pub fn regs_mut(&mut self) -> &mut Registers {
        &mut self.regs
    }

    pub fn debug_breakpoint_hit(&self) -> bool {
        self.debug_breakpoint
    }
}
