//! The host-side GP-DMA engine (`$420B` / `$43x0-$43x6`) and the WRAM B-bus
//! access ports (`$2180-$2183`), driven from the guest's captured MMIO
//! writes. The plugin stalls its CPU on the `$420B` write until the host has
//! run the transfer (`HW_DMA_STALL`), so guest-visible DMA is atomic even
//! under the polled capture ring. Register semantics cite nocash *fullsnes*
//! ("SNES DMA and HDMA Channel 0..7 Registers", "SNES Memory Work RAM
//! Access"). H-DMA is not implemented; `$420C` stays inert in the ring.

use super::*;

/// B-bus port offsets per transfer unit, indexed by DMAP bits 2-0 (fullsnes
/// 43x0h: modes 5-7 behave as the listed repeats of modes 1-3).
const UNIT: [&[u8]; 8] = [
    &[0],
    &[0, 1],
    &[0, 0],
    &[0, 0, 1, 1],
    &[0, 1, 2, 3],
    &[0, 1, 0, 1],
    &[0, 0],
    &[0, 0, 1, 1],
];

/// Whether a 24-bit A-bus address reaches WRAM (`$7E-$7F` or the low-8K
/// system-bank mirror). WRAM-to-WRAM DMA is impossible in either direction
/// (fullsnes 2183h "DMA Notes") — the one WRAM chip cannot serve both bus
/// sides of the same access.
fn a_bus_is_wram(addr: u32) -> bool {
    let bank = (addr >> 16) as u8;
    bank == 0x7E || bank == 0x7F || (bank & 0x40 == 0 && addr & 0xFFFF < 0x2000)
}

impl SgbCoprocessor {
    /// Execute a captured MDMAEN (`$420B`) write: enabled channels run
    /// channel 0 first through 7 last, back to back, and the enable bits
    /// self-clear at completion (fullsnes 420Bh) — so nothing is stored. The
    /// guest CPU has been stalled since the `$420B` write; `flush` releases
    /// it after the ring (including this transfer) is applied.
    pub(crate) fn run_gp_dma(&mut self, mask: u8) {
        for ch in 0..8 {
            if mask & 1 << ch != 0 {
                self.dma_channel(ch);
            }
        }
    }

    /// One channel's whole GP transfer. The CPU-pause timing (~8 master
    /// cycles per byte) is not modeled — the stall already suspends the
    /// guest across the transfer.
    fn dma_channel(&mut self, ch: usize) {
        let [dmap, bbad, a1l, a1h, a1b, dasl, dash] = self.dma_regs[ch];
        let mut a_addr = u16::from(a1l) | u16::from(a1h) << 8;
        let b_to_a = dmap & 0x80 != 0;
        // A-bus step, DMAP bits 4-3: 0 = increment, 2 = decrement, 1/3 =
        // fixed. The bank byte never steps (fullsnes 43x4h).
        let step = match dmap >> 3 & 3 {
            0 => 1i16,
            2 => -1,
            _ => 0,
        };
        let unit = UNIT[usize::from(dmap & 7)];
        // A 16-bit *byte* counter (not units); 0 means $10000 (fullsnes
        // 43x5h/43x6h).
        let count = match usize::from(dasl) | usize::from(dash) << 8 {
            0 => 0x1_0000,
            n => n,
        };
        for i in 0..count {
            let b_port = bbad.wrapping_add(unit[i % unit.len()]);
            let a24 = u32::from(a1b) << 16 | u32::from(a_addr);
            // ponytail: one wasm crossing per byte; bulk the A-bus side when
            // PPU VRAM uploads (the snes-ppu routing) make size matter.
            let wram_clash = (0x80..=0x83).contains(&b_port) && a_bus_is_wram(a24);
            if !wram_clash {
                if b_to_a {
                    let v = self.bbus_read(b_port);
                    let _ = self.cpu.get_mut().write_ram(a24, &[v]);
                } else {
                    let v = self
                        .cpu
                        .get_mut()
                        .read_ram(a24, 1)
                        .ok()
                        .and_then(|b| b.first().copied())
                        .unwrap_or(0);
                    self.bbus_write(b_port, v);
                }
            }
            a_addr = a_addr.wrapping_add_signed(step);
        }
        // The working registers end stepped: A1T at the final address, DAS
        // decremented to zero (fullsnes 43x2h "DMA Current Addr" / 43x5h
        // "contains 0000h on end").
        self.dma_regs[ch][2] = a_addr as u8;
        self.dma_regs[ch][3] = (a_addr >> 8) as u8;
        self.dma_regs[ch][5] = 0;
        self.dma_regs[ch][6] = 0;
    }

    /// A DMA write to B-bus port `$21xx` routes through the same consumer as
    /// a captured CPU write — when PPU routing lands there, DMA feeds it for
    /// free.
    fn bbus_write(&mut self, port: u8, val: u8) {
        self.apply_mmio(0x2100 | u16::from(port), val);
    }

    /// A DMA read from a B-bus port. Only WMDATA reads back today; the rest
    /// of the B-bus is open bus (reads 0) until the PPU lands.
    fn bbus_read(&mut self, port: u8) -> u8 {
        match port {
            0x80 => self.wmdata_read(),
            _ => 0,
        }
    }

    /// `$2180` WMDATA write: the byte lands in WRAM at WMADD, which then
    /// increments (fullsnes 2180h), wrapping within the 128 KB.
    pub(crate) fn wmdata_write(&mut self, v: u8) {
        let _ = self.cpu.get_mut().write_ram(0x7E_0000 + self.wmadd, &[v]);
        self.wmadd = (self.wmadd + 1) & 0x1_FFFF;
    }

    /// `$2180` WMDATA read (the DMA B→A half; the guest's own `$2180` reads
    /// are open bus for now — the address state machine lives host-side).
    fn wmdata_read(&mut self) -> u8 {
        let v = self
            .cpu
            .get_mut()
            .read_ram(0x7E_0000 + self.wmadd, 1)
            .ok()
            .and_then(|b| b.first().copied())
            .unwrap_or(0);
        self.wmadd = (self.wmadd + 1) & 0x1_FFFF;
        v
    }
}
