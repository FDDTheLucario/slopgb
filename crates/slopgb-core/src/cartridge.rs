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

pub struct Cartridge {
    // Cartridge work package owns ROM data, RAM, mapper state.
}

impl Cartridge {
    pub fn from_bytes(rom: Vec<u8>) -> Result<Self, CartridgeError> {
        let _ = rom;
        todo!("cartridge work package")
    }

    /// Read 0x0000-0x7FFF (banked ROM).
    pub fn read_rom(&self, addr: u16) -> u8 {
        let _ = addr;
        todo!("cartridge work package")
    }

    /// Write 0x0000-0x7FFF (mapper registers).
    pub fn write_rom(&mut self, addr: u16, value: u8) {
        let _ = (addr, value);
        todo!("cartridge work package")
    }

    /// Read 0xA000-0xBFFF (external RAM / RTC / MBC2 built-in RAM).
    pub fn read_ram(&self, addr: u16) -> u8 {
        let _ = addr;
        todo!("cartridge work package")
    }

    /// Write 0xA000-0xBFFF.
    pub fn write_ram(&mut self, addr: u16, value: u8) {
        let _ = (addr, value);
        todo!("cartridge work package")
    }

    /// Battery-backed RAM image (+ serialized RTC for MBC3), None if the
    /// cartridge has no battery.
    pub fn save_data(&self) -> Option<Vec<u8>> {
        todo!("cartridge work package")
    }

    /// Restore a [`Self::save_data`] image. Wrong-size data is ignored.
    pub fn load_save_data(&mut self, data: &[u8]) {
        let _ = data;
        todo!("cartridge work package")
    }
}
