//! Power-on RAM initialisation ([`GameBoy::init_ram`] / [`RamInit`]).

use super::*;

/// A 32 KiB MBC1+RAM+BATTERY cart with 8 KiB of external RAM, so `save_data`
/// exposes the cartridge SRAM directly.
fn ram_cart() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x03; // MBC1 + RAM + BATTERY
    rom[0x149] = 0x02; // 8 KiB RAM
    rom
}

/// The default machine (what the golden/test path uses) leaves cart SRAM at
/// 0xFF — never call `init_ram` there, so golden stays byte-identical.
#[test]
fn default_cart_sram_is_ff() {
    let gb = GameBoy::new(Model::Dmg, ram_cart()).unwrap();
    let sram = gb.save_data().expect("battery cart has save data");
    assert!(
        sram[..0x2000].iter().all(|&b| b == 0xFF),
        "the default power-on cart SRAM is 0xFF"
    );
}

/// `RamInit::Fill(b)` overwrites cart SRAM with the constant byte.
#[test]
fn fill_sets_cart_sram_to_the_byte() {
    let mut gb = GameBoy::new(Model::Dmg, ram_cart()).unwrap();
    gb.init_ram(RamInit::Fill(0x42));
    let sram = gb.save_data().unwrap();
    assert!(sram[..0x2000].iter().all(|&b| b == 0x42));
}

/// `RamInit::Random(seed)` is deterministic per seed, varies across seeds, and
/// produces real garbage (not a single repeated byte).
#[test]
fn random_is_deterministic_per_seed_and_varied() {
    let sram = |seed| {
        let mut gb = GameBoy::new(Model::Dmg, ram_cart()).unwrap();
        gb.init_ram(RamInit::Random(seed));
        gb.save_data().unwrap()[..0x2000].to_vec()
    };
    let a1 = sram(0xDEAD_BEEF);
    let a2 = sram(0xDEAD_BEEF);
    let b = sram(0x0BAD_F00D);
    assert_eq!(a1, a2, "same seed -> identical SRAM");
    assert_ne!(a1, b, "different seed -> different SRAM");
    assert!(
        a1.windows(2).any(|w| w[0] != w[1]),
        "seeded fill is varied garbage, not a constant"
    );
}

/// `RamInit::Random` also fills work RAM (not just cart SRAM) — deterministically
/// per seed. Probes WRAM at 0xC000 via the side-effect-free debug read.
#[test]
fn random_fills_work_ram_deterministically() {
    let wram0 = |seed| {
        let mut gb = GameBoy::new(Model::Cgb, ram_cart()).unwrap();
        gb.init_ram(RamInit::Random(seed));
        (0xC000u16..0xC010)
            .map(|a| gb.debug_read(a))
            .collect::<Vec<_>>()
    };
    // Default WRAM is zeroed; a seeded fill is (with overwhelming probability)
    // not all-zero, and is reproducible.
    let a = wram0(0x1234_5678);
    assert_eq!(a, wram0(0x1234_5678), "same seed -> identical WRAM");
    assert!(a.iter().any(|&b| b != 0), "seeded fill populated work RAM");
}

/// Pin the exact xorshift64 output stream through the public `init_ram` seam:
/// cart SRAM is filled first, so `save_data()[..N]` is the first N `next_u8()`
/// draws. This defeats the "swap the PRNG, every test still passes" hole — the
/// documented contract (cli.rs `DEFAULT_RAM_SEED`) is cross-run reproducibility,
/// which a different algorithm silently breaks. Values computed from the
/// xorshift64 definition (seed | 1; ^= <<13; ^= >>7; ^= <<17; byte = x >> 24).
#[test]
fn random_pins_the_exact_xorshift_stream() {
    let mut gb = GameBoy::new(Model::Dmg, ram_cart()).unwrap();
    gb.init_ram(RamInit::Random(0xDEAD_BEEF));
    let sram = gb.save_data().unwrap();
    assert_eq!(
        &sram[..16],
        &[
            0xBF, 0x29, 0x9E, 0x99, 0xC2, 0x0D, 0xC9, 0xEF, 0x6D, 0xB1, 0xD8, 0x25, 0x32, 0xBB,
            0x1C, 0xBD,
        ],
        "seed 0xDEADBEEF -> exact xorshift64 stream (a PRNG swap changes these)"
    );
}

/// `RamInit::Random` fills BOTH video RAM banks (deleting the `fill_video_ram`
/// call would pass every other test). Probes via the bank-explicit debug read,
/// which bypasses PPU mode blocking.
#[test]
fn random_fills_both_video_ram_banks() {
    let vram = |seed, bank| {
        let mut gb = GameBoy::new(Model::Cgb, ram_cart()).unwrap();
        gb.init_ram(RamInit::Random(seed));
        (0x8000u16..0x8010)
            .map(|a| gb.debug_read_banked(bank, a))
            .collect::<Vec<_>>()
    };
    let b0 = vram(0x1234_5678, 0);
    let b1 = vram(0x1234_5678, 1);
    assert_eq!(
        b0,
        vram(0x1234_5678, 0),
        "same seed -> identical VRAM bank 0"
    );
    assert!(
        b0.iter().any(|&b| b != 0),
        "seeded fill populated VRAM bank 0"
    );
    assert!(
        b1.iter().any(|&b| b != 0),
        "seeded fill populated VRAM bank 1"
    );
}

/// `RamInit::Random` fills the banked CGB work RAM (D000-DFFF, banks 1-7), not
/// just bank 0 at C000. Probes several banks via the bank-explicit debug read.
#[test]
fn random_fills_banked_cgb_work_ram() {
    let mut gb = GameBoy::new(Model::Cgb, ram_cart()).unwrap();
    gb.init_ram(RamInit::Random(0x1234_5678));
    for bank in [2u16, 5, 7] {
        let banked: Vec<u8> = (0xD000u16..0xD010)
            .map(|a| gb.debug_read_banked(bank, a))
            .collect();
        assert!(
            banked.iter().any(|&b| b != 0),
            "seeded fill populated WRAM bank {bank}"
        );
    }
}

/// `RamInit::Fill` touches ONLY cart SRAM — work RAM and video RAM stay at the
/// zeroed `new` default. Pins the deliberate Fill-vs-Random scope asymmetry
/// (Random fills all three; Fill fills only the battery-backed cart RAM).
#[test]
fn fill_touches_only_cart_sram() {
    let mut gb = GameBoy::new(Model::Cgb, ram_cart()).unwrap();
    gb.init_ram(RamInit::Fill(0x42));
    assert!(
        gb.save_data().unwrap()[..0x2000].iter().all(|&b| b == 0x42),
        "Fill sets cart SRAM"
    );
    assert!(
        (0xC000u16..0xC010).all(|a| gb.debug_read(a) == 0),
        "Fill leaves work RAM at the zeroed default"
    );
    assert!(
        (0x8000u16..0x8010).all(|a| gb.debug_read_banked(0, a) == 0),
        "Fill leaves video RAM at the zeroed default"
    );
}
