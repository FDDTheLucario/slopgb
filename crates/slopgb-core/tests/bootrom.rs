//! Boot-ROM convergence oracle (task 7): running the **real** DMG boot ROM
//! from power-on must hand off (FF50) to the exact same register state the
//! direct post-boot install produces — PC=0x0100 + the documented post-boot
//! registers. Skipped when `bootroms/dmg_boot.bin` is absent (like the test-rom
//! harness; the boot ROMs are gitignored, not vendored into the repo).

use std::path::PathBuf;

use slopgb_core::{GameBoy, Model};

/// The 48-byte Nintendo logo the DMG boot ROM verifies at `0x0104-0x0133`; a
/// mismatch makes the boot ROM lock up instead of handing off.
const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// A minimal 32 KiB ROM-only cart the DMG boot ROM accepts: the correct
/// Nintendo logo + a valid header checksum (the boot ROM also checks `0x014D`
/// and hangs on a mismatch).
fn valid_dmg_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0104..0x0134].copy_from_slice(&NINTENDO_LOGO);
    // Header checksum over 0x0134-0x014C (Pan Docs "Header Checksum").
    let mut x = 0u8;
    for &b in &rom[0x0134..=0x014C] {
        x = x.wrapping_sub(b).wrapping_sub(1);
    }
    rom[0x014D] = x;
    rom
}

fn dmg_boot_rom() -> Option<Vec<u8>> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "bootroms",
        "dmg_boot.bin",
    ]
    .iter()
    .collect();
    std::fs::read(path).ok()
}

#[test]
fn dmg_boot_rom_converges_to_post_boot() {
    let Some(boot) = dmg_boot_rom() else {
        eprintln!("skip: bootroms/dmg_boot.bin not present");
        return;
    };
    assert_eq!(boot.len(), 0x100, "DMG boot ROM is 256 bytes");
    let rom = valid_dmg_rom();

    // The direct-init post-boot machine is the oracle.
    let direct = GameBoy::new(Model::Dmg, rom.clone()).unwrap();

    // Run the real boot ROM from power-on until it writes FF50 to hand off.
    // Step (not run_frame) so we stop the instant boot_active flips — the FF50
    // write leaves PC at 0x0100, before any cart instruction executes.
    let mut booted = GameBoy::new_with_boot(Model::Dmg, rom, boot).unwrap();
    assert!(booted.boot_active(), "boot ROM mapped at power-on");
    let mut steps = 0u32;
    while booted.boot_active() && steps < 4_000_000 {
        booted.step();
        steps += 1;
    }
    assert!(
        !booted.boot_active(),
        "boot ROM handed off (FF50) within {steps} instructions"
    );

    let r = booted.cpu_regs();
    let d = direct.cpu_regs();
    assert_eq!(r.pc, 0x0100, "hands off at the cart entry point");
    assert_eq!(
        (r.af(), r.bc(), r.de(), r.hl(), r.sp, r.pc),
        (d.af(), d.bc(), d.de(), d.hl(), d.sp, d.pc),
        "the real boot ROM converges to the direct-init post-boot register state",
    );
}
