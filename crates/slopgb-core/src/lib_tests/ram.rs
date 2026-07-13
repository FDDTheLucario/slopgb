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
