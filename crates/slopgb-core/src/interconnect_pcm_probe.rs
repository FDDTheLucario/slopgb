//! CGB post-boot PCM-decay probe (was `mod pcm_decay_probe` in
//! `interconnect.rs`).

use super::*;
use crate::cartridge::Cartridge;
use crate::cpu::Bus;

#[test]
fn post_boot_beep_already_decayed_at_handoff() {
    // The CGB boot beep plays during the logo, ~0.7s before hand-off;
    // its NR12=$F3 envelope is at volume 0 by PC=0x100. NR52 keeps the
    // channel-1 status bit (enable != volume), but PCM12 reads $00
    // (oracle: misc/boot_hwio-C, misc/bits/unused_hwio-C).
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = 0x80;
    rom[0x147] = 0x00;
    let mut ic = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
    ic.apply_post_boot_state();
    assert_eq!(
        ic.read_no_tick(0xFF76),
        0,
        "beep already silent at hand-off"
    );
    assert_eq!(ic.read_no_tick(0xFF26) & 0x01, 0x01, "ch1 still enabled");
    for _ in 0..1_048_576 {
        ic.tick();
    }
    assert_eq!(ic.read_no_tick(0xFF76), 0, "stays silent");
}
