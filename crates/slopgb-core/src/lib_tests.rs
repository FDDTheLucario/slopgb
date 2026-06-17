use super::*;

fn rom_with_cgb_flag(flag: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = flag;
    rom
}

/// Pan Docs "CPU registers" (Power-Up Sequence): on CGB/AGB hardware
/// the boot ROM hands a CGB-flagged cart off with DE=$FF56 HL=$000D;
/// a DMG cart gets DE=$0008 HL=$007C (mooneye misc/boot_regs-cgb/-A —
/// every mooneye ROM is DMG-flagged). A/F/B/C are cart-independent:
/// AGB's extra `inc b` gives B=$01/F=$00 for both cart kinds.
#[test]
fn cgb_flagged_cart_boot_regs() {
    for (model, af, bc) in [(Model::Cgb, 0x1180, 0x0000), (Model::Agb, 0x1100, 0x0100)] {
        let gb = GameBoy::new(model, rom_with_cgb_flag(0x80)).unwrap();
        let r = gb.cpu_regs();
        assert_eq!(r.af(), af, "{model:?} CGB cart AF");
        assert_eq!(r.bc(), bc, "{model:?} CGB cart BC");
        assert_eq!(r.de(), 0xFF56, "{model:?} CGB cart DE");
        assert_eq!(r.hl(), 0x000D, "{model:?} CGB cart HL");

        let gb = GameBoy::new(model, rom_with_cgb_flag(0x00)).unwrap();
        let r = gb.cpu_regs();
        assert_eq!(r.af(), af, "{model:?} DMG cart AF");
        assert_eq!(r.bc(), bc, "{model:?} DMG cart BC");
        assert_eq!(r.de(), 0x0008, "{model:?} DMG cart DE");
        assert_eq!(r.hl(), 0x007C, "{model:?} DMG cart HL");
    }
}

/// A ROM that writes 0x42 to 0xC000 then self-loops:
/// `ld a,42 ; ld (C000),a ; jr -2`.
fn write_c000_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0107].copy_from_slice(&[0x3E, 0x42, 0xEA, 0x00, 0xC0, 0x18, 0xFE]);
    rom
}

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

/// A 32 KiB ROM with `bytes` placed at the entry point (0x0100).
fn exc_rom(bytes: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0100 + bytes.len()].copy_from_slice(bytes);
    rom
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

/// A ROM that turns the LCD fully on (BG+window+sprites), enables the
/// timer at its fastest rate, and triggers APU channel 1, then spins — so
/// running frames exercises the PPU sub-dot pipeline, the timer, OAM, and
/// the APU all at once (the save-state oracle needs the live machine busy).
fn savestate_oracle_rom() -> Vec<u8> {
    let prog: &[u8] = &[
        0x3E, 0xE3, 0xE0, 0x40, // ld a,E3 ; ldh (40),a   LCDC on (bg+win+obj)
        0x3E, 0x07, 0xE0, 0x07, // ld a,07 ; ldh (07),a   TAC enable, fastest
        0x3E, 0x81, 0xE0, 0x26, // ld a,81 ; ldh (26),a   NR52 APU on
        0x3E, 0x80, 0xE0, 0x11, // ld a,80 ; ldh (11),a   NR11 duty
        0x3E, 0xF3, 0xE0, 0x12, // ld a,F3 ; ldh (12),a   NR12 envelope
        0x3E, 0xFF, 0xE0, 0x13, // ld a,FF ; ldh (13),a   NR13 freq lo
        0x3E, 0x87, 0xE0, 0x14, // ld a,87 ; ldh (14),a   NR14 trigger ch1
        0x18, 0xFE, // jr -2  (spin)
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0100 + prog.len()].copy_from_slice(prog);
    // A non-trivial title + checksums so `rom_id` keying is exercised.
    rom[0x134..0x13B].copy_from_slice(b"SAVETST");
    rom[0x14D] = 0x42;
    rom[0x14E] = 0x12;
    rom[0x14F] = 0x34;
    rom
}

fn assert_machines_match(a: &GameBoy, b: &GameBoy, msg: &str) {
    assert_eq!(a.cycles(), b.cycles(), "cycles diverged: {msg}");
    let (ra, rb) = (a.cpu_regs(), b.cpu_regs());
    assert_eq!(ra.pc, rb.pc, "pc diverged: {msg}");
    assert_eq!(ra.sp, rb.sp, "sp diverged: {msg}");
    assert_eq!(
        (ra.af(), ra.bc(), ra.de(), ra.hl()),
        (rb.af(), rb.bc(), rb.de(), rb.hl()),
        "regs diverged: {msg}"
    );
    assert_eq!(a.frame(), b.frame(), "frame diverged: {msg}");
}

/// A busier oracle ROM exercising the fields the simple ch1-only DMG oracle
/// leaves at default: **MBC1 banking + cart RAM**, **all four audio channels +
/// wave RAM**, and (on a CGB machine) the **CGB IO** — SVBK / VBK / BG+OBJ
/// palette RAM. Run on both DMG and CGB so a serializer drift in any of those
/// shows up as a round-trip divergence (the simple oracle can't catch it — both
/// machines hold identical defaults there). Program lives at 0x0150 (it overruns
/// the header at 0x0134), reached by a `jp` from the entry point.
fn comprehensive_oracle_rom(cgb: bool) -> Vec<u8> {
    fn ldh(p: &mut Vec<u8>, val: u8, port: u8) {
        p.extend_from_slice(&[0x3E, val, 0xE0, port]); // ld a,val ; ldh (port),a
    }
    fn st(p: &mut Vec<u8>, val: u8, addr: u16) {
        p.extend_from_slice(&[0x3E, val, 0xEA, addr as u8, (addr >> 8) as u8]); // ld (addr),a
    }
    let mut p = Vec::new();
    ldh(&mut p, 0xE3, 0x40); // LCDC on
    ldh(&mut p, 0x07, 0x07); // TAC
    ldh(&mut p, 0x02, 0x70); // SVBK = 2 (CGB WRAM bank; DMG: inert)
    ldh(&mut p, 0x01, 0x4F); // VBK = 1  (CGB VRAM bank; DMG: inert)
    ldh(&mut p, 0x80, 0x68); // BCPS auto-inc
    ldh(&mut p, 0x1F, 0x69); // BGPD
    ldh(&mut p, 0x7C, 0x69);
    ldh(&mut p, 0x80, 0x6A); // OCPS auto-inc
    ldh(&mut p, 0x3E, 0x6B); // OGPD
    ldh(&mut p, 0x11, 0x6B);
    st(&mut p, 0x05, 0x2000); // MBC1 ROM bank1 = 5
    st(&mut p, 0x01, 0x4000); // MBC1 bank2 = 1
    st(&mut p, 0x0A, 0x0000); // MBC1 RAM enable
    st(&mut p, 0x01, 0x6000); // MBC1 mode 1
    st(&mut p, 0xAB, 0xA000); // cart RAM write
    ldh(&mut p, 0x81, 0x26); // NR52 APU on
    ldh(&mut p, 0x80, 0x11); // ch1 NR11-14
    ldh(&mut p, 0xF3, 0x12);
    ldh(&mut p, 0xFF, 0x13);
    ldh(&mut p, 0x87, 0x14);
    ldh(&mut p, 0x80, 0x16); // ch2 NR21-24
    ldh(&mut p, 0xF3, 0x17);
    ldh(&mut p, 0xFF, 0x18);
    ldh(&mut p, 0x87, 0x19);
    ldh(&mut p, 0x80, 0x1A); // ch3 NR30 DAC on
    ldh(&mut p, 0xFF, 0x1B); // NR31 length
    ldh(&mut p, 0x20, 0x1C); // NR32 volume
    ldh(&mut p, 0xA5, 0x30); // wave RAM [0]
    ldh(&mut p, 0x5A, 0x31); // wave RAM [1]
    ldh(&mut p, 0xFF, 0x1D); // NR33 freq lo
    ldh(&mut p, 0x87, 0x1E); // NR34 trigger
    ldh(&mut p, 0xFF, 0x20); // ch4 NR41 length
    ldh(&mut p, 0xF3, 0x21); // NR42 envelope
    ldh(&mut p, 0x55, 0x22); // NR43 clock (LFSR)
    ldh(&mut p, 0x87, 0x23); // NR44 trigger
    p.extend_from_slice(&[0x18, 0xFE]); // jr -2 (spin)

    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0103].copy_from_slice(&[0xC3, 0x50, 0x01]); // jp 0x0150
    rom[0x0150..0x0150 + p.len()].copy_from_slice(&p);
    rom[0x134..0x13B].copy_from_slice(b"COMPTST");
    if cgb {
        rom[0x143] = 0x80; // CGB-enhanced
    }
    rom[0x147] = 0x03; // MBC1 + RAM + BATTERY
    rom[0x148] = 0x00; // 32 KiB ROM
    rom[0x149] = 0x02; // 8 KiB RAM
    rom[0x14D] = 0x99;
    rom[0x14E] = 0xAB;
    rom[0x14F] = 0xCD;
    rom
}

/// Save `rom` on `model` at each `warmups` step count, restore into a fresh
/// (perturbed) same-ROM machine, and assert both run byte-identically forward
/// for `frames` (regs/frame each step, then full memory + audio). Any
/// write_state/read_state field that is missed/reordered/dropped diverges here.
fn assert_round_trips(model: Model, rom: &[u8], warmups: &[usize], frames: usize, label: &str) {
    for &warmup in warmups {
        let mut a = GameBoy::new(model, rom.to_vec()).unwrap();
        for _ in 0..warmup {
            a.step();
        }
        let bytes = a.save_state();

        let mut b = GameBoy::new(model, rom.to_vec()).unwrap();
        for _ in 0..5000 {
            b.step();
        }
        b.load_state(&bytes).unwrap();
        assert_machines_match(
            &a,
            &b,
            &format!("{label} right after load (warmup {warmup})"),
        );

        for i in 0..frames {
            a.run_frame();
            b.run_frame();
            assert_machines_match(&a, &b, &format!("{label} warmup {warmup} frame {i}"));
        }
        for addr in 0u32..=0xFFFF {
            let addr = addr as u16;
            assert_eq!(
                a.debug_read(addr),
                b.debug_read(addr),
                "{label} memory {addr:#06X} diverged (warmup {warmup})"
            );
        }
        let (mut sa, mut sb) = (Vec::new(), Vec::new());
        a.drain_audio_raw(&mut sa);
        b.drain_audio_raw(&mut sb);
        assert_eq!(sa, sb, "{label} audio diverged (warmup {warmup})");
    }
}

#[test]
fn save_state_round_trips_the_whole_machine() {
    // Several save points (mid-frame / many-frames-in) on the simple DMG/ch1
    // oracle.
    assert_round_trips(
        Model::Dmg,
        &savestate_oracle_rom(),
        &[777, 30_000, 70_111],
        300,
        "dmg",
    );
}

#[test]
fn save_state_round_trips_cgb_mbc_and_all_channels() {
    // The serializer fields the simple oracle leaves at default — MBC1 banking +
    // cart RAM, ch2/ch3(wave)/ch4(noise), and (CGB) SVBK/VBK/palette RAM — are
    // driven non-default here, so a drift in those write_state/read_state pairs
    // (which the DMG/ch1 oracle would round-trip-pass silently) diverges.
    let rom = comprehensive_oracle_rom(false);
    assert_round_trips(Model::Dmg, &rom, &[2000, 40_000], 150, "dmg-comprehensive");
    let cgb_rom = comprehensive_oracle_rom(true);
    assert_round_trips(
        Model::Cgb,
        &cgb_rom,
        &[2000, 40_000],
        150,
        "cgb-comprehensive",
    );
}

#[test]
fn load_state_rejects_corrupt_or_foreign_states() {
    let rom = savestate_oracle_rom();
    let mut gb = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    gb.run_frame();
    let good = gb.save_state();

    // Round-trips into the same machine.
    assert!(gb.load_state(&good).is_ok());

    // Bad magic / truncated / version.
    assert_eq!(gb.load_state(&[0; 2]), Err(StateError::Truncated));
    assert_eq!(
        gb.load_state(b"XXXX\x01\x00"),
        Err(StateError::BadMagic),
        "wrong magic"
    );
    let mut bad_ver = good.clone();
    bad_ver[4] = 0xFF; // bump the version u16
    assert_eq!(gb.load_state(&bad_ver), Err(StateError::BadVersion));

    // A state for a *different* ROM (different title) is rejected.
    let mut other_rom = rom.clone();
    other_rom[0x134..0x13B].copy_from_slice(b"OTHERXX");
    let other = GameBoy::new(Model::Dmg, other_rom).unwrap().save_state();
    assert_eq!(gb.load_state(&other), Err(StateError::RomMismatch));

    // The ROM fingerprint also pins the cartridge TYPE (0x147): a same-title ROM
    // with a different mapper is rejected, so a fingerprint collision can't
    // mis-deserialize the (variant-dispatched) mapper state.
    let mut diff_mapper = rom.clone();
    diff_mapper[0x147] = 0x03; // MBC1+RAM+BATTERY vs the original ROM-ONLY (0x00)
    let other_mapper = GameBoy::new(Model::Dmg, diff_mapper).unwrap().save_state();
    assert_eq!(gb.load_state(&other_mapper), Err(StateError::RomMismatch));

    // A failed load leaves the machine intact (atomic).
    let pc_before = gb.cpu_regs().pc;
    let _ = gb.load_state(b"XXXX");
    assert_eq!(gb.cpu_regs().pc, pc_before, "failed load is a no-op");
}

#[test]
fn clone_is_an_independent_machine_snapshot() {
    // The Quick Save/Load primitive (MN6): GameBoy: Clone must be a deep,
    // independent copy — advancing one must not touch the other.
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    gb.run_frame();
    let snap = gb.clone();
    let (pc0, cyc0) = (snap.cpu_regs().pc, snap.cycles());
    for _ in 0..10 {
        gb.run_frame();
    }
    assert_ne!(gb.cycles(), cyc0, "original advanced");
    assert_eq!(snap.cycles(), cyc0, "clone is frozen at the snapshot");
    assert_eq!(
        snap.cpu_regs().pc,
        pc0,
        "clone PC unchanged by the original"
    );
    // Restoring rewinds the machine exactly to the snapshot.
    let restored = snap.clone();
    assert_eq!(restored.cycles(), cyc0);
    assert_eq!(restored.cpu_regs().pc, pc0);
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
    assert_eq!(gb.peek(0xFF44), 0xFF, "peek must not read IO");
    assert!(
        gb.debug_read(0xFF44) <= 153,
        "debug_read should give live LY"
    );
    // Outside IO, debug_read is identical to peek (and to ROM contents).
    assert_eq!(gb.debug_read(0x0143), gb.peek(0x0143));
    assert_eq!(gb.debug_read(0x0143), 0x00); // the CGB flag we wrote
    for addr in [0x0000u16, 0x4000, 0xC000, 0xFF80, 0xFFFF] {
        assert_eq!(gb.debug_read(addr), gb.peek(addr), "non-IO {addr:#06x}");
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

/// A ROM whose entry (`0x100`) is `nop; jp 0x150` and `0x150..` is nops,
/// so PC walks 0x100 -> 0x101 -> 0x150 -> 0x151 -> 0x152 … deterministically.
fn linear_code_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100] = 0x00; // nop
    rom[0x101..0x104].copy_from_slice(&[0xC3, 0x50, 0x01]); // jp 0150
    // 0x150.. already 0x00 (nop) from the zero-fill.
    rom
}

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
    assert_eq!(gb.run_frame_until_breakpoint(&[0x151]), Some(0x151));
    assert_eq!(gb.cpu_regs().pc, 0x151);
    assert_eq!(
        gb.frame_count(),
        frames_before,
        "halted before the frame completed"
    );
}

#[test]
fn run_frame_until_breakpoint_with_no_hit_completes_a_frame_like_run_frame() {
    // No reachable breakpoint -> runs a whole frame and returns None,
    // leaving the machine exactly where a plain run_frame would.
    let mut a = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    let mut b = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
    assert_eq!(a.run_frame_until_breakpoint(&[0xBEEF]), None);
    b.run_frame();
    assert_eq!(a.frame_count(), b.frame_count());
    assert_eq!(a.cycles(), b.cycles());
    assert_eq!(a.cpu_regs().pc, b.cpu_regs().pc);
    // Empty breakpoint list is just a run_frame.
    assert_eq!(a.run_frame_until_breakpoint(&[]), None);
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

/// Link task 5: link state is transient — never serialized. A save taken with
/// a peer attached restores into a machine with no peer, and adds no bytes to
/// the state blob (the on-disk format is unchanged → golden-safe).
#[test]
fn link_state_is_not_serialized() {
    let a = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    let baseline = a.save_state();
    let mut a = a;
    a.link_connect(true);
    a.link_push_recv(0xA5);
    let with_link = a.save_state();
    assert_eq!(
        with_link.len(),
        baseline.len(),
        "link adds no bytes to the save state"
    );
    let mut b = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    b.load_state(&with_link).unwrap();
    assert!(
        !b.link_connected(),
        "link state is not restored from a save"
    );
}
