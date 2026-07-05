//! Unit tests for the PPU core (STAT IRQ events, LYC, access blocking,
//! registers). Split out of `mod.rs` for file size; compiled as
//! `super::tests` via the `#[path]` attribute there.

use super::*;

fn dmg() -> Ppu {
    Ppu::new(Model::Dmg)
}

fn cgb() -> Ppu {
    Ppu::new(Model::Cgb)
}

/// Tick `n` dots, OR-ing the returned IF bits.
fn tick_n(p: &mut Ppu, n: u32) -> u8 {
    let mut ifs = 0;
    for _ in 0..n {
        ifs |= p.tick();
    }
    ifs
}

/// Tick until the PPU sits at (line, dot); returns OR of IF bits seen.
fn run_to(p: &mut Ppu, line: u8, dot: u16) -> u8 {
    let mut ifs = 0;
    let mut guard = 0u32;
    while !(p.line == line && p.dot == dot) {
        ifs |= p.tick();
        guard += 1;
        assert!(guard < 200_000, "run_to({line},{dot}) never reached");
    }
    ifs
}

const LCDON_CYCLES: [[u32; 8]; 3] = [
    [0, 17, 60, 110, 130, 174, 224, 244],
    [1, 18, 61, 111, 131, 175, 225, 245],
    [2, 19, 62, 112, 132, 176, 226, 246],
];

fn lcdon_case(lyc: u8, pass: usize, col: usize) -> Ppu {
    let mut p = dmg();
    p.write(0xFF45, lyc);
    p.write(0xFF40, 0x81);
    tick_n(&mut p, 4 * (LCDON_CYCLES[pass][col] + 2));
    p
}

fn check_lcdon_table(lyc: u8, addr: u16, expect: &[[u8; 8]; 3]) {
    for pass in 0..3 {
        for col in 0..8 {
            let p = lcdon_case(lyc, pass, col);
            assert_eq!(
                p.read(addr),
                expect[pass][col],
                "pass {pass} col {col} (cycle {})",
                LCDON_CYCLES[pass][col]
            );
        }
    }
}

const WRITE_NOPS: [u32; 19] = [
    0, 17, 18, 60, 61, 110, 111, 112, 130, 131, 132, 174, 175, 224, 225, 226, 244, 245, 246,
];

/// PPU on a steady visible line with every OAM byte distinct, so any
/// corruption pattern is observable and attributable.
fn oam_bug_ppu(line: u8, dot: u16) -> Ppu {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, line, dot);
    for (i, byte) in p.oam.iter_mut().enumerate() {
        *byte = (i as u8) ^ 0xA5;
    }
    p
}

#[path = "mod_tests/cgb.rs"]
mod cgb;

#[path = "mod_tests/oam_bug.rs"]
mod oam_bug;

#[path = "mod_tests/stat.rs"]
mod stat;

#[path = "mod_tests/stat_lyc.rs"]
mod stat_lyc;

#[path = "mod_tests/stat_oam.rs"]
mod stat_oam;

#[path = "mod_tests/stat_engine.rs"]
mod stat_engine;
