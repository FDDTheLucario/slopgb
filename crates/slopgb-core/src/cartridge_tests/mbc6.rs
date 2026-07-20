//! MBC6 unit tests (Pan Docs "MBC6"): 8 KiB ROM/flash windows, 4 KiB RAM
//! windows, and the MX29F008 flash command set. Written against the spec;
//! the `roms/mbc6` exerciser pins the same behavior end to end.

use super::*;

/// 128 KiB MBC6 ROM (16 × 8 KiB banks): every 8 KiB half-bank stores its
/// MBC6 bank number at offset 0.
fn mbc6_rom() -> Vec<u8> {
    let mut rom = make_rom(0x20, 8, 3);
    for (b, half) in rom.chunks_exact_mut(0x2000).enumerate() {
        half[0] = b as u8;
    }
    rom
}

fn mbc6_cart() -> Cartridge {
    Cartridge::from_bytes(mbc6_rom()).unwrap()
}

/// Map the flash into window A (flash enable + select).
fn flash_on_a(c: &mut Cartridge) {
    c.write_rom(0x0C00, 0x01);
    c.write_rom(0x2800, 0x08);
}

/// The JEDEC unlock through window A: $AA to flash 2:$5555, $55 to 1:$4AAA.
/// Leaves flash bank 2 selected in window A.
fn unlock_a(c: &mut Cartridge) {
    c.write_rom(0x2000, 2);
    c.write_rom(0x5555, 0xAA);
    c.write_rom(0x2000, 1);
    c.write_rom(0x4AAA, 0x55);
    c.write_rom(0x2000, 2);
}

/// Unlock + a command byte at flash 2:$5555 through window A.
fn cmd_a(c: &mut Cartridge, cmd: u8) {
    unlock_a(c);
    c.write_rom(0x5555, cmd);
}

/// Exit the current flash mode ($F0 anywhere in the window).
fn exit_a(c: &mut Cartridge) {
    c.write_rom(0x4000, 0xF0);
}

/// Program a full 128-byte page (offsets 0-0x7F of flash bank `bank`) with
/// `f(offset)` per byte, following the hardware protocol (Pan Docs): 128
/// data loads, a commit rewrite of the final address (value ignored, must
/// not be $F0), then mode exit.
fn program_page_a(c: &mut Cartridge, bank: u8, f: impl Fn(u16) -> u8) {
    cmd_a(c, 0xA0);
    c.write_rom(0x2000, bank);
    for i in 0..128u16 {
        c.write_rom(0x4000 + i, f(i));
    }
    c.write_rom(0x407F, 0xAB);
    exit_a(c);
}

/// Program every byte of a page to `value` through window A.
fn program_a(c: &mut Cartridge, bank: u8, value: u8) {
    program_page_a(c, bank, |_| value);
}

/// Program a full 128-byte page of the hidden region (offsets 0-0x7F) with
/// `f(offset)` per byte, same protocol through the $60/$E0 command.
fn program_hidden_page_a(c: &mut Cartridge, f: impl Fn(u16) -> u8) {
    cmd_a(c, 0x60);
    cmd_a(c, 0xE0);
    c.write_rom(0x2000, 0);
    for i in 0..128u16 {
        c.write_rom(0x4000 + i, f(i));
    }
    c.write_rom(0x407F, 0xAB);
    exit_a(c);
}

/// Erase the sector containing flash bank `bank` through window A
/// ($80 family + $30 at an address inside the sector), then exit.
fn erase_sector_a(c: &mut Cartridge, bank: u8) {
    cmd_a(c, 0x80);
    unlock_a(c);
    c.write_rom(0x2000, bank);
    c.write_rom(0x4000, 0x30);
    exit_a(c);
}

/// Read window A at `addr` with flash/ROM bank `bank` selected.
fn read_a(c: &mut Cartridge, bank: u8, addr: u16) -> u8 {
    c.write_rom(0x2000, bank);
    c.read_rom(addr)
}

// --- header ---

#[test]
fn mbc6_header_accepted_with_battery() {
    let c = mbc6_cart();
    // The only MBC6 cart (Net de Get) is battery-backed; 32 KiB SRAM.
    assert!(c.has_battery);
    assert_eq!(c.ram.len(), 0x8000);
}

// --- ROM banking ---

#[test]
fn mbc6_rom_window_a_selects_8kib_banks() {
    let mut c = mbc6_cart();
    c.write_rom(0x2800, 0x00);
    for bank in 0..16 {
        assert_eq!(read_a(&mut c, bank, 0x4000), bank, "window A bank {bank}");
    }
}

#[test]
fn mbc6_rom_window_b_selects_8kib_banks() {
    let mut c = mbc6_cart();
    c.write_rom(0x3800, 0x00);
    for bank in 0..16 {
        c.write_rom(0x3000, bank);
        assert_eq!(c.read_rom(0x6000), bank, "window B bank {bank}");
    }
}

#[test]
fn mbc6_rom_windows_bank_independently() {
    let mut c = mbc6_cart();
    c.write_rom(0x2000, 4);
    c.write_rom(0x3000, 9);
    assert_eq!(c.read_rom(0x4000), 4);
    assert_eq!(c.read_rom(0x6000), 9);
    c.write_rom(0x2000, 5);
    assert_eq!(c.read_rom(0x6000), 9, "changing A must not disturb B");
    assert_eq!(c.read_rom(0x4000), 5);
}

#[test]
fn mbc6_rom_bank_register_is_7_bits() {
    let mut c = mbc6_cart();
    // Bit 7 is not part of the bank number (banks are 00-7F).
    assert_eq!(read_a(&mut c, 0x80 | 3, 0x4000), 3);
}

#[test]
fn mbc6_rom_low_area_fixed() {
    let mut c = mbc6_cart();
    c.write_rom(0x2000, 7);
    c.write_rom(0x3000, 9);
    // 0x0000-0x3FFF stays the first 16 KiB regardless of the window banks.
    assert_eq!(c.read_rom(0x0000), 0);
    assert_eq!(c.read_rom(0x2000), 1);
}

#[test]
fn mbc6_rom_bank_out_of_range_mirrors() {
    let mut c = mbc6_cart();
    // 128 KiB ROM = 16 MBC6 banks; the power-of-two pad mirrors bank 16+
    // the way unconnected high address lines do.
    assert_eq!(read_a(&mut c, 16, 0x4000), 0);
    assert_eq!(read_a(&mut c, 17, 0x4000), 1);
}

// --- RAM banking ---

#[test]
fn mbc6_ram_enable_compares_low_nibble() {
    let mut c = mbc6_cart();
    // "Mostly the same as for MBC1": the low nibble 0x0A enables.
    c.write_rom(0x0000, 0x3A);
    c.write_ram(0xA000, 0x11);
    assert_eq!(c.read_ram(0xA000), 0x11);
    c.write_rom(0x0000, 0x0B);
    assert_eq!(c.read_ram(0xA000), 0xFF, "0x0B must disable");
    // The register spans 0x0000-0x03FF; 0x0400 is the bank A register.
    c.write_rom(0x03FF, 0x0A);
    assert_eq!(c.read_ram(0xA000), 0x11);
}

#[test]
fn mbc6_ram_disabled_reads_ff_and_drops_writes() {
    let mut c = mbc6_cart();
    c.write_rom(0x0000, 0x0A);
    c.write_ram(0xA000, 0x55);
    c.write_rom(0x0000, 0x00);
    assert_eq!(c.read_ram(0xA000), 0xFF);
    c.write_ram(0xA000, 0xAA);
    c.write_rom(0x0000, 0x0A);
    assert_eq!(c.read_ram(0xA000), 0x55, "disabled write must be dropped");
}

#[test]
fn mbc6_ram_windows_bank_independently_4kib() {
    let mut c = mbc6_cart();
    c.write_rom(0x0000, 0x0A);
    for bank in 0..8u8 {
        c.write_rom(0x0400, bank);
        c.write_ram(0xA000, 0xA0 | bank);
        c.write_rom(0x0800, bank);
        c.write_ram(0xB008, 0xB0 | bank);
    }
    for bank in 0..8u8 {
        c.write_rom(0x0400, bank);
        assert_eq!(c.read_ram(0xA000), 0xA0 | bank, "window A bank {bank}");
        c.write_rom(0x0800, bank);
        assert_eq!(c.read_ram(0xB008), 0xB0 | bank, "window B bank {bank}");
    }
}

#[test]
fn mbc6_ram_windows_share_one_array() {
    let mut c = mbc6_cart();
    c.write_rom(0x0000, 0x0A);
    c.write_rom(0x0400, 3);
    c.write_rom(0x0800, 3);
    c.write_ram(0xA123, 0x77);
    assert_eq!(c.read_ram(0xB123), 0x77, "same bank, same bytes");
}

#[test]
fn mbc6_ram_bank_register_is_3_bits() {
    let mut c = mbc6_cart();
    c.write_rom(0x0000, 0x0A);
    c.write_rom(0x0400, 7);
    c.write_ram(0xA000, 0x99);
    c.write_rom(0x0400, 0x0F);
    assert_eq!(c.read_ram(0xA000), 0x99, "bank bits above 2 ignored");
}

// --- flash mapping ---

#[test]
fn mbc6_flash_select_maps_flash_and_back() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    // Never-programmed flash reads all-ones, unlike the ROM markers.
    assert_eq!(read_a(&mut c, 3, 0x4000), 0xFF);
    c.write_rom(0x2800, 0x00);
    assert_eq!(read_a(&mut c, 3, 0x4000), 3, "ROM is back after deselect");
}

#[test]
fn mbc6_flash_disabled_reads_open_bus() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    program_a(&mut c, 16, 0x00);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0x00);
    // Drop /CE (bit 0 only): the chip stops driving the bus.
    c.write_rom(0x0C00, 0x02);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0xFF);
    c.write_rom(0x0C00, 0x03);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0x00);
}

#[test]
fn mbc6_flash_commands_need_flash_mapped() {
    let mut c = mbc6_cart();
    c.write_rom(0x0C00, 0x01);
    c.write_rom(0x2800, 0x00);
    // The unlock lands in a ROM-mapped window: the chip never sees it.
    cmd_a(&mut c, 0x90);
    c.write_rom(0x2800, 0x08);
    assert_eq!(
        read_a(&mut c, 0, 0x4000),
        0xFF,
        "must read array data, not the JEDEC ID"
    );
}

// --- flash commands ---

#[test]
fn mbc6_flash_id_mode_reads_jedec_id() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    cmd_a(&mut c, 0x90);
    assert_eq!(c.read_rom(0x4000), 0xC2, "manufacturer at even addresses");
    assert_eq!(c.read_rom(0x4001), 0x81, "device at odd addresses");
    exit_a(&mut c);
    assert_eq!(c.read_rom(0x4000), 0xFF, "$F0 exits ID mode");
}

#[test]
fn mbc6_flash_unlock_requires_exact_addresses() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    // $AA at 2:$5554 is not an unlock start.
    c.write_rom(0x2000, 2);
    c.write_rom(0x5554, 0xAA);
    c.write_rom(0x2000, 1);
    c.write_rom(0x4AAA, 0x55);
    c.write_rom(0x2000, 2);
    c.write_rom(0x5555, 0x90);
    assert_eq!(c.read_rom(0x4000), 0xFF, "no ID mode without the unlock");
}

#[test]
fn mbc6_flash_program_ands_bits_and_erase_restores() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    program_a(&mut c, 16, 0x3C);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0x3C);
    // A second program without erase can only clear more bits.
    program_a(&mut c, 16, 0xC3);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0x00);
    erase_sector_a(&mut c, 16);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0xFF);
}

#[test]
fn mbc6_flash_erase_leaves_other_sectors() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    program_a(&mut c, 16, 0x11); // sector 1
    program_a(&mut c, 32, 0x22); // sector 2
    erase_sector_a(&mut c, 16);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0xFF);
    assert_eq!(read_a(&mut c, 32, 0x4000), 0x22);
}

#[test]
fn mbc6_flash_program_mode_reads_status() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    cmd_a(&mut c, 0xA0);
    c.write_rom(0x2000, 16);
    for i in 0..128u16 {
        c.write_rom(0x4000 + i, 0x55);
    }
    // Program mode reads status; the commit is instantaneous, so bit 7
    // (finished) is always set.
    assert_eq!(c.read_rom(0x4000), 0x80);
    c.write_rom(0x407F, 0x55);
    assert_eq!(c.read_rom(0x4000), 0x80);
    exit_a(&mut c);
    assert_eq!(c.read_rom(0x4000), 0x55);
}

#[test]
fn mbc6_flash_program_needs_full_page_and_commit() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    // A full page load with $F0 written instead of the commit aborts: the
    // page buffer is dropped (Pan Docs: commit is "any value (except $F0)
    // to the final address").
    cmd_a(&mut c, 0xA0);
    c.write_rom(0x2000, 16);
    for i in 0..128u16 {
        c.write_rom(0x4000 + i, 0x00);
    }
    c.write_rom(0x407F, 0xF0);
    assert_eq!(
        read_a(&mut c, 16, 0x4000),
        0xFF,
        "aborted page must not land"
    );
}

#[test]
fn mbc6_flash_commit_requires_the_final_page_address() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    cmd_a(&mut c, 0xA0);
    c.write_rom(0x2000, 16);
    for i in 0..128u16 {
        c.write_rom(0x4000 + i, 0x00);
    }
    // With the commit pending, a write anywhere but the page's final
    // address must be ignored (the chip waits for the final-address
    // rewrite or $F0)...
    c.write_rom(0x4000, 0x00);
    // ...so the $F0 abort still finds an uncommitted page.
    c.write_rom(0x407F, 0xF0);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0xFF, "stray write committed");
    // A proper commit still works afterwards.
    program_a(&mut c, 16, 0x11);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0x11);
}

#[test]
fn mbc6_flash_program_commit_value_is_ignored() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    // The commit write is a trigger, not data: the final byte keeps its
    // loaded value (Pan Docs: "writing any value ... to commit").
    program_page_a(&mut c, 16, |i| i as u8);
    assert_eq!(read_a(&mut c, 16, 0x407F), 0x7F);
}

#[test]
fn mbc6_flash_program_stores_f0_data_bytes() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    // $F0 is only special as the commit write; as one of the 128 data
    // loads it programs like any other byte.
    program_page_a(&mut c, 16, |i| if i == 0 { 0xF0 } else { 0xFF });
    assert_eq!(read_a(&mut c, 16, 0x4000), 0xF0);
    assert_eq!(c.read_rom(0x4001), 0xFF);
}

#[test]
fn mbc6_flash_erase_chip_spares_locked_sector0() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x01);
    program_a(&mut c, 4, 0x44); // sector 0 (needs WE)
    c.write_rom(0x1000, 0x00);
    program_a(&mut c, 16, 0x11); // sector 1
    cmd_a(&mut c, 0x80);
    cmd_a(&mut c, 0x10); // chip erase, WE still 0
    exit_a(&mut c);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0xFF, "sector 1 erased");
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x44, "locked sector 0 spared");
    // With WE set the chip erase reaches sector 0 too.
    c.write_rom(0x1000, 0x01);
    cmd_a(&mut c, 0x80);
    cmd_a(&mut c, 0x10);
    exit_a(&mut c);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0xFF);
}

#[test]
fn mbc6_flash_sector0_write_enable_gate() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    // WE off (the power-up default): sector 0 cannot be programmed...
    program_a(&mut c, 4, 0x00);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0xFF);
    // ...or erased; sectors 1-7 always can.
    c.write_rom(0x1000, 0x01);
    program_a(&mut c, 4, 0x55);
    c.write_rom(0x1000, 0x00);
    erase_sector_a(&mut c, 4);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x55, "erase blocked with WE=0");
    program_a(&mut c, 16, 0x11);
    assert_eq!(
        read_a(&mut c, 16, 0x4000),
        0x11,
        "sector 1 unaffected by WE"
    );
    // WE on: sector 0 opens up.
    c.write_rom(0x1000, 0x01);
    erase_sector_a(&mut c, 4);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0xFF);
}

#[test]
fn mbc6_flash_protect_sector0_command() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x01);
    program_a(&mut c, 4, 0x55);
    // Protect sector 0 (requires WE); the status byte reports it in bit 1.
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x20);
    assert_eq!(c.read_rom(0x4000), 0x82);
    exit_a(&mut c);
    // WE=1 alone is not enough now: the command protect also blocks.
    program_a(&mut c, 4, 0x00);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x55);
    // Unprotect: back to writable, status bit 1 clears.
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x40);
    assert_eq!(c.read_rom(0x4000), 0x80);
    exit_a(&mut c);
    program_a(&mut c, 4, 0x00);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x00);
}

#[test]
fn mbc6_flash_protect_command_needs_we() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x00);
    // The protect command is WE-gated (Pan Docs marks it with *).
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x20);
    exit_a(&mut c);
    c.write_rom(0x1000, 0x01);
    program_a(&mut c, 4, 0x33);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x33, "no protect was latched");
}

#[test]
fn mbc6_flash_hidden_region() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x01);
    // Program a hidden page with bytes 0x12/0x34 at offsets 0x00/0x23
    // (the $60/$E0 command family, full page-load protocol).
    program_hidden_page_a(&mut c, |i| match i {
        0x00 => 0x12,
        0x23 => 0x34,
        _ => 0xFF,
    });
    // The array itself is untouched (the hidden region is separate).
    assert_eq!(read_a(&mut c, 0, 0x4000), 0xFF);
    // Hidden-read mode ($77 twice) maps it, mirrored every 256 bytes.
    cmd_a(&mut c, 0x77);
    cmd_a(&mut c, 0x77);
    assert_eq!(c.read_rom(0x4000), 0x12);
    assert_eq!(c.read_rom(0x4023), 0x34);
    assert_eq!(
        read_a(&mut c, 2, 0x4123),
        0x34,
        "hidden mirrors at 256 bytes"
    );
    exit_a(&mut c);
    // Chip erase must not touch the hidden region...
    cmd_a(&mut c, 0x80);
    cmd_a(&mut c, 0x10);
    exit_a(&mut c);
    cmd_a(&mut c, 0x77);
    cmd_a(&mut c, 0x77);
    c.write_rom(0x2000, 0);
    assert_eq!(c.read_rom(0x4000), 0x12);
    exit_a(&mut c);
    // ...only the dedicated hidden-erase command does.
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x04);
    exit_a(&mut c);
    cmd_a(&mut c, 0x77);
    cmd_a(&mut c, 0x77);
    assert_eq!(c.read_rom(0x4000), 0xFF);
    assert_eq!(c.read_rom(0x4023), 0xFF);
    exit_a(&mut c);
}

#[test]
fn mbc6_flash_hidden_program_needs_we() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x00);
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0xE0);
    c.write_rom(0x2000, 0);
    c.write_rom(0x4000, 0x00);
    exit_a(&mut c);
    cmd_a(&mut c, 0x77);
    cmd_a(&mut c, 0x77);
    assert_eq!(c.read_rom(0x4000), 0xFF, "hidden program blocked with WE=0");
    exit_a(&mut c);
}

#[test]
fn mbc6_flash_commands_via_window_b() {
    let mut c = mbc6_cart();
    c.write_rom(0x0C00, 0x01);
    c.write_rom(0x3800, 0x08);
    // The same chip addresses through window B: 2:$7555 / 1:$6AAA.
    c.write_rom(0x3000, 2);
    c.write_rom(0x7555, 0xAA);
    c.write_rom(0x3000, 1);
    c.write_rom(0x6AAA, 0x55);
    c.write_rom(0x3000, 2);
    c.write_rom(0x7555, 0x90);
    assert_eq!(c.read_rom(0x6000), 0xC2);
    assert_eq!(c.read_rom(0x6001), 0x81);
    c.write_rom(0x6000, 0xF0);
    assert_eq!(c.read_rom(0x6000), 0xFF);
}

#[test]
fn mbc6_flash_blocked_ops_stay_in_read_mode() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x01);
    program_a(&mut c, 4, 0x55);
    c.write_rom(0x1000, 0x00);
    // A blocked operation never starts: on hardware the chip stays in read
    // mode (no status byte with bit 7 "finished" ever appears). The
    // exerciser ROM relies on this by not polling after blocked ops.
    cmd_a(&mut c, 0x80);
    unlock_a(&mut c);
    c.write_rom(0x2000, 4);
    c.write_rom(0x4000, 0x30);
    assert_eq!(c.read_rom(0x4000), 0x55, "blocked sector-0 erase");
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x04);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x55, "blocked hidden erase");
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x20);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x55, "blocked protect");
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0xE0);
    assert_eq!(read_a(&mut c, 4, 0x4000), 0x55, "blocked hidden program");
}

#[test]
fn mbc6_flash_out_of_sequence_write_clears_pending_command() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    program_a(&mut c, 16, 0x11);
    // A stray write after the armed $80 resets the whole JEDEC state
    // machine; a later unlock + $30 must not still erase.
    cmd_a(&mut c, 0x80);
    c.write_rom(0x4000, 0x00);
    unlock_a(&mut c);
    c.write_rom(0x2000, 16);
    c.write_rom(0x4000, 0x30);
    assert_eq!(
        read_a(&mut c, 16, 0x4000),
        0x11,
        "stale $80 prefix survived"
    );
}

// --- debug indicators ---

#[test]
fn mbc6_debug_bank_indicators() {
    let mut c = mbc6_cart();
    c.write_rom(0x2000, 6);
    c.write_rom(0x3000, 9);
    // The 16 KiB-granularity indicator reports the bank pair of window A;
    // the per-address variant resolves window B's independent bank.
    assert_eq!(c.cur_rom_bank(), 3);
    assert_eq!(c.rom_bank_at(0x4000), 3);
    assert_eq!(c.rom_bank_at(0x5FFF), 3);
    assert_eq!(c.rom_bank_at(0x6000), 4);
    assert_eq!(c.rom_bank_at(0x7FFF), 4);
    c.write_rom(0x0000, 0x0A);
    c.write_rom(0x0400, 5);
    c.write_rom(0x0800, 7);
    // Reported in the 8 KiB units of the banked-SRAM debug consumers
    // (dump/CDL/pin all index bank * 0x2000): 4 KiB bank 5 = pair 2.
    assert_eq!(c.cur_ram_bank(), Some(2));
    // The per-address variant resolves window B's independent pair.
    assert_eq!(c.ram_bank_at(0xA000), Some(2));
    assert_eq!(c.ram_bank_at(0xB000), Some(3));
    c.write_rom(0x0000, 0x00);
    assert_eq!(c.cur_ram_bank(), None);
    assert_eq!(c.ram_bank_at(0xB000), None);
}

// --- battery save ---

#[test]
fn mbc6_battery_save_includes_flash() {
    let mut c = mbc6_cart();
    c.write_rom(0x0000, 0x0A);
    c.write_ram(0xA000, 0x42);
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x01);
    program_a(&mut c, 16, 0x5A);
    program_hidden_page_a(&mut c, |i| if i == 0 { 0x21 } else { 0xFF });
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x20); // protect sector 0 (non-volatile on hardware)
    exit_a(&mut c);

    let img = c.save_data().unwrap();
    assert_eq!(img.len(), 0x8000 + MBC6_FLASH_SIZE + 256 + 1);

    let mut c2 = mbc6_cart();
    assert!(c2.load_save_data(&img));
    c2.write_rom(0x0000, 0x0A);
    assert_eq!(c2.read_ram(0xA000), 0x42, "SRAM restored");
    flash_on_a(&mut c2);
    assert_eq!(read_a(&mut c2, 16, 0x4000), 0x5A, "flash array restored");
    // The protect flag survives: sector 0 stays blocked even with WE set.
    c2.write_rom(0x1000, 0x01);
    program_a(&mut c2, 4, 0x00);
    assert_eq!(read_a(&mut c2, 4, 0x4000), 0xFF, "protect flag restored");
    cmd_a(&mut c2, 0x77);
    cmd_a(&mut c2, 0x77);
    c2.write_rom(0x2000, 0);
    assert_eq!(c2.read_rom(0x4000), 0x21, "hidden region restored");
}

#[test]
fn mbc6_battery_save_accepts_plain_sram() {
    let mut c = mbc6_cart();
    // A foreign SRAM-only .sav must still import; the flash stays fresh.
    assert!(c.load_save_data(&vec![0x33; 0x8000]));
    c.write_rom(0x0000, 0x0A);
    assert_eq!(c.read_ram(0xA000), 0x33);
    flash_on_a(&mut c);
    assert_eq!(read_a(&mut c, 16, 0x4000), 0xFF);
}

// --- save state ---

#[test]
fn mbc6_state_restore_masks_bank_registers() {
    let mut c = mbc6_cart();
    let mut w = crate::state::Writer::new();
    c.write_state(&mut w);
    let mut bytes = w.into_vec();
    // Layout (see Cartridge::write_state + the Mbc6 arm): u32 RAM length,
    // 0x8000 RAM bytes, then ramg, ramb_a, ramb_b, romb_a, romb_b, flash_a,
    // flash_b, flash_enable, ...
    let base = 4 + 0x8000;
    bytes[base + 1] = 0xFF; // ramb_a: out of range
    bytes[base + 3] = 0xFF; // romb_a: out of range
    bytes[base + 5] = 1; // flash_a: flash mapped
    bytes[base + 7] = 1; // flash_enable
    let mut r = crate::state::Reader::new(&bytes);
    c.read_state(&mut r).unwrap();
    // A corrupt state must restore masked like the live register writes
    // (0x7F / 0x07) — reads stay in bounds instead of panicking.
    assert_eq!(c.read_rom(0x4000), 0xFF);
    c.write_rom(0x0000, 0x0A);
    c.write_ram(0xA000, 0x21);
    assert_eq!(c.read_ram(0xA000), 0x21);
}

#[test]
fn mbc6_state_roundtrip() {
    let mut c = mbc6_cart();
    flash_on_a(&mut c);
    c.write_rom(0x1000, 0x01);
    program_a(&mut c, 16, 0x5A);
    cmd_a(&mut c, 0x60);
    cmd_a(&mut c, 0x20); // protect sector 0
    exit_a(&mut c);
    c.write_rom(0x0000, 0x0A);
    c.write_rom(0x0400, 5);
    c.write_ram(0xA000, 0x66);
    c.write_rom(0x3000, 9);

    let mut w = crate::state::Writer::new();
    c.write_state(&mut w);
    let bytes = w.into_vec();
    let mut c2 = mbc6_cart();
    let mut r = crate::state::Reader::new(&bytes);
    c2.read_state(&mut r).unwrap();

    assert_eq!(read_a(&mut c2, 16, 0x4000), 0x5A, "flash data survives");
    assert_eq!(c2.read_rom(0x6000), 9, "window B bank survives");
    assert_eq!(c2.read_ram(0xA000), 0x66, "RAM bank + contents survive");
    // The protect flag survives: sector 0 stays blocked even with WE=1.
    c2.write_rom(0x1000, 0x01);
    program_a(&mut c2, 4, 0x00);
    assert_eq!(read_a(&mut c2, 4, 0x4000), 0xFF);
}
