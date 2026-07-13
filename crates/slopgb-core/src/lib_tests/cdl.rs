//! Code/Data Log + banked debug memory access (explicit ROM/VRAM/WRAM/SRAM banks).

use super::*;

#[test]
fn cdl_load_restores_a_flag_buffer() {
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    // The buffer is sized to the machine's physical layout, not a flat 64 KiB.
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    gb.set_cdl(false);
    let mut fixture = vec![0u8; n];
    fixture[0x0150] = 4; // ROM offset 0x150 (bank 0 low area) -> X
    assert!(gb.load_cdl(&fixture), "matching-size buffer loads");
    assert_eq!(gb.cdl_flag(0x0150), 4);
    assert_eq!(
        gb.cdl_flags().unwrap(),
        &fixture[..],
        "buffer restored verbatim"
    );
    assert!(!gb.load_cdl(&[0u8; 8]), "wrong-size buffer is rejected");
}

#[test]
fn debug_read_and_cdl_reach_explicit_sram_banks() {
    let mut gb = GameBoy::new(Model::Dmg, mbc1_ram_rom()).unwrap();
    // RAMG on + MBC1 mode 1 so BANK2 selects the RAM bank (gbctr).
    gb.debug_write(0x0000, 0x0A);
    gb.debug_write(0x6000, 0x01);
    // Stamp a bank-unique byte into each of the 4 RAM banks at 0xA000.
    for bank in 0..4u8 {
        gb.debug_write(0x4000, bank);
        gb.debug_write(0xA000, 0xD0 | bank);
    }
    // Any bank is reachable regardless of the live BANK2.
    for bank in 0..4u16 {
        assert_eq!(gb.debug_read_banked(bank, 0xA000), 0xD0 | bank as u8);
    }
    // Out-of-range bank folds within the chip (bank 4 wraps to bank 0), no OOB.
    assert_eq!(
        gb.debug_read_banked(4, 0xA000),
        gb.debug_read_banked(0, 0xA000)
    );

    // CDL follows the same bank map. Craft a fixture flagging SRAM bank 2 only
    // (debug_read is side-effect-free, so it can't record a flag): the physical
    // SRAM region sits after ROM (4*0x4000) + VRAM (0x4000); bank 2 @ 0xA000 is
    // offset 2*0x2000 within it.
    gb.set_cdl(true);
    let mut fx = vec![0u8; gb.cdl_flags().unwrap().len()];
    fx[0x10000 + 0x4000 + 2 * 0x2000] = 1;
    assert!(gb.load_cdl(&fx));
    assert_eq!(gb.cdl_flag_banked(2, 0xA000), 1);
    assert_eq!(gb.cdl_flag_banked(0, 0xA000), 0, "other banks unmarked");
    // The live bank (2) agrees with the plain cdl_flag.
    gb.debug_write(0x4000, 2);
    assert_eq!(gb.cdl_flag_banked(2, 0xA000), gb.cdl_flag(0xA000));
}

#[test]
fn banked_sram_on_a_cart_without_ram_reads_ff() {
    // No RAM chip → open-bus 0xFF for every bank, CDL always 0 (never OOB).
    let mut gb = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    assert_eq!(gb.debug_read_banked(3, 0xA000), 0xFF);
    gb.set_cdl(true);
    assert_eq!(gb.cdl_flag_banked(3, 0xA000), 0);
}

#[test]
fn debug_write_banked_edits_the_named_bank_of_each_region() {
    // CGB MBC5 + 32 KiB RAM: VRAM (2 banks), SRAM (4 banks), WRAM (8 banks).
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x143] = 0x80; // CGB
    rom[0x147] = 0x1A; // MBC5+RAM
    rom[0x148] = 0x03; // 8 ROM banks
    rom[0x149] = 0x03; // 32 KiB RAM (4 banks)
    let mut gb = GameBoy::new(Model::Cgb, rom).unwrap();
    // Poke a distinct byte into a non-live bank of each region, read it back
    // banked, and confirm the *live* bank was untouched.
    for (addr, bank, val, live) in [
        (0x8000u16, 1u16, 0xE1u8, 0u16),
        (0xA000, 3, 0xE3, 0),
        (0xD000, 5, 0xE5, 1),
    ] {
        gb.debug_write_banked(bank, addr, val);
        assert_eq!(
            gb.debug_read_banked(bank, addr),
            val,
            "edit lands in {bank}"
        );
        assert_ne!(
            gb.debug_read_banked(live, addr),
            val,
            "the live bank at {addr:04X} is untouched"
        );
    }
    // WRAMX has no page-0 window: bank 0 folds to page 1 on both read and write
    // (SVBK 0 → 1), so a bank-0 edit is visible as bank 1.
    gb.debug_write_banked(0, 0xD000, 0x7C);
    assert_eq!(
        gb.debug_read_banked(1, 0xD000),
        0x7C,
        "WRAMX bank 0 aliases bank 1"
    );
    assert_eq!(
        gb.debug_read_banked(0, 0xD000),
        gb.debug_read_banked(1, 0xD000)
    );
}

#[test]
fn region_bank_count_matches_the_chip_geometry() {
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x143] = 0x80; // CGB
    rom[0x147] = 0x1A; // MBC5+RAM
    rom[0x148] = 0x03; // 8 ROM banks
    rom[0x149] = 0x03; // 32 KiB RAM = 4 banks
    let gb = GameBoy::new(Model::Cgb, rom).unwrap();
    assert_eq!(gb.region_bank_count(0x4000), 8, "ROMX");
    assert_eq!(gb.region_bank_count(0x8000), 2, "CGB VRAM");
    assert_eq!(gb.region_bank_count(0xA000), 4, "SRAM");
    assert_eq!(gb.region_bank_count(0xD000), 8, "CGB WRAM");
    assert_eq!(gb.region_bank_count(0x0100), 1, "fixed ROM0");
    assert_eq!(gb.region_bank_count(0xFF80), 1, "HRAM unbanked");
    // DMG geometry: 1 VRAM bank, 2 WRAM pages, 0 SRAM banks (no chip).
    let dmg = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    assert_eq!(dmg.region_bank_count(0x8000), 1, "DMG VRAM");
    assert_eq!(dmg.region_bank_count(0xD000), 2, "DMG WRAM");
    assert_eq!(dmg.region_bank_count(0xA000), 0, "no RAM chip");
    // A present-but-sub-8KB RAM chip (MBC2's 512 B) still rounds up to 1 bank, so
    // the viewer names its SRAM instead of dropping the label as if absent.
    let mut mbc2 = vec![0u8; 4 * 0x4000];
    mbc2[0x147] = 0x06; // MBC2+BATTERY (built-in 512×4 RAM)
    let mbc2 = GameBoy::new(Model::Dmg, mbc2).unwrap();
    assert_eq!(mbc2.region_bank_count(0xA000), 1, "MBC2 512 B RAM → 1 bank");
}

#[test]
fn debug_read_banked_reads_explicit_rom_bank() {
    // Stamp the byte at each bank's 0x4000-window base with a bank-unique value.
    let mut rom = mbc1_4bank_rom();
    for bank in 0..4usize {
        rom[bank * 0x4000] = 0xB0 | bank as u8;
    }
    let gb = GameBoy::new(Model::Dmg, rom).unwrap();
    // Any bank is reachable regardless of the live mapping at 0x4000.
    for bank in 0..4u16 {
        assert_eq!(gb.debug_read_banked(bank, 0x4000), 0xB0 | bank as u8);
    }
    // A bank matching the live mapping is identical to debug_read.
    let cur = gb.rom_bank() as u16;
    assert_eq!(gb.debug_read_banked(cur, 0x4000), gb.debug_read(0x4000));
    // An out-of-range bank folds back in (no OOB panic): bank 4 wraps to bank 0.
    assert_eq!(
        gb.debug_read_banked(4, 0x4000),
        gb.debug_read_banked(0, 0x4000)
    );
}

#[test]
fn debug_read_banked_reads_explicit_vram_and_wram_banks() {
    let mut gb = GameBoy::new(Model::Cgb, rom_with_cgb_flag(0x80)).unwrap();
    // VRAM: distinct byte per bank via VBK.
    gb.debug_write(0xFF4F, 0);
    gb.debug_write(0x8000, 0xA0);
    gb.debug_write(0xFF4F, 1);
    gb.debug_write(0x8000, 0xA1);
    assert_eq!(gb.debug_read_banked(0, 0x8000), 0xA0);
    assert_eq!(gb.debug_read_banked(1, 0x8000), 0xA1);
    // WRAMX: distinct byte per SVBK bank at 0xD000.
    gb.debug_write(0xFF70, 1);
    gb.debug_write(0xD000, 0x11);
    gb.debug_write(0xFF70, 2);
    gb.debug_write(0xD000, 0x22);
    assert_eq!(gb.debug_read_banked(1, 0xD000), 0x11);
    assert_eq!(gb.debug_read_banked(2, 0xD000), 0x22);
    // An unbanked address ignores `bank` (== debug_read).
    assert_eq!(gb.debug_read_banked(7, 0xFF80), gb.debug_read(0xFF80));
}

#[test]
fn cdl_flag_banked_reads_explicit_banks() {
    let mut gb = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    fx[0x4000] = 4; // ROM bank 1 @ 0x4000 physical
    fx[3 * 0x4000] = 1; // ROM bank 3 @ 0x4000 physical
    assert!(gb.load_cdl(&fx));
    // Any bank is reachable regardless of the live BANK1.
    assert_eq!(gb.cdl_flag_banked(1, 0x4000), 4);
    assert_eq!(gb.cdl_flag_banked(3, 0x4000), 1);
    assert_eq!(gb.cdl_flag_banked(2, 0x4000), 0, "unmarked bank reads 0");
    // A bank matching the live mapping agrees with cdl_flag.
    let cur = gb.rom_bank() as u16;
    assert_eq!(gb.cdl_flag_banked(cur, 0x4000), gb.cdl_flag(0x4000));
    // Log off → 0.
    gb.set_cdl(false);
    assert_eq!(gb.cdl_flag_banked(1, 0x4000), 0);
}

#[test]
fn cdl_is_rom_bank_aware() {
    // The flat-64K store collapsed every ROM bank onto 0x4000-0x7FFF; the
    // physical store keys each bank to its own slot (mark and read share the
    // same translation, so cdl_flag reads back what a mark would set).
    let mut gb = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    fx[0x4000] = 4; // bank 1 @ 0x4000 physical offset (1 * 0x4000)
    fx[0x8000] = 1; // bank 2 @ 0x4000 physical offset (2 * 0x4000)
    assert!(gb.load_cdl(&fx));
    gb.debug_write(0x2000, 1); // BANK1 = 1
    assert_eq!(gb.cdl_flag(0x4000), 4, "0x4000 tint follows ROM bank 1");
    gb.debug_write(0x2000, 2); // BANK1 = 2
    assert_eq!(gb.cdl_flag(0x4000), 1, "same address, distinct bank-2 slot");
}

#[test]
fn cdl_is_wram_bank_aware_and_skips_absent_sram() {
    // CGB WRAM banks (SVBK) get distinct slots; a disabled/absent-SRAM access
    // maps to no physical byte (cdl_index None -> no phantom mark).
    let mut gb = GameBoy::new(Model::Cgb, rom_with_cgb_flag(0x80)).unwrap();
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    let wbase = 0x8000 + 0x4000; // rom_len + VRAM, SRAM len 0 on this ROM-only cart
    fx[wbase + 0x1000] = 2; // WRAM bank 1 @ 0xD000 (wram_index = 1 * 0x1000)
    fx[wbase + 0x2000] = 3; // WRAM bank 2 @ 0xD000
    assert!(gb.load_cdl(&fx));
    gb.debug_write(0xFF70, 1); // SVBK = 1
    assert_eq!(gb.cdl_flag(0xD000), 2);
    gb.debug_write(0xFF70, 2); // SVBK = 2
    assert_eq!(gb.cdl_flag(0xD000), 3, "0xD000 tint follows the WRAM bank");
    assert_eq!(gb.cdl_flag(0xA000), 0, "absent SRAM maps to no byte");
}

#[test]
fn cdl_records_read_write_execute_only_when_armed() {
    // write_c000_rom: 0100 `ld a,42` (X@0100, operand R@0101), 0102 `ld (C000),a`
    // (X@0102, W@C000), 0105 `jr -2`.
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    gb.set_cdl(true);
    for _ in 0..4 {
        gb.step();
    }
    assert!(gb.cdl_flag(0x0100) & 4 != 0, "X at the first opcode");
    assert!(gb.cdl_flag(0x0102) & 4 != 0, "X at the store opcode");
    assert!(
        gb.cdl_flag(0x0101) & 1 != 0,
        "R at the immediate operand byte"
    );
    assert!(gb.cdl_flag(0xC000) & 2 != 0, "W at the stored address");
    // Disarmed: a fresh run logs nothing.
    let mut off = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    for _ in 0..4 {
        off.step();
    }
    assert_eq!(off.cdl_flag(0x0100), 0, "no log when CDL is off");
    assert_eq!(off.cdl_flag(0xC000), 0);
}

#[test]
fn cdl_logging_does_not_perturb_emulation() {
    // The same ROM + steps with CDL off vs on must leave identical machine state
    // (recording is write-only from the machine's view — golden-safe).
    let run = |cdl: bool| {
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        gb.set_cdl(cdl);
        for _ in 0..200 {
            gb.step();
        }
        let r = gb.cpu_regs();
        (
            r.af(),
            r.bc(),
            r.pc,
            r.sp,
            gb.debug_read(0xC000),
            gb.cycles(),
        )
    };
    assert_eq!(
        run(false),
        run(true),
        "CDL recording must not change emulation"
    );
}

#[test]
fn cdl_logged_ranges_off_is_empty() {
    let gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    assert!(gb.cdl_logged_ranges().is_empty(), "log off → no ranges");
}

#[test]
fn cdl_logged_ranges_span_every_region_and_bank() {
    // mbc1_ram_rom: 4 ROM banks (rom=0x10000), 2 VRAM banks (0x4000), 4 RAM
    // banks (sram base 0x14000), then WRAM (2 banks, 0x2000) + tail (0x200).
    let mut gb = GameBoy::new(Model::Dmg, mbc1_ram_rom()).unwrap();
    gb.set_cdl(true);
    let mut fx = vec![0u8; gb.cdl_flags().unwrap().len()];
    let rom = 0x10000;
    let sram = rom + 0x4000;
    let wram = sram + 0x8000;
    let tail = wram + 0x2000;
    // ROM0 (bare): two disjoint runs — a `.` gap at 0x0104 splits them.
    fx[0x0100..=0x0103].fill(4);
    fx[0x0105..=0x0106].fill(1);
    // ROMX bank 1 @ 0x6000-0x6001, and a single byte in bank 2 @ 0x4001.
    fx[rom - 0x10000 + 0x4000 + 0x2000] = 1; // bank1, 0x6000
    fx[0x4000 + 0x2001] = 2; // bank1, 0x6001
    fx[2 * 0x4000 + 0x0001] = 4; // bank2, 0x4001 (single byte)
    // SRAM bank 2 @ 0xA000 (single byte).
    fx[sram + 2 * 0x2000] = 2;
    // WRAM0 (bare) 0xC000-0xC010, WRAMX bank 1 0xD000-0xD210.
    fx[wram..=wram + 0x10].fill(1);
    fx[wram + 0x1000..=wram + 0x1000 + 0x210].fill(4);
    // Tail (bare) single byte at 0xFF80.
    fx[tail + (0xFF80 - 0xFE00)] = 4;
    assert!(gb.load_cdl(&fx));

    let mk = |bank, start, end| CdlRange { bank, start, end };
    assert_eq!(
        gb.cdl_logged_ranges(),
        vec![
            mk(0, 0x0100, 0x0103),
            mk(0, 0x0105, 0x0106),
            mk(1, 0x6000, 0x6001),
            mk(2, 0x4001, 0x4001),
            mk(2, 0xA000, 0xA000),
            mk(0, 0xC000, 0xC010),
            mk(1, 0xD000, 0xD210),
            mk(0, 0xFF80, 0xFF80),
        ],
        "one range per continuous span, region/bank order"
    );
    // Each reported range's endpoints read back set through cdl_flag_banked.
    for r in gb.cdl_logged_ranges() {
        assert_ne!(gb.cdl_flag_banked(r.bank, r.start), 0);
        assert_ne!(gb.cdl_flag_banked(r.bank, r.end), 0);
    }
}

#[test]
fn cdl_defaults_off_toggles_and_survives_a_state_load() {
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    assert_eq!(gb.cdl_flag(0x0100), 0, "off: flag reads 0");
    assert!(gb.cdl_flags().is_none(), "off: no buffer");
    gb.set_cdl(true);
    assert!(
        gb.cdl_flags().is_some_and(|b| !b.is_empty()),
        "on: buffer allocated"
    );
    assert!(
        gb.cdl_flags().unwrap().iter().all(|&f| f == 0),
        "on: all clear"
    );
    // A save-state load leaves the CDL untouched — it is live UI state, not
    // serialized — so the buffer stays enabled across a load.
    let snap = gb.save_state();
    gb.load_state(&snap).unwrap();
    assert!(gb.cdl_flags().is_some(), "CDL survives a state load");
    gb.set_cdl(false);
    assert!(gb.cdl_flags().is_none(), "off drops the buffer");
    assert_eq!(gb.cdl_flag(0x0100), 0);
}
