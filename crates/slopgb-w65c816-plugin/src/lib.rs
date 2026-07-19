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
//! ## Memory model (fullsnes "SNES Memory Map", adapted to the SGB cart)
//!
//! - `$7E-$7F`: 128 KB WRAM; banks `$00-$3F`/`$80-$BF` mirror its first 8 KB
//!   at `$0000-$1FFF` (so an SGB `JUMP $001800` lands in `$7E:1800`).
//! - System banks `$8000-$FFFF`: the SGB's SNES BIOS ROM region, RAM-backed
//!   as **one 32 KB image aliased across the system banks** — slopgb never
//!   ships the real ROM, the host installs original clean-room firmware here
//!   (and the reset vector at `$00:FFFC`).
//! - System banks `$2140-$2143`: the four APU comm ports (host-mediated);
//!   `$6000-$7FFF`: the ICD2 register block (`icd2.rs`).
//! - Everything else (unmapped I/O, HiROM banks `$40-$7D`/`$C0-$FF`) is open
//!   bus: reads 0, writes dropped — the SGB cart maps nothing there.

#![deny(unsafe_code)]

mod icd2;
mod mmio;

use icd2::{CHAR_ROW_LEN, ICD2_STATE_LEN, Icd2};
pub use mmio::MMIO_RING_CAP;
use mmio::{MMIO_STATE_LEN, Mmio};
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
/// flags, the 128 KB WRAM, the 32 KB program area, 8 bytes of comm-port
/// latches, the ICD2 block, the MMIO block, the NMI-pending flag, an 8-byte
/// cycle counter, and the APU port-write ring (count + overflow + entries).
const STATE_LEN: usize =
    18 + WRAM_LEN + PROG_LEN + 8 + ICD2_STATE_LEN + MMIO_STATE_LEN + 1 + 8 + 3 + 2 * PORT_RING_CAP;
/// 128 KB of work RAM at `$7E-$7F`.
const WRAM_LEN: usize = 0x2_0000;
/// The 32 KB `$8000-$FFFF` program area (the RAM-backed BIOS ROM region).
const PROG_LEN: usize = 0x8000;

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
/// `R len 3 + 3*MMIO_RING_CAP` (drains): `[n_lo, n_hi, overflow]` then `n`
/// captured MMIO writes as `(addr_lo, addr_hi, val)` triples, oldest first.
pub const HW_MMIO_RING: u32 = HOST_WIN + 0x1000;
/// `W len L` at `HW_SHADOW + i`: the CPU-read shadows for `$4200 + i`
/// (`i < 0x20`); offsets `0x20`/`0x21` are the `$4016`/`$4017` serial bytes.
pub const HW_SHADOW: u32 = HOST_WIN + 0x2000;
/// `W len 1`: nonzero requests an NMI (delivered at the next instruction
/// boundary inside `run_until`, consumed once; zero cancels a pending one).
/// `R len 1`: the pending flag.
pub const HW_NMI: u32 = HOST_WIN + 0x3000;
/// `R len 1`: whether a `$420B` write stalled the CPU awaiting host DMA
/// service. `W len 1` zero: the host executed the transfer, resume.
pub const HW_DMA_STALL: u32 = HOST_WIN + 0x3001;
/// `R len 3 + 2*PORT_RING_CAP` (drains): `[n_lo, n_hi, overflow]` then `n`
/// ordered `(port, val)` APU-port writes — every write, no dedup (a
/// same-value index write is the SPC700 IPL protocol's "data valid" edge).
/// A host replaying these one at a time preserves multi-step handshakes
/// that final-latch snapshots alias.
pub const HW_PORT_RING: u32 = HOST_WIN + 0x4000;
/// `R len 2 + 2*PAD_RING_CAP` (drains): the ordered ICD2 pad-latch write
/// ring — `[n, overflow]` then `n` `(reg, value)` pairs. The latches carry
/// sub-frame protocol sequences (ACK handshakes, one-shot phase triggers)
/// that a per-flush latch snapshot aliases away.
pub const HW_PAD_RING: u32 = HOST_WIN + 0x5000;
/// `R len N` at `+ off`: N successive *bus* reads of ICD2 space
/// (`$6000 + (off + i & $1FFF)`), side effects included — the host's GP-DMA
/// engine sources A-bus reads of the ICD2 through here (a raw `read_ram`
/// has no device semantics: `$7800`'s auto-increment must run per byte).
pub const HW_ICD2_BUS: u32 = HOST_WIN + 0x6000;
/// Port-ring capacity in captured writes (a flush window is ~2.5 K CPU
/// cycles; the resident shim's 4-write loop peaks near 340).
// Sized for the writes one whole flush window can produce: the host drains
// once per flush, but the clocking loop runs a `run_until` slice per
// scanline, so a tight upload pump (a few cycles per STA) can bank thousands
// of events before the next drain. 16384 leaves >2x headroom over the
// theoretical per-frame STA rate.
pub const PORT_RING_CAP: usize = 16384;

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

/// Guest SNES memory (WRAM + the program area) + the comm-port latches + the
/// ICD2 block the hosted CPU talks to.
struct SnesBus {
    /// 128 KB WRAM at `$7E-$7F` (bank-0/`$80` low 8 KB mirror its start).
    wram: Box<[u8; WRAM_LEN]>,
    /// The `$8000-$FFFF` program area, one 32 KB image aliased across the
    /// system banks (the RAM-backed BIOS ROM region the host installs
    /// firmware into).
    prog: Box<[u8; PROG_LEN]>,
    /// Host -> chip: what the last `port_write` deposited (CPU reads it).
    port_in: [u8; N_PORTS],
    /// Chip -> host: what the CPU wrote (the host's `port_read` returns it).
    port_out: [u8; N_PORTS],
    /// The SGB ICD2 register block at `$6000-$7FFF` (see `icd2.rs`).
    icd2: Icd2,
    /// The MMIO write-capture ring + read shadows (see `mmio.rs`).
    mmio: Mmio,
    /// Ordered `(port, val)` APU-port writes awaiting host replay (see
    /// [`HW_PORT_RING`]); `port_ring_of` is the sticky overflow flag.
    port_ring: Vec<(u8, u8)>,
    port_ring_of: bool,
}

/// Heap-allocate a zeroed fixed-size byte array (the arrays are too big for
/// the wasm stack; the length always matches, so the conversion can't fail).
fn boxed_zeroed<const N: usize>() -> Box<[u8; N]> {
    vec![0u8; N]
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

impl SnesBus {
    fn new() -> Self {
        SnesBus {
            wram: boxed_zeroed(),
            prog: boxed_zeroed(),
            port_in: [0; N_PORTS],
            port_out: [0; N_PORTS],
            icd2: Icd2::new(),
            mmio: Mmio::new(),
            port_ring: Vec::new(),
            port_ring_of: false,
        }
    }

    /// Whether `addr` sits in a system bank (`$00-$3F`/`$80-$BF`, i.e.
    /// A22=0) — where the I/O windows (ports, ICD2) respond. `$7E-$7F` have
    /// A22=1, so WRAM is never shadowed by I/O.
    fn system_bank(addr: u32) -> bool {
        (addr >> 16) as u8 & 0x40 == 0
    }

    /// The ICD2 window: `$6000-$7FFF` in the system banks (the chip decodes
    /// A22=0; fullsnes "SGB I/O Map").
    fn icd2_addr(addr: u32) -> Option<u16> {
        let low = (addr & 0xFFFF) as u16;
        (Self::system_bank(addr) && (0x6000..=0x7FFF).contains(&low)).then_some(low)
    }

    /// The comm-port index an address maps to, if any (`$2140-$2143` in the
    /// system banks). `None` means not a port.
    fn port_index(addr: u32) -> Option<usize> {
        let low = (addr & 0xFFFF) as u16;
        (Self::system_bank(addr) && (PORT_BASE..PORT_BASE + N_PORTS as u16).contains(&low))
            .then(|| (low - PORT_BASE) as usize)
    }

    /// The RAM-backed byte behind `addr`, or `None` for I/O space and open
    /// bus (fullsnes "SNES Memory Map": WRAM at `$7E-$7F` + the bank-0 low
    /// mirror; the system banks' `$8000-$FFFF` program area; nothing else is
    /// memory on the SGB cart).
    fn mem_slot(&mut self, addr: u32) -> Option<&mut u8> {
        let bank = (addr >> 16) as u8;
        let low = (addr & 0xFFFF) as usize;
        match bank {
            0x7E => Some(&mut self.wram[low]),
            0x7F => Some(&mut self.wram[0x1_0000 + low]),
            b if b & 0x40 == 0 => {
                if low < 0x2000 {
                    Some(&mut self.wram[low])
                } else if low >= 0x8000 {
                    Some(&mut self.prog[low - 0x8000])
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl Bus for SnesBus {
    fn read(&mut self, addr: u32) -> u8 {
        let bank = (addr >> 16) as u8;
        let low = (addr & 0xFFFF) as u16;

        // WRAM short-circuit: banks $7E/$7F and the low-8K mirror never
        // reach the capture windows, ICD2 block, or APU ports.
        if bank == 0x7E || bank == 0x7F {
            if let Some(slot) = self.mem_slot(addr) {
                return *slot;
            }
        }
        if bank & 0x40 == 0 && low < 0x2000 {
            // Low-bank WRAM mirror ($0000-$1FFF in system banks).
            if let Some(slot) = self.mem_slot(addr) {
                return *slot;
            }
        }
        if bank & 0x40 == 0 && low >= 0x8000 {
            // Program area ($8000-$FFFF in system banks): not captured.
            if let Some(slot) = self.mem_slot(addr) {
                return *slot;
            }
        }

        if let Some(p) = Self::port_index(addr) {
            return self.port_in[p];
        }
        if let Some(a) = Self::icd2_addr(addr) {
            return self.icd2.cpu_read(a);
        }
        if Self::system_bank(addr) {
            if let Some(v) = self.mmio.cpu_read(low) {
                return v;
            }
        }
        self.mem_slot(addr).map_or(0, |b| *b)
    }

    fn write(&mut self, addr: u32, value: u8) {
        let bank = (addr >> 16) as u8;
        let low = (addr & 0xFFFF) as u16;

        // WRAM short-circuit: banks $7E/$7F and the low-8K mirror never
        // reach the capture windows, ICD2 block, or APU ports.
        if bank == 0x7E || bank == 0x7F {
            if let Some(slot) = self.mem_slot(addr) {
                *slot = value;
            }
            return;
        }
        if bank & 0x40 == 0 && low < 0x2000 {
            // Low-bank WRAM mirror ($0000-$1FFF in system banks).
            if let Some(slot) = self.mem_slot(addr) {
                *slot = value;
            }
            return;
        }
        if bank & 0x40 == 0 && low >= 0x8000 {
            // Program area ($8000-$FFFF in system banks): not captured.
            if let Some(slot) = self.mem_slot(addr) {
                *slot = value;
            }
            return;
        }

        if let Some(p) = Self::port_index(addr) {
            // Ring every write for ordered host replay (see HW_PORT_RING).
            if self.port_ring.len() >= PORT_RING_CAP {
                self.port_ring_of = true;
            } else {
                self.port_ring.push((p as u8, value));
            }
            self.port_out[p] = value;
            return;
        }
        if let Some(a) = Self::icd2_addr(addr) {
            return self.icd2.cpu_write(a, value);
        }
        if Self::system_bank(addr) && self.mmio.cpu_write(low, value) {
            return;
        }
        if let Some(b) = self.mem_slot(addr) {
            *b = value;
        }
    }
}

/// The 65C816 coprocessor: a CPU over [`SnesBus`], clocked by the host.
struct W65816Cop {
    cpu: Cpu,
    bus: SnesBus,
    /// Cycles executed since the last reset (the chip's own cycle domain).
    cycles: u64,
    /// A host-requested NMI awaiting the next instruction boundary.
    nmi_pending: bool,
}

impl W65816Cop {
    /// Load the demo program + reset vector into a freshly zeroed memory.
    fn install_program(&mut self) {
        self.bus.wram.fill(0);
        self.bus.prog.fill(0);
        let org = (PROG_ORG as usize) - 0x8000;
        self.bus.prog[org..org + DEMO.len()].copy_from_slice(&DEMO);
        let vec = RESET_VEC - 0x8000;
        self.bus.prog[vec] = PROG_ORG as u8;
        self.bus.prog[vec + 1] = (PROG_ORG >> 8) as u8;
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
            HW_NMI => {
                if let [v] = *bytes {
                    self.nmi_pending = v != 0;
                }
            }
            HW_DMA_STALL => {
                if let [0] = *bytes {
                    self.bus.mmio.host_clear_dma_stall();
                }
            }
            a if (HW_SHADOW..HW_SHADOW + 0x22).contains(&a) => {
                let mmio = &mut self.bus.mmio;
                for (j, &v) in bytes.iter().enumerate() {
                    let off = (a - HW_SHADOW) as usize + j;
                    match off {
                        0x20 | 0x21 => mmio.host_set_joy_serial_byte(off - 0x20, v),
                        o if o < 0x20 => mmio.host_set_shadow(o as u8, v),
                        _ => {}
                    }
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
        let icd2 = &mut self.bus.icd2;
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
            HW_NMI => {
                if let Some(slot) = out.first_mut() {
                    *slot = u8::from(self.nmi_pending);
                }
            }
            HW_DMA_STALL => {
                if let Some(slot) = out.first_mut() {
                    *slot = u8::from(self.bus.mmio.dma_stall());
                }
            }
            a if (HW_ICD2_BUS..HW_ICD2_BUS + 0x2000).contains(&a) => {
                let off = a - HW_ICD2_BUS;
                for (i, slot) in out.iter_mut().enumerate() {
                    let reg = 0x6000 + ((off + i as u32) & 0x1FFF) as u16;
                    *slot = icd2.cpu_read(reg);
                }
            }
            HW_PAD_RING if out.len() >= 2 => {
                // A header-sized read (no room for entries) reports the
                // pending count without draining, so an idle host can skip
                // the bulk copy; a larger read drains what fits.
                let fits = (out.len() - 2) / 2;
                if fits == 0 {
                    out[0] = icd2.pad_ring_pending().min(255) as u8;
                } else {
                    let (ring, of) = icd2.host_drain_pad_ring(fits);
                    out[0] = ring.len() as u8;
                    out[1] = u8::from(of);
                    for (i, &(r, v)) in ring.iter().enumerate() {
                        out[2 + i * 2] = r;
                        out[3 + i * 2] = v;
                    }
                }
            }
            HW_PORT_RING if out.len() >= 3 => {
                // Header-sized read: pending count only, nothing drains.
                let fits = (out.len() - 3) / 2;
                if fits == 0 {
                    let n = self.bus.port_ring.len();
                    out[0] = n as u8;
                    out[1] = (n >> 8) as u8;
                    out[2] = u8::from(self.bus.port_ring_of);
                } else {
                    let n = self.bus.port_ring.len().min(fits);
                    out[0] = n as u8;
                    out[1] = (n >> 8) as u8;
                    out[2] = u8::from(std::mem::take(&mut self.bus.port_ring_of));
                    for (i, &(p, v)) in self.bus.port_ring.iter().take(n).enumerate() {
                        out[3 + i * 2] = p;
                        out[4 + i * 2] = v;
                    }
                    self.bus.port_ring.drain(..n);
                }
            }
            HW_MMIO_RING if out.len() >= 3 => {
                let mmio = &mut self.bus.mmio;
                // Header-sized read: pending count only, nothing drains —
                // an idle host skips the bulk copy. A larger read drains
                // only what it can carry (never losing captured writes).
                let fits = (out.len() - 3) / 3;
                if fits == 0 {
                    let n = mmio.pending() as u16;
                    out[0] = n as u8;
                    out[1] = (n >> 8) as u8;
                } else {
                    let drained = mmio.host_drain_up_to(fits);
                    let n = drained.len() as u16;
                    out[0] = n as u8;
                    out[1] = (n >> 8) as u8;
                    out[2] = u8::from(mmio.overflowed());
                    for (i, &(a, v)) in drained.iter().enumerate() {
                        let base = 3 + i * 3;
                        out[base] = a as u8;
                        out[base + 1] = (a >> 8) as u8;
                        out[base + 2] = v;
                    }
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
            nmi_pending: false,
        };
        me.reset();
        me
    }

    fn reset(&mut self) {
        self.install_program();
        self.bus.port_in = [0; N_PORTS];
        self.bus.port_out = [0; N_PORTS];
        self.bus.icd2 = Icd2::new();
        self.bus.mmio = Mmio::new();
        self.cpu = Cpu::new();
        // Load PC from the emulation-mode reset vector, like real power-on.
        let lo = self.bus.read(RESET_VEC as u32) as u16;
        let hi = self.bus.read(RESET_VEC as u32 + 1) as u16;
        self.cpu.regs.pc = lo | (hi << 8);
        self.cycles = 0;
        self.nmi_pending = false;
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
            if self.bus.mmio.dma_stall() {
                // A $420B write paused the CPU for the host-run DMA transfer
                // (fullsnes 420Bh); absorb like STP — a pending NMI stays
                // pending until the CPU resumes.
                self.cycles = target_cycle;
                break;
            }
            if self.nmi_pending {
                // /NMI is sampled at instruction boundaries; deliver once
                // (also wakes a WAI-ing CPU).
                self.nmi_pending = false;
                self.cycles += self.cpu.nmi(&mut self.bus);
                continue;
            }
            if self.cpu.waiting {
                // WAI: nothing retires until an interrupt arrives; absorb
                // the idle span like STP (the host clock keeps advancing).
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

    /// Poke `bytes` into SNES memory at the 24-bit `addr` — how the host
    /// installs resident firmware or lands an SGB `DATA_SND`/`DATA_TRN`
    /// block. Bytes aimed at I/O space or open bus are dropped (a raw
    /// install is not a bus access). Addresses at/above [`HOST_WIN`] are the
    /// out-of-band ICD2 pump channel instead (the 24-bit bus can never reach
    /// them).
    fn write_ram(&mut self, addr: u32, bytes: &[u8]) {
        if addr >= HOST_WIN {
            return self.host_window_write(addr, bytes);
        }
        for (i, &b) in bytes.iter().enumerate() {
            let a = addr.wrapping_add(i as u32) & 0xFF_FFFF;
            if let Some(slot) = self.bus.mem_slot(a) {
                *slot = b;
            }
        }
    }

    fn read_ram(&mut self, addr: u32, len: usize) -> Vec<u8> {
        if addr >= HOST_WIN {
            return self.host_window_read(addr, len);
        }
        (0..len)
            .map(|i| {
                let a = addr.wrapping_add(i as u32) & 0xFF_FFFF;
                self.bus.mem_slot(a).map_or(0, |b| *b)
            })
            .collect()
    }

    /// Serialize the register file + halt flags, WRAM, the program area, the
    /// comm-port latches, the ICD2 block, and the host-side cycle counter.
    fn save_state(&self) -> Vec<u8> {
        let r = &self.cpu.regs;
        let mut buf = Vec::with_capacity(STATE_LEN);
        for v in [r.a, r.x, r.y, r.s, r.d, r.pc] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf.extend_from_slice(&[r.pbr, r.dbr, r.p, r.e as u8]);
        buf.push(self.cpu.stopped as u8);
        buf.push(self.cpu.waiting as u8);
        buf.extend_from_slice(&self.bus.wram[..]);
        buf.extend_from_slice(&self.bus.prog[..]);
        buf.extend_from_slice(&self.bus.port_in);
        buf.extend_from_slice(&self.bus.port_out);
        self.bus.icd2.save_state(&mut buf);
        self.bus.mmio.save_state(&mut buf);
        buf.push(u8::from(self.nmi_pending));
        buf.extend_from_slice(&self.cycles.to_le_bytes());
        buf.extend_from_slice(&(self.bus.port_ring.len() as u16).to_le_bytes());
        buf.push(u8::from(self.bus.port_ring_of));
        for &(p, v) in &self.bus.port_ring {
            buf.extend_from_slice(&[p, v]);
        }
        for _ in self.bus.port_ring.len()..PORT_RING_CAP {
            buf.extend_from_slice(&[0, 0]);
        }
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
        self.bus.wram.copy_from_slice(c.take(WRAM_LEN));
        self.bus.prog.copy_from_slice(c.take(PROG_LEN));
        self.bus.port_in.copy_from_slice(c.take(N_PORTS));
        self.bus.port_out.copy_from_slice(c.take(N_PORTS));
        self.bus.icd2.load_state(c.take(ICD2_STATE_LEN));
        self.bus.mmio.load_state(c.take(MMIO_STATE_LEN));
        self.nmi_pending = c.u8() != 0;
        self.cycles = c.u64();
        let n = usize::from(c.u16()).min(PORT_RING_CAP);
        self.bus.port_ring_of = c.u8() != 0;
        self.bus.port_ring.clear();
        for i in 0..PORT_RING_CAP {
            let e = c.take(2);
            if i < n {
                self.bus.port_ring.push((e[0], e[1]));
            }
        }
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
