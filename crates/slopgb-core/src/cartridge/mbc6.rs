//! MBC6 (cartridge type 0x20): window banking + the MX29F008 flash chip.
//!
//! Pan Docs "MBC6": the switchable areas are half the usual size — two 8 KiB
//! ROM/flash windows (A at 0x4000-0x5FFF, B at 0x6000-0x7FFF) and two 4 KiB
//! RAM windows (A at 0xA000-0xAFFF, B at 0xB000-0xBFFF), each with its own
//! bank register. Either window can map the 1 MiB flash in place of the ROM,
//! putting the flash's JEDEC command interface directly on the cartridge bus.
//! Pinned end to end by the `roms/mbc6` exerciser (`tests/mbc6.rs`).

use super::*;

impl Mbc6Flash {
    pub(super) fn new() -> Self {
        Mbc6Flash {
            // A never-programmed flash reads all-ones, like an erased one.
            data: vec![0xFF; MBC6_FLASH_SIZE],
            hidden: [0xFF; 256],
            mode: FlashMode::Read,
            seq: 0,
            prefix: 0,
            protect: false,
            buf: [0xFF; 128],
            page: None,
            loaded: 0,
            busy: 0,
        }
    }

    /// Drop any in-flight page load (mode entry, abort, or commit done).
    fn page_reset(&mut self) {
        self.buf = [0xFF; 128];
        self.page = None;
        self.loaded = 0;
    }

    /// Status byte read back during/after operations: bit 7 = finished
    /// (0 while an embedded operation's `busy` time runs), bit 1 =
    /// sector 0 protected by the Protect Sector 0 command. The timeout
    /// bit 4 never rises: it reports an operation exceeding the chip's
    /// internal retry limit, a failure a healthy modeled chip cannot have.
    fn status(&self) -> u8 {
        let done = if self.busy == 0 { 0x80 } else { 0x00 };
        done | if self.protect { 0x02 } else { 0x00 }
    }

    /// May sector 0 be erased/programmed? Both protection layers must be
    /// open: the Flash Write Enable register bit (`we`, the /WP pin) and the
    /// command-set protect flag.
    fn sector0_writable(&self, we: bool) -> bool {
        we && !self.protect
    }

    fn read(&self, addr: usize) -> u8 {
        match self.mode {
            FlashMode::Read => self.data[addr],
            FlashMode::Id => {
                if addr & 1 == 0 {
                    0xC2
                } else {
                    0x81
                }
            }
            FlashMode::HiddenRead => self.hidden[addr & 0xFF],
            // Erase/program/protect operations "make the flash read out
            // status bytes instead of values" (Pan Docs).
            FlashMode::Program | FlashMode::ProgramHidden | FlashMode::Status => self.status(),
        }
    }

    /// A write with the chip selected. `we` is the Flash Write Enable
    /// register bit (the /WP pin), sampled per write.
    fn write(&mut self, addr: usize, value: u8, we: bool) {
        // A running embedded operation ignores the bus ($F0 included —
        // program/erase cannot be aborted on this part) until it elapses.
        if self.busy > 0 {
            return;
        }
        match self.mode {
            FlashMode::Program => self.program_write(addr, value, we, false),
            FlashMode::ProgramHidden => self.program_write(addr & 0xFF, value, we, true),
            _ => self.command_write(addr, value, we),
        }
    }

    /// A write during Program/ProgramHidden, following the chip's page
    /// protocol (Pan Docs): the first 128 writes load the page buffer —
    /// any value, $F0 included, is data — then a rewrite of the page's
    /// final address commits ("any value (except $F0)": the trigger's
    /// value is not data), while $F0 there aborts without programming.
    /// Programming can only clear bits (AND); only an erase sets them back.
    fn program_write(&mut self, addr: usize, value: u8, we: bool, hidden: bool) {
        if self.loaded < 128 {
            if self.page.is_none() {
                self.page = Some(addr & !0x7F);
            }
            self.buf[addr & 0x7F] = value;
            self.loaded += 1;
        } else if value == 0xF0 {
            self.page_reset();
            self.mode = FlashMode::Read;
        } else if self.page == Some(addr & !0x7F) && addr & 0x7F == 0x7F {
            self.commit_page(we, hidden);
        }
        // Any other write while the commit is pending is ignored.
    }

    /// Apply a completed page load. Sector 0 and the hidden region commit
    /// only when write-enabled — a blocked commit never starts: back to
    /// read mode with nothing applied.
    fn commit_page(&mut self, we: bool, hidden: bool) {
        let Some(page) = self.page else { return };
        let allowed = if hidden {
            we
        } else {
            page >= MBC6_FLASH_SECTOR_SIZE || self.sector0_writable(we)
        };
        self.mode = if allowed {
            if hidden {
                // The hidden region has two 128-byte pages.
                let base = page & 0x80;
                for (i, &b) in self.buf.iter().enumerate() {
                    self.hidden[base + i] &= b;
                }
            } else {
                for (i, &b) in self.buf.iter().enumerate() {
                    self.data[page + i] &= b;
                }
            }
            self.busy = MBC6_FLASH_PROGRAM_CYCLES;
            FlashMode::Status
        } else {
            FlashMode::Read
        };
        self.page_reset();
    }

    /// The JEDEC unlock state machine: $AA to $5555, $55 to $2AAA, then the
    /// command byte (flash addresses; the MBC6 banking arranges them as
    /// 2:5555/1:4AAA on the cartridge bus).
    fn command_write(&mut self, addr: usize, value: u8, we: bool) {
        // $F0 exits any command mode (MX29F008 reset-to-read).
        if value == 0xF0 {
            self.mode = FlashMode::Read;
            self.seq = 0;
            self.prefix = 0;
            return;
        }
        match (self.seq, addr, value) {
            (0, 0x5555, 0xAA) => self.seq = 1,
            (1, 0x2AAA, 0x55) => self.seq = 2,
            (2, _, _) => {
                self.seq = 0;
                self.dispatch(addr, value, we);
            }
            // An out-of-sequence cycle resets the whole JEDEC state
            // machine, including a pending two-cycle command prefix.
            _ => {
                self.seq = 0;
                self.prefix = 0;
            }
        }
    }

    /// The command byte after a completed unlock. Single-cycle commands act
    /// immediately; the erase/extended/hidden-read families set a prefix and
    /// need a second unlock + command byte.
    fn dispatch(&mut self, addr: usize, value: u8, we: bool) {
        let prefix = self.prefix;
        self.prefix = 0;
        match (prefix, value) {
            (0, 0x80) | (0, 0x60) | (0, 0x77) if addr == 0x5555 => self.prefix = value,
            (0, 0x90) if addr == 0x5555 => self.mode = FlashMode::Id,
            (0, 0xA0) if addr == 0x5555 => {
                self.page_reset();
                self.mode = FlashMode::Program;
            }
            // Erase the 128 KiB sector containing the addressed byte; the
            // low address bits are irrelevant (Pan Docs). A write-protected
            // operation never starts: the chip stays in read mode (no
            // status byte ever shows "finished" — the exerciser ROM relies
            // on reads returning array data after a blocked op).
            (0x80, 0x30) => {
                self.mode = if self.erase_sector(addr / MBC6_FLASH_SECTOR_SIZE, we) {
                    self.busy = MBC6_FLASH_SECTOR_ERASE_CYCLES;
                    FlashMode::Status
                } else {
                    FlashMode::Read
                };
            }
            (0x80, 0x10) if addr == 0x5555 => {
                // Chip erase: all sectors; sector 0 only when unprotected
                // (its gate is inside erase_sector). The hidden region is
                // not touched (Pan Docs). Sectors 1-7 always run, so the
                // operation as a whole starts regardless of /WP.
                for sector in 0..MBC6_FLASH_SIZE / MBC6_FLASH_SECTOR_SIZE {
                    self.erase_sector(sector, we);
                }
                self.busy = MBC6_FLASH_CHIP_ERASE_CYCLES;
                self.mode = FlashMode::Status;
            }
            (0x60, 0x04) if addr == 0x5555 => {
                self.mode = if we {
                    self.hidden = [0xFF; 256];
                    self.busy = MBC6_FLASH_SECTOR_ERASE_CYCLES;
                    FlashMode::Status
                } else {
                    FlashMode::Read
                };
            }
            (0x60, 0xE0) if addr == 0x5555 => {
                // Hidden-region programming is /WP-gated as a whole.
                self.mode = if we {
                    self.page_reset();
                    FlashMode::ProgramHidden
                } else {
                    FlashMode::Read
                };
            }
            (0x60, 0x40) if addr == 0x5555 => {
                self.mode = if we {
                    self.protect = false;
                    self.busy = MBC6_FLASH_PROGRAM_CYCLES;
                    FlashMode::Status
                } else {
                    FlashMode::Read
                };
            }
            (0x60, 0x20) if addr == 0x5555 => {
                self.mode = if we {
                    self.protect = true;
                    self.busy = MBC6_FLASH_PROGRAM_CYCLES;
                    FlashMode::Status
                } else {
                    FlashMode::Read
                };
            }
            (0x77, 0x77) if addr == 0x5555 => self.mode = FlashMode::HiddenRead,
            _ => {}
        }
    }

    /// Erase one 128 KiB sector to 0xFF. Sector 0 is skipped while either
    /// protection layer is closed; returns whether the erase ran.
    fn erase_sector(&mut self, sector: usize, we: bool) -> bool {
        if sector == 0 && !self.sector0_writable(we) {
            return false;
        }
        let start = sector * MBC6_FLASH_SECTOR_SIZE;
        self.data[start..start + MBC6_FLASH_SECTOR_SIZE].fill(0xFF);
        true
    }
}

impl Cartridge {
    /// The (bank, flash-mapped) pair for a CPU address in a switchable
    /// window (0x4000-0x5FFF = A, 0x6000-0x7FFF = B).
    fn mbc6_window(&self, addr: u16) -> (usize, bool) {
        let Mapper::Mbc6 {
            romb_a,
            romb_b,
            flash_a,
            flash_b,
            ..
        } = self.mapper
        else {
            unreachable!("mbc6_window on a non-MBC6 mapper");
        };
        if addr < 0x6000 {
            (usize::from(romb_a), flash_a)
        } else {
            (usize::from(romb_b), flash_b)
        }
    }

    pub(super) fn mbc6_read_rom(&self, addr: u16) -> u8 {
        let Mapper::Mbc6 {
            flash_enable,
            ref flash,
            ..
        } = self.mapper
        else {
            unreachable!("mbc6_read_rom on a non-MBC6 mapper");
        };
        if addr < 0x4000 {
            return self.rom[usize::from(addr) & (self.rom.len() - 1)];
        }
        let (bank, mapped_flash) = self.mbc6_window(addr);
        let off = bank * MBC6_ROM_BANK_SIZE + usize::from(addr & 0x1FFF);
        if mapped_flash {
            // /CE deasserted: the chip does not drive the bus.
            if flash_enable { flash.read(off) } else { 0xFF }
        } else {
            self.rom[off & (self.rom.len() - 1)]
        }
    }

    /// Byte-exact ROM offset for the CDL. A flash-mapped window reports the
    /// ROM byte the same bank would address (the CDL tracks ROM only).
    pub(super) fn mbc6_rom_offset(&self, addr: u16) -> usize {
        if addr < 0x4000 {
            return usize::from(addr) & (self.rom.len() - 1);
        }
        let (bank, _) = self.mbc6_window(addr);
        (bank * MBC6_ROM_BANK_SIZE + usize::from(addr & 0x1FFF)) & (self.rom.len() - 1)
    }

    pub(super) fn mbc6_write_rom(&mut self, addr: u16, value: u8) {
        let Mapper::Mbc6 {
            ramg,
            ramb_a,
            ramb_b,
            romb_a,
            romb_b,
            flash_a,
            flash_b,
            flash_enable,
            flash_we,
            flash,
        } = &mut self.mapper
        else {
            unreachable!("mbc6_write_rom on a non-MBC6 mapper");
        };
        match addr {
            // Pan Docs: "mostly the same as for MBC1" — the low nibble 0x0A
            // enables both RAM windows.
            0x0000..=0x03FF => *ramg = value & 0x0F == 0x0A,
            0x0400..=0x07FF => *ramb_a = value & 0x07,
            0x0800..=0x0BFF => *ramb_b = value & 0x07,
            0x0C00..=0x0FFF => *flash_enable = value & 0x01 != 0,
            // Pan Docs places this register at 0x1000 without a range;
            // decode it at the 1 KiB granularity of its neighbors.
            0x1000..=0x13FF => *flash_we = value & 0x01 != 0,
            0x2000..=0x27FF => *romb_a = value & 0x7F,
            // 0x00 maps the ROM, 0x08 the flash: bit 3 selects.
            0x2800..=0x2FFF => *flash_a = value & 0x08 != 0,
            0x3000..=0x37FF => *romb_b = value & 0x7F,
            0x3800..=0x3FFF => *flash_b = value & 0x08 != 0,
            // Window writes reach the flash chip when it is mapped there
            // and enabled; a ROM-mapped window ignores writes.
            0x4000..=0x7FFF => {
                let (bank, mapped_flash) = if addr < 0x6000 {
                    (*romb_a, *flash_a)
                } else {
                    (*romb_b, *flash_b)
                };
                if mapped_flash && *flash_enable {
                    let off = usize::from(bank) * MBC6_ROM_BANK_SIZE + usize::from(addr & 0x1FFF);
                    flash.write(off, value, *flash_we);
                }
            }
            // No registers at 0x1400-0x1FFF; write_rom is never called
            // above 0x7FFF.
            _ => {}
        }
    }

    /// RAM byte index for an MBC6 window access: `base` is the selected
    /// bank's byte offset, the window spans 4 KiB. Mirrors across smaller
    /// RAM chips like `ram_index`; `None` without a RAM chip.
    pub(super) fn mbc6_ram_index(&self, base: usize, addr: u16) -> Option<usize> {
        if self.ram.is_empty() {
            return None;
        }
        debug_assert!(self.ram.len().is_power_of_two());
        Some((base + usize::from(addr & 0x0FFF)) & (self.ram.len() - 1))
    }
}
