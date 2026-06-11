//! Cartridge: header parsing and MBC mappers. Cartridge work package.
//!
//! Supported mappers: none (32 KiB), MBC1 (incl. 8 Mbit multicart detection),
//! MBC2, MBC3 (+RTC), MBC5. Mooneye `emulator-only/` is the oracle for
//! banking edge cases (register bit widths, RAMG gating, bank-0 aliasing,
//! mode 1 behavior, unused-bit masking).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    /// ROM image smaller than one bank / header incomplete.
    TooSmall,
    /// Cartridge-type byte (0x147) we do not support.
    UnsupportedMapper(u8),
    /// Declared ROM/RAM size inconsistent or unsupported.
    BadHeader,
}

impl fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CartridgeError::TooSmall => write!(f, "ROM image too small"),
            CartridgeError::UnsupportedMapper(t) => {
                write!(f, "unsupported cartridge type {t:#04x}")
            }
            CartridgeError::BadHeader => write!(f, "inconsistent cartridge header"),
        }
    }
}

impl std::error::Error for CartridgeError {}

/// The Nintendo logo bitmap found at 0x104 in every bootable ROM. Used for
/// MBC1 multicart detection (mooneye-gb heuristic).
const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

const ROM_BANK_SIZE: usize = 0x4000;
const RAM_BANK_SIZE: usize = 0x2000;
/// T-cycles (dots) per RTC second at the 4.194304 MHz master clock.
const CYCLES_PER_SECOND: u32 = 4_194_304;
/// Size of the RTC block appended to [`Cartridge::save_data`] images.
const RTC_SAVE_LEN: usize = 16;

/// MBC3 real-time clock. Driven deterministically from emulated cycles via
/// [`Cartridge::tick_rtc`]; never reads the host clock.
///
/// Register layout (gbctr / Pan Docs "MBC3 RTC"):
/// - S (0x08): seconds, 6-bit counter
/// - M (0x09): minutes, 6-bit counter
/// - H (0x0A): hours, 5-bit counter
/// - DL (0x0B): day counter low 8 bits
/// - DH (0x0C): bit 0 = day bit 8, bit 6 = halt, bit 7 = day carry (sticky)
///
/// Out-of-range values behave like real hardware (verified by rtc3test):
/// e.g. seconds 60..63 keep counting and wrap to 0 *without* a minute carry,
/// because the carry is generated only on the 59 -> 60 transition.
#[derive(Clone)]
struct Rtc {
    /// Live registers, indexed S, M, H, DL, DH. Stored pre-masked.
    regs: [u8; 5],
    /// Snapshot taken by the 0x00 -> 0x01 latch sequence.
    latched: [u8; 5],
    /// Sub-second T-cycle accumulator, < [`CYCLES_PER_SECOND`].
    subsec: u32,
    /// Last value written to the 0x6000-0x7FFF latch register.
    latch_prev: u8,
}

/// Write masks for the five RTC registers: unimplemented bits read back 0.
const RTC_MASKS: [u8; 5] = [0x3F, 0x3F, 0x1F, 0xFF, 0xC1];

const RTC_DH: usize = 4;
const RTC_HALT: u8 = 0x40;
const RTC_CARRY: u8 = 0x80;

impl Rtc {
    fn new() -> Self {
        Rtc {
            regs: [0; 5],
            latched: [0; 5],
            subsec: 0,
            // Power-on value chosen so a lone 0x01 write does not latch.
            latch_prev: 0xFF,
        }
    }

    fn write_latch(&mut self, value: u8) {
        // Pan Docs: writing 0x00 then 0x01 latches the current time.
        if self.latch_prev == 0x00 && value == 0x01 {
            self.latched = self.regs;
        }
        self.latch_prev = value;
    }

    fn write_reg(&mut self, index: usize, value: u8) {
        self.regs[index] = value & RTC_MASKS[index];
        if index == 0 {
            // Writing the seconds register resets the internal sub-second
            // divider (rtc3test "sub-second writes" on hardware).
            self.subsec = 0;
        }
    }

    fn halted(&self) -> bool {
        self.regs[RTC_DH] & RTC_HALT != 0
    }

    fn tick_cycles(&mut self, t_cycles: u32) {
        if self.halted() {
            return;
        }
        let total = u64::from(self.subsec) + u64::from(t_cycles);
        self.subsec = (total % u64::from(CYCLES_PER_SECOND)) as u32;
        for _ in 0..total / u64::from(CYCLES_PER_SECOND) {
            self.tick_second();
        }
    }

    fn tick_second(&mut self) {
        let [s, m, h, dl, dh] = &mut self.regs;
        // Each counter wraps at its bit width; the carry into the next
        // counter fires only when the nominal limit (60/60/24) is hit.
        *s = (*s + 1) & 0x3F;
        if *s != 60 {
            return;
        }
        *s = 0;
        *m = (*m + 1) & 0x3F;
        if *m != 60 {
            return;
        }
        *m = 0;
        *h = (*h + 1) & 0x1F;
        if *h != 24 {
            return;
        }
        *h = 0;
        let day = ((u16::from(*dh & 0x01) << 8) | u16::from(*dl)) + 1;
        *dl = day as u8;
        *dh = (*dh & !0x01) | ((day >> 8) as u8 & 0x01);
        if day == 512 {
            // 9-bit day counter overflowed: sticky carry flag.
            *dh |= RTC_CARRY;
        }
    }
}

enum Mapper {
    /// 0x00, 0x08, 0x09: ROM directly mapped, optional always-enabled RAM.
    None,
    /// 0x01-0x03. `multicart` switches to MBC1M wiring: BANK1 is 4 bits wide
    /// and BANK2 drives ROM address bits 18-19 instead of 19-20.
    Mbc1 {
        ramg: bool,
        bank1: u8,
        bank2: u8,
        mode: bool,
        multicart: bool,
    },
    /// 0x05, 0x06: 4-bit ROMB, 512 half-bytes of built-in RAM.
    Mbc2 { ramg: bool, romb: u8 },
    /// 0x0F-0x13: 7-bit ROMB, RAM banks 0-7 / RTC registers 0x08-0x0C.
    Mbc3 {
        ramg: bool,
        romb: u8,
        ramb: u8,
        rtc: Option<Rtc>,
    },
    /// 0x19-0x1E: 9-bit ROM bank (0 allowed), 4-bit RAMB, optional rumble.
    Mbc5 {
        ramg: bool,
        romb0: u8,
        romb1: u8,
        ramb: u8,
        rumble_cart: bool,
        rumble: bool,
    },
}

pub struct Cartridge {
    /// ROM image, padded with 0xFF to a power of two (>= 32 KiB) so the bank
    /// mask mirrors reads the way unconnected high ROM address lines do.
    rom: Vec<u8>,
    /// External RAM (for MBC2: 512 entries, low nibble significant).
    ram: Vec<u8>,
    mapper: Mapper,
    has_battery: bool,
}

impl Cartridge {
    pub fn from_bytes(mut rom: Vec<u8>) -> Result<Self, CartridgeError> {
        // Need at least a complete header (0x100-0x14F).
        if rom.len() < 0x150 {
            return Err(CartridgeError::TooSmall);
        }
        let cart_type = rom[0x147];
        // ROM size code (0x148): banks = 2 << code, codes 0-8 are defined.
        if rom[0x148] > 8 {
            return Err(CartridgeError::BadHeader);
        }
        // RAM size code (0x149). Code 1 is officially unused, but a few
        // homebrew ROMs use it meaning 2 KiB; accept it for robustness.
        let ram_len = match rom[0x149] {
            0 => 0,
            1 => 0x800,
            2 => 0x2000,
            3 => 0x8000,
            4 => 0x20000,
            5 => 0x10000,
            _ => return Err(CartridgeError::BadHeader),
        };

        // Pad the image to a power of two so `bank & (banks - 1)` mirrors
        // undersized images exactly like a physical ROM chip whose upper
        // address lines are ignored. Multicart detection keys on the
        // *unpadded* dump size, so capture it first.
        let orig_len = rom.len();
        let padded = rom.len().next_power_of_two().max(2 * ROM_BANK_SIZE);
        rom.resize(padded, 0xFF);

        let mapper = match cart_type {
            0x00 | 0x08 | 0x09 => Mapper::None,
            0x01..=0x03 => Mapper::Mbc1 {
                ramg: false,
                bank1: 1,
                bank2: 0,
                mode: false,
                multicart: detect_mbc1_multicart(&rom, orig_len),
            },
            0x05 | 0x06 => Mapper::Mbc2 {
                ramg: false,
                romb: 1,
            },
            0x0F..=0x13 => Mapper::Mbc3 {
                ramg: false,
                romb: 1,
                ramb: 0,
                // Only 0x0F/0x10 (MBC3+TIMER...) have the RTC crystal.
                rtc: matches!(cart_type, 0x0F | 0x10).then(Rtc::new),
            },
            0x19..=0x1E => Mapper::Mbc5 {
                ramg: false,
                romb0: 1,
                romb1: 0,
                ramb: 0,
                rumble_cart: matches!(cart_type, 0x1C..=0x1E),
                rumble: false,
            },
            t => return Err(CartridgeError::UnsupportedMapper(t)),
        };

        // MBC2 has 512 half-bytes built in; the header RAM size is 0.
        let ram_len = if matches!(mapper, Mapper::Mbc2 { .. }) {
            512
        } else {
            ram_len
        };

        Ok(Cartridge {
            rom,
            ram: vec![0xFF; ram_len],
            mapper,
            has_battery: matches!(
                cart_type,
                0x03 | 0x06 | 0x09 | 0x0F | 0x10 | 0x13 | 0x1B | 0x1E
            ),
        })
    }

    /// `rom.len()` is a power of two, so this masks bank numbers the way the
    /// unconnected upper address lines of the actual chip do.
    fn rom_bank_mask(&self) -> usize {
        self.rom.len() / ROM_BANK_SIZE - 1
    }

    fn rom_at(&self, bank: usize, addr: u16) -> u8 {
        let offset = (bank & self.rom_bank_mask()) * ROM_BANK_SIZE + usize::from(addr & 0x3FFF);
        self.rom[offset]
    }

    /// Read 0x0000-0x7FFF (banked ROM).
    pub fn read_rom(&self, addr: u16) -> u8 {
        let low_area = addr < 0x4000;
        let bank = match self.mapper {
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
                    if mode {
                        high_bits
                    } else {
                        0
                    }
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
        };
        self.rom_at(bank, addr)
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
            } => match addr {
                0x0000..=0x1FFF => *ramg = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    *romb = value & 0x7F;
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

    fn rtc(&self) -> Option<&Rtc> {
        match &self.mapper {
            Mapper::Mbc3 { rtc, .. } => rtc.as_ref(),
            _ => None,
        }
    }

    /// Battery-backed RAM image (+ serialized RTC for MBC3), None if the
    /// cartridge has no battery.
    ///
    /// Format: the raw RAM contents (MBC2: 512 bytes, low nibble valid),
    /// followed — for RTC carts (types 0x0F/0x10) — by a 16-byte block:
    /// live S,M,H,DL,DH; latched S,M,H,DL,DH; sub-second T-cycle counter as
    /// little-endian u32; the last latch register write; one zero pad byte.
    pub fn save_data(&self) -> Option<Vec<u8>> {
        if !self.has_battery {
            return None;
        }
        let mut data = self.ram.clone();
        if let Some(rtc) = self.rtc() {
            data.extend_from_slice(&rtc.regs);
            data.extend_from_slice(&rtc.latched);
            data.extend_from_slice(&rtc.subsec.to_le_bytes());
            // latch_prev so an armed 0x00 -> 0x01 latch sequence survives a
            // save taken between the two writes.
            data.extend_from_slice(&[rtc.latch_prev, 0]);
        }
        Some(data)
    }

    /// Restore a [`Self::save_data`] image; also accepts the de-facto .sav
    /// layouts of other emulators. Returns whether anything was restored.
    ///
    /// The RAM prefix is loaded whenever `data` is at least RAM-sized; the
    /// trailing block is then interpreted as RTC state if the cartridge has
    /// an RTC: either our own 16-byte block ([`Self::save_data`]) or the
    /// 44/48-byte footer written by VBA/mGBA/BGB/SameBoy (five 4-byte LE
    /// live registers, five 4-byte LE latched registers, 32/64-bit
    /// timestamp). An unknown trailer size skips only the RTC restore, so
    /// e.g. a Pokemon G/S/C save imported from another emulator never loses
    /// its RAM. Data shorter than the RAM is rejected (returns false).
    pub fn load_save_data(&mut self, data: &[u8]) -> bool {
        if !self.has_battery || data.len() < self.ram.len() {
            return false;
        }
        let (ram, trailer) = data.split_at(self.ram.len());
        self.ram.copy_from_slice(ram);
        let rtc_restored = self.load_rtc_trailer(trailer);
        !self.ram.is_empty() || rtc_restored
    }

    /// Parse the post-RAM trailer of a save image into the RTC, if any.
    /// Returns whether RTC state was restored.
    fn load_rtc_trailer(&mut self, trailer: &[u8]) -> bool {
        let Mapper::Mbc3 { rtc: Some(rtc), .. } = &mut self.mapper else {
            return false;
        };
        match trailer.len() {
            // Our own block, see `save_data`.
            RTC_SAVE_LEN => {
                for (i, (reg, mask)) in rtc.regs.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[i] & mask;
                }
                for (i, (reg, mask)) in rtc.latched.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[5 + i] & mask;
                }
                let subsec = u32::from_le_bytes(trailer[10..14].try_into().unwrap());
                rtc.subsec = subsec % CYCLES_PER_SECOND;
                rtc.latch_prev = trailer[14];
                true
            }
            // De-facto VBA footer (also mGBA/BGB/SameBoy): each register is
            // stored as a 4-byte LE word (only the low byte is meaningful),
            // five live then five latched, then a 32- or 64-bit host
            // timestamp we ignore (our RTC is deterministic and never reads
            // the host clock).
            44 | 48 => {
                for (i, (reg, mask)) in rtc.regs.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[4 * i] & mask;
                }
                for (i, (reg, mask)) in rtc.latched.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[20 + 4 * i] & mask;
                }
                rtc.subsec = 0;
                true
            }
            _ => false,
        }
    }

    /// Advance the MBC3 real-time clock by `t_cycles` T-cycles (dots) of
    /// wall-clock time (4_194_304 per second; in CGB double speed mode pass
    /// dots, not CPU cycles, so wall time stays correct). Deterministic: the
    /// RTC never reads the host clock. No-op for carts without an RTC.
    pub fn tick_rtc(&mut self, t_cycles: u32) {
        if let Mapper::Mbc3 { rtc: Some(rtc), .. } = &mut self.mapper {
            rtc.tick_cycles(t_cycles);
        }
    }

    /// Rumble motor state (MBC5 rumble carts, types 0x1C-0x1E); always false
    /// for other cartridges.
    pub fn rumble(&self) -> bool {
        matches!(self.mapper, Mapper::Mbc5 { rumble: true, .. })
    }
}

/// What the 0xA000-0xBFFF window currently addresses.
enum RamTarget {
    /// External RAM bank (pre-masking).
    Ram(usize),
    /// MBC2 built-in half-byte RAM.
    Mbc2,
    /// MBC3 RTC register index 0-4 (S, M, H, DL, DH).
    Rtc(usize),
}

/// mooneye-gb's MBC1 multicart heuristic: multicarts can't be told apart from
/// normal carts by the header, but every known MBC1 multicart is exactly
/// 8 Mbit and contains a Nintendo logo in the header position of each 256 KiB
/// "game slot". Two or more logos (one is just the boot header) mean
/// multicart wiring.
///
/// `orig_len` is the dump size *before* power-of-two padding: a
/// non-power-of-two dump between 512 KiB and 1 MiB pads to 1 MiB but is not
/// 8 Mbit, so it must not become multicart-eligible.
fn detect_mbc1_multicart(rom: &[u8], orig_len: usize) -> bool {
    if orig_len != 0x100000 {
        return false;
    }
    let logos = rom
        .chunks_exact(0x40000)
        .filter(|chunk| chunk[0x104..0x104 + NINTENDO_LOGO.len()] == NINTENDO_LOGO)
        .count();
    logos >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a ROM image: `banks` 16 KiB banks where the first two bytes of
    /// each bank hold the bank index (little endian), plus the header fields
    /// we care about. The header ROM size code is derived from `banks`.
    fn make_rom(cart_type: u8, banks: usize, ram_size_code: u8) -> Vec<u8> {
        assert!(banks.is_power_of_two() && banks >= 2);
        let mut rom = vec![0u8; banks * 0x4000];
        for (b, bank) in rom.chunks_exact_mut(0x4000).enumerate() {
            bank[0] = b as u8;
            bank[1] = (b >> 8) as u8;
        }
        rom[0x147] = cart_type;
        rom[0x148] = (banks.trailing_zeros() - 1) as u8;
        rom[0x149] = ram_size_code;
        rom
    }

    fn cart(cart_type: u8, banks: usize, ram_size_code: u8) -> Cartridge {
        Cartridge::from_bytes(make_rom(cart_type, banks, ram_size_code)).unwrap()
    }

    /// Bank index encoded at the start of each ROM bank by [`make_rom`].
    fn bank_at(c: &Cartridge, base: u16) -> u16 {
        u16::from(c.read_rom(base)) | (u16::from(c.read_rom(base + 1)) << 8)
    }

    // --- header parsing ---

    #[test]
    fn header_too_small_rejected() {
        assert_eq!(
            Cartridge::from_bytes(vec![0; 0x100]).err(),
            Some(CartridgeError::TooSmall)
        );
    }

    #[test]
    fn header_unsupported_mapper_rejected() {
        // 0x0B = MMM01, not supported.
        let rom = make_rom(0x0B, 2, 0);
        assert_eq!(
            Cartridge::from_bytes(rom).err(),
            Some(CartridgeError::UnsupportedMapper(0x0B))
        );
    }

    #[test]
    fn header_bad_ram_size_rejected() {
        let rom = make_rom(0x03, 2, 6);
        assert_eq!(
            Cartridge::from_bytes(rom).err(),
            Some(CartridgeError::BadHeader)
        );
    }

    #[test]
    fn header_bad_rom_size_rejected() {
        let mut rom = make_rom(0x01, 2, 0);
        rom[0x148] = 9;
        assert_eq!(
            Cartridge::from_bytes(rom).err(),
            Some(CartridgeError::BadHeader)
        );
    }

    #[test]
    fn header_ram_size_code_1_accepted_as_2kib() {
        // Code 1 is officially unused; accept as 2 KiB for robustness.
        let mut c = cart(0x02, 2, 1);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x5A);
        assert_eq!(c.read_ram(0xA000), 0x5A);
        // 2 KiB mirrors across the 8 KiB window (upper address lines unused).
        assert_eq!(c.read_ram(0xA800), 0x5A);
        assert_eq!(c.read_ram(0xB800), 0x5A);
    }

    #[test]
    fn undersized_rom_padded_with_ff() {
        let mut rom = make_rom(0x00, 2, 0);
        rom.truncate(0x4000); // only one bank of data
        let c = Cartridge::from_bytes(rom).unwrap();
        assert_eq!(c.read_rom(0x0000), 0x00);
        assert_eq!(c.read_rom(0x4000), 0xFF);
        assert_eq!(c.read_rom(0x7FFF), 0xFF);
    }

    #[test]
    fn rom_banking_masked_by_actual_size_not_header() {
        // Header claims 32 banks but only 4 banks of data are present:
        // bank select wraps at the actual (power-of-two padded) size.
        let mut rom = make_rom(0x01, 4, 0);
        rom[0x148] = 4; // claims 32 banks
        let mut c = Cartridge::from_bytes(rom).unwrap();
        c.write_rom(0x2000, 5);
        assert_eq!(bank_at(&c, 0x4000), 5 & 3);
    }

    // --- no MBC ---

    #[test]
    fn nombc_maps_rom_directly() {
        let c = cart(0x00, 2, 0);
        assert_eq!(bank_at(&c, 0x0000), 0);
        assert_eq!(bank_at(&c, 0x4000), 1);
    }

    #[test]
    fn nombc_rom_writes_ignored() {
        let mut c = cart(0x00, 2, 0);
        c.write_rom(0x2000, 1);
        c.write_rom(0x0000, 0x0A);
        assert_eq!(bank_at(&c, 0x0000), 0);
        assert_eq!(bank_at(&c, 0x4000), 1);
    }

    #[test]
    fn nombc_ram_always_enabled() {
        let mut c = cart(0x08, 2, 2);
        c.write_ram(0xA123, 0x42);
        assert_eq!(c.read_ram(0xA123), 0x42);
    }

    #[test]
    fn nombc_without_ram_reads_ff() {
        let mut c = cart(0x00, 2, 0);
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn nombc_battery_save() {
        let mut c = cart(0x09, 2, 2);
        c.write_ram(0xA000, 0x42);
        let save = c.save_data().unwrap();
        assert_eq!(save.len(), 0x2000);
        assert_eq!(save[0], 0x42);
        assert!(cart(0x08, 2, 2).save_data().is_none());
        assert!(cart(0x00, 2, 0).save_data().is_none());
    }

    #[test]
    fn load_save_data_too_short_rejected() {
        let mut c = cart(0x09, 2, 2);
        c.write_ram(0xA000, 0x42);
        assert!(!c.load_save_data(&[0x99; 16]));
        assert_eq!(c.read_ram(0xA000), 0x42);
        assert!(c.load_save_data(&[0x99; 0x2000]));
        assert_eq!(c.read_ram(0xA000), 0x99);
    }

    #[test]
    fn load_save_data_without_battery_rejected() {
        let mut c = cart(0x08, 2, 2); // ROM+RAM, no battery
        assert!(!c.load_save_data(&[0x99; 0x2000]));
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0x42);
    }

    #[test]
    fn load_save_data_oversized_loads_ram_prefix() {
        // Foreign .sav files may carry trailers we don't understand; the
        // compatible RAM prefix must still be imported.
        let mut c = cart(0x09, 2, 2);
        let mut data = vec![0x99; 0x2000];
        data.extend_from_slice(&[0xAB; 7]);
        assert!(c.load_save_data(&data));
        assert_eq!(c.read_ram(0xA000), 0x99);
    }

    // --- MBC1 ---

    #[test]
    fn mbc1_initial_state() {
        // BANK1 powers up as 1, RAMG disabled (mbc1/bits_bank1, bits_ramg).
        let c = cart(0x03, 4, 2);
        assert_eq!(bank_at(&c, 0x0000), 0);
        assert_eq!(bank_at(&c, 0x4000), 1);
        assert_eq!(c.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn mbc1_ramg_low_nibble_only() {
        // RAMG compares only the low nibble against 0b1010 (mbc1/bits_ramg).
        let mut c = cart(0x03, 4, 2);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0x42);
        c.write_rom(0x1FFF, 0x00);
        assert_eq!(c.read_ram(0xA000), 0xFF);
        c.write_rom(0x1234, 0xFA); // upper nibble ignored
        assert_eq!(c.read_ram(0xA000), 0x42);
        c.write_rom(0x0000, 0x0B);
        assert_eq!(c.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn mbc1_ram_disabled_ignores_writes() {
        let mut c = cart(0x03, 4, 2);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x42);
        c.write_rom(0x0000, 0x00);
        c.write_ram(0xA000, 0x99); // ignored
        assert_eq!(c.read_ram(0xA000), 0xFF);
        c.write_rom(0x0000, 0x0A);
        assert_eq!(c.read_ram(0xA000), 0x42);
    }

    #[test]
    fn mbc1_ramg_not_writable_outside_0000_1fff() {
        let mut c = cart(0x03, 4, 2);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x2000, 0x00); // BANK1 register, not RAMG
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0x42);
    }

    #[test]
    fn mbc1_bank1_zero_substitution_after_5bit_mask() {
        let mut c = cart(0x01, 64, 0);
        // Raw 5-bit value 0 becomes 1.
        c.write_rom(0x2000, 0x00);
        assert_eq!(bank_at(&c, 0x4000), 1);
        // 0x20 & 0x1F == 0, so it also becomes 1 (mbc1/rom_8Mb: bank 32 -> 33
        // only via BANK2; writing 32 to BANK1 selects bank 1).
        c.write_rom(0x3FFF, 0x20);
        assert_eq!(bank_at(&c, 0x4000), 1);
        // Upper bits ignored: 0xE3 -> 3 (mooneye sets high bits to expose bugs).
        c.write_rom(0x2000, 0xE3);
        assert_eq!(bank_at(&c, 0x4000), 3);
    }

    #[test]
    fn mbc1_bank0_substitution_before_size_mask() {
        // 4-bank ROM: writing 0x10 keeps BANK1=16 (non-zero, no substitution),
        // and the size mask maps it to bank 0 (mbc1/rom_512kb expected table:
        // bank number 4 -> 0, 16 -> 0, but 0 -> 1 and 32 -> 1).
        let mut c = cart(0x01, 4, 0);
        c.write_rom(0x2000, 0x10);
        assert_eq!(bank_at(&c, 0x4000), 0);
        c.write_rom(0x2000, 0x04);
        assert_eq!(bank_at(&c, 0x4000), 0);
        c.write_rom(0x2000, 0x13);
        assert_eq!(bank_at(&c, 0x4000), 3);
        c.write_rom(0x2000, 0x00);
        assert_eq!(bank_at(&c, 0x4000), 1);
    }

    #[test]
    fn mbc1_bank2_two_bits_and_rom_mapping() {
        let mut c = cart(0x01, 64, 0);
        // High bits of the written value are ignored (mooneye harness writes
        // value | 0b11111100).
        c.write_rom(0x4000, 0xFC | 0x01);
        c.write_rom(0x2000, 0x02);
        assert_eq!(bank_at(&c, 0x4000), (1 << 5) | 2);
        // Mode 0: 0x0000-0x3FFF is always bank 0.
        assert_eq!(bank_at(&c, 0x0000), 0);
    }

    #[test]
    fn mbc1_mode1_maps_bank2_in_low_area() {
        // mbc1/rom_8Mb expected table: mode 1, BANK2=1 -> low area bank 32.
        let mut c = cart(0x01, 64, 0);
        c.write_rom(0x5FFF, 0x01);
        c.write_rom(0x6000, 0x01);
        assert_eq!(bank_at(&c, 0x0000), 32);
        // Mode register only looks at bit 0.
        c.write_rom(0x7FFF, 0xFE);
        assert_eq!(bank_at(&c, 0x0000), 0);
    }

    #[test]
    fn mbc1_mode1_low_area_masked_by_rom_size() {
        // 4-bank ROM: (BANK2 << 5) & 3 == 0 always (mbc1/rom_512kb mode 1).
        let mut c = cart(0x01, 4, 0);
        c.write_rom(0x4000, 0x03);
        c.write_rom(0x6000, 0x01);
        assert_eq!(bank_at(&c, 0x0000), 0);
    }

    #[test]
    fn mbc1_ram_banking_32k() {
        // mbc1/bits_bank2 + bits_mode: with 32 KiB RAM, BANK2 selects the RAM
        // bank in mode 1 and is ignored in mode 0.
        let mut c = cart(0x03, 4, 3);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x6000, 0x01);
        for bank in 0..4 {
            c.write_rom(0x4000, bank);
            c.write_ram(0xA000, 0x10 + bank);
        }
        for bank in 0..4 {
            c.write_rom(0x4000, bank);
            assert_eq!(c.read_ram(0xA000), 0x10 + bank);
        }
        // Mode 0: always bank 0, regardless of BANK2.
        c.write_rom(0x6000, 0x00);
        c.write_rom(0x4000, 0x03);
        assert_eq!(c.read_ram(0xA000), 0x10);
    }

    #[test]
    fn mbc1_ram_8k_mirrors_under_banking() {
        // 8 KiB RAM has no A13/A14: mode 1 banking mirrors the same memory.
        let mut c = cart(0x03, 4, 2);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x6000, 0x01);
        c.write_rom(0x4000, 0x00);
        c.write_ram(0xA000, 0x42);
        c.write_rom(0x4000, 0x01);
        assert_eq!(c.read_ram(0xA000), 0x42);
    }

    // --- MBC1 multicart ("MBC1M") ---

    /// 1 MiB MBC1 image with the Nintendo logo planted at `0x104 + n*0x40000`
    /// for each n in `logo_chunks`.
    fn multicart_rom(logo_chunks: &[usize]) -> Vec<u8> {
        let mut rom = make_rom(0x01, 64, 0);
        for &n in logo_chunks {
            let base = n * 0x40000 + 0x104;
            rom[base..base + NINTENDO_LOGO.len()].copy_from_slice(&NINTENDO_LOGO);
        }
        rom
    }

    #[test]
    fn mbc1_multicart_detected_and_wired_4bit() {
        let mut c = Cartridge::from_bytes(multicart_rom(&[0, 1, 2, 3])).unwrap();
        // BANK1 bit 4 is not wired: writing 0x10 selects effective bank 0
        // within the current 16-bank "game" (multicart_rom_8Mb expected table:
        // bank number 16 -> 0).
        c.write_rom(0x2000, 0x10);
        assert_eq!(bank_at(&c, 0x4000), 0);
        // BANK2 shifts to bits 4-5; zero substitution still on the raw value:
        // bank number 32 -> BANK1=0 -> 1 -> (1 << 4) | 1 = 17.
        c.write_rom(0x2000, 0x00);
        c.write_rom(0x4000, 0x01);
        assert_eq!(bank_at(&c, 0x4000), 17);
        // Mode 1 low area: BANK2=2 -> bank 32.
        c.write_rom(0x4000, 0x02);
        c.write_rom(0x6000, 0x01);
        assert_eq!(bank_at(&c, 0x0000), 32);
    }

    #[test]
    fn mbc1_multicart_needs_at_least_two_logos() {
        let mut c = Cartridge::from_bytes(multicart_rom(&[0])).unwrap();
        c.write_rom(0x2000, 0x10);
        assert_eq!(bank_at(&c, 0x4000), 16); // normal MBC1 wiring
    }

    #[test]
    fn mbc1_multicart_requires_unpadded_1mib_dump() {
        // mooneye-gb's heuristic keys on the *dump* being exactly 8 Mbit. A
        // 768 KiB dump pads to 1 MiB internally but must stay normal MBC1
        // even with multiple logos present.
        let mut rom = multicart_rom(&[0, 1, 2]);
        rom.truncate(0xC0000);
        let mut c = Cartridge::from_bytes(rom).unwrap();
        c.write_rom(0x2000, 0x10);
        assert_eq!(bank_at(&c, 0x4000), 16); // normal MBC1 wiring
    }

    #[test]
    fn mbc1_multicart_requires_exactly_1mib() {
        // 2 MiB ROM with logos at every 256 KiB boundary is not a multicart.
        let mut rom = make_rom(0x01, 128, 0);
        for n in 0..8 {
            let base = n * 0x40000 + 0x104;
            rom[base..base + NINTENDO_LOGO.len()].copy_from_slice(&NINTENDO_LOGO);
        }
        let mut c = Cartridge::from_bytes(rom).unwrap();
        c.write_rom(0x2000, 0x10);
        assert_eq!(bank_at(&c, 0x4000), 16);
    }

    // --- MBC2 ---

    #[test]
    fn mbc2_initial_state() {
        let c = cart(0x06, 4, 0);
        assert_eq!(bank_at(&c, 0x0000), 0);
        assert_eq!(bank_at(&c, 0x4000), 1);
        assert_eq!(c.read_ram(0xA000), 0xFF); // RAM disabled at power-on
    }

    #[test]
    fn mbc2_register_select_by_address_bit_8() {
        // gbctr: in 0x0000-0x3FFF, A8=0 addresses RAMG, A8=1 addresses ROMB.
        let mut c = cart(0x06, 4, 0);
        c.write_rom(0x0000, 0x0A); // A8=0 -> RAMG
        c.write_ram(0xA000, 0x05);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 0x05);
        c.write_rom(0x0100, 0x03); // A8=1 -> ROMB
        assert_eq!(bank_at(&c, 0x4000), 3);
        // Mirrors across the whole 0x0000-0x3FFF range (mbc2/bits_ramg walks
        // every address; mbc2/bits_romb walks every odd-A8 address).
        c.write_rom(0x3FFF, 0x01); // A8=1 -> ROMB
        assert_eq!(bank_at(&c, 0x4000), 1);
        c.write_rom(0x3EFF, 0x00); // A8=0 -> RAMG disable
        assert_eq!(c.read_ram(0xA000), 0xFF);
        c.write_rom(0x3EFF, 0x0A);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 0x05);
    }

    #[test]
    fn mbc2_ramg_low_nibble_only() {
        // mbc2/bits_ramg expectation table repeats every 16 values: only the
        // low nibble is compared.
        let mut c = cart(0x06, 4, 0);
        c.write_rom(0x0000, 0xFA);
        c.write_ram(0xA000, 0x05);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 0x05);
        c.write_rom(0x0000, 0x1B);
        assert_eq!(c.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn mbc2_romb_4bit_zero_substitution() {
        let mut c = cart(0x06, 16, 0);
        c.write_rom(0x2100, 0xF0); // & 0x0F == 0 -> bank 1
        assert_eq!(bank_at(&c, 0x4000), 1);
        c.write_rom(0x2100, 0xF3);
        assert_eq!(bank_at(&c, 0x4000), 3);
        c.write_rom(0x2100, 0x0F);
        assert_eq!(bank_at(&c, 0x4000), 15);
    }

    #[test]
    fn mbc2_rom_bank_masked_to_size() {
        let mut c = cart(0x06, 4, 0);
        c.write_rom(0x2100, 0x05);
        assert_eq!(bank_at(&c, 0x4000), 1);
    }

    #[test]
    fn mbc2_writes_above_0x3fff_have_no_effect() {
        // mbc2/bits_unused: writes to 0x4000-0x7FFF change nothing.
        let mut c = cart(0x06, 16, 0);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x2100, 0x01);
        c.write_ram(0xA000, 0x05);
        c.write_rom(0x4000, 0x00);
        c.write_rom(0x7FFF, 0xFF);
        c.write_rom(0x5555, 0x03);
        assert_eq!(bank_at(&c, 0x4000), 1);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 0x05);
    }

    #[test]
    fn mbc2_ram_nibbles_and_echo() {
        // 512 half-bytes echoed across A000-BFFF; upper nibble reads as 1s.
        let mut c = cart(0x06, 4, 0);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0xA5);
        assert_eq!(c.read_ram(0xA000), 0xF5);
        assert_eq!(c.read_ram(0xA200), 0xF5); // echo
        assert_eq!(c.read_ram(0xBE00), 0xF5); // echo
        c.write_ram(0xBFFF, 0x03); // echoes to 0xA1FF
        assert_eq!(c.read_ram(0xA1FF), 0xF3);
    }

    #[test]
    fn mbc2_no_ram_banking_register() {
        // mbc2/ram round 4: writing 0x4000 must not bank RAM.
        let mut c = cart(0x06, 4, 0);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x05);
        c.write_rom(0x4000, 0x01);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 0x05);
    }

    #[test]
    fn mbc2_save_data() {
        let mut c = cart(0x06, 4, 0);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x05);
        let save = c.save_data().unwrap();
        assert_eq!(save.len(), 512);
        assert_eq!(save[0] & 0x0F, 0x05);
        assert!(cart(0x05, 4, 0).save_data().is_none());
    }

    // --- MBC3 ---

    #[test]
    fn mbc3_romb_7bit_zero_substitution() {
        let mut c = cart(0x11, 128, 0);
        c.write_rom(0x2000, 0x80); // & 0x7F == 0 -> bank 1
        assert_eq!(bank_at(&c, 0x4000), 1);
        c.write_rom(0x3FFF, 0x7F);
        assert_eq!(bank_at(&c, 0x4000), 127);
        assert_eq!(bank_at(&c, 0x0000), 0); // low area fixed
    }

    #[test]
    fn mbc3_ram_banking() {
        let mut c = cart(0x13, 4, 3);
        c.write_rom(0x0000, 0x0A);
        for bank in 0..4 {
            c.write_rom(0x4000, bank);
            c.write_ram(0xA000, 0x20 + bank);
        }
        for bank in 0..4 {
            c.write_rom(0x5FFF, bank);
            assert_eq!(c.read_ram(0xA000), 0x20 + bank);
        }
    }

    #[test]
    fn mbc3_ramg_gates_ram() {
        let mut c = cart(0x13, 4, 3);
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0xFF);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0x42);
        c.write_rom(0x0000, 0x0F);
        assert_eq!(c.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn mbc3_unmapped_ramb_reads_ff() {
        let mut c = cart(0x13, 4, 3);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x0D); // not a RAM bank, not an RTC register
        assert_eq!(c.read_ram(0xA000), 0xFF);
        c.write_rom(0x4000, 0x08); // RTC register, but this cart has no RTC
        assert_eq!(c.read_ram(0xA000), 0xFF);
    }

    /// RTC cart helper: type 0x10 (MBC3+TIMER+RAM+BATTERY), 32 KiB RAM.
    fn rtc_cart() -> Cartridge {
        let mut c = cart(0x10, 4, 3);
        c.write_rom(0x0000, 0x0A); // enable RAM/RTC access
        c
    }

    fn rtc_write(c: &mut Cartridge, reg: u8, value: u8) {
        c.write_rom(0x4000, reg);
        c.write_ram(0xA000, value);
    }

    fn rtc_read_latched(c: &mut Cartridge, reg: u8) -> u8 {
        c.write_rom(0x4000, reg);
        c.read_ram(0xA000)
    }

    fn rtc_latch(c: &mut Cartridge) {
        c.write_rom(0x6000, 0x00);
        c.write_rom(0x6000, 0x01);
    }

    #[test]
    fn mbc3_rtc_latch_requires_0_then_1() {
        let mut c = rtc_cart();
        rtc_write(&mut c, 0x08, 10);
        // Reads return the *latched* value, which is still 0.
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0);
        // A lone 0x01 write does not latch.
        c.write_rom(0x6000, 0x01);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 10);
        // Latched value is stable while the live clock ticks.
        c.tick_rtc(CYCLES_PER_SECOND);
        c.tick_rtc(CYCLES_PER_SECOND);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 10);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 12);
    }

    #[test]
    fn mbc3_rtc_full_carry_chain_and_day_carry() {
        let mut c = rtc_cart();
        rtc_write(&mut c, 0x08, 59);
        rtc_write(&mut c, 0x09, 59);
        rtc_write(&mut c, 0x0A, 23);
        rtc_write(&mut c, 0x0B, 0xFF);
        rtc_write(&mut c, 0x0C, 0x01); // day = 511
        c.tick_rtc(CYCLES_PER_SECOND);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0);
        assert_eq!(rtc_read_latched(&mut c, 0x09), 0);
        assert_eq!(rtc_read_latched(&mut c, 0x0A), 0);
        assert_eq!(rtc_read_latched(&mut c, 0x0B), 0);
        // Day wrapped 511 -> 0: bit 0 clear, carry (bit 7) set.
        assert_eq!(rtc_read_latched(&mut c, 0x0C), 0x80);
        // Carry is sticky until written.
        c.tick_rtc(CYCLES_PER_SECOND);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x0C), 0x80);
    }

    #[test]
    fn mbc3_rtc_halt_stops_clock() {
        let mut c = rtc_cart();
        rtc_write(&mut c, 0x0C, 0x40); // halt
        c.tick_rtc(CYCLES_PER_SECOND);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0);
        rtc_write(&mut c, 0x0C, 0x00); // resume
        c.tick_rtc(CYCLES_PER_SECOND);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 1);
    }

    #[test]
    fn mbc3_rtc_seconds_write_resets_subsecond_counter() {
        let mut c = rtc_cart();
        c.tick_rtc(CYCLES_PER_SECOND / 2);
        rtc_write(&mut c, 0x08, 0); // resets the sub-second divider
        c.tick_rtc(CYCLES_PER_SECOND * 3 / 4);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0);
        c.tick_rtc(CYCLES_PER_SECOND / 4);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 1);
    }

    #[test]
    fn mbc3_rtc_out_of_range_seconds_wrap_without_carry() {
        // Seconds is a 6-bit counter: 60..63 count up and wrap to 0 without
        // a minute carry (verified by rtc3test on hardware).
        let mut c = rtc_cart();
        rtc_write(&mut c, 0x08, 63);
        c.tick_rtc(CYCLES_PER_SECOND);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0);
        assert_eq!(rtc_read_latched(&mut c, 0x09), 0); // no minute carry
    }

    #[test]
    fn mbc3_rtc_register_write_masks() {
        let mut c = rtc_cart();
        rtc_write(&mut c, 0x08, 0xFF);
        rtc_write(&mut c, 0x09, 0xFF);
        rtc_write(&mut c, 0x0A, 0xFF);
        rtc_write(&mut c, 0x0C, 0xBF);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0x3F);
        assert_eq!(rtc_read_latched(&mut c, 0x09), 0x3F);
        assert_eq!(rtc_read_latched(&mut c, 0x0A), 0x1F);
        assert_eq!(rtc_read_latched(&mut c, 0x0C), 0x81); // mask 0xC1
    }

    #[test]
    fn mbc3_save_data_roundtrip_with_rtc() {
        let mut c = rtc_cart();
        c.write_rom(0x4000, 0x00);
        c.write_ram(0xA000, 0x42);
        rtc_write(&mut c, 0x08, 7);
        rtc_latch(&mut c);
        let save = c.save_data().unwrap();
        assert_eq!(save.len(), 0x8000 + 16);

        let mut c2 = rtc_cart();
        c2.load_save_data(&save);
        c2.write_rom(0x4000, 0x00);
        assert_eq!(c2.read_ram(0xA000), 0x42);
        // Both live and latched RTC state restored.
        assert_eq!(rtc_read_latched(&mut c2, 0x08), 7);
        c2.tick_rtc(CYCLES_PER_SECOND);
        rtc_latch(&mut c2);
        assert_eq!(rtc_read_latched(&mut c2, 0x08), 8);
    }

    #[test]
    fn mbc3_save_data_without_rtc_is_ram_only() {
        let mut c = cart(0x13, 4, 3);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x42);
        let save = c.save_data().unwrap();
        assert_eq!(save.len(), 0x8000);
        assert!(cart(0x11, 4, 0).save_data().is_none());
        assert!(cart(0x12, 4, 3).save_data().is_none());
    }

    #[test]
    fn mbc3_rtc_only_cart_saves_rtc_block() {
        // Type 0x0F: MBC3+TIMER+BATTERY, no RAM.
        let c = cart(0x0F, 4, 0);
        let save = c.save_data().unwrap();
        assert_eq!(save.len(), 16);
    }

    #[test]
    fn mbc3_rtc_armed_latch_survives_save_roundtrip() {
        // A save taken between the 0x00 and 0x01 latch writes must keep the
        // latch armed: latch_prev travels in save byte ram_len + 14.
        let mut c = rtc_cart();
        rtc_write(&mut c, 0x08, 5);
        c.write_rom(0x6000, 0x00); // arm the latch
        let save = c.save_data().unwrap();
        let mut c2 = rtc_cart();
        assert!(c2.load_save_data(&save));
        c2.write_rom(0x6000, 0x01); // completes the armed sequence
        assert_eq!(rtc_read_latched(&mut c2, 0x08), 5);

        // Conversely, an un-armed save must not let a lone 0x01 latch.
        let mut c3 = rtc_cart();
        rtc_write(&mut c3, 0x08, 5);
        let save = c3.save_data().unwrap();
        let mut c4 = rtc_cart();
        assert!(c4.load_save_data(&save));
        c4.write_rom(0x6000, 0x01);
        assert_eq!(rtc_read_latched(&mut c4, 0x08), 0);
    }

    /// Build a VBA/mGBA/BGB-style RTC footer: five live + five latched
    /// registers, each as a 4-byte little-endian word, then a timestamp of
    /// `ts_len` (4 or 8) bytes.
    fn vba_footer(live: [u8; 5], latched: [u8; 5], ts_len: usize) -> Vec<u8> {
        let mut f = Vec::new();
        for reg in live.into_iter().chain(latched) {
            f.extend_from_slice(&u32::from(reg).to_le_bytes());
        }
        f.resize(f.len() + ts_len, 0xEE);
        f
    }

    #[test]
    fn mbc3_rtc_accepts_vba_style_footer() {
        for ts_len in [4usize, 8] {
            let mut data = vec![0x42; 0x8000];
            data.extend_from_slice(&vba_footer(
                [7, 8, 9, 10, 1],
                [11, 12, 13, 14, 0xFF],
                ts_len,
            ));
            let mut c = rtc_cart();
            assert!(c.load_save_data(&data));
            c.write_rom(0x4000, 0x00);
            assert_eq!(c.read_ram(0xA000), 0x42);
            // Latched registers restored (DH masked to its 0xC1 wired bits).
            assert_eq!(rtc_read_latched(&mut c, 0x08), 11);
            assert_eq!(rtc_read_latched(&mut c, 0x0C), 0xC1);
            // Live registers restored: latch and observe them.
            rtc_latch(&mut c);
            assert_eq!(rtc_read_latched(&mut c, 0x08), 7);
            assert_eq!(rtc_read_latched(&mut c, 0x09), 8);
            assert_eq!(rtc_read_latched(&mut c, 0x0A), 9);
            assert_eq!(rtc_read_latched(&mut c, 0x0B), 10);
            assert_eq!(rtc_read_latched(&mut c, 0x0C), 1);
        }
    }

    #[test]
    fn mbc3_rtc_unknown_footer_still_loads_ram() {
        // e.g. importing a Pokemon G/S/C save with a footer size we don't
        // recognize: the RAM portion is compatible and must be applied; only
        // the RTC restore is skipped.
        let mut data = vec![0x42; 0x8000];
        data.extend_from_slice(&[0xEE; 20]);
        let mut c = rtc_cart();
        assert!(c.load_save_data(&data));
        c.write_rom(0x4000, 0x00);
        assert_eq!(c.read_ram(0xA000), 0x42);
        rtc_latch(&mut c);
        assert_eq!(rtc_read_latched(&mut c, 0x08), 0); // RTC untouched
    }

    #[test]
    fn mbc3_rtc_only_cart_rejects_unknown_image() {
        // No RAM and an unparseable trailer: nothing was applied.
        let mut c = cart(0x0F, 4, 0);
        assert!(!c.load_save_data(&[0xEE; 20]));
        let save = c.save_data().unwrap();
        assert!(c.load_save_data(&save));
    }

    // --- MBC5 ---

    #[test]
    fn mbc5_initial_state() {
        let c = cart(0x19, 4, 0);
        assert_eq!(bank_at(&c, 0x0000), 0);
        assert_eq!(bank_at(&c, 0x4000), 1);
    }

    #[test]
    fn mbc5_bank_0_selectable() {
        // mbc5/rom_512kb: bank number 0 maps bank 0 (no substitution).
        let mut c = cart(0x19, 4, 0);
        c.write_rom(0x2000, 0x00);
        assert_eq!(bank_at(&c, 0x4000), 0);
    }

    #[test]
    fn mbc5_9bit_banking() {
        let mut c = cart(0x19, 512, 0);
        c.write_rom(0x2000, 0x34);
        c.write_rom(0x3000, 0x01);
        assert_eq!(bank_at(&c, 0x4000), 0x134);
        // ROMB1 only keeps bit 0 (mooneye harness writes value | 0b11111110).
        c.write_rom(0x3FFF, 0xFE);
        assert_eq!(bank_at(&c, 0x4000), 0x034);
        c.write_rom(0x2FFF, 0xFF);
        assert_eq!(bank_at(&c, 0x4000), 0x0FF);
    }

    #[test]
    fn mbc5_bank_masked_to_rom_size() {
        let mut c = cart(0x19, 4, 0);
        c.write_rom(0x2000, 0x05);
        assert_eq!(bank_at(&c, 0x4000), 1);
        c.write_rom(0x2000, 0x00);
        c.write_rom(0x3000, 0x01); // bank 256 & 3 == 0
        assert_eq!(bank_at(&c, 0x4000), 0);
    }

    #[test]
    fn mbc5_writes_0x6000_have_no_effect() {
        let mut c = cart(0x19, 4, 0);
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x6000, 0x01);
        c.write_rom(0x7FFF, 0xFF);
        assert_eq!(bank_at(&c, 0x4000), 2);
        assert_eq!(bank_at(&c, 0x0000), 0);
    }

    #[test]
    fn mbc5_ramg_compares_all_8_bits() {
        // gbctr: unlike MBC1, MBC5 compares the full written byte; only 0x0A
        // enables RAM.
        let mut c = cart(0x1A, 4, 3);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0x42);
        c.write_rom(0x1FFF, 0x1A); // low nibble 0xA but not 0x0A -> disable
        assert_eq!(c.read_ram(0xA000), 0xFF);
        c.write_rom(0x0000, 0x0A);
        assert_eq!(c.read_ram(0xA000), 0x42);
    }

    #[test]
    fn mbc5_ram_banking_16_banks() {
        let mut c = cart(0x1B, 4, 4); // 128 KiB RAM
        c.write_rom(0x0000, 0x0A);
        for bank in 0..16 {
            c.write_rom(0x4000, bank);
            c.write_ram(0xA000, 0x30 + bank);
        }
        for bank in 0..16 {
            c.write_rom(0x5FFF, bank);
            assert_eq!(c.read_ram(0xA000), 0x30 + bank);
        }
    }

    #[test]
    fn mbc5_ramb_masked_to_ram_size() {
        let mut c = cart(0x1B, 4, 3); // 32 KiB RAM = 4 banks
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x00);
        c.write_ram(0xA000, 0x42);
        c.write_rom(0x4000, 0x08); // & (4-1) wraps to bank 0
        assert_eq!(c.read_ram(0xA000), 0x42);
    }

    #[test]
    fn mbc5_rumble_motor_bit() {
        let mut c = cart(0x1E, 4, 3); // rumble cart
        assert!(!c.rumble());
        c.write_rom(0x0000, 0x0A);
        // Bit 3 drives the motor and is excluded from RAM bank selection.
        c.write_rom(0x4000, 0x08);
        assert!(c.rumble());
        c.write_rom(0x4000, 0x05);
        c.write_ram(0xA000, 0x42);
        assert!(!c.rumble());
        c.write_rom(0x4000, 0x0D); // bank 5 + motor on
        assert!(c.rumble());
        assert_eq!(c.read_ram(0xA000), 0x42); // still RAM bank 5
    }

    #[test]
    fn mbc5_non_rumble_cart_never_rumbles() {
        let mut c = cart(0x1B, 4, 4);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x08);
        assert!(!c.rumble());
        // ...and bit 3 acts as a normal RAM bank bit.
        c.write_ram(0xA000, 0x55);
        c.write_rom(0x4000, 0x00);
        c.write_rom(0x4000, 0x08);
        assert_eq!(c.read_ram(0xA000), 0x55);
    }

    #[test]
    fn mbc5_battery_save() {
        let mut c = cart(0x1E, 4, 3);
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x42);
        let save = c.save_data().unwrap();
        assert_eq!(save.len(), 0x8000);
        assert!(cart(0x19, 4, 0).save_data().is_none());
        assert!(cart(0x1C, 4, 0).save_data().is_none());
        assert!(cart(0x1B, 4, 3).save_data().is_some());
    }

    #[test]
    fn rumble_false_for_non_mbc5() {
        assert!(!cart(0x00, 2, 0).rumble());
        assert!(!cart(0x01, 4, 0).rumble());
        let mut c = cart(0x10, 4, 3);
        c.tick_rtc(123); // tick_rtc on RTC cart is fine
        cart(0x01, 4, 0).tick_rtc(123); // and a no-op elsewhere
    }
}
