//! Cross-module smoke tests: synthetic ROMs running on the full `GameBoy`
//! (CPU + interconnect + peripherals), exercising the mooneye breakpoint
//! protocol, interrupt delivery end-to-end, and OAM DMA driven by real CPU
//! code.

use slopgb_core::cpu::Bus;
use slopgb_core::{Button, GameBoy, Model};

/// A minimal valid 32 KiB ROM (cart type 0, size codes 0) with `chunks` of
/// machine code / data placed at absolute addresses.
fn build_rom(chunks: &[(usize, &[u8])]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    for &(addr, bytes) in chunks {
        rom[addr..addr + bytes.len()].copy_from_slice(bytes);
    }
    rom
}

/// Step until the `LD B,B` breakpoint, with an instruction-count bound.
fn run_to_breakpoint(gb: &mut GameBoy, max_steps: u32) {
    for _ in 0..max_steps {
        if gb.debug_breakpoint_hit() {
            return;
        }
        gb.step();
    }
    panic!(
        "no LD B,B breakpoint after {max_steps} steps (PC={:04X})",
        gb.cpu_regs().pc
    );
}

#[test]
fn post_boot_cpu_registers_per_model() {
    let rom = build_rom(&[(0x100, &[0x40])]); // immediate LD B,B
    for (model, af) in [
        (Model::Dmg, 0x01B0u16),
        (Model::Mgb, 0xFFB0),
        (Model::Sgb, 0x0100),
        (Model::Cgb, 0x1180),
    ] {
        let gb = GameBoy::new(model, rom.clone()).unwrap();
        let r = gb.cpu_regs();
        assert_eq!(r.af(), af, "{model:?}");
        assert_eq!(r.sp, 0xFFFE, "{model:?}");
        assert_eq!(r.pc, 0x0100, "{model:?}");
    }
}

#[test]
fn auto_model_uses_cgb_flag() {
    let mut rom = build_rom(&[]);
    assert_eq!(GameBoy::auto_model(&rom), Model::Dmg);
    rom[0x143] = 0x80;
    assert_eq!(GameBoy::auto_model(&rom), Model::Cgb);
    rom[0x143] = 0xC0;
    assert_eq!(GameBoy::auto_model(&rom), Model::Cgb);
}

/// A NOP slide into LD B,B: the breakpoint must trip with exact timing
/// (11 instructions x 1 M-cycle x 4 T).
#[test]
fn nop_slide_reaches_breakpoint_with_exact_timing() {
    let mut code = [0x00u8; 11];
    code[10] = 0x40; // LD B,B
    let mut gb = GameBoy::new(Model::Dmg, build_rom(&[(0x100, &code)])).unwrap();
    run_to_breakpoint(&mut gb, 100);
    assert_eq!(gb.cpu_regs().pc, 0x010B);
    assert_eq!(gb.cycles(), 44);
}

/// Timer interrupt end-to-end: TIMA overflow must wake HALT and dispatch to
/// the $50 vector, where LD B,B fires.
#[test]
fn timer_interrupt_dispatches_to_vector() {
    let program: &[u8] = &[
        0x3E, 0x04, // ld a,$04
        0xE0, 0xFF, // ldh (IE),a      ; timer only
        0x3E, 0x05, // ld a,$05
        0xE0, 0x07, // ldh (TAC),a     ; running, 16 T per increment
        0x3E, 0xF0, // ld a,$F0
        0xE0, 0x05, // ldh (TIMA),a
        0xAF, // xor a
        0xE0, 0x0F, // ldh (IF),a      ; clear the boot-pending vblank
        0xFB, // ei
        0x00, // nop (EI delay)
        0x76, // halt
        0x00, // nop
    ];
    let rom = build_rom(&[(0x100, program), (0x50, &[0x40, 0xC9])]);
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    run_to_breakpoint(&mut gb, 10_000);
    assert_eq!(gb.cpu_regs().pc, 0x0051, "stopped inside the $50 handler");
}

/// Joypad interrupt end-to-end: a button press on the selected column must
/// wake HALT and dispatch to the $60 vector.
#[test]
fn joypad_press_dispatches_to_vector() {
    let program: &[u8] = &[
        0x3E, 0x10, // ld a,$10
        0xE0, 0x00, // ldh (P1),a      ; select the button column
        0x3E, 0x10, // ld a,$10
        0xE0, 0xFF, // ldh (IE),a      ; joypad only
        0xAF, // xor a
        0xE0, 0x0F, // ldh (IF),a
        0xFB, // ei
        0x00, // nop
        0x76, // halt
        0x18, 0xFE, // jr -2 (spin if it falls through)
    ];
    let rom = build_rom(&[(0x100, program), (0x60, &[0x40, 0xC9])]);
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    // Let the program reach HALT, then press a button.
    for _ in 0..100 {
        gb.step();
    }
    assert!(!gb.debug_breakpoint_hit(), "no interrupt before the press");
    gb.press(Button::A);
    run_to_breakpoint(&mut gb, 100);
    assert_eq!(gb.cpu_regs().pc, 0x0061, "stopped inside the $60 handler");
}

/// OAM DMA driven entirely by CPU code, like real games do it: copy a
/// trampoline to HRAM, start a transfer from ROM page $12, busy-wait inside
/// HRAM, then compare all 160 OAM bytes against the source. C = $99 marks
/// success, C = $00 a mismatch.
#[test]
fn oam_dma_end_to_end_from_cpu_code() {
    let main: &[u8] = &[
        0xF0, 0x44, // 0100: ldh a,(LY)
        0xFE, 0x90, // 0102: cp $90
        0x20, 0xFA, // 0104: jr nz,$0100   ; wait for vblank
        0xAF, // 0106: xor a
        0xE0, 0x40, // 0107: ldh (LCDC),a  ; LCD off: OAM stays readable
        0x21, 0x80, 0xFF, // 0109: ld hl,$FF80
        0x11, 0x50, 0x01, // 010C: ld de,$0150
        0x06, 0x08, // 010F: ld b,8
        0x1A, // 0111: ld a,(de)
        0x22, // 0112: ld (hl+),a
        0x13, // 0113: inc de
        0x05, // 0114: dec b
        0x20, 0xFA, // 0115: jr nz,$0111   ; copy trampoline to HRAM
        0x3E, 0x12, // 0117: ld a,$12      ; source page
        0xCD, 0x80, 0xFF, // 0119: call $FF80
        0x21, 0x00, 0xFE, // 011C: ld hl,$FE00
        0x11, 0x00, 0x12, // 011F: ld de,$1200
        0x06, 0xA0, // 0122: ld b,160
        0x1A, // 0124: ld a,(de)
        0xBE, // 0125: cp (hl)
        0x20, 0x08, // 0126: jr nz,$0130
        0x23, // 0128: inc hl
        0x13, // 0129: inc de
        0x05, // 012A: dec b
        0x20, 0xF7, // 012B: jr nz,$0124
        0x0E, 0x99, // 012D: ld c,$99
        0x40, // 012F: ld b,b          ; pass
        0x0E, 0x00, // 0130: ld c,$00
        0x40, // 0132: ld b,b          ; fail
    ];
    let trampoline: &[u8] = &[
        0xE0, 0x46, // ldh (DMA),a
        0x3E, 0x28, // ld a,40
        0x3D, // dec a
        0x20, 0xFD, // jr nz,-3        ; 40 x 4 M-cycles > 162
        0xC9, // ret
    ];
    let pattern: Vec<u8> = (0..160u32).map(|i| (i * 7 + 3) as u8).collect();
    let rom = build_rom(&[(0x100, main), (0x150, trampoline), (0x1200, &pattern)]);
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    run_to_breakpoint(&mut gb, 1_000_000);
    let r = gb.cpu_regs();
    assert_eq!(
        r.c,
        0x99,
        "OAM contents mismatch at index {}",
        160 - r.b as u32
    );
    assert_eq!(r.pc, 0x0130);
}

/// HALT freezing an in-flight OAM DMA, end to end through real CPU code —
/// the exact `ldh (DMA),a / nop / halt` sequence of
/// test-roms-src/madness/mgb_oam_dma_halt_sprites.s `hiram_test`. The asm's
/// hardware-verified result pins the freeze point: with the transfer started
/// 2 M-cycles before the HALT fetch, bytes 0 and 1 are copied and byte 2 is
/// left mid-access, its old OAM value intact ("OAM DMA is in the middle of
/// OAM access (but not proceeding with it!)") — i.e. the core clock gate
/// engages only after the post-HALT prefetch M-cycle.
#[test]
fn halt_freezes_inflight_oam_dma_end_to_end() {
    let main: &[u8] = &[
        0x3E, 0x30, // 0100: ld a,$30
        0xEA, 0x02, 0xFE, // 0102: ld ($FE02),a  ; old OAM byte 2 (LCD off)
        0x3E, 0x40, // 0105: ld a,$40
        0xEA, 0x03, 0xFE, // 0107: ld ($FE03),a  ; old OAM byte 3
        0x21, 0x80, 0xFF, // 010A: ld hl,$FF80
        0x11, 0x50, 0x01, // 010D: ld de,$0150
        0x06, 0x08, // 0110: ld b,8
        0x1A, // 0112: ld a,(de)
        0x22, // 0113: ld (hl+),a
        0x13, // 0114: inc de
        0x05, // 0115: dec b
        0x20, 0xFA, // 0116: jr nz,$0112   ; copy hiram routine to HRAM
        0xC3, 0x80, 0xFF, // 0118: jp $FF80
    ];
    let hiram: &[u8] = &[
        0x3E, 0x20, // FF80: ld a,$20      ; source page $2000
        0xE0, 0x46, // FF82: ldh (DMA),a   ; cycle W: transfer requested
        0x00, // FF84: nop                 ; W+1: setup delay elapses
        0x76, // FF85: halt                ; fetch at W+2 (byte 0 copies);
        //            post-HALT prefetch at W+3 (byte 1 copies); frozen after.
        0x00, // FF86: nop
    ];
    let mut source = vec![0xFFu8; 160];
    source[0] = 0xA0;
    source[1] = 0xA1;
    source[2] = 0x1A; // the in-flight byte that must never commit
    source[3] = 0xA3;
    let rom = build_rom(&[(0x100, main), (0x150, hiram), (0x2000, &source)]);
    let cart = slopgb_core::cartridge::Cartridge::from_bytes(rom).unwrap();
    // Interconnect without post-boot state: LCD off, so the direct OAM
    // writes land and OAM stays CPU-readable during the frozen transfer.
    let mut bus = slopgb_core::interconnect::Interconnect::new(Model::Mgb, cart);
    let mut cpu = slopgb_core::cpu::Cpu::new(Model::Mgb);
    // Comfortably more M-cycles than program + a full 160-byte transfer, so
    // a missing freeze fails as "transfer completed", not "still running".
    for _ in 0..400 {
        cpu.step(&mut bus);
    }
    // Frozen mid-transfer: copied bytes stay, byte 2 keeps its old value,
    // and no further byte was touched. The test bus reads tick the machine,
    // which must not advance the gated DMA either.
    assert_eq!(bus.read(0xFE00), 0xA0, "byte 0 copied before the freeze");
    assert_eq!(
        bus.read(0xFE01),
        0xA1,
        "byte 1 copied by the post-HALT prefetch cycle"
    );
    assert_eq!(
        bus.read(0xFE02),
        0x30,
        "byte 2 frozen mid-access: old value intact"
    );
    assert_eq!(bus.read(0xFE03), 0x40, "byte 3 untouched");
    assert_eq!(
        bus.read(0xFF46),
        0x20,
        "DMA register reads back the source page"
    );
}

/// The interconnect alone, driven through the `Bus` trait: an OAM DMA from
/// cartridge ROM lands in OAM with the documented 1 M-cycle setup delay.
#[test]
fn oam_dma_via_bus_trait() {
    let pattern: Vec<u8> = (0..160u32).map(|i| (i as u8) ^ 0xA5).collect();
    let rom = build_rom(&[(0x1200, &pattern)]);
    let cart = slopgb_core::cartridge::Cartridge::from_bytes(rom).unwrap();
    let mut bus = slopgb_core::interconnect::Interconnect::new(Model::Dmg, cart);
    bus.write(0xFF46, 0x12);
    for _ in 0..160 {
        bus.tick();
    }
    assert_eq!(bus.read(0xFE00), 0xFF, "still locked on the last cycle");
    assert_eq!(bus.read(0xFE00), 0xA5, "unlocked right after");
    assert_eq!(bus.read(0xFE9F), 159 ^ 0xA5);
}

/// `run_frame` makes forward progress both with the LCD on (frame_count
/// advances) and off (cycle deadline).
#[test]
fn run_frame_progresses() {
    // LCD stays on post-boot: frames advance.
    let rom = build_rom(&[(0x100, &[0x18, 0xFE])]); // jr -2
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    let f0 = gb.frame_count();
    gb.run_frame();
    assert_eq!(gb.frame_count(), f0 + 1);

    // LCD off: run_frame falls back to the cycle deadline.
    let rom = build_rom(&[(0x100, &[0xAF, 0xE0, 0x40, 0x18, 0xFE])]);
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    let c0 = gb.cycles();
    gb.run_frame();
    gb.run_frame();
    assert!(gb.cycles() >= c0 + 70224, "deadline progress with LCD off");
}
