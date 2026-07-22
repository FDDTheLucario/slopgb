//! Panic-freedom fuzz smoke for the untrusted parsers a real user feeds: the ROM
//! image, the `.sav` battery file, the savestate blob, and the CDL file. All are
//! attacker/accident-controlled bytes; none may ever panic, hang, or over-allocate
//! — they must return `Err`/`false` or a valid machine. A panic here fails the
//! test. Dep-free (core is zero-dep; this test keeps the discipline): a seeded
//! xorshift PRNG stands in for `rand`, so runs are deterministic + reproducible.

use slopgb_core::{GameBoy, Model};

/// xorshift64* — deterministic, dep-free. Seeded per target so a failure is
/// reproducible from the seed in the panic message.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 33) as u8
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.byte()).collect()
    }
}

const MODELS: [Model; 7] = [
    Model::Dmg0,
    Model::Dmg,
    Model::Mgb,
    Model::Sgb,
    Model::Sgb2,
    Model::Cgb,
    Model::Agb,
];

/// A minimal valid 32 KiB ROM-only cart (header type $00) — the seed for building
/// a live machine to fuzz the post-construction parsers against.
fn valid_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x00; // ROM ONLY
    rom[0x149] = 0x00; // no RAM
    rom
}

fn live_gb() -> GameBoy {
    GameBoy::new(Model::Cgb, valid_rom()).expect("valid seed cart builds")
}

#[test]
fn new_never_panics_on_arbitrary_rom_bytes() {
    // The ROM image is the first thing a user hands the emulator (drag-drop, CLI).
    // Every size class + every model, arbitrary bytes: only Ok or CartridgeError.
    let mut rng = Rng::new(0xC0FF_EE12);
    for _ in 0..60_000 {
        let len = match rng.below(4) {
            0 => rng.below(0x160),            // tiny / header-boundary sizes
            1 => 0x8000,                      // exactly one bank
            2 => 0x8000 * (1 + rng.below(8)), // whole-bank multiples
            _ => rng.below(0x40001),          // arbitrary, up to 256 KiB
        };
        let rom = rng.bytes(len);
        let model = MODELS[rng.below(MODELS.len())];
        let _ = GameBoy::new(model, rom); // must not panic
    }
}

#[test]
fn new_never_panics_on_valid_header_random_body() {
    // Header-shaped noise reaches deeper into mapper setup than pure garbage
    // (which trips the header check early): plant a plausible header, randomize
    // the type/size bytes + body, across all models.
    let mut rng = Rng::new(0x0BAD_C0DE);
    for _ in 0..60_000 {
        let banks = 1 + rng.below(16);
        let mut rom = rng.bytes(0x4000 * banks);
        rom[0x147] = rng.byte(); // cartridge type (mapper) — any value
        rom[0x148] = rng.byte(); // ROM size code
        rom[0x149] = rng.byte(); // RAM size code
        rom[0x143] = rng.byte(); // CGB flag
        let model = MODELS[rng.below(MODELS.len())];
        let _ = GameBoy::new(model, rom);
    }
}

#[test]
fn load_save_data_never_panics_on_arbitrary_bytes() {
    // The `.sav` battery file — its size need not match the cart. Any bytes in,
    // only true/false out (a wrong-size/no-battery save is rejected, never a panic).
    let mut gb = live_gb();
    let mut rng = Rng::new(0x5A0EDA7A);
    for _ in 0..40_000 {
        let n = rng.below(0x9000);
        let data = rng.bytes(n);
        let _ = gb.load_save_data(&data);
    }
}

#[test]
fn load_cdl_never_panics_on_arbitrary_bytes() {
    let mut gb = live_gb();
    let mut rng = Rng::new(0xCD10_FADE);
    for _ in 0..40_000 {
        let n = rng.below(0x30000);
        let flags = rng.bytes(n);
        let _ = gb.load_cdl(&flags);
    }
}

#[test]
fn load_state_never_panics_on_arbitrary_or_mutated_bytes() {
    // Savestate: the deepest parser (rebuilds whole-machine state). Two modes —
    // pure random (usually rejected at the magic/length gate) and single-byte
    // mutations of a *valid* blob (which slip past the gate and exercise the
    // field decoders). Neither may panic; a bad blob leaves the machine intact.
    let mut gb = live_gb();
    for _ in 0..200 {
        gb.run_frame();
    }
    let good = gb.save_state();

    let mut rng = Rng::new(0x57A7_E5F0);
    // Pure-random blobs.
    for _ in 0..20_000 {
        let n = rng.below(good.len() + 64);
        let bytes = rng.bytes(n);
        let _ = gb.load_state(&bytes);
    }
    // Mutated-valid blobs: flip 1..=8 random bytes of a real state.
    for _ in 0..20_000 {
        let mut m = good.clone();
        for _ in 0..1 + rng.below(8) {
            let i = rng.below(m.len());
            m[i] ^= rng.byte();
        }
        let _ = gb.load_state(&m);
    }
}

#[test]
fn savestate_round_trips_exactly() {
    // The happy path the fuzz targets bracket: a real save/load is lossless.
    let mut a = live_gb();
    for _ in 0..300 {
        a.run_frame();
    }
    let blob = a.save_state();
    let regs = a.cpu_regs();
    let cyc = a.cycles();

    let mut b = live_gb();
    b.load_state(&blob).expect("a freshly-produced state loads");
    assert_eq!(b.cpu_regs().pc, regs.pc, "PC survives the round trip");
    assert_eq!(b.cycles(), cyc, "cycle counter survives the round trip");
    assert_eq!(
        b.save_state(),
        blob,
        "re-serialized state is byte-identical"
    );
}
