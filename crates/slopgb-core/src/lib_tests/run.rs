//! Run control (`run_until`/`run_frame`/`run_slice`) and serial-link behavior.

use super::*;

#[test]
fn run_until_breakpoint_stops_at_the_address() {
    let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    assert_eq!(gb.cpu_regs().pc, 0x100);
    // 0x100 nop -> 0x101 jp -> 0x150 nop -> 0x151. bp at 0x151.
    assert_eq!(gb.run_until_breakpoint(&[0x151], 100), Some(0x151));
    assert_eq!(gb.cpu_regs().pc, 0x151);
}

#[test]
fn run_until_breakpoint_respects_the_step_limit() {
    let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    // No reachable breakpoint -> runs the cap, returns None.
    assert_eq!(gb.run_until_breakpoint(&[0xBEEF], 5), None);
    assert_eq!(gb.run_until_breakpoint(&[], 3), None);
}

#[test]
fn run_until_breakpoint_advances_off_the_current_pc() {
    let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    // A breakpoint on the *current* PC must not stop instantly — one step
    // moves to 0x101, which isn't the (already-left) 0x100.
    assert_eq!(gb.run_until_breakpoint(&[0x100], 1), None);
    assert_eq!(gb.cpu_regs().pc, 0x101);
}

#[test]
fn run_frame_until_breakpoint_halts_at_a_breakpoint_mid_frame() {
    let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    assert_eq!(gb.cpu_regs().pc, 0x100);
    let frames_before = gb.frame_count();
    // 0x100 nop -> 0x101 jp -> 0x150 nop -> 0x151: stops within a handful of
    // cycles, far short of a full frame's worth of dots.
    assert_eq!(gb.run_frame_until_breakpoint(&[(0x151, None)]), Some(0x151));
    assert_eq!(gb.cpu_regs().pc, 0x151);
    assert_eq!(
        gb.frame_count(),
        frames_before,
        "halted before the frame completed"
    );
}

#[test]
fn run_frame_until_breakpoint_qualifies_a_breakpoint_by_rom_bank() {
    // Entry switches to ROM bank 2 (MBC1) then jumps into the switchable area at
    // 0x4000, so PC reaches 0x4000 while rom_bank() == 2.
    let mut rom = mbc1_4bank_rom();
    rom[0x100..0x108].copy_from_slice(&[
        0x3E, 0x02, // ld a,$02
        0xEA, 0x00, 0x20, // ld ($2000),a  -> MBC1 ROM bank = 2
        0xC3, 0x00, 0x40, // jp $4000
    ]);
    // Bank 2's 0x4000 (file offset 2*0x4000) stays nops from the zero-fill.

    // A breakpoint qualified to the *wrong* bank never fires: the frame runs out.
    let mut gb = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    assert_eq!(gb.run_frame_until_breakpoint(&[(0x4000, Some(1))]), None);

    // Qualified to the live bank (2), and the bank-agnostic form, both halt.
    let mut gb = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    assert_eq!(
        gb.run_frame_until_breakpoint(&[(0x4000, Some(2))]),
        Some(0x4000)
    );
    assert_eq!(gb.rom_bank(), 2);
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    assert_eq!(
        gb.run_frame_until_breakpoint(&[(0x4000, None)]),
        Some(0x4000)
    );
}

#[test]
fn run_frame_until_breakpoint_with_no_hit_completes_a_frame_like_run_frame() {
    // No reachable breakpoint -> runs a whole frame and returns None,
    // leaving the machine exactly where a plain run_frame would.
    let mut a = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    let mut b = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    assert_eq!(a.run_frame_until_breakpoint(&[(0xBEEF, None)]), None);
    b.run_frame();
    assert_eq!(a.frame_count(), b.frame_count());
    assert_eq!(a.cycles(), b.cycles());
    assert_eq!(a.cpu_regs().pc, b.cpu_regs().pc);
    // Empty breakpoint list is just a run_frame.
    assert_eq!(a.run_frame_until_breakpoint(&[]), None);
}

/// Link task 7: the GameBoy link API is inert when disconnected and toggles
/// the connection when used.
#[test]
fn gameboy_link_api_inert_when_disconnected() {
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    assert!(!gb.link_connected());
    assert_eq!(gb.link_take_send(), None);
    // No slave transfer armed: a delivered byte is a no-op raising no serial IF.
    assert_eq!(gb.link_slave_transfer(0x12), None);
    assert_eq!(gb.debug_read(0xFF0F) & 0x08, 0, "no spurious serial IF");
    gb.link_connect(true);
    assert!(gb.link_connected());
    gb.link_connect(false);
    assert!(!gb.link_connected());
}

/// Link task 3: `run_frame` yields when a connected master stalls (lockstep)
/// before the frame completes; a disconnected machine never stalls.
#[test]
fn run_frame_yields_on_link_stall() {
    // ld a,$42 ; ldh ($01),a ; ld a,$81 ; ldh ($02),a ; jr -2 (self-loop)
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x010A]
        .copy_from_slice(&[0x3E, 0x42, 0xE0, 0x01, 0x3E, 0x81, 0xE0, 0x02, 0x18, 0xFE]);
    let mut gb = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    gb.link_connect(true);
    let frame0 = gb.frame_count();
    gb.run_frame();
    assert!(gb.link_stalled(), "master stalled awaiting the peer byte");
    assert_eq!(
        gb.frame_count(),
        frame0,
        "run_frame yielded before finishing the frame"
    );
    // Disconnected: the same ROM never stalls; run_frame finishes a full frame.
    let mut gb2 = GameBoy::new(Model::Dmg, rom).unwrap();
    let f0 = gb2.frame_count();
    gb2.run_frame();
    assert!(!gb2.link_stalled());
    assert_eq!(
        gb2.frame_count(),
        f0 + 1,
        "disconnected frame runs to completion"
    );
}

/// Link task 4: disconnecting while a master is stalled folds the serial
/// interrupt into FF0F so the emulated CPU's serial wait can't hang.
#[test]
fn link_disconnect_while_stalled_raises_if() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x010A]
        .copy_from_slice(&[0x3E, 0x42, 0xE0, 0x01, 0x3E, 0x81, 0xE0, 0x02, 0x18, 0xFE]);
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    gb.link_connect(true);
    gb.run_frame(); // master stalls
    assert!(gb.link_stalled());
    assert_eq!(
        gb.debug_read(0xFF0F) & 0x08,
        0,
        "no serial IF while stalled"
    );
    gb.link_connect(false);
    assert!(!gb.link_stalled());
    assert_eq!(
        gb.debug_read(0xFF0F) & 0x08,
        0x08,
        "disconnect raises serial IF (CPU unblocks)"
    );
}

/// Speedup: `run_slice` runs a bounded number of cycles (the frontend's chunked
/// link pump), stopping at the cycle budget — and a disconnected machine never
/// stalls, so a slice is just a cycle-bounded run.
#[test]
fn run_slice_runs_bounded_cycles() {
    // A self-looping ROM (jr -2) so the slice is pure cycle accounting.
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0102].copy_from_slice(&[0x18, 0xFE]); // jr -2
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    let c0 = gb.cycles();
    gb.run_slice(4096);
    let elapsed = gb.cycles() - c0;
    assert!(
        (4096..4096 + 24).contains(&elapsed),
        "ran ~one slice of cycles, got {elapsed}"
    );
}
