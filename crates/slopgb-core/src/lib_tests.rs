use super::*;

fn rom_with_cgb_flag(flag: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = flag;
    rom
}

/// Post-C3-flip default guard (#11cu): every production `GameBoy::new` must
/// construct on the coherent EAGER-value clock — `leading_edge_reads` ON,
/// `tier2_reclock` (the disproven read-deferred variant) still OFF. The
/// eager-value flip is the shipped default; the deferred `tier2_reclock`
/// must never be the default. `new_with_reclock` (the deferred path) stays
/// test/probe-only. Needs no ROM bundle, always runs.
#[test]
fn production_new_is_c3_eager_default() {
    for model in [Model::Dmg, Model::Cgb, Model::Agb] {
        let gb = GameBoy::new(model, rom_with_cgb_flag(0x00)).unwrap();
        assert_eq!(
            gb.reclock_flags(),
            (true, false),
            "{model:?}: production GameBoy::new must be C3 eager (leading_edge ON, tier2 OFF)"
        );
    }
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
        // Pending APU output is not part of the savestate contract (v3 stopped
        // serializing the `raw_samples`/`samples` queues) — drain a's backlog
        // so it starts the post-save run with an empty queue, matching b after
        // load. Draining only empties the output vec; emulation state is intact.
        a.drain_audio_raw(&mut Vec::new());
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
fn cdl_load_restores_a_flag_buffer() {
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    // The buffer is sized to the machine's physical layout, not a flat 64 KiB.
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    gb.set_cdl(false);
    let mut fixture = vec![0u8; n];
    fixture[0x0150] = 4; // ROM offset 0x150 (bank 0 low area) -> X
    assert!(gb.load_cdl(&fixture), "matching-size buffer loads");
    assert_eq!(gb.cdl_flag(0x0150), 4);
    assert_eq!(gb.cdl_flags().unwrap(), &fixture[..], "buffer restored verbatim");
    assert!(!gb.load_cdl(&[0u8; 8]), "wrong-size buffer is rejected");
}

/// A 4-bank MBC1 ROM (no RAM), for exercising bank-aware address translation.
fn mbc1_4bank_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 4 * 0x4000];
    rom[0x147] = 0x01; // MBC1
    rom[0x148] = 0x01; // 4 banks
    rom
}

/// A 4-bank MBC1 ROM with 32 KiB (4-bank) cart RAM, for the banked-SRAM debug
/// read + CDL. Header 0x149=0x03 = 32 KiB RAM; mapper 0x03 = MBC1+RAM+BATTERY.
fn mbc1_ram_rom() -> Vec<u8> {
    let mut rom = mbc1_4bank_rom();
    rom[0x147] = 0x03; // MBC1+RAM+BATTERY
    rom[0x149] = 0x03; // 32 KiB RAM (4 banks)
    rom
}

#[test]
fn debug_read_and_cdl_reach_explicit_sram_banks() {
    let mut gb = GameBoy::new(Model::Dmg, mbc1_ram_rom()).unwrap();
    // RAMG on + MBC1 mode 1 so BANK2 selects the RAM bank (gbctr).
    gb.debug_write(0x0000, 0x0A);
    gb.debug_write(0x6000, 0x01);
    // Stamp a bank-unique byte into each of the 4 RAM banks at 0xA000.
    for bank in 0..4u8 {
        gb.debug_write(0x4000, bank);
        gb.debug_write(0xA000, 0xD0 | bank);
    }
    // Any bank is reachable regardless of the live BANK2.
    for bank in 0..4u16 {
        assert_eq!(gb.debug_read_banked(bank, 0xA000), 0xD0 | bank as u8);
    }
    // Out-of-range bank folds within the chip (bank 4 wraps to bank 0), no OOB.
    assert_eq!(gb.debug_read_banked(4, 0xA000), gb.debug_read_banked(0, 0xA000));

    // CDL follows the same bank map. Craft a fixture flagging SRAM bank 2 only
    // (debug_read is side-effect-free, so it can't record a flag): the physical
    // SRAM region sits after ROM (4*0x4000) + VRAM (0x4000); bank 2 @ 0xA000 is
    // offset 2*0x2000 within it.
    gb.set_cdl(true);
    let mut fx = vec![0u8; gb.cdl_flags().unwrap().len()];
    fx[0x10000 + 0x4000 + 2 * 0x2000] = 1;
    assert!(gb.load_cdl(&fx));
    assert_eq!(gb.cdl_flag_banked(2, 0xA000), 1);
    assert_eq!(gb.cdl_flag_banked(0, 0xA000), 0, "other banks unmarked");
    // The live bank (2) agrees with the plain cdl_flag.
    gb.debug_write(0x4000, 2);
    assert_eq!(gb.cdl_flag_banked(2, 0xA000), gb.cdl_flag(0xA000));
}

#[test]
fn banked_sram_on_a_cart_without_ram_reads_ff() {
    // No RAM chip → open-bus 0xFF for every bank, CDL always 0 (never OOB).
    let mut gb = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    assert_eq!(gb.debug_read_banked(3, 0xA000), 0xFF);
    gb.set_cdl(true);
    assert_eq!(gb.cdl_flag_banked(3, 0xA000), 0);
}

#[test]
fn debug_write_banked_edits_the_named_bank_of_each_region() {
    // CGB MBC5 + 32 KiB RAM: VRAM (2 banks), SRAM (4 banks), WRAM (8 banks).
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x143] = 0x80; // CGB
    rom[0x147] = 0x1A; // MBC5+RAM
    rom[0x148] = 0x03; // 8 ROM banks
    rom[0x149] = 0x03; // 32 KiB RAM (4 banks)
    let mut gb = GameBoy::new(Model::Cgb, rom).unwrap();
    // Poke a distinct byte into a non-live bank of each region, read it back
    // banked, and confirm the *live* bank was untouched.
    for (addr, bank, val, live) in
        [(0x8000u16, 1u16, 0xE1u8, 0u16), (0xA000, 3, 0xE3, 0), (0xD000, 5, 0xE5, 1)]
    {
        gb.debug_write_banked(bank, addr, val);
        assert_eq!(gb.debug_read_banked(bank, addr), val, "edit lands in {bank}");
        assert_ne!(
            gb.debug_read_banked(live, addr),
            val,
            "the live bank at {addr:04X} is untouched"
        );
    }
    // WRAMX has no page-0 window: bank 0 folds to page 1 on both read and write
    // (SVBK 0 → 1), so a bank-0 edit is visible as bank 1.
    gb.debug_write_banked(0, 0xD000, 0x7C);
    assert_eq!(gb.debug_read_banked(1, 0xD000), 0x7C, "WRAMX bank 0 aliases bank 1");
    assert_eq!(gb.debug_read_banked(0, 0xD000), gb.debug_read_banked(1, 0xD000));
}

#[test]
fn region_bank_count_matches_the_chip_geometry() {
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x143] = 0x80; // CGB
    rom[0x147] = 0x1A; // MBC5+RAM
    rom[0x148] = 0x03; // 8 ROM banks
    rom[0x149] = 0x03; // 32 KiB RAM = 4 banks
    let gb = GameBoy::new(Model::Cgb, rom).unwrap();
    assert_eq!(gb.region_bank_count(0x4000), 8, "ROMX");
    assert_eq!(gb.region_bank_count(0x8000), 2, "CGB VRAM");
    assert_eq!(gb.region_bank_count(0xA000), 4, "SRAM");
    assert_eq!(gb.region_bank_count(0xD000), 8, "CGB WRAM");
    assert_eq!(gb.region_bank_count(0x0100), 1, "fixed ROM0");
    assert_eq!(gb.region_bank_count(0xFF80), 1, "HRAM unbanked");
    // DMG geometry: 1 VRAM bank, 2 WRAM pages, 0 SRAM banks (no chip).
    let dmg = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    assert_eq!(dmg.region_bank_count(0x8000), 1, "DMG VRAM");
    assert_eq!(dmg.region_bank_count(0xD000), 2, "DMG WRAM");
    assert_eq!(dmg.region_bank_count(0xA000), 0, "no RAM chip");
    // A present-but-sub-8KB RAM chip (MBC2's 512 B) still rounds up to 1 bank, so
    // the viewer names its SRAM instead of dropping the label as if absent.
    let mut mbc2 = vec![0u8; 4 * 0x4000];
    mbc2[0x147] = 0x06; // MBC2+BATTERY (built-in 512×4 RAM)
    let mbc2 = GameBoy::new(Model::Dmg, mbc2).unwrap();
    assert_eq!(mbc2.region_bank_count(0xA000), 1, "MBC2 512 B RAM → 1 bank");
}

#[test]
fn debug_read_banked_reads_explicit_rom_bank() {
    // Stamp the byte at each bank's 0x4000-window base with a bank-unique value.
    let mut rom = mbc1_4bank_rom();
    for bank in 0..4usize {
        rom[bank * 0x4000] = 0xB0 | bank as u8;
    }
    let gb = GameBoy::new(Model::Dmg, rom).unwrap();
    // Any bank is reachable regardless of the live mapping at 0x4000.
    for bank in 0..4u16 {
        assert_eq!(gb.debug_read_banked(bank, 0x4000), 0xB0 | bank as u8);
    }
    // A bank matching the live mapping is identical to debug_read.
    let cur = gb.rom_bank() as u16;
    assert_eq!(gb.debug_read_banked(cur, 0x4000), gb.debug_read(0x4000));
    // An out-of-range bank folds back in (no OOB panic): bank 4 wraps to bank 0.
    assert_eq!(gb.debug_read_banked(4, 0x4000), gb.debug_read_banked(0, 0x4000));
}

#[test]
fn debug_read_banked_reads_explicit_vram_and_wram_banks() {
    let mut gb = GameBoy::new(Model::Cgb, rom_with_cgb_flag(0x80)).unwrap();
    // VRAM: distinct byte per bank via VBK.
    gb.debug_write(0xFF4F, 0);
    gb.debug_write(0x8000, 0xA0);
    gb.debug_write(0xFF4F, 1);
    gb.debug_write(0x8000, 0xA1);
    assert_eq!(gb.debug_read_banked(0, 0x8000), 0xA0);
    assert_eq!(gb.debug_read_banked(1, 0x8000), 0xA1);
    // WRAMX: distinct byte per SVBK bank at 0xD000.
    gb.debug_write(0xFF70, 1);
    gb.debug_write(0xD000, 0x11);
    gb.debug_write(0xFF70, 2);
    gb.debug_write(0xD000, 0x22);
    assert_eq!(gb.debug_read_banked(1, 0xD000), 0x11);
    assert_eq!(gb.debug_read_banked(2, 0xD000), 0x22);
    // An unbanked address ignores `bank` (== debug_read).
    assert_eq!(gb.debug_read_banked(7, 0xFF80), gb.debug_read(0xFF80));
}

#[test]
fn cdl_flag_banked_reads_explicit_banks() {
    let mut gb = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    fx[0x4000] = 4; // ROM bank 1 @ 0x4000 physical
    fx[3 * 0x4000] = 1; // ROM bank 3 @ 0x4000 physical
    assert!(gb.load_cdl(&fx));
    // Any bank is reachable regardless of the live BANK1.
    assert_eq!(gb.cdl_flag_banked(1, 0x4000), 4);
    assert_eq!(gb.cdl_flag_banked(3, 0x4000), 1);
    assert_eq!(gb.cdl_flag_banked(2, 0x4000), 0, "unmarked bank reads 0");
    // A bank matching the live mapping agrees with cdl_flag.
    let cur = gb.rom_bank() as u16;
    assert_eq!(gb.cdl_flag_banked(cur, 0x4000), gb.cdl_flag(0x4000));
    // Log off → 0.
    gb.set_cdl(false);
    assert_eq!(gb.cdl_flag_banked(1, 0x4000), 0);
}

#[test]
fn cdl_is_rom_bank_aware() {
    // The flat-64K store collapsed every ROM bank onto 0x4000-0x7FFF; the
    // physical store keys each bank to its own slot (mark and read share the
    // same translation, so cdl_flag reads back what a mark would set).
    let mut gb = GameBoy::new(Model::Dmg, mbc1_4bank_rom()).unwrap();
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    fx[0x4000] = 4; // bank 1 @ 0x4000 physical offset (1 * 0x4000)
    fx[0x8000] = 1; // bank 2 @ 0x4000 physical offset (2 * 0x4000)
    assert!(gb.load_cdl(&fx));
    gb.debug_write(0x2000, 1); // BANK1 = 1
    assert_eq!(gb.cdl_flag(0x4000), 4, "0x4000 tint follows ROM bank 1");
    gb.debug_write(0x2000, 2); // BANK1 = 2
    assert_eq!(gb.cdl_flag(0x4000), 1, "same address, distinct bank-2 slot");
}

#[test]
fn cdl_is_wram_bank_aware_and_skips_absent_sram() {
    // CGB WRAM banks (SVBK) get distinct slots; a disabled/absent-SRAM access
    // maps to no physical byte (cdl_index None -> no phantom mark).
    let mut gb = GameBoy::new(Model::Cgb, rom_with_cgb_flag(0x80)).unwrap();
    gb.set_cdl(true);
    let n = gb.cdl_flags().unwrap().len();
    let mut fx = vec![0u8; n];
    let wbase = 0x8000 + 0x4000; // rom_len + VRAM, SRAM len 0 on this ROM-only cart
    fx[wbase + 0x1000] = 2; // WRAM bank 1 @ 0xD000 (wram_index = 1 * 0x1000)
    fx[wbase + 0x2000] = 3; // WRAM bank 2 @ 0xD000
    assert!(gb.load_cdl(&fx));
    gb.debug_write(0xFF70, 1); // SVBK = 1
    assert_eq!(gb.cdl_flag(0xD000), 2);
    gb.debug_write(0xFF70, 2); // SVBK = 2
    assert_eq!(gb.cdl_flag(0xD000), 3, "0xD000 tint follows the WRAM bank");
    assert_eq!(gb.cdl_flag(0xA000), 0, "absent SRAM maps to no byte");
}

#[test]
fn cdl_records_read_write_execute_only_when_armed() {
    // write_c000_rom: 0100 `ld a,42` (X@0100, operand R@0101), 0102 `ld (C000),a`
    // (X@0102, W@C000), 0105 `jr -2`.
    let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    gb.set_cdl(true);
    for _ in 0..4 {
        gb.step();
    }
    assert!(gb.cdl_flag(0x0100) & 4 != 0, "X at the first opcode");
    assert!(gb.cdl_flag(0x0102) & 4 != 0, "X at the store opcode");
    assert!(gb.cdl_flag(0x0101) & 1 != 0, "R at the immediate operand byte");
    assert!(gb.cdl_flag(0xC000) & 2 != 0, "W at the stored address");
    // Disarmed: a fresh run logs nothing.
    let mut off = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    for _ in 0..4 {
        off.step();
    }
    assert_eq!(off.cdl_flag(0x0100), 0, "no log when CDL is off");
    assert_eq!(off.cdl_flag(0xC000), 0);
}

#[test]
fn cdl_logging_does_not_perturb_emulation() {
    // The same ROM + steps with CDL off vs on must leave identical machine state
    // (recording is write-only from the machine's view — golden-safe).
    let run = |cdl: bool| {
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        gb.set_cdl(cdl);
        for _ in 0..200 {
            gb.step();
        }
        let r = gb.cpu_regs();
        (r.af(), r.bc(), r.pc, r.sp, gb.debug_read(0xC000), gb.cycles())
    };
    assert_eq!(run(false), run(true), "CDL recording must not change emulation");
}

#[test]
fn cdl_defaults_off_toggles_and_survives_a_state_load() {
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    assert_eq!(gb.cdl_flag(0x0100), 0, "off: flag reads 0");
    assert!(gb.cdl_flags().is_none(), "off: no buffer");
    gb.set_cdl(true);
    assert!(gb.cdl_flags().is_some_and(|b| !b.is_empty()), "on: buffer allocated");
    assert!(gb.cdl_flags().unwrap().iter().all(|&f| f == 0), "on: all clear");
    // A save-state load leaves the CDL untouched — it is live UI state, not
    // serialized — so the buffer stays enabled across a load.
    let snap = gb.save_state();
    gb.load_state(&snap).unwrap();
    assert!(gb.cdl_flags().is_some(), "CDL survives a state load");
    gb.set_cdl(false);
    assert!(gb.cdl_flags().is_none(), "off drops the buffer");
    assert_eq!(gb.cdl_flag(0x0100), 0);
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
        assert_eq!(gb.debug_read(addr), gb.peek_no_io(addr), "non-IO {addr:#06x}");
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
    assert_eq!(gb2.frame_count(), f0 + 1, "disconnected frame runs to completion");
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
    assert_eq!(gb.debug_read(0xFF0F) & 0x08, 0, "no serial IF while stalled");
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

/// Boot-ROM task 5: `new_with_boot` runs from the boot ROM in power-on state.
#[test]
fn new_with_boot_starts_at_power_on() {
    let boot: Vec<u8> = (0..0x100u16).map(|i| (i as u8) ^ 0xC3).collect();
    let gb = GameBoy::new_with_boot(Model::Dmg, write_c000_rom(), boot.clone()).unwrap();
    assert_eq!(gb.cpu_regs().pc, 0x0000, "boots from the reset vector");
    assert_eq!(gb.cpu_regs().sp, 0, "power-on SP");
    assert!(gb.boot_active(), "boot ROM mapped");
    assert_eq!(
        gb.debug_read(0x0000),
        boot[0],
        "first instruction is from the boot ROM"
    );
    assert_eq!(
        gb.debug_read(0xFF40),
        0x00,
        "LCD off at power-on (the boot ROM turns it on)"
    );
}

/// A wrong-size boot ROM cannot be mapped: `new_with_boot` ignores it and falls
/// back to the post-boot install (a valid machine, `boot_active` false), rather
/// than running from a half-mapped, broken power-on state.
#[test]
fn new_with_boot_wrong_size_falls_back_to_post_boot() {
    let direct = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    for bad in [0usize, 0x80, 0x200, 0x900] {
        let gb = GameBoy::new_with_boot(Model::Dmg, write_c000_rom(), vec![0u8; bad]).unwrap();
        assert!(!gb.boot_active(), "wrong-size ({bad}) boot ROM not mapped");
        let (r, d) = (gb.cpu_regs(), direct.cpu_regs());
        assert_eq!(
            (r.af(), r.bc(), r.de(), r.hl(), r.sp, r.pc),
            (d.af(), d.bc(), d.de(), d.hl(), d.sp, d.pc),
            "falls back to the exact post-boot register state ({bad})"
        );
    }
    // CGB class wants 2304 B: a 256 B (DMG-size) image is wrong here too.
    let gb = GameBoy::new_with_boot(Model::Cgb, write_c000_rom(), vec![0u8; 0x100]).unwrap();
    assert!(!gb.boot_active(), "256 B boot ROM is wrong for a CGB model");
}

/// Boot-ROM task 6 (golden guard): `new` (no boot ROM) is unchanged — no boot
/// ROM mapped, post-boot entry + registers, exactly as before this feature.
#[test]
fn new_without_boot_is_unchanged() {
    let gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    assert!(!gb.boot_active(), "no boot ROM mapped on the default path");
    let r = gb.cpu_regs();
    let pb = Registers::post_boot(Model::Dmg);
    assert_eq!(r.pc, 0x0100, "starts post-boot at the cart entry");
    assert_eq!(
        (r.af(), r.bc(), r.de(), r.hl(), r.sp, r.pc),
        (pb.af(), pb.bc(), pb.de(), pb.hl(), pb.sp, pb.pc),
        "post-boot register state unchanged"
    );
}
