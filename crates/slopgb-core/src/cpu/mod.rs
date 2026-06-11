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
/// * The access part of [`read`](Bus::read) must have no side effects — a
///   read may differ from [`tick`](Bus::tick) only in the value it returns.
///   The halted CPU issues a discarded prefetch read of PC every idle
///   M-cycle to model its NOP-loop-equivalent wake timing (see
///   `execute::step`), even though the halted CPU performs no bus accesses
///   on hardware; a side-effecting read would turn those into phantom
///   accesses.
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
    /// it and return true; otherwise return false and the CPU enters stop
    /// mode, sleeping until [`pending`](Bus::pending) becomes non-zero
    /// (joypad wake). Takes no time.
    fn stop(&mut self) -> bool;
    /// Halt/stop mode gated the CPU core clock off (`true`) or the CPU woke
    /// up (`false`). The OAM DMA controller runs on that clock and freezes
    /// with it (madness/mgb_oam_dma_halt_sprites.s; see
    /// `Interconnect::set_cpu_halted`). The CPU engages the gate only
    /// *after* the post-HALT prefetch M-cycle (see `execute::step`). Takes
    /// no time; calls are idempotent. Defaults to a no-op for `Bus`
    /// implementations that do not model the DMA engine.
    fn set_halted(&mut self, _halted: bool) {}
}

/// SM83 CPU. Owns architectural registers, IME, halt state.
pub struct Cpu {
    regs: Registers,
    /// Interrupt master enable.
    ime: bool,
    /// EI executed, IME turns on after the *next* instruction.
    ime_pending: bool,
    halted: bool,
    /// STOP executed without an armed speed switch: the CPU sleeps,
    /// consuming tick cycles until the joypad wakes it (modelled as a
    /// pending interrupt; see `execute::step`).
    stopped: bool,
    /// Halt bug armed: next opcode fetch does not increment PC.
    halt_bug: bool,
    /// Set once `LD B,B` (0x40) executes — mooneye "test done" breakpoint.
    debug_breakpoint: bool,
    /// CPU fetched an illegal opcode and is permanently locked up,
    /// consuming tick cycles forever.
    locked: bool,
}

impl Cpu {
    /// CPU with the post-boot register values of `model`.
    pub fn new(model: Model) -> Self {
        Self {
            regs: Registers::post_boot(model),
            ime: false,
            ime_pending: false,
            halted: false,
            stopped: false,
            halt_bug: false,
            debug_breakpoint: false,
            locked: false,
        }
    }

    /// Run one instruction (including any interrupt dispatch that precedes
    /// it), one idle M-cycle of halt or stop mode, or a halt wake (the
    /// waking cycle plus dispatch and/or the next instruction).
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
