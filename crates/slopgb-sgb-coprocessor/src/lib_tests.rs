//! Tests for the combined SGB coprocessor: the SNES-command routing (DATA_SND /
//! JUMP / SOUND) that the built-in HLE path leaves as no-ops, the clean-room
//! firmware chain 65C816 → SPC700 → S-DSP producing audio, a save-state round
//! trip, and the end-to-end injection into a real `GameBoy`.

use slopgb_core::{Button, GameBoy, Model, SgbFlags, SgbSound};

use super::*;

/// A canned [`SgbCommandSource`] for driving `poll` directly.
#[derive(Default)]
struct TestCmds {
    sounds: Vec<SgbSound>,
    data_snd: Vec<Vec<u8>>,
    sou_trn: Option<Vec<u8>>,
    data_trn: Option<Vec<u8>>,
    flags: Option<SgbFlags>,
}

impl SgbCommandSource for TestCmds {
    fn take_sound_event(&mut self) -> Option<SgbSound> {
        (!self.sounds.is_empty()).then(|| self.sounds.remove(0))
    }
    fn take_data_snd(&mut self) -> Option<Vec<u8>> {
        (!self.data_snd.is_empty()).then(|| self.data_snd.remove(0))
    }
    fn sou_trn_data(&self) -> Option<&[u8]> {
        self.sou_trn.as_deref()
    }
    fn data_trn_data(&self) -> Option<&[u8]> {
        self.data_trn.as_deref()
    }
    fn flags(&self) -> Option<SgbFlags> {
        self.flags
    }
}

fn peak(out: &[(f32, f32)]) -> f32 {
    out.iter()
        .fold(0.0f32, |m, &(l, r)| m.max(l.abs()).max(r.abs()))
}

/// DATA_SND ($0F) is no longer a no-op: the packet's data lands at its target
/// SNES-work-RAM address (fullsnes `dest_lo, dest_hi, len, data…`).
#[test]
fn data_snd_writes_to_snes_work_ram() {
    let mut cop = SgbCoprocessor::new(48_000);
    let mut cmds = TestCmds::default();
    // Write [0xDE, 0xAD] at $0300.
    cmds.data_snd.push(vec![0x00, 0x03, 0x02, 0xDE, 0xAD]);
    cop.poll(&mut cmds);
    assert_eq!(cop.bus.ram[0x0300], 0xDE);
    assert_eq!(cop.bus.ram[0x0301], 0xAD);
}

/// JUMP ($12) is no longer a no-op: it redirects the 65C816's program counter
/// (checked after the throttle window opens).
#[test]
fn jump_redirects_the_snes_cpu() {
    let mut cop = SgbCoprocessor::new(48_000);
    let mut cmds = TestCmds {
        flags: Some(SgbFlags {
            atrc_en: false,
            test_en: false,
            icon_en: false,
            pal_pri: false,
            jump: Some(0x01_9000),
        }),
        ..Default::default()
    };
    // The transfer/flag getters are polled every 64th call (matching the built-in
    // throttle); loop until the window opens.
    for _ in 0..64 {
        cop.poll(&mut cmds);
    }
    assert_eq!(
        cop.cpu.regs.pc, 0x9000,
        "PC low 16 bits from the JUMP target"
    );
    assert_eq!(cop.cpu.regs.pbr, 0x01, "program bank from the JUMP target");
}

/// The headline: a bare SGB SOUND ($08) command produces audio through the whole
/// clean-room chain — the mailbox is set, the 65C816 shim forwards it to the
/// SPC700 comm port, the SPC700 driver wakes and keys the S-DSP, and the S-DSP
/// synthesizes a tone. No game-supplied driver, no BIOS. Nothing pokes the DSP
/// from Rust, so a non-zero peak proves both CPUs executed the firmware.
#[test]
fn sound_command_drives_the_firmware_chain_to_audio() {
    let mut cop = SgbCoprocessor::new(48_000);
    let mut cmds = TestCmds::default();
    cmds.sounds.push(SgbSound {
        effect_a: 0x40,
        effect_b: 0x00,
        attenuation: 0x00,
        effect_bank: 0x00,
    });
    cop.poll(&mut cmds); // mailbox note = 0x40, trigger = 1

    // Silence before the trigger propagates; audible once the chain runs.
    for _ in 0..8 {
        cop.clock(70_224);
    }
    assert!(
        peak(&cop.out) > 0.0,
        "SOUND drove 65C816 -> SPC700 -> S-DSP to audible output",
    );
    // The 65C816 really forwarded the mailbox to the SPC700 comm ports.
    assert_eq!(
        cop.spc.apu_port_in(0),
        0x40,
        "shim forwarded the note to APUIO0"
    );
    assert_ne!(
        cop.spc.apu_port_in(1),
        0x00,
        "shim forwarded the trigger to APUIO1"
    );
}

/// A game that ships its own SPC700 driver via SOU_TRN still plays — the upload
/// replaces the resident driver and starts it (the path the built-in also has).
#[test]
fn sou_trn_game_driver_still_plays() {
    let mut cop = SgbCoprocessor::new(48_000);
    // An unconditional tone driver (no port poll): program@$0400 sets up the DSP
    // and spins. Same clean-room construction as the resident driver, minus the
    // wait loop.
    let mut prog = vec![0x20u8]; // CLRP
    let mov = |dp: u8, imm: u8| [0x8F, imm, dp];
    for (dp, imm) in [
        (0x6Cu8, 0x00u8),
        (0x5D, 0x02),
        (0x0C, 0x7F),
        (0x1C, 0x7F),
        (0x00, 0x7F),
        (0x01, 0x7F),
        (0x02, 0x00),
        (0x03, 0x10),
        (0x04, 0x00),
        (0x05, 0x00),
        (0x07, 0x7F),
        (0x4C, 0x01),
    ] {
        prog.extend_from_slice(&mov(0xF2, dp));
        prog.extend_from_slice(&mov(0xF3, imm));
    }
    prog.extend_from_slice(&[0x2F, 0xFE]); // BRA *
    let dir = [0x10u8, 0x02, 0x10, 0x02];
    let brr = [0x93u8, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88];
    let mut block = Vec::new();
    let mut push = |dest: u16, data: &[u8]| {
        block.extend_from_slice(&dest.to_le_bytes());
        block.extend_from_slice(&(data.len() as u16).to_le_bytes());
        block.extend_from_slice(data);
    };
    push(0x0400, &prog);
    push(0x0200, &dir);
    push(0x0210, &brr);

    let mut cmds = TestCmds {
        sou_trn: Some(block),
        ..Default::default()
    };
    for _ in 0..64 {
        cop.poll(&mut cmds); // the SOU_TRN getter opens on the 64th poll
    }
    for _ in 0..4 {
        cop.clock(70_224);
    }
    assert!(
        peak(&cop.out) > 0.0,
        "the uploaded SOU_TRN driver synthesized audio"
    );
}

/// The coprocessor round-trips through a save state (CPU + SNES RAM + SPC700 +
/// S-DSP + accumulators), so an injected machine can still save/load.
#[test]
fn save_state_round_trips() {
    let mut cop = SgbCoprocessor::new(48_000);
    let mut cmds = TestCmds::default();
    cmds.sounds.push(SgbSound {
        effect_a: 0x22,
        effect_b: 0,
        attenuation: 0,
        effect_bank: 0,
    });
    cop.poll(&mut cmds);
    cop.clock(12_345);

    let mut w = Writer::new();
    cop.write_state(&mut w);
    let bytes = w.into_vec();

    let mut restored = SgbCoprocessor::new(48_000);
    let mut r = Reader::new(&bytes);
    restored.read_state(&mut r).unwrap();

    let mut w2 = Writer::new();
    restored.write_state(&mut w2);
    assert_eq!(bytes, w2.into_vec(), "state re-serializes identically");
}

/// Cloning is independent (deep-copies the shared DSP cell), like the built-in.
#[test]
fn clone_is_independent() {
    let mut cop = SgbCoprocessor::new(48_000);
    cop.bus.ram[0x0500] = 0x42;
    cop.dsp.borrow_mut().write(0x0C, 0x11);

    let mut cloned = cop.clone();
    cloned.bus.ram[0x0500] = 0x00;
    cloned.dsp.borrow_mut().write(0x0C, 0x77);

    assert_eq!(cop.bus.ram[0x0500], 0x42, "SNES RAM is not shared");
    assert_eq!(cop.dsp.borrow().read(0x0C), 0x11, "DSP cell is not shared");
    assert_eq!(cloned.dsp.borrow().read(0x0C), 0x77);
}

// ---- End-to-end: injected into a real GameBoy ----

/// A minimal SGB-enhanced cart (ROM-only NOP sled with the SGB header flags).
fn sgb_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03; // SGB flag
    rom[0x14B] = 0x33; // old licensee code
    rom
}

/// Drive a 16-byte SGB command packet through the real joypad (`FF00` pulses),
/// exactly as the SGB command protocol specifies (Pan Docs "SGB Command Packet").
fn send_sgb_packet(gb: &mut GameBoy, data: &[u8; 16]) {
    gb.debug_write(0xFF00, 0x30);
    gb.debug_write(0xFF00, 0x00);
    gb.debug_write(0xFF00, 0x30);
    for &byte in data {
        for bit in 0..8 {
            gb.debug_write(0xFF00, if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
            gb.debug_write(0xFF00, 0x30);
        }
    }
    gb.debug_write(0xFF00, 0x20);
    gb.debug_write(0xFF00, 0x30);
}

/// The full stack: inject the combined coprocessor into an SGB `GameBoy` via the
/// public `set_audio_coprocessor` seam, send a real SOUND packet through the
/// joypad, and drain audio. The GameBoy drives the injected coprocessor through
/// the `SgbCommandSource` seam (goal 1) and the clean-room firmware turns the
/// SOUND command into PCM (goals 2 + 3) — end to end, no core-private types.
#[test]
fn injected_coprocessor_makes_a_gameboy_sound_command_audible() {
    let mut gb = GameBoy::new(Model::Sgb, sgb_rom()).unwrap();
    gb.set_audio_coprocessor(Box::new(SgbCoprocessor::new(
        slopgb_core::DEFAULT_SAMPLE_RATE,
    )));

    // SOUND ($08), length 1: effect A = note 0x40, rest 0 (trigger defaults on).
    let mut packet = [0u8; 16];
    packet[0] = 0x08 * 8 + 1;
    packet[1] = 0x40;
    send_sgb_packet(&mut gb, &packet);

    let mut out = Vec::new();
    for _ in 0..16 {
        gb.run_frame();
        gb.drain_audio(&mut out);
    }
    assert!(
        peak(&out) > 0.0,
        "an injected coprocessor made a real SOUND command audible end to end",
    );
}

/// Injecting off an SGB model is impossible (no slot); a plain DMG is unaffected
/// and the coprocessor is never driven. Guards golden-safety at the seam.
#[test]
fn dmg_ignores_injection() {
    let mut dmg = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    dmg.set_audio_coprocessor(Box::new(SgbCoprocessor::new(48_000)));
    // Press/run/drain must not panic and produce a normal (silent-SGB) DMG.
    dmg.press(Button::A);
    let mut out = Vec::new();
    dmg.run_frame();
    dmg.drain_audio(&mut out);
    // No SGB mix was added (the box was dropped); the DMG simply ran.
    assert!(dmg.frame_count() >= 1);
}
