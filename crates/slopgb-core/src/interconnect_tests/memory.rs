//! `interconnect_tests` — memory tests (split for file size).

use super::*;

#[test]
fn rom_reads_route_to_cartridge() {
    let mut b = ic(Model::Dmg);
    assert_eq!(b.read(0x1000), 0x5A);
    assert_eq!(b.read(0x1001), 0x5B);
}

#[test]
fn wram_and_echo_are_the_same_memory() {
    let mut b = ic(Model::Dmg);
    b.write(0xC000, 0x11);
    b.write(0xDDFF, 0x22);
    assert_eq!(b.read(0xE000), 0x11);
    assert_eq!(b.read(0xFDFF), 0x22);
    b.write(0xE123, 0x33);
    assert_eq!(b.read(0xC123), 0x33);
}

#[test]
fn hram_round_trips() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF80, 0xAB);
    b.write(0xFFFE, 0xCD);
    assert_eq!(b.read(0xFF80), 0xAB);
    assert_eq!(b.read(0xFFFE), 0xCD);
}

#[test]
fn ie_stores_all_8_bits() {
    let mut b = ic(Model::Dmg);
    b.write(0xFFFF, 0xFF);
    assert_eq!(b.read(0xFFFF), 0xFF);
    b.write(0xFFFF, 0xE4);
    assert_eq!(b.read(0xFFFF), 0xE4);
}

#[test]
fn if_upper_three_bits_read_one() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF0F, 0x00);
    assert_eq!(b.read(0xFF0F), 0xE0);
    b.write(0xFF0F, 0xFF);
    assert_eq!(b.read(0xFF0F), 0xFF);
    assert_eq!(b.pending(), 0); // IE = 0
    b.write(0xFFFF, 0x1F);
    assert_eq!(b.pending(), 0x1F);
    b.ack(0);
    assert_eq!(b.read(0xFF0F), 0xFE);
}

#[test]
fn ff50_reads_ff_and_ignores_writes() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF50, 0x00);
    assert_eq!(b.read(0xFF50), 0xFF);
}

#[test]
fn unmapped_io_reads_ff() {
    let mut b = ic(Model::Dmg);
    for addr in [
        0xFF03, 0xFF08, 0xFF0E, 0xFF4C, 0xFF4E, 0xFF57, 0xFF6D, 0xFF7F,
    ] {
        assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
    }
}

#[test]
fn dmg_has_no_cgb_registers() {
    let mut b = ic(Model::Dmg);
    for addr in [
        0xFF4D, 0xFF4F, 0xFF51, 0xFF52, 0xFF53, 0xFF54, 0xFF55, 0xFF56, 0xFF68, 0xFF69, 0xFF6A,
        0xFF6B, 0xFF6C, 0xFF70, 0xFF72, 0xFF73, 0xFF74, 0xFF75, 0xFF76, 0xFF77,
    ] {
        b.write(addr, 0x00);
        assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
    }
}

/// CGB WRAM has its own bus: a WRAM-source transfer leaves the
/// external bus alone, and a ROM-source transfer never puts its byte
/// on the WRAM bus — a WRAM-region read mid-transfer goes through the
/// conflict *redirect* (same cell here: FF46 bit 4 = 0, offset 0)
/// rather than observing the ROM byte.
#[test]
fn cgb_wram_is_a_separate_bus() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0x00); // ROM source
    b.tick();
    b.tick();
    assert_eq!(b.read(0xC000), 0x80, "no ROM byte on the CGB WRAM bus");
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0); // WRAM source
    b.tick();
    b.tick();
    assert_eq!(b.read(0x1000), 0x5A, "ROM does not conflict with CGB WRAM");
    assert_eq!(b.read(0xC050), 0x82, "WRAM read sees DMA byte 2");
}

#[test]
fn prohibited_area_dmg() {
    let mut b = ic(Model::Dmg);
    assert_eq!(b.read(0xFEA0), 0x00, "LCD off: OAM idle");
    b.write(0xFEA0, 0x55); // writes ignored
    assert_eq!(b.read(0xFEA0), 0x00);
    b.write(0xFF40, 0x91);
    // Advance into mode 3 of a steady line (the glitched enable line
    // blocks from dot 78 already, take line 1 to be safe).
    ticks(&mut b, (452 + 120) / 4);
    assert_eq!(b.read(0xFEA0), 0xFF, "OAM locked: reads $FF");
}

/// FEA0-FEFF on CPU CGB C (the silicon [`Model::Cgb`] pins, see
/// ARCHITECTURE §CGB revision policy): extra OAM RAM whose low address
/// bits 3-4 don't decode, so each of the 24 cells is mirrored 4 times
/// across the region (Pan Docs "FEA0-FEFF range", revisions 0-D;
/// gambatte-core memory.cpp indexes `ioamhram_[(p - 0xFE00) & 0xE7]`;
/// pinned by gambatte oamdma_srcXXXX_busypushFEA1/FF01 cgb04c rows,
/// whose markers written there survive a dropped mid-DMA push).
#[test]
fn prohibited_area_cgb_c_is_extra_ram_with_mirrors() {
    let mut b = ic(Model::Cgb);
    b.write(0xFEA0, 0x12);
    b.write(0xFEC1, 0x34);
    b.write(0xFEFF, 0x56);
    assert_eq!(b.read(0xFEA0), 0x12);
    for mirror in [0xFEA8, 0xFEB0, 0xFEB8] {
        assert_eq!(b.read(mirror), 0x12, "{mirror:04X} mirrors FEA0");
    }
    assert_eq!(b.read(0xFEC9), 0x34, "FEC9 mirrors FEC1");
    assert_eq!(b.read(0xFEF7), 0x56, "FEF7 mirrors FEFF");
    assert_eq!(b.read(0xFEA1), 0x00, "distinct cell untouched");
}

/// The extra RAM sits behind the same OAM gating as FE00-FE9F: $FF /
/// dropped while a DMA byte is in flight (gambatte memory.cpp:
/// `oamDmaPos_ < oam_size` guards both paths).
#[test]
fn cgb_extra_ram_blocked_during_oam_dma() {
    let mut b = ic(Model::Cgb);
    b.write(0xFEA0, 0x12);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.tick(); // setup
    b.write(0xFEA0, 0x99); // in flight: dropped
    assert_eq!(b.read(0xFEA0), 0xFF, "in flight: reads $FF");
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFEA0), 0x12, "marker survived the transfer");
}

/// AGB (and CGB revision E) instead echo the high nibble of the low
/// address byte twice (Pan Docs "FEA0-FEFF range").
#[test]
fn prohibited_area_agb_echoes_high_nibble() {
    let mut b = ic(Model::Agb);
    assert_eq!(b.read(0xFEA3), 0xAA);
    assert_eq!(b.read(0xFEB0), 0xBB);
    assert_eq!(b.read(0xFEFF), 0xFF);
}

#[test]
fn cgb_dmg_compat_mode_disables_cgb_only_registers() {
    let mut b = ic(Model::Cgb); // DMG cart on CGB hardware
    assert!(!b.cgb_mode);
    for addr in [
        0xFF4D, 0xFF51, 0xFF55, 0xFF56, 0xFF69, 0xFF6B, 0xFF70, 0xFF74,
    ] {
        b.write(addr, 0x00);
        assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
    }
    assert_eq!(b.read(0xFF4F), 0xFE, "VBK still reads bank 0");
    b.write(0xFF4F, 0x01); // locked: write ignored
    assert_eq!(b.read(0xFF4F), 0xFE);
    // FF72/73/75 exist in both modes (boot_hwio-C).
    b.write(0xFF72, 0xAB);
    assert_eq!(b.read(0xFF72), 0xAB);
    b.write(0xFF75, 0xFF);
    assert_eq!(b.read(0xFF75), 0xFF);
    b.write(0xFF75, 0x00);
    assert_eq!(b.read(0xFF75), 0x8F);
    assert_eq!(b.read(0xFF76), 0x00);
    assert_eq!(b.read(0xFF77), 0x00);
    // SVBK locked: D000 stays bank 1.
    b.write(0xC000, 1);
    b.write(0xD000, 2);
    b.write(0xFF70, 0x03);
    assert_eq!(b.read(0xD000), 2);
}

#[test]
fn cgb_mode_decodes_only_header_bit7() {
    // Pan Docs "CGB flag" (0x143): the CGB boot ROM tests only bit 7,
    // so 0x84 enables CGB mode just like 0x80/0xC0 — and `auto_model`
    // must agree (shared predicate, `cartridge::cgb_flag`).
    let mut rom = test_rom();
    rom[0x143] = 0x84;
    assert_eq!(crate::GameBoy::auto_model(&rom), Model::Cgb);
    let b = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
    assert!(b.cgb_mode);
}

#[test]
fn cgb_mode_vbk_banks_vram() {
    let mut b = ic_cgb_mode();
    b.write(0x8000, 0x11);
    b.write(0xFF4F, 0x01);
    assert_eq!(b.read(0xFF4F), 0xFF);
    assert_eq!(b.read(0x8000), 0x00);
    assert_eq!(b.ppu().vram_bank(), 1);
    b.write(0x8000, 0x22);
    b.write(0xFF4F, 0xFE); // only bit 0 matters
    assert_eq!(b.read(0x8000), 0x11);
    assert_eq!(b.ppu().vram_bank(), 0);
    b.write(0xFF4F, 0x01);
    assert_eq!(b.read(0x8000), 0x22);
}

#[test]
fn cgb_mode_svbk_banks_wram() {
    let mut b = ic_cgb_mode();
    assert_eq!(b.read(0xFF70), 0xF8);
    for bank in 1..8u8 {
        b.write(0xFF70, bank);
        b.write(0xD000, 0xB0 + bank);
    }
    for bank in 1..8u8 {
        b.write(0xFF70, 0xF8 | bank); // upper bits ignored
        assert_eq!(b.read(0xFF70), 0xF8 | bank);
        assert_eq!(b.read(0xD000), 0xB0 + bank, "bank {bank}");
    }
    // Bank 0 selects bank 1; C000 region is always bank 0.
    b.write(0xFF70, 0x00);
    assert_eq!(b.read(0xD000), 0xB1);
    b.write(0xC000, 0x77);
    assert_eq!(b.read(0xC000), 0x77);
    assert_eq!(b.read(0xE000), 0x77);
    // Echo of D000 region follows the bank.
    b.write(0xFF70, 0x04);
    assert_eq!(b.read(0xF000), 0xB4);
}

/// `peek` takes `&self`: it ticks nothing and observes raw memory —
/// WRAM/echo, HRAM, OAM, IE — without advancing time.
#[test]
fn peek_reads_plain_memory_without_time() {
    let mut b = ic(Model::Dmg);
    b.write_no_tick(0xC123, 0x11);
    b.write_no_tick(0xFF80, 0x22);
    b.write_no_tick(0xFE05, 0x33);
    b.write_no_tick(0xFFFF, 0xE4);
    let cycles = b.cycles();
    assert_eq!(b.peek(0xC123), 0x11);
    assert_eq!(b.peek(0xE123), 0x11, "echo");
    assert_eq!(b.peek(0xFF80), 0x22);
    assert_eq!(b.peek(0xFE05), 0x33);
    assert_eq!(b.peek(0xFFFF), 0xE4);
    assert_eq!(b.cycles(), cycles, "no time passed");
}

/// `peek` is omniscient by design: it ignores the PPU's mode-based
/// VRAM/OAM lockout that makes a real CPU read return $FF.
#[test]
fn peek_ignores_ppu_access_blocking() {
    let mut b = ic(Model::Dmg);
    b.write_no_tick(0x8500, 0x44);
    b.write_no_tick(0xFE00, 0x55);
    b.write(0xFF40, 0x91); // LCD on
    // Into mode 3 of the glitched first line: VRAM and OAM locked.
    ticks(&mut b, (452 + 120) / 4);
    assert_eq!(b.read(0x8500), 0xFF, "real VRAM read: locked");
    assert_eq!(b.read(0xFE00), 0xFF, "real OAM read: locked");
    assert_eq!(b.peek(0x8500), 0x44);
    assert_eq!(b.peek(0xFE00), 0x55);
}

/// IO registers are not peekable; the whole FF00-FF7F range (and the
/// FEA0-FEFF prohibited area) reads $FF through `peek`.
#[test]
fn peek_io_reads_ff() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF40, 0x91);
    assert_eq!(b.read(0xFF40), 0x91, "real IO read works");
    assert_eq!(b.peek(0xFF40), 0xFF, "peek does not");
    assert_eq!(b.peek(0xFF00), 0xFF);
    assert_eq!(b.peek(0xFF0F), 0xFF);
    assert_eq!(b.peek(0xFEA0), 0xFF);
}

/// `peek` follows the live VBK/SVBK banking on CGB.
#[test]
fn peek_follows_cgb_banking() {
    let mut b = ic_cgb_mode();
    b.write(0x8000, 0x11);
    b.write(0xFF4F, 0x01);
    b.write(0x8000, 0x22);
    assert_eq!(b.peek(0x8000), 0x22, "active VRAM bank");
    b.write(0xFF4F, 0x00);
    assert_eq!(b.peek(0x8000), 0x11);
    b.write(0xFF70, 0x03);
    b.write(0xD000, 0x33);
    b.write(0xFF70, 0x04);
    b.write(0xD000, 0x44);
    assert_eq!(b.peek(0xD000), 0x44, "active WRAM bank");
    assert_eq!(b.peek(0xF000), 0x44, "echo follows the bank");
    b.write(0xFF70, 0x03);
    assert_eq!(b.peek(0xD000), 0x33);
}
