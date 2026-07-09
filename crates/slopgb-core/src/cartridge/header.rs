//! Cartridge header parsing: size codes, mapper detection, CGB/SGB flags.

use super::*;

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
            gg: Vec::new(),
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
}
