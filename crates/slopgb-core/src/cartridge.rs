//! Cartridge: header parsing and MBC mappers. Cartridge work package.
//!
//! Supported mappers: none (32 KiB), MBC1 (incl. 8 Mbit multicart detection),
//! MBC2, MBC3 (+RTC), MBC5. Mooneye `emulator-only/` is the oracle for
//! banking edge cases (register bit widths, RAMG gating, bank-0 aliasing,
//! mode 1 behavior, unused-bit masking).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CartridgeError {
    /// ROM image smaller than one bank / header incomplete.
    TooSmall,
    /// Cartridge-type byte (0x147) we do not support.
    UnsupportedMapper(u8),
    /// Declared ROM/RAM size code (0x148/0x149) unsupported; carries the
    /// offending byte.
    BadHeader(u8),
}

impl fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CartridgeError::TooSmall => write!(f, "ROM image too small"),
            CartridgeError::UnsupportedMapper(t) => {
                write!(f, "unsupported cartridge type {t:#04x}")
            }
            CartridgeError::BadHeader(b) => {
                write!(f, "inconsistent cartridge header (size code {b:#04x})")
            }
        }
    }
}

impl std::error::Error for CartridgeError {}

/// True if a CGB-support flag byte (header 0x143) requests CGB mode.
///
/// Pan Docs "CGB flag": the conventional values are 0x80 (CGB-enhanced) and
/// 0xC0 (CGB-only), but hardware (the CGB boot ROM) decodes only bit 7, so
/// e.g. 0x84 also enables CGB mode. Single source of truth for both
/// [`crate::GameBoy::auto_model`] and the interconnect's CGB-mode gate.
pub fn cgb_flag(byte: u8) -> bool {
    byte & 0x80 != 0
}

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

#[derive(Clone)]
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
    /// `mbc30` selects the MBC30 wiring (Pan Docs "MBC3": up to 4 MiB ROM /
    /// 64 KiB RAM): ROMB grows to 8 bits. Detected like SameBoy
    /// (Core/mbc.c): an MBC3-type cart whose ROM exceeds 2 MiB or whose
    /// RAM exceeds 32 KiB can only be the MBC30 chip.
    Mbc3 {
        ramg: bool,
        romb: u8,
        ramb: u8,
        rtc: Option<Rtc>,
        mbc30: bool,
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

#[derive(Clone)]
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
            return Err(CartridgeError::BadHeader(rom[0x148]));
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
            _ => return Err(CartridgeError::BadHeader(rom[0x149])),
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
                // See the Mapper::Mbc3 docs; rom is already padded to a
                // power of two, which preserves the > 2 MiB predicate.
                mbc30: rom.len() > 0x200000 || ram_len > 0x8000,
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

    /// True if this cartridge's header requests CGB mode (see [`cgb_flag`]).
    pub fn supports_cgb(&self) -> bool {
        cgb_flag(self.rom[0x143])
    }

    /// True if the header unlocks SGB functions: SGB flag (0x146) == 0x03
    /// *and* old licensee code (0x14B) == 0x33 (Pan Docs "SGB flag": the
    /// SGB ignores command packets otherwise; SameBoy's HLE BIOS checks
    /// exactly these two header bytes, Core/sgb.c).
    pub fn supports_sgb(&self) -> bool {
        self.rom[0x146] == 0x03 && self.rom[0x14B] == 0x33
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
#[path = "cartridge_tests.rs"]
mod tests;
