//! Routing for the guest's captured SNES MMIO writes: the batched pure-PPU
//! port run, and the per-register dispatch that hands MSU-1, WRAM-access,
//! CPU I/O, GP-DMA and PPU writes to their consumers.

use super::*;

impl SgbCoprocessor {
    /// Apply a buffered run of pure-PPU `(port, val)` pairs as one batched
    /// plugin call, in order, and clear the buffer. No-op when empty or
    /// without a PPU plugin (matching the unbatched path's routing).
    pub(crate) fn flush_ppu_run(&mut self, run: &mut Vec<u8>) {
        if run.is_empty() {
            return;
        }
        if let Some(ppu) = &self.ppu {
            let _ = ppu.borrow_mut().write_ram(PPU_HW_PORTS, run);
        }
        run.clear();
    }

    /// Apply one captured MMIO write from the guest (also the target of DMA
    /// B-bus writes — `dma::bbus_write` routes through here). `$2000-$2007`
    /// reaches the MSU-1 plugin; the clocking loop consumes NMITIMEN; the DMA
    /// engine consumes the channel registers, MDMAEN, and the WRAM access
    /// ports; the rest of the B bus (`$2100-$21FF`) goes to the PPU plugin.
    /// Anything else is inert.
    pub(crate) fn apply_mmio(&mut self, addr: u16, val: u8) {
        match addr {
            // MSU-1 register window: the resident 65C816 handler a game's SGB
            // driver uploaded writes seek/track/volume/control here, and the host
            // forwards them to the MSU-1 plugin (comm port n == `$2000 + n`).
            0x2000..=0x2007 => {
                if let Some(msu) = &self.msu {
                    let _ = msu.borrow_mut().port_write((addr - 0x2000) as u8, val);
                }
            }
            0x2180 => self.wmdata_write(val),
            0x2181 => self.wmadd = self.wmadd & 0x1_FF00 | u32::from(val),
            0x2182 => self.wmadd = self.wmadd & 0x1_00FF | u32::from(val) << 8,
            // WMADDH: one bit — WMADD addresses 128 KB (fullsnes 2183h).
            0x2183 => self.wmadd = self.wmadd & 0xFFFF | u32::from(val & 1) << 16,
            0x4200 => self.nmitimen = val,
            0x420B => self.run_gp_dma(val),
            0x4300..=0x437F if usize::from(addr & 0xF) < 7 => {
                self.dma_regs[usize::from(addr >> 4 & 7)][usize::from(addr & 0xF)] = val;
            }
            // Every other B-bus port belongs to the PPU when one is loaded
            // (unknown ports are inert inside the chip). $2140-$2143 only
            // arrive via DMA (the CPU-side APU ports route earlier) — a
            // DMA-to-APU transfer is unimplemented and lands inert too.
            0x2100..=0x21FF => {
                if (0x2102..=0x2103).contains(&addr) && std::env::var_os("SLOPGB_OAMDBG").is_some()
                {
                    eprintln!("OAMREG {addr:04X}={val:02X}");
                }
                if addr == 0x2100 {
                    self.last_inidisp = val; // diagnostics (debug_status)
                    // The display shows a picture: not force-blanked and
                    // brightness above zero (fullsnes 2100h). Arms the
                    // frame handoff — see `take_snes_frame`.
                    if val & 0x80 == 0 && val & 0x0F != 0 {
                        self.snes_live = true;
                    }
                }
                if let Some(ppu) = &self.ppu {
                    let _ = ppu.borrow_mut().port_write((addr - 0x2100) as u8, val);
                }
            }
            _ => {}
        }
    }
}
