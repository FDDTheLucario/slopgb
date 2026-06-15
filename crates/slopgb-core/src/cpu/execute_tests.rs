//! Unit tests for instruction decode/execute. Split out of `execute.rs`
//! purely for file size; compiled as `super::tests` via the `#[path]`
//! attribute on the module declaration there.

use super::super::{Bus, Cpu, Registers, flags};
use super::step;
use Ev::{Read, ReadInc, Tick, TickAddr, Write};

/// One bus event == one M-cycle. The index in [`TestBus::log`] is the
/// cycle index, so comparing whole logs asserts both the kind and the
/// exact cycle position of every access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Read(u16, u8),
    Write(u16, u8),
    Tick,
    /// Internal cycle with a 16-bit inc/dec-unit value on the address bus
    /// ([`Bus::tick_addr`] — DMG OAM bug trigger).
    TickAddr(u16),
    /// Read in the same M-cycle as a 16-bit increment/decrement of the
    /// address register ([`Bus::read_inc`]).
    ReadInc(u16, u8),
}

/// 64 KiB flat RAM. IF lives at 0xFF0F and IE at 0xFFFF inside `mem`, so
/// stack pushes that land on IE behave exactly like on hardware.
struct TestBus {
    mem: Vec<u8>,
    log: Vec<Ev>,
    stop_result: bool,
    stop_calls: u32,
    /// `(skipped_addr, interrupt_pending)` of the most recent
    /// [`Bus::stop`] call.
    stop_args: Option<(u16, bool)>,
    /// M-cycles elapsed since construction (`take_log` does not reset
    /// this).
    cycles: usize,
    /// Simulated peripheral IRQ: during M-cycle `at` (0-based), OR
    /// `bits` into IF - models e.g. the PPU raising the vblank IF bit
    /// partway through a cycle, before that cycle's memory access.
    raise_if: Option<(usize, u8)>,
    /// `raise_if` models a timer IF committed on the *last* T-substep of
    /// its M-cycle: invisible to [`Bus::pending_halt_wake`] during the
    /// cycle it is raised (visible from the next cycle on), mirroring the
    /// real interconnect's substep-aware halt-exit sampling.
    late_if: bool,
    /// Current core-clock gate level as driven through
    /// [`Bus::set_halted`] (the calls are idempotent).
    halted_state: bool,
    /// Gate *transitions*, as (M-cycles elapsed when the call arrived,
    /// new level) — pins exactly when halt/stop engage and release the
    /// core clock gate.
    halt_calls: Vec<(usize, bool)>,
}

impl TestBus {
    fn new() -> Self {
        Self {
            mem: vec![0; 0x10000],
            log: Vec::new(),
            stop_result: false,
            stop_calls: 0,
            stop_args: None,
            cycles: 0,
            raise_if: None,
            late_if: false,
            halted_state: false,
            halt_calls: Vec::new(),
        }
    }

    /// Start of every M-cycle: advance the simulated peripherals
    /// (tick-then-access, same ordering as the real interconnect).
    fn advance(&mut self) {
        if let Some((at, bits)) = self.raise_if {
            if at == self.cycles {
                self.mem[0xFF0F] |= bits;
            }
        }
        self.cycles += 1;
    }

    fn load(&mut self, addr: u16, bytes: &[u8]) {
        for (i, &b) in bytes.iter().enumerate() {
            self.mem[usize::from(addr) + i] = b;
        }
    }

    fn take_log(&mut self) -> Vec<Ev> {
        std::mem::take(&mut self.log)
    }
}

impl Bus for TestBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.advance();
        let v = self.mem[usize::from(addr)];
        self.log.push(Read(addr, v));
        v
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.advance();
        self.log.push(Write(addr, value));
        self.mem[usize::from(addr)] = value;
    }

    fn tick(&mut self) {
        self.advance();
        self.log.push(Tick);
    }

    fn tick_addr(&mut self, value: u16) {
        self.advance();
        self.log.push(TickAddr(value));
    }

    fn read_inc(&mut self, addr: u16) -> u8 {
        self.advance();
        let v = self.mem[usize::from(addr)];
        self.log.push(ReadInc(addr, v));
        v
    }

    fn pending(&self) -> u8 {
        self.mem[0xFF0F] & self.mem[0xFFFF] & 0x1F
    }

    fn pending_halt_wake(&self) -> u8 {
        let mut p = self.pending();
        if self.late_if {
            if let Some((at, bits)) = self.raise_if {
                // `advance` has already incremented `cycles`, so the bits
                // raised during the current M-cycle (at == cycles - 1) are
                // the ones the halt-exit sampling missed.
                if at + 1 == self.cycles {
                    p &= !bits;
                }
            }
        }
        p
    }

    fn ack(&mut self, bit: u8) {
        self.mem[0xFF0F] &= !(1 << bit);
    }

    fn stop(&mut self, skipped_addr: u16, interrupt_pending: bool) -> bool {
        self.stop_calls += 1;
        self.stop_args = Some((skipped_addr, interrupt_pending));
        self.stop_result
    }

    fn set_halted(&mut self, halted: bool) {
        if self.halted_state != halted {
            self.halted_state = halted;
            self.halt_calls.push((self.cycles, halted));
        }
    }
}

const PC0: u16 = 0xC000;
const SP0: u16 = 0xD000;

/// Build an F-register value from individual flags.
fn fl(z: bool, n: bool, h: bool, c: bool) -> u8 {
    (u8::from(z) << 7) | (u8::from(n) << 6) | (u8::from(h) << 5) | (u8::from(c) << 4)
}

fn cpu() -> Cpu {
    let mut regs = Registers::default();
    regs.pc = PC0;
    regs.sp = SP0;
    Cpu {
        regs,
        ime: false,
        ime_pending: false,
        halted: false,
        stopped: false,
        halt_bug: false,
        debug_breakpoint: false,
        locked: false,
    }
}

fn bus(program: &[u8]) -> TestBus {
    let mut b = TestBus::new();
    b.load(PC0, program);
    b
}

/// Independent DAA model for add mode (N=0), written from the algorithm
/// description in gbctr: accumulate the adjustment, apply it in one
/// step, carry set by the 0x60 correction. Subtract mode is
/// deliberately not re-derived here: a flag-based reference would share
/// `op_daa`'s structure and could not catch a shared misunderstanding.
/// It is checked against decimal arithmetic instead, in
/// `daa_after_sub_computes_bcd_difference_for_all_operands`.
fn daa_add_ref(a: u8, h: bool, c: bool) -> (u8, bool, bool) {
    let mut adjust = 0u8;
    let mut carry = c;
    if h || a & 0x0F > 0x09 {
        adjust += 0x06;
    }
    if c || a > 0x99 {
        adjust += 0x60;
        carry = true;
    }
    let r = a.wrapping_add(adjust);
    (r, r == 0, carry)
}

/// Expected M-cycles per base opcode, given the branch outcome of each
/// condition code. Numbers from the gbctr instruction tables.
fn base_cycles(op: u8, taken: impl Fn(u8) -> bool) -> usize {
    match op {
        0x01 | 0x11 | 0x21 | 0x31 => 3,
        0x02 | 0x12 | 0x22 | 0x32 | 0x0A | 0x1A | 0x2A | 0x3A => 2,
        0x03 | 0x13 | 0x23 | 0x33 | 0x0B | 0x1B | 0x2B | 0x3B => 2,
        0x34..=0x36 => 3,
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x3E => 2,
        0x08 => 5,
        0x09 | 0x19 | 0x29 | 0x39 => 2,
        0x18 => 3,
        0x20 | 0x28 | 0x30 | 0x38 => {
            if taken((op >> 3) & 3) {
                3
            } else {
                2
            }
        }
        0x76 => 1, // HALT (no pending interrupt in the sweep)
        0x40..=0x7F => {
            if (op >> 3) & 7 == 6 || op & 7 == 6 {
                2
            } else {
                1
            }
        }
        0x80..=0xBF => {
            if op & 7 == 6 {
                2
            } else {
                1
            }
        }
        0xC0 | 0xC8 | 0xD0 | 0xD8 => {
            if taken((op >> 3) & 3) {
                5
            } else {
                2
            }
        }
        0xC1 | 0xD1 | 0xE1 | 0xF1 => 3,
        0xC2 | 0xCA | 0xD2 | 0xDA => {
            if taken((op >> 3) & 3) {
                4
            } else {
                3
            }
        }
        0xC3 => 4,
        0xC4 | 0xCC | 0xD4 | 0xDC => {
            if taken((op >> 3) & 3) {
                6
            } else {
                3
            }
        }
        0xC5 | 0xD5 | 0xE5 | 0xF5 => 4,
        0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => 2,
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => 4,
        0xC9 | 0xD9 => 4,
        0xCD => 6,
        0xE0 | 0xF0 => 3,
        0xE2 | 0xF2 => 2,
        0xE8 => 4,
        0xE9 => 1,
        0xEA | 0xFA => 4,
        0xF8 => 3,
        0xF9 => 2,
        // 1-cycle ops, STOP, illegal opcodes (lock after the fetch)
        _ => 1,
    }
}

fn run_sweep(flags: u8) {
    let taken = |cc: u8| match cc {
        0 => flags & flags::Z == 0,
        1 => flags & flags::Z != 0,
        2 => flags & flags::C == 0,
        _ => flags & flags::C != 0,
    };
    for op in 0..=255u8 {
        if op == 0xCB {
            continue; // prefix, swept separately
        }
        let mut c = cpu();
        c.regs.set_f(flags);
        c.regs.set_hl(0xC800);
        c.regs.set_bc(0xC700);
        c.regs.set_de(0xC701);
        let mut b = bus(&[op, 0x00, 0x00]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log.len(),
            base_cycles(op, taken),
            "opcode {op:#04x} flags {flags:#04x}"
        );
        assert_eq!(
            c.regs.f() & 0x0F,
            0,
            "opcode {op:#04x} dirtied F low nibble"
        );
    }
}

#[path = "execute_tests/alu.rs"]
mod alu;

#[path = "execute_tests/control.rs"]
mod control;

#[path = "execute_tests/cycles.rs"]
mod cycles;

#[path = "execute_tests/interrupts.rs"]
mod interrupts;

#[path = "execute_tests/load.rs"]
mod load;
