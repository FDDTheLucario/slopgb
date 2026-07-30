use super::*;

/// A minimal but realistic APU RAM image: one directory entry, one
/// instrument entry, and one looping BRR sample (the coprocessor's known
/// square block — see `brr_tests.rs`).
fn synthetic_apu_ram() -> [u8; 0x1_0000] {
    let mut ram = [0u8; 0x1_0000];
    let brr_addr: usize = 0x2000;
    let square = [0x93u8, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88];
    ram[brr_addr..brr_addr + 9].copy_from_slice(&square);

    let dir_entry = brr_addr; // dir[0]: start = brr_addr, loop = brr_addr (loops on itself)
    ram[DIR_DEST as usize] = (dir_entry & 0xFF) as u8;
    ram[DIR_DEST as usize + 1] = (dir_entry >> 8) as u8;
    ram[DIR_DEST as usize + 2] = (dir_entry & 0xFF) as u8;
    ram[DIR_DEST as usize + 3] = (dir_entry >> 8) as u8;

    let e = INSTR_DEST as usize;
    ram[e] = 0; // SRCN 0
    ram[e + 1] = 0x9F; // ADSR1: enable | decay_rate=1 | attack_rate=15
    ram[e + 2] = (3 << 5) | 10; // ADSR2: sustain_level=3 | sustain_rate=10
    ram[e + 3] = 0x7F; // GAIN: direct, near-full
    ram[e + 4] = 0x10; // base16 hi (big-endian): $1000 = unity
    ram[e + 5] = 0x00;
    ram
}

#[test]
fn export_then_import_round_trips_srcn_and_tuning() {
    let ram = synthetic_apu_ram();
    let sf2_bytes = export_sf2(&ram, DIR_DEST, INSTR_DEST, 64, 1).expect("export must succeed");

    // The SF2 itself must be parseable by our own reader.
    let parsed = reader::parse(&sf2_bytes).expect("exported SF2 must parse");
    assert_eq!(parsed.samples.len(), 1);
    assert_eq!(parsed.instruments.len(), 1);
    assert!(parsed.instruments[0].loops);

    let regions = import_sf2(&sf2_bytes).expect("import must succeed");
    assert_eq!(regions.dir.len(), 4);
    assert_eq!(regions.instr.len(), 6);
    assert!(!regions.brr.is_empty());

    assert_eq!(regions.instr[0], 0, "SRCN 0 must round-trip");
    assert_eq!(
        regions.instr[1], 0x9F,
        "ADSR1 must round-trip exactly for this rate pair"
    );
    assert_eq!(
        regions.instr[2],
        (3 << 5) | 10,
        "ADSR2 must round-trip exactly for this rate pair"
    );
    let base16 = (u16::from(regions.instr[4]) << 8) | u16::from(regions.instr[5]);
    assert_eq!(base16, 0x1000, "unity base16 must round-trip exactly");

    // The BRR decodes to the same square wave the synthetic ROM used.
    let decoded = crate::brr::decode(&regions.brr, 0).expect("re-encoded BRR must decode");
    let mut expected = [3584i16; 16];
    expected[8..].fill(-4096);
    assert_eq!(decoded.pcm.as_slice(), &expected[..]);
    assert!(decoded.loops);
}

#[test]
fn export_rejects_srcn_outside_directory() {
    let mut ram = synthetic_apu_ram();
    ram[INSTR_DEST as usize] = 5; // SRCN 5, but n_dir will be 1
    assert!(export_sf2(&ram, DIR_DEST, INSTR_DEST, 1, 1).is_err());
}

#[test]
fn export_rejects_zero_instruments() {
    let ram = synthetic_apu_ram();
    assert!(export_sf2(&ram, DIR_DEST, INSTR_DEST, 64, 0).is_err());
}
