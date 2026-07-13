//! Unit tests for the interconnect (memory map, DMA engines, IO routing,
//! sub-dot access machinery, speed switch). Split out of `interconnect.rs`
//! for file size; compiled as `super::tests` via the `#[path]` attribute.

use super::*;
// ---- memory map -----------------------------------------------------

// ---- halt-exit IE & IF sampling (Bus::pending_halt_wake) ------------

// ---- dispatch-ack source sync-ahead (gambatte Memory::ackIrq) -------

// ---- tick-then-access -----------------------------------------------

// ---- OAM DMA ---------------------------------------------------------

// ---- OAM DMA bus-conflict writes and CGB quirks ----------------------
//
// Semantics mirrored from gambatte-core memory.cpp (nontrivial_read /
// nontrivial_write OAM-DMA conflict blocks) and calibrated against the
// hardware-recorded gambatte/oamdma expectation matrix; per-test
// citations name the pinning ROMs.

// ---- prohibited area ------------------------------------------------

// ---- CGB registers and modes ------------------------------------------

// ---- CGB VRAM DMA -----------------------------------------------------

// ---- OAM DMA x VRAM DMA bus composition -------------------------------

// ---- peek (side-effect-free harness view) -----------------------------

// ---- post-boot state ---------------------------------------------------

// ---- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ------

/// 32 KiB no-MBC cart. `0x1000..0x1100` carries a recognisable pattern
/// for DMA source tests.
fn test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    for i in 0..0x100usize {
        rom[0x1000 + i] = (i as u8) ^ 0x5A;
    }
    rom
}

// These interconnect unit tests build the production (eager-value) machine.
// The eager clock's correctness is pinned by the gbtr battery + mooneye, not
// these micro-timing units.
fn ic(model: Model) -> Interconnect {
    Interconnect::new(model, Cartridge::from_bytes(test_rom()).unwrap())
}

fn ic_cgb_mode() -> Interconnect {
    let mut rom = test_rom();
    rom[0x143] = 0x80;
    Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap())
}

fn ticks(b: &mut Interconnect, n: u32) {
    for _ in 0..n {
        b.tick();
    }
}

/// Arm the timer so that the reload + IF commit lands on the last
/// T-substep of M-cycle 5 (div starts at 0, TAC bit 3 = 16 T period:
/// falling edge at div 16 on the last substep of cycle 4, reload one
/// cycle later on the same substep).
fn arm_late_timer_irq(b: &mut Interconnect) {
    b.ie = 0x04;
    b.timer.write(0xFF07, 0x05);
    b.timer.write(0xFF05, 0xFF);
}

/// Fill WRAM 0xC000.. with `base+i` through untimed writes.
fn fill_wram(b: &mut Interconnect, addr: u16, base: u8, len: u16) {
    for i in 0..len {
        b.write_no_tick(addr + i, base.wrapping_add(i as u8));
    }
}

fn setup_gdma_regs(b: &mut Interconnect, src: u16, dst: u16) {
    b.write(0xFF51, (src >> 8) as u8);
    b.write(0xFF52, src as u8);
    b.write(0xFF53, (dst >> 8) as u8);
    b.write(0xFF54, dst as u8);
}

fn booted(model: Model) -> Interconnect {
    let mut b = ic(model);
    b.apply_post_boot_state();
    b
}

/// Interconnect with the LCD freshly enabled (`ic` powers on with the
/// LCD off; the enable glitch line passes before any scan window).
fn ic_lcd_on(model: Model) -> Interconnect {
    let mut b = ic(model);
    b.write(0xFF40, 0x91);
    b
}

/// Distinct OAM fill through the DMA-engine path (ignores blocking,
/// takes no machine time).
fn fill_oam_distinct(b: &mut Interconnect) {
    for i in 0..0xA0u8 {
        b.ppu_mut().oam_dma_write(i, i ^ 0xA5);
    }
}

fn oam_snapshot(b: &Interconnect) -> [u8; 0xA0] {
    let mut snap = [0u8; 0xA0];
    for (i, byte) in snap.iter_mut().enumerate() {
        *byte = b.peek_no_io(0xFE00 + i as u16);
    }
    snap
}

/// Tick until the *next* M-cycle's access lands on scan row `row`
/// (every access advances the machine one M-cycle first, so park one
/// row short).
fn park_before_oam_row(b: &mut Interconnect, row: u8) {
    assert!((0x10..=0x98).contains(&row) && row % 8 == 0);
    for _ in 0..200_000 {
        if b.ppu.oam_bug_row() == Some(row - 8) {
            return;
        }
        b.tick();
    }
    panic!("scan row {row:#04x} never reached");
}

#[path = "interconnect_tests/boot.rs"]
mod boot;

#[path = "interconnect_tests/hdma.rs"]
mod hdma;

#[path = "interconnect_tests/irq.rs"]
mod irq;

#[path = "interconnect_tests/memory.rs"]
mod memory;

#[path = "interconnect_tests/oam_bug.rs"]
mod oam_bug;

#[path = "interconnect_tests/oam_dma.rs"]
mod oam_dma;

#[path = "interconnect_tests/speed.rs"]
mod speed;

#[path = "interconnect_tests/subdot.rs"]
mod subdot;
