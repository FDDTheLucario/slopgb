use super::*;

fn rom_with_cgb_flag(flag: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = flag;
    rom
}

/// A ROM that writes 0x42 to 0xC000 then self-loops:
/// `ld a,42 ; ld (C000),a ; jr -2`.
fn write_c000_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0107].copy_from_slice(&[0x3E, 0x42, 0xEA, 0x00, 0xC0, 0x18, 0xFE]);
    rom
}

/// A 32 KiB ROM with `bytes` placed at the entry point (0x0100).
fn exc_rom(bytes: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0100 + bytes.len()].copy_from_slice(bytes);
    rom
}

#[test]
fn oam_dma_bad_access_exception_breaks_only_when_armed() {
    // `ld a,C0 ; ldh (46),a ; jr -2`: kick an OAM DMA (source 0xC000) then spin
    // from ROM. Once the transfer is running, the spin's own opcode fetch is a
    // non-HRAM access contended with the DMA — bgb's "OAM DMA bad access".
    let rom = exc_rom(&[0x3E, 0xC0, 0xE0, 0x46, 0x18, 0xFE]);

    // Unarmed (mask 0) is a no-op: the free run never halts on it.
    let mut off = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    assert_eq!(
        off.run_frame_until_breakpoint(&[]),
        None,
        "no break when the exception is disarmed"
    );

    // Armed: the contended fetch during the transfer halts the run.
    let mut on = GameBoy::new(Model::Dmg, rom).unwrap();
    on.set_exceptions(EXC_OAM_DMA_BAD);
    assert!(
        on.run_frame_until_breakpoint(&[]).is_some(),
        "a non-HRAM access during an OAM DMA breaks"
    );
}

#[test]
fn incdec16_fexx_exception_breaks_only_when_armed() {
    // `ld hl,FE00 ; inc hl ; jr -2`: the INC HL drives HL=FE00 onto the bus, the
    // OAM-corruption trigger — bgb's "break on 16 bits inc/dec FE00-FEFF".
    let rom = exc_rom(&[0x21, 0x00, 0xFE, 0x23, 0x18, 0xFE]);

    let mut off = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    assert_eq!(
        off.run_frame_until_breakpoint(&[]),
        None,
        "no break when disarmed"
    );

    let mut on = GameBoy::new(Model::Dmg, rom).unwrap();
    on.set_exceptions(EXC_INCDEC_FEXX);
    assert_eq!(
        on.run_frame_until_breakpoint(&[]),
        Some(0xFE00),
        "INC HL from FE00 breaks at that address"
    );
}

#[test]
fn halt_cycles_counts_only_time_spent_halted() {
    // `ld a,0 ; ldh (FF0F),a ; ldh (FFFF),a ; halt`: clear IF + IE so HALT
    // (IME=0) has no wake condition and stays halted for the rest of the frame
    // while the PPU keeps running — so halt_cycles is most of the frame.
    let mut gb = GameBoy::new(
        Model::Dmg,
        exc_rom(&[0x3E, 0x00, 0xE0, 0x0F, 0xE0, 0xFF, 0x76]),
    )
    .unwrap();
    assert_eq!(gb.halt_cycles(), 0, "nothing halted yet");
    gb.run_frame();
    let (c, h) = (gb.cycles(), gb.halt_cycles());
    assert!(h > 0, "some cycles were spent halted");
    assert!(h <= c, "halt cycles never exceed total ({h} > {c})");
    assert!(
        h > c / 2,
        "a HALT-only frame is halted most of the time ({h}/{c})"
    );

    // A NOP-only ROM never halts, so the counter stays put.
    let mut nop = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    nop.run_frame();
    assert_eq!(nop.halt_cycles(), 0, "no halt cycles without a HALT");
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

/// A ROM whose entry (`0x100`) is `nop; jp 0x150` and `0x150..` is nops,
/// so PC walks 0x100 -> 0x101 -> 0x150 -> 0x151 -> 0x152 … deterministically.
fn linear_code_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100] = 0x00; // nop
    rom[0x101..0x104].copy_from_slice(&[0xC3, 0x50, 0x01]); // jp 0150
    // 0x150.. already 0x00 (nop) from the zero-fill.
    rom
}

/// Drive a 16-byte SGB command packet through the real `Joypad` as a P1 pulse
/// stream (Pan Docs "SGB Command Packet"): arm, reset pulse, 128 data bits
/// LSB-first (`$10` = 1, `$20` = 0, each followed by `$30`), then a `$20` stop
/// bit. Goes through `debug_write(0xFF00)` → the interconnect's SGB drain.
fn send_sgb_packet(gb: &mut GameBoy, data: &[u8; 16]) {
    gb.debug_write(0xFF00, 0x30); // arm the pulse receiver
    gb.debug_write(0xFF00, 0x00); // reset pulse: open the packet
    gb.debug_write(0xFF00, 0x30);
    for &byte in data {
        for bit in 0..8 {
            gb.debug_write(0xFF00, if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
            gb.debug_write(0xFF00, 0x30);
        }
    }
    gb.debug_write(0xFF00, 0x20); // stop bit closes the packet
    gb.debug_write(0xFF00, 0x30);
}

// Test category modules (split for the 1000-line cap); each is a `mod`
// via `use super::*`, reaching the shared fixtures above + the crate items.
#[path = "lib_tests/boot.rs"]
mod boot;
#[path = "lib_tests/cdl.rs"]
mod cdl;
#[path = "lib_tests/debug.rs"]
mod debug;
#[path = "lib_tests/ram.rs"]
mod ram;
#[path = "lib_tests/run.rs"]
mod run;
#[path = "lib_tests/savestate.rs"]
mod savestate;
#[path = "lib_tests/sgb.rs"]
mod sgb;
