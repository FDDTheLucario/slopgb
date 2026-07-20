//! Unit tests for the cartridge/MBC layer. Split out of `cartridge.rs`
//! for file size; compiled as `super::tests` via the `#[path]` attribute.

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

#[path = "cartridge_tests/banks.rs"]
mod banks;

#[path = "cartridge_tests/mbc6.rs"]
mod mbc6;

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
        Some(CartridgeError::BadHeader(6))
    );
}

#[test]
fn header_bad_rom_size_rejected() {
    let mut rom = make_rom(0x01, 2, 0);
    rom[0x148] = 9;
    assert_eq!(
        Cartridge::from_bytes(rom).err(),
        Some(CartridgeError::BadHeader(9))
    );
}

#[test]
fn bad_header_display_includes_offending_byte() {
    let err = CartridgeError::BadHeader(0x09);
    assert!(err.to_string().contains("0x09"), "{err}");
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
fn mbc30_8bit_romb_on_4mb_rom() {
    // A 4 MiB MBC3-type cart can only be the MBC30 chip (Pan Docs
    // "MBC3"; the mbc3-tester ROM): all 8 ROMB bits are wired.
    let mut c = cart(0x11, 256, 0);
    c.write_rom(0x2000, 0x80);
    assert_eq!(bank_at(&c, 0x4000), 128);
    c.write_rom(0x2000, 0xFF);
    assert_eq!(bank_at(&c, 0x4000), 255);
    // Zero substitution still applies to the full 8-bit value.
    c.write_rom(0x2000, 0x00);
    assert_eq!(bank_at(&c, 0x4000), 1);
}

#[test]
fn mbc30_detected_by_64kb_ram() {
    // 64 KiB RAM (8 banks) implies MBC30 even with a small ROM
    // (SameBoy Core/mbc.c detection; Pokemon Crystal JP shape).
    let mut c = cart(0x13, 4, 5);
    // 8-bit ROMB: 0x80 stays bank 128 (masked to the 4-bank ROM = 0),
    // not the 7-bit chip's zero-substituted bank 1.
    c.write_rom(0x2000, 0x80);
    assert_eq!(bank_at(&c, 0x4000), 0);
    // All 8 RAM banks are distinct.
    c.write_rom(0x0000, 0x0A);
    for bank in 0..8 {
        c.write_rom(0x4000, bank);
        c.write_ram(0xA000, 0x40 + bank);
    }
    for bank in 0..8 {
        c.write_rom(0x4000, bank);
        assert_eq!(c.read_ram(0xA000), 0x40 + bank);
    }
}

#[test]
fn supports_sgb_requires_flag_and_old_licensee() {
    // Pan Docs "SGB flag" (0x146 == 0x03) and old licensee code
    // (0x14B == 0x33) must both match.
    let mut rom = make_rom(0x00, 2, 0);
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33;
    assert!(Cartridge::from_bytes(rom.clone()).unwrap().supports_sgb());
    rom[0x14B] = 0x01;
    assert!(!Cartridge::from_bytes(rom.clone()).unwrap().supports_sgb());
    rom[0x14B] = 0x33;
    rom[0x146] = 0x00;
    assert!(!Cartridge::from_bytes(rom).unwrap().supports_sgb());
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
    // Content round-trip: the byte we wrote must actually appear in the save
    // image (RAM bank 0 selected at reset → offset 0), so a right-sized but
    // zeroed buffer would fail rather than pass on length alone.
    assert_eq!(
        save[0], 0x42,
        "written RAM byte survives into the save image"
    );
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
