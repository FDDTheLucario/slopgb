//! Snapshot must be a faithful owned copy of the machine's observable state.

use slopgb_core::{GameBoy, Model};
use slopgb_plugin_api::Reg;

use super::Snapshot;

fn blank() -> GameBoy {
    // Minimal valid cartridge: 32 KiB of zeroes.
    GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap()
}

#[test]
fn read_matches_debug_read_across_the_map() {
    let gb = blank();
    let snap = Snapshot::capture(&gb);
    for addr in [0x0000u16, 0x0100, 0x00FF, 0x8000, 0xC000, 0xFF44, 0xFFFF] {
        assert_eq!(snap.read(addr), gb.debug_read(addr), "addr {addr:#06X}");
    }
}

#[test]
fn registers_match_the_cpu_and_io() {
    let gb = blank();
    let snap = Snapshot::capture(&gb);
    let r = gb.cpu_regs();
    assert_eq!(snap.reg(Reg::Af), r.af());
    assert_eq!(snap.reg(Reg::Bc), r.bc());
    assert_eq!(snap.reg(Reg::De), r.de());
    assert_eq!(snap.reg(Reg::Hl), r.hl());
    assert_eq!(snap.reg(Reg::Sp), r.sp);
    assert_eq!(snap.reg(Reg::Pc), r.pc);
    assert_eq!(snap.reg(Reg::Lcdc), u16::from(gb.debug_read(0xFF40)));
    assert_eq!(snap.reg(Reg::Stat), u16::from(gb.debug_read(0xFF41)));
    assert_eq!(snap.reg(Reg::Ly), u16::from(gb.debug_read(0xFF44)));
}
