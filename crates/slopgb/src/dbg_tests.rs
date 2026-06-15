use super::*;
use slopgb_core::{GameBoy, Model};

/// A ROM whose reset vector runs a `call`, then `nop`s, with a subroutine that
/// returns. Laid out so PC=0x0100 is the call.
fn call_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    // 0x0100: call 0x0150 ; 0x0103: nop ; 0x0104: nop
    rom[0x0100] = 0xCD;
    rom[0x0101] = 0x50;
    rom[0x0102] = 0x01;
    rom[0x0103] = 0x00;
    // 0x0150: subroutine — nop; ret
    rom[0x0150] = 0x00;
    rom[0x0151] = 0xC9;
    rom
}

fn machine(rom: Vec<u8>) -> GameBoy {
    let gb = GameBoy::new(Model::Dmg, rom).expect("rom loads");
    // The core initialises to the post-boot DMG state, entering at 0x0100.
    assert_eq!(gb.cpu_regs().pc, 0x0100, "DMG enters at 0x0100");
    gb
}

#[test]
fn is_subroutine_call_matches_call_and_rst_only() {
    assert!(is_subroutine_call(0xCD), "call nn");
    assert!(is_subroutine_call(0xC4), "call nz");
    assert!(is_subroutine_call(0xDC), "call c");
    assert!(is_subroutine_call(0xC7), "rst 00");
    assert!(is_subroutine_call(0xFF), "rst 38");
    assert!(!is_subroutine_call(0x00), "nop");
    assert!(!is_subroutine_call(0xC3), "jp nn");
    assert!(!is_subroutine_call(0xC9), "ret");
}

#[test]
fn toggle_break_flips_and_reports() {
    let mut d = Debugger::default();
    assert!(!d.is_broken());
    assert!(d.toggle_break());
    assert!(d.is_broken());
    assert!(!d.toggle_break());
    assert!(!d.is_broken());
}

#[test]
fn step_advances_one_instruction() {
    let d = Debugger::default();
    let mut gb = machine(call_rom());
    let pc0 = gb.cpu_regs().pc;
    d.step(&mut gb);
    assert_ne!(gb.cpu_regs().pc, pc0, "PC moved off the start line");
}

#[test]
fn step_over_a_call_lands_after_the_call() {
    let d = Debugger::default();
    let mut gb = machine(call_rom());
    assert_eq!(gb.cpu_regs().pc, 0x0100);
    d.step_over(&mut gb);
    // The 3-byte call returns to 0x0103, where step-over stops.
    assert_eq!(
        gb.cpu_regs().pc,
        0x0103,
        "stopped after the call, not inside"
    );
}

#[test]
fn step_over_a_plain_instruction_is_one_step() {
    let d = Debugger::default();
    // ROM that starts with two nops.
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100] = 0x00;
    rom[0x0101] = 0x00;
    let mut gb = machine(rom);
    d.step_over(&mut gb);
    assert_eq!(gb.cpu_regs().pc, 0x0101, "single-stepped a nop");
}

#[test]
fn breakpoints_toggle_on_off_and_report() {
    let mut bp = Breakpoints::default();
    assert!(bp.is_empty());
    assert!(bp.toggle(0x0150), "first toggle sets it");
    assert!(bp.contains(0x0150));
    assert!(!bp.is_empty());
    assert_eq!(bp.pc_list(), vec![0x0150]);
    assert!(!bp.toggle(0x0150), "second toggle clears it");
    assert!(!bp.contains(0x0150));
    assert!(bp.is_empty());
}

#[test]
fn pc_list_is_sorted_and_deduped() {
    let mut bp = Breakpoints::default();
    bp.toggle(0x0200);
    bp.toggle(0x0100);
    bp.toggle(0x0200); // clears 0x0200
    bp.toggle(0x0150);
    assert_eq!(bp.pc_list(), vec![0x0100, 0x0150], "sorted, 0x0200 removed");
}

#[test]
fn apply_toggle_breakpoint_updates_the_set() {
    let mut d = Debugger::default();
    let mut gb = machine(call_rom());
    d.apply(&mut gb, DebugAction::ToggleBreakpoint(0x0150));
    assert!(d.breakpoints().contains(0x0150));
    d.apply(&mut gb, DebugAction::ToggleBreakpoint(0x0150));
    assert!(!d.breakpoints().contains(0x0150));
}

#[test]
fn apply_run_to_cursor_stops_at_the_address_and_breaks() {
    let mut d = Debugger::default();
    let mut gb = machine(call_rom());
    assert_eq!(gb.cpu_regs().pc, 0x0100);
    // 0x0100 call -> 0x0150 (subroutine). Run to 0x0150.
    d.apply(&mut gb, DebugAction::RunToCursor(0x0150));
    assert_eq!(gb.cpu_regs().pc, 0x0150, "ran to the cursor address");
    assert!(d.is_broken(), "run-to-cursor halts the debugger there");
}
