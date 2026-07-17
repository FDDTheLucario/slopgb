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

mod icd2;

use icd2::{CHAR_ROW_LEN, ICD2_STATE_LEN, Icd2};
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
/// Serialized [`W65816Cop::save_state`] length: 18 bytes of registers + halt
/// flags, the 64 KB RAM, 8 bytes of comm-port latches, the ICD2 block, an
/// 8-byte cycle counter.
const STATE_LEN: usize = 18 + 0x1_0000 + 8 + ICD2_STATE_LEN + 8;

/// Host-window base for `write_ram`/`read_ram`: the 65C816 bus is 24-bit, so
/// any address at or above this can never be chip memory — it is the host's
/// out-of-band pump channel into the ICD2 block (packet deposit, pad
/// readback, LCD-row shadow). Addresses below keep their raw-memory-install
/// meaning (firmware, `DATA_SND`/`DATA_TRN` payloads).
pub const HOST_WIN: u32 = 0x0100_0000;
/// `W HOST_WIN + 0, len 16`: deposit a packet + raise the `$6002` flag.
/// `R HOST_WIN + 0, len 1`: the flag (deposit only when clear).
pub const HW_PACKET: u32 = HOST_WIN;
/// `R len 5`: the four `$6004-$6007` pad latches + the sticky written flag.
pub const HW_PADS: u32 = HOST_WIN + 0x11;
/// `W len 2`: the `$6000` shadows `[lcd_row, write_row]`.
pub const HW_LCD_ROW: u32 = HOST_WIN + 0x16;
/// `R len 1`: the last `$6003` control write.
pub const HW_CONTROL: u32 = HOST_WIN + 0x17;
/// `W len 320` at `HW_CHAR_ROWS + row * 320`: load a character-buffer row.
pub const HW_CHAR_ROWS: u32 = HOST_WIN + 0x20;

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

/// Guest SNES RAM + the comm-port latches + the ICD2 block the hosted CPU
/// talks to.
struct SnesBus {
    /// 64 KB of RAM, aliased across every 65C816 bank.
    ram: Box<[u8; 0x1_0000]>,
    /// Host -> chip: what the last `port_write` deposited (CPU reads it).
    port_in: [u8; N_PORTS],
    /// Chip -> host: what the CPU wrote (the host's `port_read` returns it).
    port_out: [u8; N_PORTS],
    /// The SGB ICD2 register block at `$6000-$7FFF` (see `icd2.rs`).
    icd2: Icd2,
}

impl SnesBus {
    fn new() -> Self {
        SnesBus {
            ram: Box::new([0u8; 0x1_0000]),
            port_in: [0; N_PORTS],
            port_out: [0; N_PORTS],
            icd2: Icd2::new(),
        }
    }

    /// The ICD2 window: `$6000-$7FFF` in any bank (banks alias in the flat
    /// map; the real chip responds wherever A22=0 — refined with the full
    /// memory map).
    fn icd2_addr(addr: u32) -> Option<u16> {
        let low = (addr & 0xFFFF) as u16;
        (0x6000..=0x7FFF).contains(&low).then_some(low)
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
        if let Some(p) = Self::port_index(addr) {
            return self.port_in[p];
        }
        if let Some(a) = Self::icd2_addr(addr) {
            return self.icd2.cpu_read(a);
        }
        self.ram[(addr & 0xFFFF) as usize]
    }

    fn write(&mut self, addr: u32, value: u8) {
        if let Some(p) = Self::port_index(addr) {
            self.port_out[p] = value;
            return;
        }
        if let Some(a) = Self::icd2_addr(addr) {
            return self.icd2.cpu_write(a, value);
        }
        self.ram[(addr & 0xFFFF) as usize] = value;
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

impl W65816Cop {
    /// The host window's write half (see the `HW_*` constants): packet
    /// deposit, `$6000` shadows, character-row loads. Unknown offsets are
    /// ignored (a newer host talking to an older plugin degrades quietly).
    fn host_window_write(&mut self, addr: u32, bytes: &[u8]) {
        let icd2 = &mut self.bus.icd2;
        match addr {
            HW_PACKET => {
                if let Ok(p) = <&[u8; 16]>::try_from(bytes) {
                    icd2.host_deposit_packet(p);
                }
            }
            HW_LCD_ROW => {
                if let [lcd_row, write_row] = *bytes {
                    icd2.host_set_lcd_row(lcd_row, write_row);
                }
            }
            a if (HW_CHAR_ROWS..HW_CHAR_ROWS + (icd2::CHAR_ROWS * CHAR_ROW_LEN) as u32)
                .contains(&a) =>
            {
                let off = (a - HW_CHAR_ROWS) as usize;
                // Row-aligned only (see HW_CHAR_ROWS): a misaligned write is
                // a host bug, dropped rather than silently row-start-mapped.
                if off % CHAR_ROW_LEN == 0 {
                    icd2.host_load_char_row(off / CHAR_ROW_LEN, bytes);
                }
            }
            _ => {}
        }
    }

    /// The host window's read half: the packet flag, the pad latches + the
    /// sticky written flag, the `$6003` capture. Unknown offsets read zeros.
    fn host_window_read(&mut self, addr: u32, len: usize) -> Vec<u8> {
        let icd2 = &self.bus.icd2;
        let mut out = vec![0u8; len];
        match addr {
            HW_PACKET => {
                if let Some(slot) = out.first_mut() {
                    *slot = u8::from(icd2.packet_pending());
                }
            }
            HW_PADS => {
                let (pads, written) = icd2.host_pads();
                for (slot, &v) in out.iter_mut().zip(pads.iter()) {
                    *slot = v;
                }
                if let Some(slot) = out.get_mut(4) {
                    *slot = u8::from(written);
                }
            }
            HW_CONTROL => {
                if let Some(slot) = out.first_mut() {
                    *slot = icd2.host_control();
                }
            }
            _ => {}
        }
        out
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
        self.bus.icd2 = Icd2::new();
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

    /// Redirect the CPU to a 24-bit `bank<<16 | pc` target and un-halt it — how
    /// the host points the CPU at freshly installed firmware or applies an SGB
    /// `JUMP`. Clearing `stopped`/`waiting` lets the target actually run.
    fn set_pc(&mut self, addr: u32) {
        self.cpu.regs.pbr = (addr >> 16) as u8;
        self.cpu.regs.pc = addr as u16;
        self.cpu.stopped = false;
        self.cpu.waiting = false;
    }

    /// Poke `bytes` into SNES RAM at `addr` (wrapping the 64 KB bank) — how the
    /// host installs resident firmware or lands an SGB `DATA_SND` block.
    /// Addresses at/above [`HOST_WIN`] are the out-of-band ICD2 pump channel
    /// instead (the 24-bit bus can never reach them).
    fn write_ram(&mut self, addr: u32, bytes: &[u8]) {
        if addr >= HOST_WIN {
            return self.host_window_write(addr, bytes);
        }
        for (i, &b) in bytes.iter().enumerate() {
            self.bus.ram[(addr.wrapping_add(i as u32) & 0xFFFF) as usize] = b;
        }
    }

    fn read_ram(&mut self, addr: u32, len: usize) -> Vec<u8> {
        if addr >= HOST_WIN {
            return self.host_window_read(addr, len);
        }
        (0..len)
            .map(|i| self.bus.ram[(addr.wrapping_add(i as u32) & 0xFFFF) as usize])
            .collect()
    }

    /// Serialize the register file + halt flags, the 64 KB RAM, the comm-port
    /// latches, and the host-side cycle counter.
    fn save_state(&self) -> Vec<u8> {
        let r = &self.cpu.regs;
        let mut buf = Vec::with_capacity(STATE_LEN);
        for v in [r.a, r.x, r.y, r.s, r.d, r.pc] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf.extend_from_slice(&[r.pbr, r.dbr, r.p, r.e as u8]);
        buf.push(self.cpu.stopped as u8);
        buf.push(self.cpu.waiting as u8);
        buf.extend_from_slice(&self.bus.ram[..]);
        buf.extend_from_slice(&self.bus.port_in);
        buf.extend_from_slice(&self.bus.port_out);
        self.bus.icd2.save_state(&mut buf);
        buf.extend_from_slice(&self.cycles.to_le_bytes());
        debug_assert_eq!(buf.len(), STATE_LEN);
        buf
    }

    /// Restore state produced by [`Self::save_state`]. A truncated/foreign
    /// buffer is ignored (the chip keeps its current state) rather than panic.
    fn load_state(&mut self, bytes: &[u8]) {
        if bytes.len() != STATE_LEN {
            return;
        }
        let mut c = Cursor { b: bytes, pos: 0 };
        let r = &mut self.cpu.regs;
        r.a = c.u16();
        r.x = c.u16();
        r.y = c.u16();
        r.s = c.u16();
        r.d = c.u16();
        r.pc = c.u16();
        r.pbr = c.u8();
        r.dbr = c.u8();
        r.p = c.u8();
        r.e = c.u8() != 0;
        self.cpu.stopped = c.u8() != 0;
        self.cpu.waiting = c.u8() != 0;
        self.bus.ram.copy_from_slice(c.take(0x1_0000));
        self.bus.port_in.copy_from_slice(c.take(N_PORTS));
        self.bus.port_out.copy_from_slice(c.take(N_PORTS));
        self.bus.icd2.load_state(c.take(ICD2_STATE_LEN));
        self.cycles = c.u64();
    }
}

/// A minimal little-endian read cursor for [`W65816Cop::load_state`]. Only
/// entered after a length check, so every `take` is in bounds.
struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        s
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().unwrap())
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().unwrap())
    }
}

slopgb_coprocessor_plugin!(W65816Cop);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
