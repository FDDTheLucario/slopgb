//! `interconnect_tests` — speed tests (split for file size).

use super::*;

#[test]
fn key1_speed_switch_via_stop() {
    // Register semantics only: `interrupt_pending = true` takes the
    // instantaneous-switch path (SameBoy gates the pause and the
    // skipped-byte read on !interrupt_pending), keeping the pause
    // machinery out of this test (covered separately below).
    let mut b = ic_cgb_mode();
    assert_eq!(b.read(0xFF4D), 0x7E);
    assert!(!b.stop(0x0000, true), "not armed: deep stop");
    b.write(0xFF4D, 0xFF);
    assert_eq!(b.read(0xFF4D), 0x7F);
    ticks(&mut b, 100);
    assert!(b.stop(0x0000, true), "armed: switch performed");
    assert_eq!(b.read(0xFF4D), 0xFE, "double speed, no longer armed");
    assert_eq!(b.read(0xFF04), 0x00, "STOP reset DIV");
    // Switch back.
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, true));
    assert_eq!(b.read(0xFF4D), 0x7E);
}

/// With IE & IF pending an armed switch is instantaneous — no
/// skipped-byte read, no pause (SameBoy sm83_cpu.c stop() gates both
/// on !interrupt_pending; age caution/spsw-interrupts).
#[test]
fn speed_switch_with_pending_interrupt_takes_no_time() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, true));
    assert_eq!(b.cycles() - c0, 0);
    assert_eq!(b.read(0xFF4D), 0xFE);
}

#[test]
fn stop_resets_div_on_dmg() {
    let mut b = ic(Model::Dmg);
    ticks(&mut b, 100);
    assert_ne!(b.read(0xFF04), 0);
    assert!(!b.stop(0x0000, true));
    assert_eq!(b.read(0xFF04), 0);
}

/// STOP's skipped byte costs one real read M-cycle when no interrupt
/// is pending (SameBoy sm83_cpu.c stop(): `cycle_read(gb, gb->pc++)`),
/// and none when one is (1-byte-opcode path).
#[test]
fn stop_skipped_byte_costs_one_read_cycle() {
    let mut b = ic(Model::Dmg);
    let c0 = b.cycles();
    assert!(!b.stop(0x0000, false));
    assert_eq!(b.cycles() - c0, 4, "one read M-cycle");
    let c0 = b.cycles();
    assert!(!b.stop(0x0000, true));
    assert_eq!(b.cycles() - c0, 0, "pending interrupt: no read");
}

/// The STOP-triggered switch pauses the CPU while the rest of the
/// machine runs: ~0x8000 M-cycles measured on the *new* clock
/// (gambatte memory.cpp Memory::stop:
/// `intreq_.setEventTime<intevent_unhalt>(cc + 0x20000 + 4)` with cc
/// counting 4 per M-cycle at either speed — so the dot cost doubles
/// when leaving double speed; the gambatte speedchange LY families
/// pin that asymmetry against SameBoy's flat 0x20008 8-MHz countdown).
#[test]
fn speed_switch_pause_advances_machine_on_the_new_clock() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, false));
    // Read + internal cycle at the old pace (4 dots each, gambatte
    // re-paces the LCD at cc + 8 when entering), pause at the new.
    assert_eq!(b.cycles() - c0, 2 * 4 + 0x7FFF * 2);
    // Switching back re-paces from the read cycle on (cc + 0). The eager
    // clock's post-switch CPU↔PPU realignment advances k half-dots on the
    // leave (here k=6, dsa7=4), adding k/2 = 3 machine cycles.
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, false));
    assert_eq!(b.cycles() - c0, 0x8001 * 4 + 3);
}

/// DIV restarts from the STOP reset and TIMA keeps counting M-cycles
/// through the pause: TAC=$04 (4096 Hz, +1 per 256 M-cycles) over
/// 0x8001 M-cycles yields exactly 0x80 (gambatte speedchange_tima00_1a
/// expects $80).
#[test]
fn speed_switch_pause_ticks_tima_from_div_reset() {
    let mut b = ic_cgb_mode();
    b.write(0xFF07, 0x04);
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, false));
    assert_eq!(b.read_no_tick(0xFF05), 0x80);
}

/// The PPU keeps running through the pause: entering double speed
/// costs 65542 dots = 143 lines + 334 dots (speedchange_ly44_m3_ly:
/// LY 0x44 reads 0x39 = 0x44 + 143 mod 154 after the switch).
#[test]
fn speed_switch_pause_runs_the_ppu() {
    let mut b = ic_cgb_mode();
    b.write(0xFF40, 0x91);
    ticks(&mut b, 113); // glitched enable line is 452 dots: line 1 dot 0
    assert_eq!(b.read_no_tick(0xFF44), 1);
    b.write(0xFF4D, 0x01); // +4 dots (line 1 dot 4)
    assert!(b.stop(0x0000, false));
    // 65542 more dots: 143 full lines + 338 dots into line 144.
    assert_eq!(b.read_no_tick(0xFF44), 144);
}

/// IE & IF != 0 ends the pause early, exactly like halt mode
/// (gambatte's pause is a halt: the halted intevent_interrupts path
/// unhalts it).
#[test]
fn speed_switch_pause_cut_short_by_interrupt() {
    let mut b = ic_cgb_mode();
    b.write(0xFFFF, 0x04);
    b.write(0xFF07, 0x05); // 262144 Hz: +1 per 4 M-cycles
    b.write(0xFF05, 0xF0);
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, false));
    let elapsed_m = (b.cycles() - c0 - 8) / 2; // pause M-cycles
    assert!(elapsed_m < 0x100, "TIMA IRQ after ~64 M, got {elapsed_m}");
    assert_ne!(b.pending(), 0);
}

#[test]
fn double_speed_halves_dots_per_m_cycle() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    b.stop(0x0000, true);
    let c0 = b.cycles();
    b.tick();
    assert_eq!(b.cycles() - c0, 2, "2 dots per M-cycle in double speed");
    // LY advances half as fast: a 456-dot line takes 228 M-cycles.
    b.write(0xFF40, 0x91);
    ticks(&mut b, 226); // glitched enable line is 452 dots
    assert_eq!(b.read(0xFF44), 1);
}
