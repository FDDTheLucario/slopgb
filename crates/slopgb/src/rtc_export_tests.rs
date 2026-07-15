use super::*;

fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

#[test]
fn vba_footer_places_live_latched_and_timestamp_at_the_offsets_core_parses() {
    // Matches slopgb-core `load_save_data`: live regs at 4*i, latched at
    // 20 + 4*i, timestamp at 40 (8-byte LE).
    let live = [0x11, 0x22, 0x33, 0x44, 0xC1];
    let latched = [0x55, 0x66, 0x77, 0x88, 0x81];
    let f = vba_footer(live, latched, 0x0102_0304_0506_0708);
    for (i, &r) in live.iter().enumerate() {
        assert_eq!(le32(&f, 4 * i), u32::from(r), "live reg {i}");
    }
    for (i, &r) in latched.iter().enumerate() {
        assert_eq!(le32(&f, 20 + 4 * i), u32::from(r), "latched reg {i}");
    }
    assert_eq!(
        u64::from_le_bytes(f[40..48].try_into().unwrap()),
        0x0102_0304_0506_0708,
        "timestamp"
    );
}

#[test]
fn vba_footer_round_trips_through_the_core_parser() {
    // The real proof: a footer our writer emits, fed to the core's `.sav`
    // loader, restores the same register values (masked). Uses a minimal
    // MBC3+RTC+RAM+BATTERY cart (type 0x10).
    use slopgb_core::{GameBoy, Model};
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x10; // MBC3+TIMER+RAM+BATTERY
    rom[0x148] = 0x02; // 32 KiB ROM (already sized)
    rom[0x149] = 0x02; // 8 KiB RAM
    let mut gb = GameBoy::new(Model::Dmg, rom).expect("valid cart");
    let ram = gb.battery_sram().expect("battery cart");
    let live = [0x1E, 0x0A, 0x05, 0x64, 0x01]; // 30s 10m 5h day-low=100 DH carry|halt bits
    let latched = [0x00, 0x00, 0x00, 0x00, 0x00];
    let mut image = ram;
    image.extend_from_slice(&vba_footer(live, latched, 1_700_000_000));
    assert!(gb.load_save_data(&image), "core accepts the VBA image");
    // The core masks each register (RTC_MASKS); compare against those masks.
    let (glive, glatched) = gb.rtc_state().expect("rtc present");
    let masks = [0x3F, 0x3F, 0x1F, 0xFF, 0xC1];
    for i in 0..5 {
        assert_eq!(glive[i], live[i] & masks[i], "live reg {i} round-trips");
        assert_eq!(glatched[i], latched[i] & masks[i], "latched reg {i}");
    }
}
