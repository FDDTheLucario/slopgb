//! The CPU: register file plus the fetch/execute step. Every bus access counts
//! one cycle (`read8`/`write8`); internal operations count via `io`, so the
//! returned cycle total matches the SingleStepTests `cycles` array length. All
//! behaviour is derived from the WDC W65C816S datasheet and those vectors —
//! never from another emulator (clean-room).

use crate::{Bus, Regs, flag};

/// A 65C816 core over a generic [`Bus`]. Holds the register file and, while a
/// `step` runs, the cycle counter for that instruction.
#[derive(Clone, Debug)]
pub struct Cpu {
    /// Programmer-visible registers + mode flags.
    pub regs: Regs,
    /// Cycles accrued by the instruction currently executing (reset per `step`).
    cycles: u64,
    /// Cycle budget for the current instruction. Only the block-move ops
    /// (`MVN`/`MVP`), which loop, consult it: they yield once it is reached so a
    /// long move can be split across calls. Every other instruction ignores it.
    cap: u64,
    /// Set by `STP`; cleared only by RESET. `WAI` sets `waiting` until an
    /// interrupt. Both are inspected by the host loop.
    pub stopped: bool,
    /// Set by `WAI`; cleared when an interrupt (or, for the vectors, the next
    /// step) resumes the CPU.
    pub waiting: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// A CPU in the post-RESET state (registers per [`Regs::at_reset`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            regs: Regs::at_reset(),
            cycles: 0,
            cap: u64::MAX,
            stopped: false,
            waiting: false,
        }
    }

    /// Build a CPU around an explicit register state (used to seed test vectors).
    #[must_use]
    pub fn from_regs(regs: Regs) -> Self {
        Self {
            regs,
            cycles: 0,
            cap: u64::MAX,
            stopped: false,
            waiting: false,
        }
    }

    /// Fetch, decode and execute one instruction. Returns the cycle count. In
    /// emulation mode the stack high byte is pinned to `01` at both the start and
    /// the end of the instruction (datasheet: SH is hardwired to 01 when E=1);
    /// multi-byte pushes step `S` linearly in between, so a word push can address
    /// across the page-1 boundary before SH is re-pinned.
    pub fn step(&mut self, bus: &mut impl Bus) -> u64 {
        self.step_bounded(bus, u64::MAX)
    }

    /// Like [`step`](Self::step), but a block move (`MVN`/`MVP`) yields once it
    /// has spent `max_cycles`, leaving `PC` at the opcode so the next call
    /// resumes it. Every other instruction runs to completion regardless of the
    /// budget. Returns the cycles actually spent.
    pub fn step_bounded(&mut self, bus: &mut impl Bus, max_cycles: u64) -> u64 {
        self.cycles = 0;
        self.cap = max_cycles;
        self.pin_emulation_stack();
        let opcode = self.fetch8(bus);
        self.dispatch(opcode, bus);
        self.pin_emulation_stack();
        self.cycles
    }

    /// Deliver a hardware NMI between instructions (see
    /// [`nmi_sequence`](Self::nmi_sequence) for the datasheet semantics).
    /// Returns the cycles spent.
    pub fn nmi(&mut self, bus: &mut impl Bus) -> u64 {
        self.cycles = 0;
        self.cap = u64::MAX;
        self.pin_emulation_stack();
        self.nmi_sequence(bus);
        self.pin_emulation_stack();
        self.cycles
    }

    /// Whether the current instruction has reached its cycle budget (block moves
    /// only).
    pub(crate) fn cap_reached(&self) -> bool {
        self.cycles >= self.cap
    }

    /// Force the stack high byte to `01` while in emulation mode.
    fn pin_emulation_stack(&mut self) {
        if self.regs.e {
            self.regs.s = 0x0100 | (self.regs.s & 0x00FF);
        }
    }

    // --- bus + cycle primitives ---------------------------------------------

    /// Read one byte at a 24-bit address; one cycle.
    pub(crate) fn read8(&mut self, bus: &mut impl Bus, addr: u32) -> u8 {
        self.cycles += 1;
        bus.read(addr & 0x00FF_FFFF)
    }

    /// Write one byte at a 24-bit address; one cycle.
    pub(crate) fn write8(&mut self, bus: &mut impl Bus, addr: u32, value: u8) {
        self.cycles += 1;
        bus.write(addr & 0x00FF_FFFF, value);
    }

    /// An internal-operation cycle: advances the clock but touches no memory
    /// (the vectors record these with a `null` data value).
    pub(crate) fn io(&mut self) {
        self.cycles += 1;
    }

    // --- program-counter fetches --------------------------------------------

    /// Fetch a byte at `PBR:PC` and advance `PC` (wraps within the bank).
    pub(crate) fn fetch8(&mut self, bus: &mut impl Bus) -> u8 {
        let addr = ((self.regs.pbr as u32) << 16) | self.regs.pc as u32;
        let v = self.read8(bus, addr);
        self.regs.pc = self.regs.pc.wrapping_add(1);
        v
    }

    /// Fetch a little-endian 16-bit operand from the program stream.
    pub(crate) fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        lo | (hi << 8)
    }

    /// Fetch a little-endian 24-bit operand from the program stream.
    pub(crate) fn fetch24(&mut self, bus: &mut impl Bus) -> u32 {
        let lo = self.fetch8(bus) as u32;
        let mid = self.fetch8(bus) as u32;
        let hi = self.fetch8(bus) as u32;
        lo | (mid << 8) | (hi << 16)
    }

    // --- width-aware data access --------------------------------------------

    /// Read 8- or 16-bit data at `addr`. `bank0` keeps the high-byte address in
    /// bank 0 (direct-page and stack data wrap at `$FFFF`); otherwise the
    /// address increments through the full 24-bit space (absolute/long can cross
    /// a bank boundary).
    pub(crate) fn read_data(
        &mut self,
        bus: &mut impl Bus,
        addr: u32,
        wide: bool,
        bank0: bool,
    ) -> u16 {
        let lo = self.read8(bus, addr) as u16;
        if !wide {
            return lo;
        }
        let hi = self.read8(bus, next_addr(addr, bank0)) as u16;
        lo | (hi << 8)
    }

    /// Write 8- or 16-bit data at `addr` (low byte first), honouring `bank0`
    /// wrapping as [`read_data`].
    pub(crate) fn write_data(
        &mut self,
        bus: &mut impl Bus,
        addr: u32,
        value: u16,
        wide: bool,
        bank0: bool,
    ) {
        self.write8(bus, addr, value as u8);
        if wide {
            self.write8(bus, next_addr(addr, bank0), (value >> 8) as u8);
        }
    }

    /// Write back a read-modify-write result: high byte first (at `addr+1`),
    /// then the low byte, the reverse of a normal store (datasheet RMW timing,
    /// confirmed by the vectors' write order).
    pub(crate) fn write_data_rmw(
        &mut self,
        bus: &mut impl Bus,
        addr: u32,
        value: u16,
        wide: bool,
        bank0: bool,
    ) {
        if wide {
            self.write8(bus, next_addr(addr, bank0), (value >> 8) as u8);
        }
        self.write8(bus, addr, value as u8);
    }

    // --- flag helpers -------------------------------------------------------

    /// Set or clear a `P` flag.
    pub(crate) fn set_flag(&mut self, mask: u8, on: bool) {
        if on {
            self.regs.p |= mask;
        } else {
            self.regs.p &= !mask;
        }
    }

    /// Set N and Z from a result, inspecting only the low byte when narrow.
    pub(crate) fn set_nz(&mut self, value: u16, wide: bool) {
        let masked = if wide { value } else { value & 0x00FF };
        let sign = if wide { 0x8000 } else { 0x0080 };
        self.set_flag(flag::Z, masked == 0);
        self.set_flag(flag::N, masked & sign != 0);
    }
}

/// The address of the byte one past `addr`, wrapping in bank 0 when `bank0`.
fn next_addr(addr: u32, bank0: bool) -> u32 {
    if bank0 {
        (addr.wrapping_add(1)) & 0x0000_FFFF
    } else {
        (addr.wrapping_add(1)) & 0x00FF_FFFF
    }
}
