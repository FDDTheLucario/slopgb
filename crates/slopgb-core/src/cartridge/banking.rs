//! ROM/RAM banking + mapper register writes, and the debug offset accessors.

use super::*;

impl Cartridge {
    /// `rom.len()` is a power of two, so this masks bank numbers the way the
    /// unconnected upper address lines of the actual chip do.
    fn rom_bank_mask(&self) -> usize {
        self.rom.len() / ROM_BANK_SIZE - 1
    }

    fn rom_at(&self, bank: usize, addr: u16) -> u8 {
        let offset = (bank & self.rom_bank_mask()) * ROM_BANK_SIZE + usize::from(addr & 0x3FFF);
        self.rom[offset]
    }

    /// Read a ROM byte at an **explicit** bank (bank-masked/wrapped exactly like
    /// the live mapper, so an out-of-range bank folds back in and never indexes
    /// OOB), for the MCP/debug banked disassembler + memory dump. Only the low
    /// 14 bits of `addr` matter. Side-effect-free (`&self`).
    #[must_use]
    pub fn rom_read_banked(&self, bank: u16, addr: u16) -> u8 {
        self.rom_at(usize::from(bank), addr)
    }

    /// The ROM bank number the mapper selects for the given address area
    /// (`low_area` = 0x0000-0x3FFF vs the switchable 0x4000-0x7FFF), pre-mask.
    /// Shared by [`read_rom`](Self::read_rom) and the debug
    /// [`cur_rom_bank`](Self::cur_rom_bank) so the two cannot disagree.
    fn rom_bank_for(&self, low_area: bool) -> usize {
        match self.mapper {
            Mapper::None => usize::from(!low_area),
            Mapper::Mbc1 {
                bank1,
                bank2,
                mode,
                multicart,
                ..
            } => {
                // gbctr: BANK2 drives the two ROM address lines above BANK1.
                // Multicart wiring leaves BANK1 bit 4 unconnected, so BANK2
                // shifts down to bits 4-5.
                let (shift, bank1_mask) = if multicart { (4, 0x0F) } else { (5, 0x1F) };
                let high_bits = usize::from(bank2) << shift;
                if low_area {
                    // Mode 0 forces zeros on the upper lines for 0x0000-0x3FFF.
                    if mode { high_bits } else { 0 }
                } else {
                    high_bits | usize::from(bank1 & bank1_mask)
                }
            }
            Mapper::Mbc2 { romb, .. } => {
                if low_area {
                    0
                } else {
                    usize::from(romb)
                }
            }
            Mapper::Mbc3 { romb, .. } => {
                if low_area {
                    0
                } else {
                    usize::from(romb)
                }
            }
            Mapper::Mbc5 { romb0, romb1, .. } => {
                if low_area {
                    0
                } else {
                    usize::from(romb1) << 8 | usize::from(romb0)
                }
            }
        }
    }

    /// Read 0x0000-0x7FFF (banked ROM), applying any Game Genie patch. The patch
    /// list is empty in production, so this is byte-identical there (the empty
    /// check is skipped and the raw ROM byte returns unchanged).
    pub fn read_rom(&self, addr: u16) -> u8 {
        let byte = self.rom_at(self.rom_bank_for(addr < 0x4000), addr);
        if self.gg.is_empty() {
            return byte;
        }
        for p in &self.gg {
            // 6-digit codes have no compare (unconditional); 9-digit patch only
            // when the current byte matches (so bank-switched code stays correct).
            if p.addr == addr && p.compare.is_none_or(|c| c == byte) {
                return p.value;
            }
        }
        byte
    }

    /// Set the Game Genie ROM patches (from the frontend cheat engine). Empty =
    /// no patching = byte-identical `read_rom`. A default-off mutating debug hook.
    pub fn set_gg_patches(&mut self, patches: Vec<GgPatch>) {
        self.gg = patches;
    }

    /// The ROM bank currently mapped at 0x4000-0x7FFF (size-masked the way the
    /// chip's unconnected address lines wrap), for the debug bank indicator.
    /// Side-effect-free.
    #[must_use]
    pub fn cur_rom_bank(&self) -> usize {
        self.rom_bank_for(false) & self.rom_bank_mask()
    }

    /// The external-RAM bank currently visible at 0xA000, or `None` when RAM is
    /// disabled/absent or an RTC register (not a RAM bank) is mapped instead —
    /// for the debug bank indicator. Side-effect-free.
    #[must_use]
    pub fn cur_ram_bank(&self) -> Option<usize> {
        // A cart with no RAM chip has no bank to report, even if a mapper would
        // nominally select one (e.g. the None mapper, or RAMG enabled with no
        // chip) — the indicator shows "--" rather than a phantom bank 0.
        if self.ram.is_empty() {
            return None;
        }
        match self.ram_target()? {
            RamTarget::Ram(bank) => Some(bank),
            // MBC2 has a single built-in 512×4-bit RAM: "bank 0".
            RamTarget::Mbc2 => Some(0),
            // An RTC register is mapped, not a RAM bank.
            RamTarget::Rtc(_) => None,
        }
    }

    /// Physical ROM offset for a CPU address in 0x0000-0x7FFF (bank-resolved and
    /// size-masked, so it indexes the same byte [`read_rom`](Self::read_rom)
    /// returns) — for the bank-aware CDL. Side-effect-free.
    #[must_use]
    pub fn rom_offset(&self, addr: u16) -> usize {
        (self.rom_bank_for(addr < 0x4000) & self.rom_bank_mask()) * ROM_BANK_SIZE
            + usize::from(addr & 0x3FFF)
    }

    /// Physical external-RAM offset for an **explicit** RAM bank (`ram_index`
    /// masked to the chip size, so an out-of-range bank folds in), or `None` when
    /// the cart has no RAM — for the MCP/debug banked SRAM dump + CDL. Reads raw
    /// RAM bytes ignoring RAMG/RTC mapping; MBC2's 512×4 chip mirrors and only the
    /// low nibble is meaningful. Side-effect-free.
    #[must_use]
    pub fn ram_offset_banked(&self, bank: u16, addr: u16) -> Option<usize> {
        self.ram_index(usize::from(bank), addr)
    }

    /// Read an explicit RAM bank for the debug memory dump (open-bus `0xFF` with
    /// no RAM chip), the SRAM analogue of [`rom_read_banked`](Self::rom_read_banked).
    /// Side-effect-free.
    #[must_use]
    pub fn ram_read_banked(&self, bank: u16, addr: u16) -> u8 {
        self.ram_offset_banked(bank, addr)
            .map_or(0xFF, |i| self.ram[i])
    }

    /// Write raw bytes to an explicit RAM bank for the debug memory editor
    /// (no-op with no RAM chip), the SRAM analogue of the banked read. Bypasses
    /// RAMG so a paused debugger can poke a disabled/other bank; stores the raw
    /// byte (MBC2 keeps only the low nibble on a real read). Debug-only.
    pub fn ram_write_banked(&mut self, bank: u16, addr: u16, value: u8) {
        if let Some(i) = self.ram_offset_banked(bank, addr) {
            self.ram[i] = value;
        }
    }

    /// Physical external-RAM offset for a CPU address in 0xA000-0xBFFF, or `None`
    /// when no RAM byte is addressed there (RAM disabled/absent, or an RTC
    /// register mapped) — for the bank-aware CDL. Side-effect-free.
    #[must_use]
    pub fn ram_offset(&self, addr: u16) -> Option<usize> {
        match self.ram_target()? {
            RamTarget::Ram(bank) => self.ram_index(bank, addr),
            // MBC2's 512×4-bit RAM mirrors across the window at addr & 0x1FF.
            RamTarget::Mbc2 => Some(usize::from(addr & 0x1FF)),
            // An RTC register is not a RAM byte.
            RamTarget::Rtc(_) => None,
        }
    }

    /// Physical ROM size in bytes (power-of-two padded), for the CDL layout.
    #[must_use]
    pub fn rom_len(&self) -> usize {
        self.rom.len()
    }

    /// Physical external-RAM size in bytes (0 when the cart has no RAM chip),
    /// for the CDL layout.
    #[must_use]
    pub fn ram_len(&self) -> usize {
        self.ram.len()
    }

    /// Write 0x0000-0x7FFF (mapper registers).
    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match &mut self.mapper {
            Mapper::None => {}
            Mapper::Mbc1 {
                ramg,
                bank1,
                bank2,
                mode,
                ..
            } => match addr {
                // gbctr: RAMG compares only the low nibble against 0b1010.
                0x0000..=0x1FFF => *ramg = value & 0x0F == 0x0A,
                // BANK1 is 5 bits; the all-zeros value is bumped to 1 *on the
                // raw 5-bit register value*, before any ROM-size masking
                // (mbc1/rom_512kb: writing 0x10 selects bank 16 & mask = 0,
                // but writing 0x00 selects bank 1).
                0x2000..=0x3FFF => {
                    *bank1 = value & 0x1F;
                    if *bank1 == 0 {
                        *bank1 = 1;
                    }
                }
                0x4000..=0x5FFF => *bank2 = value & 0x03,
                _ => *mode = value & 0x01 != 0,
            },
            Mapper::Mbc2 { ramg, romb } => {
                // gbctr: a single register range 0x0000-0x3FFF; address bit 8
                // selects RAMG (0) or ROMB (1). 0x4000-0x7FFF does nothing
                // (mbc2/bits_unused).
                if addr < 0x4000 {
                    if addr & 0x0100 == 0 {
                        *ramg = value & 0x0F == 0x0A;
                    } else {
                        *romb = value & 0x0F;
                        if *romb == 0 {
                            *romb = 1;
                        }
                    }
                }
            }
            Mapper::Mbc3 {
                ramg,
                romb,
                ramb,
                rtc,
                mbc30,
            } => match addr {
                0x0000..=0x1FFF => *ramg = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    // MBC3 wires 7 ROM-bank lines, MBC30 all 8 (Pan Docs
                    // "MBC3"; SameBoy Core/mbc.c masks only non-MBC30).
                    // Zero substitution applies to the masked value.
                    *romb = value & if *mbc30 { 0xFF } else { 0x7F };
                    if *romb == 0 {
                        *romb = 1;
                    }
                }
                0x4000..=0x5FFF => *ramb = value & 0x0F,
                _ => {
                    if let Some(rtc) = rtc {
                        rtc.write_latch(value);
                    }
                }
            },
            Mapper::Mbc5 {
                ramg,
                romb0,
                romb1,
                ramb,
                rumble_cart,
                rumble,
            } => match addr {
                // gbctr: unlike MBC1, MBC5 compares the full 8-bit value;
                // only exactly 0x0A enables RAM.
                0x0000..=0x1FFF => *ramg = value == 0x0A,
                0x2000..=0x2FFF => *romb0 = value,
                0x3000..=0x3FFF => *romb1 = value & 0x01,
                0x4000..=0x5FFF => {
                    if *rumble_cart {
                        // Pan Docs: on rumble carts RAMB bit 3 drives the
                        // motor and is not part of the RAM bank number.
                        *rumble = value & 0x08 != 0;
                        *ramb = value & 0x07;
                    } else {
                        *ramb = value & 0x0F;
                    }
                }
                // No register at 0x6000-0x7FFF on MBC5.
                _ => {}
            },
        }
    }

    /// RAM byte index for `bank`/`addr`, mirrored at the RAM chip size
    /// (always a power of two), or None if there is no RAM at all.
    fn ram_index(&self, bank: usize, addr: u16) -> Option<usize> {
        if self.ram.is_empty() {
            return None;
        }
        // The mask below mirrors instead of corrupting only because every
        // RAM size chosen in `from_bytes` is a power of two; catch any
        // future size-code addition that breaks this.
        debug_assert!(self.ram.len().is_power_of_two());
        Some((bank * RAM_BANK_SIZE + usize::from(addr & 0x1FFF)) & (self.ram.len() - 1))
    }

    /// Which RAM bank (or RTC register) is currently visible at 0xA000.
    /// Returns None when the area is unmapped or disabled.
    fn ram_target(&self) -> Option<RamTarget> {
        match &self.mapper {
            Mapper::None => Some(RamTarget::Ram(0)),
            Mapper::Mbc1 {
                ramg, bank2, mode, ..
            } => {
                if !*ramg {
                    return None;
                }
                // gbctr: in mode 0 the RAM address lines from BANK2 are 0.
                Some(RamTarget::Ram(if *mode { usize::from(*bank2) } else { 0 }))
            }
            Mapper::Mbc2 { ramg, .. } => ramg.then_some(RamTarget::Mbc2),
            Mapper::Mbc3 {
                ramg, ramb, rtc, ..
            } => {
                if !*ramg {
                    return None;
                }
                match *ramb {
                    // 0x00-0x07 to support MBC30 (8 RAM banks); smaller RAM
                    // chips mirror via `ram_index`.
                    0x00..=0x07 => Some(RamTarget::Ram(usize::from(*ramb))),
                    0x08..=0x0C if rtc.is_some() => Some(RamTarget::Rtc(usize::from(*ramb - 0x08))),
                    _ => None,
                }
            }
            Mapper::Mbc5 { ramg, ramb, .. } => ramg.then_some(RamTarget::Ram(usize::from(*ramb))),
        }
    }

    /// Read 0xA000-0xBFFF (external RAM / RTC / MBC2 built-in RAM).
    pub fn read_ram(&self, addr: u16) -> u8 {
        match self.ram_target() {
            // Disabled or absent RAM reads as open bus 0xFF.
            None => 0xFF,
            Some(RamTarget::Ram(bank)) => match self.ram_index(bank, addr) {
                Some(i) => self.ram[i],
                None => 0xFF,
            },
            // MBC2: 512 half-bytes mirrored across the whole window; the
            // upper data bits are not driven and read as 1s.
            Some(RamTarget::Mbc2) => 0xF0 | self.ram[usize::from(addr & 0x1FF)],
            Some(RamTarget::Rtc(reg)) => match &self.mapper {
                // Reads see the *latched* registers (Pan Docs).
                Mapper::Mbc3 { rtc: Some(rtc), .. } => rtc.latched[reg],
                // `ram_target` yields Rtc only for Mbc3 with rtc.is_some().
                _ => unreachable!("RamTarget::Rtc implies MBC3 with RTC"),
            },
        }
    }

    /// Write 0xA000-0xBFFF.
    pub fn write_ram(&mut self, addr: u16, value: u8) {
        match self.ram_target() {
            None => {}
            Some(RamTarget::Ram(bank)) => {
                if let Some(i) = self.ram_index(bank, addr) {
                    self.ram[i] = value;
                }
            }
            Some(RamTarget::Mbc2) => self.ram[usize::from(addr & 0x1FF)] = value & 0x0F,
            Some(RamTarget::Rtc(reg)) => {
                if let Mapper::Mbc3 { rtc: Some(rtc), .. } = &mut self.mapper {
                    rtc.write_reg(reg, value);
                }
            }
        }
    }
}
