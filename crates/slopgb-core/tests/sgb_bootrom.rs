//! End-to-end oracle for the clean-room SGB boot ROM (`boot/slopgb_sgb_boot.bin`,
//! built from `boot/slopgb_sgb_boot.asm`). Unlike the copyrighted `dmg_boot.bin`
//! oracle in `bootrom.rs`, this asset is committed (original MIT work), so the
//! test always runs.
//!
//! Running the real 256-byte boot ROM from power-on must:
//!   1. hand off (write FF50) leaving `PC = 0x0100`,
//!   2. converge to the documented SGB post-boot CPU register state
//!      (mooneye `boot_regs-sgb`, == the direct-init `GameBoy::new(Sgb)`), and
//!   3. leave the documented SGB post-boot IO state we deterministically write
//!      (LCDC on, BGP, and P1 = $30 — the last write of the SGB header
//!      handshake, so a $30 select proves the handshake actually ran).

use slopgb_core::{GameBoy, Model};

/// The 48-byte Nintendo logo every real cart carries at `0x0104-0x0133`.
const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// A minimal 32 KiB ROM-only cart that unlocks SGB functions: the Nintendo logo
/// (so the boot ROM's logo unpack has real data), a valid header checksum, and
/// the SGB flag `0x146 = 0x03` + old-licensee `0x14B = 0x33` (Pan Docs "SGB
/// flag" — the SGB joypad packet port is only wired for an SGB-flagged cart).
fn valid_sgb_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0104..0x0134].copy_from_slice(&NINTENDO_LOGO);
    rom[0x0146] = 0x03; // SGB flag
    rom[0x014B] = 0x33; // old licensee code (SGB requires it)
    // Header checksum over 0x0134-0x014C (Pan Docs "Header Checksum").
    let mut x = 0u8;
    for &b in &rom[0x0134..=0x014C] {
        x = x.wrapping_sub(b).wrapping_sub(1);
    }
    rom[0x014D] = x;
    rom
}

fn sgb_boot_rom() -> Vec<u8> {
    let path: std::path::PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "boot",
        "slopgb_sgb_boot.bin",
    ]
    .iter()
    .collect();
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn sgb_boot_rom_converges_to_post_boot() {
    let boot = sgb_boot_rom();
    assert_eq!(boot.len(), 0x100, "SGB boot ROM is 256 bytes (DMG-class)");
    let rom = valid_sgb_rom();

    // The direct-init post-boot machine is the oracle for the register state.
    let direct = GameBoy::new(Model::Sgb, rom.clone()).unwrap();

    // Run the real boot ROM from power-on until it writes FF50 to hand off.
    let mut booted = GameBoy::new_with_boot(Model::Sgb, rom, boot).unwrap();
    assert!(booted.boot_active(), "boot ROM mapped at power-on");
    let mut steps = 0u32;
    while booted.boot_active() && steps < 8_000_000 {
        booted.step();
        steps += 1;
    }
    assert!(
        !booted.boot_active(),
        "boot ROM handed off (FF50) within {steps} instructions",
    );

    let r = booted.cpu_regs();
    let d = direct.cpu_regs();
    assert_eq!(r.pc, 0x0100, "hands off at the cart entry point");
    assert_eq!(
        (r.af(), r.bc(), r.de(), r.hl(), r.sp, r.pc),
        (d.af(), d.bc(), d.de(), d.hl(), d.sp, d.pc),
        "boot ROM converges to the documented SGB post-boot registers \
         (A=01 F=00 BC=0014 DE=0000 HL=C060 SP=FFFE)",
    );

    // IO state we deterministically install.
    assert_eq!(
        booted.debug_read(0xFF40),
        0x91,
        "LCDC: LCD+BG on, $8000 tiles"
    );
    assert_eq!(booted.debug_read(0xFF47), 0xFC, "BGP: the boot palette");
    assert_eq!(
        booted.debug_read(0xFF50),
        0xFF,
        "FF50 reads $FF once the boot ROM is unmapped",
    );

    // Only the SGB header handshake writes P1; a $30 select nibble (P1 read
    // $FF, both columns deselected) proves the handshake executed and matches
    // the documented SGB post-boot P1 (mooneye boot_hwio-S: P1 = $30).
    assert_eq!(
        booted.debug_read(0xFF00),
        0xFF,
        "P1 select = $30 after the SGB header handshake ran",
    );
}
