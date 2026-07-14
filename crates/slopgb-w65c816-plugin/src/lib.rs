//! Coprocessor plugin wrapping the clean-room 65C816 (`slopgb-w65c816`) — the
//! SNES-side CPU the Super Game Boy runs — as a host-driven wasm subsystem.
//!
//! The chip's whole memory (SNES RAM + a small program area) lives inside the
//! sandbox; only the four comm ports cross the host boundary, so a host clocks
//! the CPU with [`Coprocessor::run_until`] and exchanges bytes with
//! [`Coprocessor::port_write`] / [`Coprocessor::port_read`]. This is the LLE
//! route for the SNES side; `slopgb-core`'s built-in SGB path is HLE and never
//! runs a 65C816.
//!
//! ## Memory model
//!
//! A flat 64 KB bank-0 RAM, aliased across every bank (`addr & 0xFFFF`). The
//! four comm ports are mapped at `$2140-$2143` (the SNES APU I/O window the SGB
//! program uses), so a CPU read there returns the host's latest `port_write` and
//! a CPU write there is picked up by the host's `port_read`.
//!
// ponytail: flat bank-0 RAM + a built-in demo program. Hosting the *real* SGB
// SNES driver (goal 4) needs a program-load path — a full LoROM/HiROM map with
// 128 KB WRAM mirrors and either a bulk-load ABI call or a port-streamed
// loader. Deferred until that integration; this proves the reset/clock/port
// boundary end to end with a program that actually executes.

#![deny(unsafe_code)]

use slopgb_plugin_api::{Coprocessor, slopgb_coprocessor_plugin};
use slopgb_w65c816::{Bus, Cpu};

/// Comm ports the plugin exposes (SNES APU I/O has four).
const N_PORTS: usize = 4;
/// Bank-0 low address of comm port 0 (`$2140`); ports 1-3 follow.
const PORT_BASE: u16 = 0x2140;
/// Where the demo program is loaded, and the emulation-mode reset vector value.
const PROG_ORG: u16 = 0x8000;
/// Emulation-mode reset vector location (`$00FFFC-$00FFFD`).
const RESET_VEC: usize = 0xFFFC;

/// A tiny 8-bit (emulation-mode) program: echo comm port 1 (host input) + 7 to
/// comm port 0 (host output), forever. It proves the full round trip — a host
/// value crosses in through a port, a real 65C816 transforms it across many
/// `run_until` cycles, and the result crosses back out.
///
/// ```text
/// $8000  LDA $2141   ; A = port_in[1]  (host input)
/// $8003  CLC
/// $8004  ADC #$07    ; A += 7
/// $8006  STA $2140   ; port_out[0] = A (host output)
/// $8009  BRA $8000   ; loop
/// ```
const DEMO: [u8; 11] = [
    0xAD, 0x41, 0x21, // LDA $2141
    0x18, // CLC
    0x69, 0x07, // ADC #$07
    0x8D, 0x40, 0x21, // STA $2140
    0x80, 0xF5, // BRA -11 -> $8000
];

/// Guest SNES RAM + the comm-port latches the hosted CPU talks to.
struct SnesBus {
    /// 64 KB of RAM, aliased across every 65C816 bank.
    ram: Box<[u8; 0x1_0000]>,
    /// Host -> chip: what the last `port_write` deposited (CPU reads it).
    port_in: [u8; N_PORTS],
    /// Chip -> host: what the CPU wrote (the host's `port_read` returns it).
    port_out: [u8; N_PORTS],
}

impl SnesBus {
    fn new() -> Self {
        SnesBus {
            ram: Box::new([0u8; 0x1_0000]),
            port_in: [0; N_PORTS],
            port_out: [0; N_PORTS],
        }
    }

    /// The comm-port index an address maps to, if any (`$2140-$2143` in any
    /// bank, since banks alias). `None` means plain RAM.
    fn port_index(addr: u32) -> Option<usize> {
        let low = (addr & 0xFFFF) as u16;
        let base = PORT_BASE;
        (low >= base && low < base + N_PORTS as u16).then(|| (low - base) as usize)
    }
}

impl Bus for SnesBus {
    fn read(&mut self, addr: u32) -> u8 {
        match Self::port_index(addr) {
            Some(p) => self.port_in[p],
            None => self.ram[(addr & 0xFFFF) as usize],
        }
    }

    fn write(&mut self, addr: u32, value: u8) {
        match Self::port_index(addr) {
            Some(p) => self.port_out[p] = value,
            None => self.ram[(addr & 0xFFFF) as usize] = value,
        }
    }
}

/// The 65C816 coprocessor: a CPU over [`SnesBus`], clocked by the host.
struct W65816Cop {
    cpu: Cpu,
    bus: SnesBus,
    /// Cycles executed since the last reset (the chip's own cycle domain).
    cycles: u64,
}

impl W65816Cop {
    /// Load the demo program + reset vector into a freshly zeroed RAM.
    fn install_program(&mut self) {
        self.bus.ram.fill(0);
        let org = PROG_ORG as usize;
        self.bus.ram[org..org + DEMO.len()].copy_from_slice(&DEMO);
        self.bus.ram[RESET_VEC] = PROG_ORG as u8;
        self.bus.ram[RESET_VEC + 1] = (PROG_ORG >> 8) as u8;
    }
}

impl Coprocessor for W65816Cop {
    fn new() -> Self {
        let mut me = W65816Cop {
            cpu: Cpu::new(),
            bus: SnesBus::new(),
            cycles: 0,
        };
        me.reset();
        me
    }

    fn reset(&mut self) {
        self.install_program();
        self.bus.port_in = [0; N_PORTS];
        self.bus.port_out = [0; N_PORTS];
        self.cpu = Cpu::new();
        // Load PC from the emulation-mode reset vector, like real power-on.
        let lo = self.bus.read(RESET_VEC as u32) as u16;
        let hi = self.bus.read(RESET_VEC as u32 + 1) as u16;
        self.cpu.regs.pc = lo | (hi << 8);
        self.cycles = 0;
    }

    fn run_until(&mut self, target_cycle: u64) -> u64 {
        while self.cycles < target_cycle {
            if self.cpu.stopped {
                // STP halted the oscillator: no instructions retire, but the
                // host's clock still advances, so honor the "reach the target"
                // contract by absorbing the idle span.
                self.cycles = target_cycle;
                break;
            }
            self.cycles += self.cpu.step(&mut self.bus);
        }
        self.cycles
    }

    fn port_write(&mut self, port: u8, val: u8) {
        if (port as usize) < N_PORTS {
            self.bus.port_in[port as usize] = val;
        }
    }

    fn port_read(&mut self, port: u8) -> u8 {
        if (port as usize) < N_PORTS {
            self.bus.port_out[port as usize]
        } else {
            0
        }
    }
}

slopgb_coprocessor_plugin!(W65816Cop);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
