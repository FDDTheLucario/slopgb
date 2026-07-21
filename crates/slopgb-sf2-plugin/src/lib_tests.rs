//! Native tests for the SF2-to-`.smpl` converter wrapper. These drive
//! `convert` directly (target-independent); `run_until`/`read_file` cross the
//! wasm-only ABI and are not exercised natively.

use super::*;

/// A minimal synthetic APU RAM image (one dir entry, one instrument, one
/// looping BRR sample) — the same shape `slopgb-sf2`'s own `mapping_tests.rs`
/// uses — just enough for `export_sf2` to build a valid, tiny SF2 file.
fn synthetic_apu_ram() -> [u8; 0x1_0000] {
    let mut ram = [0u8; 0x1_0000];
    let brr_addr: usize = 0x2000;
    let square = [0x93u8, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88];
    ram[brr_addr..brr_addr + 9].copy_from_slice(&square);
    // dir[0]: start = loop = brr_addr (a self-looping one-block sample).
    ram[slopgb_sf2::DIR_DEST as usize] = (brr_addr & 0xFF) as u8;
    ram[slopgb_sf2::DIR_DEST as usize + 1] = (brr_addr >> 8) as u8;
    ram[slopgb_sf2::DIR_DEST as usize + 2] = (brr_addr & 0xFF) as u8;
    ram[slopgb_sf2::DIR_DEST as usize + 3] = (brr_addr >> 8) as u8;
    let e = slopgb_sf2::INSTR_DEST as usize;
    ram[e] = 0; // SRCN 0
    ram[e + 1] = 0x9F; // ADSR1
    ram[e + 2] = (3 << 5) | 10; // ADSR2
    ram[e + 3] = 0x7F; // GAIN
    ram[e + 4] = 0x10; // base16 hi: $1000 = unity
    ram[e + 5] = 0x00;
    ram
}

/// A converted `.smpl` payload deserializes back to the same regions
/// `import_sf2` produces directly from the SF2 bytes.
#[test]
fn convert_matches_native_import_sf2() {
    let ram = synthetic_apu_ram();
    let sf2_bytes = slopgb_sf2::export_sf2(&ram, slopgb_sf2::DIR_DEST, slopgb_sf2::INSTR_DEST, 64, 1)
        .expect("export must succeed");

    let payload = convert(&sf2_bytes).expect("a valid SF2 converts");
    let via_cache = slopgb_sf2::cache::deserialize(&payload).expect("valid .smpl payload");
    let direct = slopgb_sf2::import_sf2(&sf2_bytes).expect("direct import must succeed");

    assert_eq!(via_cache.dir, direct.dir, "dir region matches");
    assert_eq!(via_cache.instr, direct.instr, "instrument table matches");
    assert_eq!(via_cache.brr, direct.brr, "BRR data matches");
}

/// A bad/empty input yields `None`, not a panic.
#[test]
fn convert_rejects_bad_input() {
    assert!(convert(&[]).is_none(), "empty input has no samples");
    assert!(
        convert(b"not an sf2 file at all").is_none(),
        "garbage input fails to parse"
    );
}
