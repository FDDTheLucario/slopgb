//! APU RAM + the 64-byte IPL boot ROM overlay, and the address decoder.
//!
//! Memory map (fullsnes, "SNES APU Memory Map"):
//! - `$0000-$00EF` — RAM (direct page 0)
//! - `$00F0-$00FF` — I/O registers (see [`super::ports`])
//! - `$0100-$01FF` — RAM (page 1, stack; direct page 1 when `P=1`)
//! - `$0200-$FFBF` — RAM
//! - `$FFC0-$FFFF` — IPL ROM when `$F1` bit 7 is set, else RAM
//!
//! RAM underlies the whole space, including the IPL region: reads there return
//! the ROM while it's mapped, but writes always fall through to RAM.

use super::*;

/// The 64-byte SPC700 IPL boot ROM (`$FFC0-$FFFF`). This is the well-known,
/// invariant SNES APU boot loader; the reset vector at `$FFFE/$FFFF` = `$FFC0`
/// points at its start. Bytes verified against nocash **fullsnes** ("SNES APU
/// I/O Ports — IPL Boot ROM") and bsnes `SMP::iplrom`.
pub(super) const IPL_ROM: [u8; 64] = [
    0xCD, 0xEF, 0xBD, 0xE8, 0x00, 0xC6, 0x1D, 0xD0, // FFC0
    0xFC, 0x8F, 0xAA, 0xF4, 0x8F, 0xBB, 0xF5, 0x78, // FFC8
    0xCC, 0xF4, 0xD0, 0xFB, 0x2F, 0x19, 0xEB, 0xF4, // FFD0
    0xD0, 0xFC, 0x7E, 0xF4, 0xD0, 0x0B, 0xE4, 0xF5, // FFD8
    0xCB, 0xF4, 0xD7, 0x00, 0xFC, 0xD0, 0xF3, 0xAB, // FFE0
    0x01, 0x10, 0xEF, 0x7E, 0xF4, 0x10, 0xEB, 0xBA, // FFE8
    0xF6, 0xDA, 0x00, 0xBA, 0xF4, 0xC4, 0xF4, 0xDD, // FFF0
    0x5D, 0xD0, 0xDB, 0x1F, 0x00, 0x00, 0xC0, 0xFF, // FFF8
];

impl Spc700 {
    /// `true` when the IPL ROM is currently mapped over `$FFC0-$FFFF`.
    pub(super) fn ipl_enabled(&self) -> bool {
        self.control & 0x80 != 0
    }

    /// Read a byte with full address decoding (I/O ports + IPL overlay). In
    /// flat-memory mode every address is plain RAM (`SingleStepTests` harness).
    pub(super) fn read8(&mut self, addr: u16) -> u8 {
        if self.flat_mem {
            return self.ram[addr as usize];
        }
        match addr {
            0x00F0..=0x00FF => self.io_read(addr as u8),
            0xFFC0..=0xFFFF if self.ipl_enabled() => IPL_ROM[(addr - 0xFFC0) as usize],
            _ => self.ram[addr as usize],
        }
    }

    /// Write a byte with full address decoding. Writes to the IPL region fall
    /// through to the underlying RAM (the ROM shadows reads only).
    pub(super) fn write8(&mut self, addr: u16, v: u8) {
        if self.flat_mem {
            self.ram[addr as usize] = v;
            return;
        }
        match addr {
            0x00F0..=0x00FF => self.io_write(addr as u8, v),
            _ => self.ram[addr as usize] = v,
        }
    }
}
