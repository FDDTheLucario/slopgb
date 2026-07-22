//! Cartridge: header parsing and MBC mappers. Cartridge work package.
//!
//! Supported mappers: none (32 KiB), MBC1 (incl. 8 Mbit multicart detection),
//! MBC2, MBC3 (+RTC), MBC5, MBC6 (+flash). Mooneye `emulator-only/` is the
//! oracle for banking edge cases (register bit widths, RAMG gating, bank-0
//! aliasing, mode 1 behavior, unused-bit masking); the committed `roms/mbc6`
//! exerciser pins MBC6.

use std::fmt;

// Behavior-preserving submodules (each a second `impl` block via `use
// super::*`). The types, their fields, the consts and the free helpers stay
// here; the impls move out by concern: `header` (from_bytes + CGB/SGB flags),
// `banking` (ROM/RAM banking + mapper writes), `save` (battery images + RTC
// tick), `rtc` (the RTC clock), `state` (manual save-state serialization).
mod banking;
mod header;
mod mbc6;
mod rtc;
mod save;
mod state;

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
/// MBC6 banks are half/quarter the usual size (Pan Docs "MBC6"): 8 KiB ROM
/// windows at 0x4000/0x6000, 4 KiB RAM windows at 0xA000/0xB000.
const MBC6_ROM_BANK_SIZE: usize = 0x2000;
const MBC6_RAM_BANK_SIZE: usize = 0x1000;
/// The MBC6 flash chip (Macronix MX29F008): 1 MiB in eight 128 KiB sectors.
const MBC6_FLASH_SIZE: usize = 0x100000;
const MBC6_FLASH_SECTOR_SIZE: usize = 0x20000;
/// MX29F008 embedded-operation durations, in T-cycles of wall time (dots at
/// 4.194304 MHz; in double speed the caller passes dots, like the RTC).
/// Pan Docs gives no timings, so these are order-of-magnitude typical
/// figures for the part family: ~1.5 ms for a 128-byte page program (and
/// the non-volatile protect bit), ~0.5 s for a block erase, chip erase =
/// the eight sectors in sequence. Status bit 7 reads 0 until they elapse.
const MBC6_FLASH_PROGRAM_CYCLES: u32 = 6_291;
const MBC6_FLASH_SECTOR_ERASE_CYCLES: u32 = 2_097_152;
const MBC6_FLASH_CHIP_ERASE_CYCLES: u32 = 8 * MBC6_FLASH_SECTOR_ERASE_CYCLES;
/// T-cycles (dots) per RTC second at the 4.194304 MHz master clock.
const CYCLES_PER_SECOND: u32 = 4_194_304;
/// Size of the RTC block appended to [`Cartridge::save_data`] images.
const RTC_SAVE_LEN: usize = 16;

/// MBC3 real-time clock. Driven deterministically from emulated cycles via
/// [`Cartridge::tick_time`]; never reads the host clock.
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

/// What reads from a flash-mapped MBC6 window return (Pan Docs "MBC6",
/// MX29F008 JEDEC command set). `Read` maps the flash array itself; every
/// other mode substitutes command results for array bytes until a $F0 write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FlashMode {
    Read,
    /// JEDEC ID: 0xC2 (Macronix) at even addresses, 0x81 at odd.
    Id,
    /// The hidden 256-byte region is mapped instead of the array.
    HiddenRead,
    /// Writes program (AND into) array bytes; reads return the status byte.
    Program,
    /// Writes program the hidden region; reads return the status byte.
    ProgramHidden,
    /// An erase/protect operation completed; reads return the status byte.
    Status,
}

/// The MBC6 cart's MX29F008 flash: a 1 MiB array in eight 128 KiB sectors
/// plus a hidden 256-byte region, commanded through JEDEC $5555/$2AAA unlock
/// writes. Embedded operations run on the emulated clock (`busy`, the
/// `MBC6_FLASH_*_CYCLES` durations); programming can only clear bits,
/// erasing sets 0xFF.
#[derive(Clone)]
struct Mbc6Flash {
    data: Vec<u8>,
    hidden: [u8; 256],
    mode: FlashMode,
    /// Unlock progress: 0 = idle, 1 = $AA@$5555 seen, 2 = $55@$2AAA seen.
    seq: u8,
    /// First half of a two-cycle command ($80 erase / $60 extended / $77
    /// hidden-read), 0 when none is pending.
    prefix: u8,
    /// Sector-0 protection set by the Protect Sector 0 command (non-volatile
    /// on hardware, a second layer on top of the Flash Write Enable bit).
    protect: bool,
    /// Program-mode page buffer: 128 pending bytes (0xFF = untouched slot),
    /// ANDed into the array/hidden region only by the commit write.
    buf: [u8; 128],
    /// Base offset of the page being loaded, latched by the first data
    /// write of a program operation; None before it.
    page: Option<usize>,
    /// Data writes seen in this page load; the commit is armed at 128.
    loaded: u8,
    /// T-cycles until the running embedded operation finishes. While
    /// nonzero the chip ignores bus writes and status bit 7 reads 0;
    /// decremented by [`Cartridge::tick_time`].
    busy: u32,
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
    /// 0x20: two independently switchable 8 KiB ROM/flash windows
    /// (A: 0x4000-0x5FFF, B: 0x6000-0x7FFF) and two 4 KiB RAM windows
    /// (A: 0xA000-0xAFFF, B: 0xB000-0xBFFF), plus the MX29F008 flash chip
    /// either window can map in place of the ROM (Pan Docs "MBC6").
    Mbc6 {
        ramg: bool,
        /// RAM bank per window, 3 bits each.
        ramb_a: u8,
        ramb_b: u8,
        /// ROM/flash bank per window, 7 bits each.
        romb_a: u8,
        romb_b: u8,
        /// Per-window ROM (false) vs flash (true) select.
        flash_a: bool,
        flash_b: bool,
        /// The flash chip's /CE gate: flash-mapped windows read open bus
        /// (0xFF) and drop writes while disabled.
        flash_enable: bool,
        /// The flash /WP pin: gates erase/program of sector 0 + the hidden
        /// region (register 0x1000, default off after power-up).
        flash_we: bool,
        /// Boxed to keep the Mapper enum near the size of its other
        /// variants (the chip state is ~300 bytes inline).
        flash: Box<Mbc6Flash>,
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
    /// Game Genie ROM-patch cheats (empty in production → `read_rom` is
    /// byte-identical; a default-off mutating debug hook, set by the frontend
    /// cheat engine). See [`Self::set_gg_patches`].
    gg: Vec<GgPatch>,
}

/// A Game Genie ROM patch: substitute `value` when the CPU reads `addr`, gated
/// (for 9-digit codes) on the current byte matching `compare`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GgPatch {
    pub addr: u16,
    pub value: u8,
    pub compare: Option<u8>,
}

/// What the 0xA000-0xBFFF window currently addresses.
enum RamTarget {
    /// External RAM bank (pre-masking), in 8 KiB [`RAM_BANK_SIZE`] units.
    Ram(usize),
    /// MBC2 built-in half-byte RAM.
    Mbc2,
    /// MBC3 RTC register index 0-4 (S, M, H, DL, DH).
    Rtc(usize),
    /// MBC6 4 KiB RAM bank, as the byte base offset of the selected bank
    /// (the two windows address independent banks, see `ram_target`).
    Mbc6(usize),
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
