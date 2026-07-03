//! SM83 CPU core: registers, decode/execute, interrupt dispatch.
//!
//! The CPU is the machine's clock master. Every memory access it performs is
//! one M-cycle; internal cycles with no bus access call [`Bus::tick`]. The
//! `Bus` implementation advances every peripheral by one M-cycle at the
//! *start* of each of these calls, then performs the access. The CPU itself
//! never counts time beyond issuing these calls.

mod execute;
mod registers;

pub use registers::{Registers, flags};

use crate::model::Model;

/// One M-cycle granular view of the rest of the machine, as seen by the CPU.
///
/// Contract (see docs/ARCHITECTURE.md §Timing):
/// * Each of [`read`](Bus::read), [`write`](Bus::write), [`tick`](Bus::tick)
///   and their OAM-bug-carrying variants [`tick_addr`](Bus::tick_addr) /
///   [`read_inc`](Bus::read_inc) advances the machine by exactly one
///   M-cycle, then performs the access (if any).
/// * The access part of [`read`](Bus::read) must have no side effects
///   beyond the DMG OAM corruption bug (any $FE00-$FEFF access during the
///   mode-2 OAM scan corrupts OAM — Pan Docs "OAM Corruption Bug");
///   otherwise a read may differ from [`tick`](Bus::tick) only in the
///   value it returns. The halted CPU issues a discarded prefetch read of
///   PC every idle M-cycle to model its NOP-loop-equivalent wake timing
///   (see `execute::step`), even though the halted CPU performs no bus
///   accesses on hardware; the interconnect therefore suppresses the OAM
///   bug while the core clock is gated off (see
///   `Interconnect::set_cpu_halted`), keeping those phantom reads
///   side-effect-free.
/// * [`pending`](Bus::pending) and [`ack`](Bus::ack) take no time.
pub trait Bus {
    /// One M-cycle ending in a memory read.
    fn read(&mut self, addr: u16) -> u8;
    /// One M-cycle ending in a memory write.
    fn write(&mut self, addr: u16, value: u8);
    /// One M-cycle with no memory access.
    fn tick(&mut self);
    /// One M-cycle with no memory access, but with `value` — a 16-bit
    /// register the SM83's 16-bit increment/decrement unit operates on
    /// this cycle (INC rr/DEC rr, the PUSH/CALL/RST pre-push cycle's SP,
    /// LD SP,HL's HL) — driven onto the address bus. A value in
    /// $FE00-$FEFF during a DMG-family mode-2 OAM scan triggers the OAM
    /// corruption bug's write pattern (Pan Docs "OAM Corruption Bug");
    /// otherwise identical to [`tick`](Bus::tick), which is the default.
    fn tick_addr(&mut self, _value: u16) {
        self.tick();
    }
    /// One M-cycle ending in a memory read whose address register is
    /// incremented/decremented by the 16-bit inc/dec unit in the same
    /// cycle (POP/RET reads via SP, LD A,(HL+)/(HL-)). A $FE00-$FEFF
    /// address during a DMG-family mode-2 OAM scan triggers the OAM
    /// corruption bug's "read during increase" pattern instead of the
    /// plain read pattern; otherwise identical to [`read`](Bus::read),
    /// which is the default.
    fn read_inc(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }
    /// `IF & IE & 0x1F` right now. Takes no time.
    fn pending(&self) -> u8;
    /// `IF & IE & 0x1F` as seen by the halted CPU's wake check (both the
    /// IME=1 dispatch and the IME=0 resume). Takes no time; defaults to
    /// [`pending`](Bus::pending).
    ///
    /// The SM83's halt-exit logic samples IE & IF earlier *within* the
    /// M-cycle than the end-of-cycle view [`pending`](Bus::pending)
    /// models (SameBoy sm83_cpu.c, `GB_cpu_run`: mid-cycle on DMG,
    /// start-of-cycle on CGB): an IF bit committed after that point — in
    /// practice the timer reload's IF, which lands on the last T-substep —
    /// is missed until the next cycle, waking the CPU one M-cycle later
    /// than a running-CPU dispatch would (gambatte tima/tc*_irq_*;
    /// wilbertpol acceptance/timer/timer_if rounds 5/6 vs 3/4). The
    /// running CPU's end-of-fetch sampling keeps using
    /// [`pending`](Bus::pending) — that contract is frozen.
    fn pending_halt_wake(&self) -> u8 {
        self.pending()
    }
    /// PORT 2 (#11bc, the sub-M-cycle WAKE clock) — the halt loop's wake
    /// sample, allowed to advance the machine to its true sample T first.
    /// SameBoy's DMG halt loop advances 2 T, samples `interrupt_queue`,
    /// then advances the remaining 2 (`GB_cpu_run`, `sm83_cpu.c:1621-1628`),
    /// so the halt-exit check runs on a HALF-M-cycle grid and a wake resumes
    /// the CPU — and its whole dispatch + handler read stream — at that
    /// sub-M-cycle T (the deferred clock keeps the 2-T offset until the
    /// machine re-aligns). The default (production, and every
    /// non-interconnect test bus) is the plain end-sampled
    /// [`pending_halt_wake`](Bus::pending_halt_wake); the interconnect
    /// overrides it on the tier2 deferred path for the DMG family.
    fn pending_halt_wake_mid(&mut self) -> u8 {
        self.pending_halt_wake()
    }
    /// #11bf — `IF & IE & 0x1F` as seen by HALT's own entry decision (the
    /// halt-bug / no-halt arm). SameBoy's `halt()` performs the prefetch
    /// `cycle_read` (advancing the machine through the HALT opcode-fetch
    /// M-cycle) and then checks IE & IF, so the entry decision observes the
    /// machine at the fetch's END (t0+4), one M-cycle past the deferred
    /// leading-edge view `pending()` gives (sm83_cpu.c:1036-1058). The
    /// default (production, non-interconnect buses) keeps `pending()`; the
    /// interconnect overrides it on the tier2 deferred path.
    fn pending_halt_entry(&mut self) -> u8 {
        self.pending()
    }
    /// #11bf — `IF & IE & 0x1F` as seen by the running CPU's end-of-fetch
    /// dispatch check. SameBoy's `cycle_read` advances the machine through
    /// the opcode-fetch M-cycle before `GB_cpu_run`'s interrupt check reads
    /// IF, so a rise landing INSIDE the fetch M-cycle still dispatches at
    /// that boundary; the deferred leading-edge `pending()` view is one
    /// M-cycle stale there. The default (production, non-interconnect
    /// buses) keeps `pending()`; the interconnect overrides it on the tier2
    /// deferred path.
    fn pending_dispatch(&mut self) -> u8 {
        self.pending()
    }
    /// Clear bit `bit` (0..=4) of IF. Takes no time.
    fn ack(&mut self, bit: u8);
    /// CPU executed STOP: if a speed switch is armed (CGB KEY1.0), perform
    /// it and return true; otherwise return false and the CPU enters stop
    /// mode, sleeping until [`pending`](Bus::pending) becomes non-zero
    /// (joypad wake).
    ///
    /// Takes *time*: the bus runs STOP's whole tail so it can sequence the
    /// machine cycles against the speed toggle. With `interrupt_pending`
    /// false (the caller's end-of-fetch [`pending`](Bus::pending) sample)
    /// the skipped byte at `skipped_addr` costs one real read M-cycle
    /// (SameBoy sm83_cpu.c `stop()`: `cycle_read(gb, gb->pc++)` happens
    /// only without a pending interrupt), and an armed switch then pauses
    /// the CPU for ~0x8000 M-cycles on the new clock while the rest of the
    /// machine keeps running (gambatte-core memory.cpp `Memory::stop`:
    /// `intreq_.setEventTime<intevent_unhalt>(cc + 0x20000 + 4)`). With
    /// `interrupt_pending` true STOP stays a 1-byte opcode and an armed
    /// switch happens instantly with no pause (SameBoy gates both on
    /// `!interrupt_pending`; age caution/spsw-interrupts).
    fn stop(&mut self, skipped_addr: u16, interrupt_pending: bool) -> bool;
    /// Halt/stop mode gated the CPU core clock off (`true`) or the CPU woke
    /// up (`false`). The OAM DMA controller runs on that clock and freezes
    /// with it (madness/mgb_oam_dma_halt_sprites.s; see
    /// `Interconnect::set_cpu_halted`). The CPU engages the gate only
    /// *after* the post-HALT prefetch M-cycle (see `execute::step`). Takes
    /// no time; calls are idempotent. Defaults to a no-op for `Bus`
    /// implementations that do not model the DMA engine.
    fn set_halted(&mut self, _halted: bool) {}

    /// Whether the port Stage-B Tier-2 dispatch reclock is active. When `true`,
    /// the interrupt dispatch latches the IF-ack / vector AFTER the low push
    /// (SameBoy `sm83_cpu.c:1690`, the M5+2 latch) and calls
    /// [`dispatch_retime`](Bus::dispatch_retime) there; when `false` (the
    /// default, and production) the dispatch acks before the low push exactly
    /// as before — byte-identical. Takes no time.
    fn dispatch_reclock(&self) -> bool {
        false
    }
    /// Port Stage B (Tier 2): the interrupt-dispatch vector retime
    /// (`sm83_cpu.c:1690` `pending -= 2; flush; pending = 2`) — re-parks the
    /// clock so the vector fetch + first handler reads sample 2 dots early, and
    /// advances the deferred machine by the 2 T it commits. Called only when
    /// [`dispatch_reclock`](Bus::dispatch_reclock) is true, after the low push
    /// commits (`pending == 4 > 2`). Defaults to a no-op.
    fn dispatch_retime(&mut self) {}
    /// Instruction boundary: drain the deferred-commit clock's parked debt
    /// (SameBoy `flush_pending_cycles`, `sm83_cpu.c:336`). Called exactly
    /// once per [`super::step`] invocation, after the instruction (or idle /
    /// dispatch step) completes, so the next instruction begins at a clean
    /// cc+0. Takes no time. Inert in port Stage S1 — the clock it drains is
    /// write-only scaffold that nothing samples yet; it becomes load-bearing
    /// at S2 (leading-edge reads). Defaults to a no-op for `Bus`
    /// implementations without a cycle clock.
    fn flush_pending(&mut self) {}
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

    /// True once the CPU has executed an undefined opcode and entered the
    /// permanent hard-lock (gbctr "undefined opcodes": the CPU hangs and
    /// interrupts do not wake it). Harness hook — wilbertpol's mooneye
    /// fork ends its tests with 0xED.
    pub fn debug_undefined_hit(&self) -> bool {
        self.locked
    }
}
