//! Current ROM/RAM bank indicators (debug, golden-safe `&self`). Split out of
//! `cartridge_tests.rs` to keep that file under the 1000-line cap; compiled as
//! `super::banks` via the `#[path]` attribute, sharing its `make_rom`/`cart`/
//! `bank_at` helpers through `use super::*`.

use super::*;

#[test]
fn cur_rom_bank_matches_high_area_mapping() {
    // None mapper: high area is fixed bank 1.
    assert_eq!(cart(0x00, 2, 0).cur_rom_bank(), 1);
    // MBC1: BANK2 drives the lines above BANK1 — (1<<5)|5 = 0x25.
    let mut c = cart(0x01, 64, 0);
    c.write_rom(0x2000, 5); // BANK1 = 5
    c.write_rom(0x4000, 1); // BANK2 = 1
    assert_eq!(c.cur_rom_bank(), 0x25);
    assert_eq!(c.cur_rom_bank(), bank_at(&c, 0x4000) as usize);
    // MBC3: 7-bit ROMB.
    let mut c = cart(0x11, 128, 0);
    c.write_rom(0x2000, 0x42);
    assert_eq!(c.cur_rom_bank(), 0x42);
    assert_eq!(c.cur_rom_bank(), bank_at(&c, 0x4000) as usize);
    // MBC5: 9-bit ROMB (romb1 is bit 8).
    let mut c = cart(0x19, 512, 0);
    c.write_rom(0x2000, 0x05); // ROMB0
    c.write_rom(0x3000, 0x01); // ROMB1 (bit 8)
    assert_eq!(c.cur_rom_bank(), 0x105);
    assert_eq!(c.cur_rom_bank(), bank_at(&c, 0x4000) as usize);
}

#[test]
fn cur_ram_bank_reflects_enable_and_mode() {
    // MBC1 + RAM (32 KiB = 4 banks): disabled → None; mode 1 → BANK2; mode 0 → 0.
    let mut c = cart(0x03, 4, 3);
    assert_eq!(c.cur_ram_bank(), None, "RAM disabled at power-on");
    c.write_rom(0x0000, 0x0A); // RAMG enable
    c.write_rom(0x4000, 2); // BANK2 = 2
    c.write_rom(0x6000, 0x01); // mode 1: RAM banking
    assert_eq!(c.cur_ram_bank(), Some(2));
    c.write_rom(0x6000, 0x00); // mode 0: RAM bank forced 0
    assert_eq!(c.cur_ram_bank(), Some(0));
    c.write_rom(0x0000, 0x00); // RAMG disable
    assert_eq!(c.cur_ram_bank(), None);
    // MBC5 + RAM: the selected RAM bank.
    let mut c = cart(0x1A, 4, 3);
    c.write_rom(0x0000, 0x0A);
    c.write_rom(0x4000, 3);
    assert_eq!(c.cur_ram_bank(), Some(3));
}

#[test]
fn cur_ram_bank_is_none_without_a_ram_chip() {
    // A cart with no external RAM has no bank to report, even where a mapper
    // would nominally select one (None mapper; or RAMG enabled with no chip).
    assert_eq!(cart(0x00, 2, 0).cur_ram_bank(), None, "no-MBC, no RAM");
    let mut c = cart(0x01, 2, 0); // MBC1, RAM size code 0 = no RAM
    c.write_rom(0x0000, 0x0A); // RAMG enable — still no chip
    assert_eq!(c.cur_ram_bank(), None);
}
