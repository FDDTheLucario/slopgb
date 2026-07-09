//! Debug hooks + accessors: watchpoints, profiler, exception break, register/memory pokes, channel mute.

use super::*;

#[test]
fn watchpoint_halts_the_free_run_on_a_matching_access() {
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    gb.set_watchpoints(&[Watchpoint {
        addr: 0xC000,
        read: false,
        write: true,
    }]);
    // The write to 0xC000 halts the frame at that address.
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0xC000));
}

#[test]
fn watchpoint_kind_and_emptiness_are_respected() {
    // A read-only watchpoint at 0xC000 does NOT fire on the write.
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    gb.set_watchpoints(&[Watchpoint {
        addr: 0xC000,
        read: true,
        write: false,
    }]);
    assert_eq!(
        gb.run_frame_until_breakpoint(&[]),
        None,
        "a read watchpoint ignores the write"
    );
    // Golden-safety: with no watchpoints set, the frame runs to completion.
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
}

#[test]
fn profiler_tallies_executed_instruction_addresses() {
    // The execution profiler (MB5): an opt-in per-PC instruction tally that
    // is inert (no map) until enabled, so it never perturbs a golden run.
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    assert!(!gb.profiling(), "off by default");
    assert_eq!(gb.profile_seen(), 0);
    assert_eq!(gb.profile_count(0x0100), 0);

    gb.set_profiling(true);
    assert!(gb.profiling());
    // ld a,42 @0100 ; ld (C000),a @0102 ; jr -2 @0105 (then self-loops).
    gb.step();
    gb.step();
    gb.step();
    assert_eq!(gb.profile_count(0x0100), 1, "ld a,42 executed once");
    assert_eq!(gb.profile_count(0x0102), 1, "ld (C000),a executed once");
    assert_eq!(gb.profile_count(0x0105), 1, "jr executed once");
    assert_eq!(gb.profile_seen(), 3, "three distinct addresses seen");
    gb.step(); // the jr self-loops back to 0x0105
    assert_eq!(gb.profile_count(0x0105), 2);
    assert_eq!(
        gb.profile_seen(),
        3,
        "seen counts distinct addresses, not hits"
    );

    // "clear buffer" keeps logging on but zeroes the counts.
    gb.clear_profile();
    assert!(gb.profiling());
    assert_eq!(gb.profile_seen(), 0);
    assert_eq!(gb.profile_count(0x0105), 0);

    // Disabling drops the tally; stepping no longer records anything.
    gb.set_profiling(false);
    assert!(!gb.profiling());
    gb.step();
    assert_eq!(gb.profile_seen(), 0, "no tally while profiling is off");
}

#[test]
fn profiler_break_mode_halts_on_first_execution() {
    // bgb's coverage break: the free run stops the first time each address
    // executes, then continues past it.
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    gb.set_profiling(true);
    gb.set_profile_break(true);
    assert!(gb.profile_break());
    // 0100 (ld a), 0102 (ld (C000),a), 0105 (jr) each halt once on first run.
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0100));
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0102));
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0105));
    // The jr self-loops over only already-seen addresses → no more halts.
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    // Disabling break mode keeps logging: no halts, but the tally still grows.
    let before = gb.profile_count(0x0105);
    gb.set_profile_break(false);
    assert!(!gb.profile_break());
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    assert!(gb.profile_count(0x0105) > before);
}

#[test]
fn exception_break_defaults_inert() {
    // Options → Exceptions: nothing armed by default ⇒ the free run never
    // halts on these conditions (golden-safe — the mask is 0 on every
    // golden/test path).
    let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0x40, 0x18, 0xFE])).unwrap();
    assert_eq!(gb.exceptions(), 0, "no exception armed by default");
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
}

#[test]
fn exception_break_on_ld_b_b() {
    // ld b,b (40h) ; jr -2 — halts at the ld b,b when armed.
    let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0x40, 0x18, 0xFE])).unwrap();
    gb.set_exceptions(EXC_LD_B_B);
    assert_eq!(gb.exceptions(), EXC_LD_B_B);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0100));
    // The invalid-opcode mask does NOT fire on a (legal) ld b,b.
    let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0x40, 0x18, 0xFE])).unwrap();
    gb.set_exceptions(EXC_INVALID_OPCODE);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
}

#[test]
fn exception_break_on_invalid_opcode() {
    // 0xDD is one of the 11 undefined SM83 opcodes (the CPU hard-locks).
    let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0xDD])).unwrap();
    gb.set_exceptions(EXC_INVALID_OPCODE);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0100));
    // The ld-b,b mask does NOT fire on an invalid opcode.
    let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0xDD])).unwrap();
    gb.set_exceptions(EXC_LD_B_B);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
}

#[test]
fn exception_break_on_echo_ram_access() {
    // ld a,(E000) ; jr -5 — a CPU read of echo RAM (E000-FDFF) halts.
    let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0xFA, 0x00, 0xE0, 0x18, 0xFB])).unwrap();
    gb.set_exceptions(EXC_ECHO_RAM);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0xE000));
    // A work-RAM (C000) access is NOT echo RAM → no halt.
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    gb.set_exceptions(EXC_ECHO_RAM);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
}

#[test]
fn exception_break_on_lcd_off_outside_vblank() {
    // 16 NOPs (the DMG boot hands off mid-vblank at LY 0; the PPU leaves
    // mode 1 a few M-cycles in) then: xor a ; ldh (40),a ; ldh (40),a ;
    // jr -2 — two writes of FF40←0 well outside vblank.
    let mut prog = vec![0x00u8; 16];
    prog.extend_from_slice(&[0xAF, 0xE0, 0x40, 0xE0, 0x40, 0x18, 0xFE]);
    let rom = exc_rom(&prog);
    // Armed from boot: the LCD is on (LCDC=0x91) and the first FF40←0 write
    // lands outside vblank, so it halts.
    let mut gb = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    gb.set_exceptions(EXC_LCD_OFF_VBLANK);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0xFF40));
    // Already off: step the NOPs + xor + first write disarmed (LCD now off),
    // then arm — the second FF40←0 write must NOT halt (LCD already off).
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    for _ in 0..18 {
        gb.step(); // 16 NOPs, xor a, first ldh (40),a -> LCD off
    }
    gb.set_exceptions(EXC_LCD_OFF_VBLANK);
    assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
}

#[test]
fn debug_set_reg_writes_each_register_pair() {
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    gb.debug_set_reg(DebugReg::Af, 0x12FF); // F low nibble must mask to 0
    gb.debug_set_reg(DebugReg::Bc, 0x1234);
    gb.debug_set_reg(DebugReg::De, 0x5678);
    gb.debug_set_reg(DebugReg::Hl, 0x9ABC);
    gb.debug_set_reg(DebugReg::Sp, 0xD000);
    gb.debug_set_reg(DebugReg::Pc, 0x0150);
    let r = gb.cpu_regs();
    assert_eq!(r.af(), 0x12F0, "AF written, F low nibble masked");
    assert_eq!(r.bc(), 0x1234);
    assert_eq!(r.de(), 0x5678);
    assert_eq!(r.hl(), 0x9ABC);
    assert_eq!(r.sp, 0xD000);
    assert_eq!(r.pc, 0x0150);
}

#[test]
fn debug_call_pushes_return_addr_and_jumps() {
    // bgb "Call cursor": push the current PC (little-endian) and set
    // PC=target, so a later RET returns to where execution was.
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    gb.debug_set_reg(DebugReg::Sp, 0xD000);
    gb.debug_set_reg(DebugReg::Pc, 0x1234);
    gb.debug_call(0x4000);
    let r = gb.cpu_regs();
    assert_eq!(r.sp, 0xCFFE, "SP descended by 2");
    assert_eq!(r.pc, 0x4000, "PC jumped to the target");
    assert_eq!(gb.debug_read(0xCFFE), 0x34, "return low byte");
    assert_eq!(gb.debug_read(0xCFFF), 0x12, "return high byte");
}

#[test]
fn debug_write_round_trips_through_debug_read() {
    // Live-debugger byte poke (memory-viewer edit / freeze): write a byte
    // time-free and read it back. WRAM lands unconditionally.
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    gb.debug_write(0xC000, 0x42);
    assert_eq!(gb.debug_read(0xC000), 0x42, "written byte reads back");
    gb.debug_write(0xC000, 0x00);
    assert_eq!(gb.debug_read(0xC000), 0x00, "overwrite reads back");
}

#[test]
fn channel_mute_round_trips_and_defaults_off() {
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    for ch in 1..=4 {
        assert!(!gb.channel_muted(ch), "ch{ch} audible at power-on");
    }
    gb.set_channel_mute(3, true);
    assert!(gb.channel_muted(3));
    assert!(!gb.channel_muted(2), "only ch3 muted");
    gb.set_channel_mute(3, false);
    assert!(!gb.channel_muted(3));
    // Out-of-range channels are ignored (no panic).
    gb.set_channel_mute(0, true);
    gb.set_channel_mute(9, true);
    assert!(!gb.channel_muted(0) && !gb.channel_muted(9));
}

#[test]
fn debug_read_resolves_io_but_peek_does_not() {
    let gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    // peek keeps IO out of band ($FF); debug_read returns the live value.
    // Post-boot LY is a valid scanline (0..=153), so it can't be the $FF
    // peek hands back — proving debug_read took the io_read path.
    assert_eq!(gb.peek_no_io(0xFF44), 0xFF, "peek must not read IO");
    assert!(
        gb.debug_read(0xFF44) <= 153,
        "debug_read should give live LY"
    );
    // Outside IO, debug_read is identical to peek (and to ROM contents).
    assert_eq!(gb.debug_read(0x0143), gb.peek_no_io(0x0143));
    assert_eq!(gb.debug_read(0x0143), 0x00); // the CGB flag we wrote
    for addr in [0x0000u16, 0x4000, 0xC000, 0xFF80, 0xFFFF] {
        assert_eq!(
            gb.debug_read(addr),
            gb.peek_no_io(addr),
            "non-IO {addr:#06x}"
        );
    }
}

#[test]
fn stack_descends_from_sp_little_endian() {
    let gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    let sp = gb.cpu_regs().sp;
    let s = gb.stack(3);
    assert_eq!(s.len(), 3);
    // Addresses descend by two from SP (bgb's stack pane order).
    assert_eq!(s[0].0, sp);
    assert_eq!(s[1].0, sp.wrapping_sub(2));
    assert_eq!(s[2].0, sp.wrapping_sub(4));
    // Each word is the little-endian pair at its address.
    for &(addr, word) in &s {
        let want =
            u16::from(gb.debug_read(addr)) | (u16::from(gb.debug_read(addr.wrapping_add(1))) << 8);
        assert_eq!(word, want, "word @ {addr:#06x}");
    }
}

#[test]
fn ime_accessors_track_the_ei_delay() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100] = 0xFB; // ei; 0x101.. stay nop
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    assert!(!gb.ime() && !gb.ime_pending(), "post-boot: interrupts off");
    assert!(!gb.double_speed(), "DMG is never double-speed");
    gb.step(); // ei: arms the pending enable, IME still off
    assert!(!gb.ime(), "IME stays off the instruction after EI");
    assert!(gb.ime_pending(), "EI arms the pending enable");
    gb.step(); // the following instruction commits IME
    assert!(gb.ime(), "IME enabled one instruction after EI");
    assert!(!gb.ime_pending(), "pending cleared once applied");
}
