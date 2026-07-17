//! Tests for the plugin-backed SGB coprocessor: the SNES-command routing
//! (DATA_SND / JUMP / SOUND) that the built-in HLE path leaves as no-ops, the
//! clean-room firmware chain 65C816 → SPC700 → S-DSP producing audio, a
//! save-state round trip, and the end-to-end injection into a real `GameBoy` —
//! all now routed through the two loaded wasm coprocessor plugins.
//!
//! Each test builds the two plugin crates for `wasm32` and loads them; it skips
//! (rather than fails) if the wasm target is unavailable, mirroring the
//! `slopgb-plugin-host` round-trip tests.

use std::process::Command;
use std::sync::OnceLock;

use slopgb_core::{Button, GameBoy, Model, SgbFlags, SgbSound};

use super::*;

/// Build a plugin crate for `wasm32` and read its artifact. `None` if the wasm
/// target (or the build) is unavailable.
fn build(pkg: &str, stem: &str) -> Option<Vec<u8>> {
    let manifest = format!("{}/../{pkg}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    // A unit test has no `CARGO_TARGET_TMPDIR`; a stable per-plugin dir under the
    // system temp keeps the wasm build cached across runs.
    let target = std::env::temp_dir().join(format!("slopgb-sgb-cop-{stem}"));
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            &manifest,
        ])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let wasm = target.join(format!("wasm32-unknown-unknown/release/{stem}.wasm"));
    fs::read(wasm).ok()
}

/// The two plugin `.wasm` blobs (built once, shared across tests). `None` when
/// the wasm toolchain is unavailable → the test skips.
fn plugins() -> Option<(Vec<u8>, Vec<u8>)> {
    static CACHE: OnceLock<Option<(Vec<u8>, Vec<u8>)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            Some((
                build("slopgb-spc700-plugin", "slopgb_spc700_plugin")?,
                build("slopgb-w65c816-plugin", "slopgb_w65c816_plugin")?,
            ))
        })
        .clone()
}

/// Build a coprocessor from the freshly built plugins, or `None` to skip.
fn build_cop(rate: u32) -> Option<SgbCoprocessor> {
    let (spc, cpu) = plugins()?;
    Some(SgbCoprocessor::from_wasm(&spc, &cpu, rate).unwrap())
}

/// A canned [`SgbCommandSource`] for driving `poll` directly.
#[derive(Default)]
struct TestCmds {
    sounds: Vec<SgbSound>,
    data_snd: Vec<Vec<u8>>,
    sou_trn: Option<Vec<u8>>,
    data_trn: Option<Vec<u8>>,
    flags: Option<SgbFlags>,
    packets: Vec<[u8; 16]>,
}

impl SgbCommandSource for TestCmds {
    fn take_packet(&mut self) -> Option<[u8; 16]> {
        (!self.packets.is_empty()).then(|| self.packets.remove(0))
    }
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

/// Read `len` bytes of the 65C816 plugin's SNES RAM (test observability).
fn cpu_ram(cop: &SgbCoprocessor, addr: u32, len: usize) -> Vec<u8> {
    cop.cpu.borrow_mut().read_ram(addr, len).unwrap()
}

/// The ICD2 pump end to end through the real wasm chips: a teed GB packet is
/// deposited into the plugin's ICD2 mailbox (only when the `$6002` flag is
/// clear), a guest SNES program consumes it and answers on the `$6004` pad
/// latch, and the answer surfaces through `AudioCoprocessor::joypad_feed` —
/// gated by the sticky written flag (None until the program writes).
#[test]
fn icd2_pump_round_trips_packet_to_joypad_feed() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    assert_eq!(cop.joypad_feed(), None, "no feed before the program writes");

    // wait: LDA $6002 / AND #$01 / BEQ wait — spin on the packet flag, then
    // LDA $7000 (clears the flag) / STA $6004 (pad latch) / STP.
    let prog = [
        0xAD, 0x02, 0x60, // LDA $6002
        0x29, 0x01, // AND #$01
        0xF0, 0xF9, // BEQ -7
        0xAD, 0x00, 0x70, // LDA $7000
        0x8D, 0x04, 0x60, // STA $6004
        0xDB, // STP
    ];
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }

    let mut pkt = [0u8; 16];
    pkt[0] = 0xEF; // the ACK byte the program echoes onto the pad latch
    let mut cmds = TestCmds {
        packets: vec![pkt],
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert!(cmds.packets.is_empty(), "poll drained the tee");
    // Two pump chunks: one deposits + runs the program, the next re-reads the
    // pad latches after the answer.
    cop.clock(4096 * 4);

    assert_eq!(
        cop.joypad_feed(),
        Some([0xEF, 0xFF, 0xFF, 0xFF]),
        "the program's pad answer feeds the GB joypad"
    );
}

/// The pump's LCD-row shadow: the guest reads `$6000` and sees the GB frame
/// position the host wrote (row 17 = last row / vblank).
#[test]
fn icd2_pump_maintains_lcd_row_shadow() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    // wait: LDA $6000 / LSR ×3 (drop the write-buffer bits) / CMP #$11 /
    // BNE wait — spin until the shadow shows vblank, then STA $0300 / STP.
    let prog = [
        0xAD, 0x00, 0x60, // LDA $6000
        0x4A, 0x4A, 0x4A, // LSR A ×3
        0xC9, 0x11, // CMP #$11
        0xD0, 0xF6, // BNE -10
        0x8D, 0x00, 0x03, // STA $0300
        0xDB, // STP
    ];
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    // ~152 GB lines into the frame: inside vblank -> row $11.
    cop.clock(456 * 152);
    assert_eq!(
        cpu_ram(&cop, 0x0300, 1),
        vec![0x11],
        "guest sees the vblank character row"
    );
}

/// DATA_TRN ($10) routes its 4 KB payload into SNES WRAM at the destination
/// carried in the teed packet header (Pan Docs "SGB Command $10": dest lo,
/// hi, bank — the pilot sends 81 00 01 7F = $7F:0100). On real hardware the
/// SGB BIOS performs this copy; the host pump assumes that duty. The payload
/// must NOT land in SPC (audio) RAM — that was a misroute.
#[test]
fn data_trn_lands_at_packet_dest_in_wram() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut pkt = [0u8; 16];
    pkt[0] = 0x81; // DATA_TRN, one packet
    pkt[1] = 0x00; // dest lo
    pkt[2] = 0x01; // dest hi
    pkt[3] = 0x7F; // dest bank -> $7F:0100
    let payload: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let mut cmds = TestCmds {
        packets: vec![pkt],
        data_trn: Some(payload.clone()),
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    // The transfer getter is throttled (polled every 64th call), so keep
    // polling until the edge-detect fires.
    for _ in 0..64 {
        cop.poll(&mut cmds);
    }
    assert_eq!(
        cpu_ram(&cop, 0x7F_0100, 4096),
        payload,
        "payload at the packet's WRAM dest"
    );
    let spc_head = cop.spc.borrow_mut().read_ram(0x0100, 16).unwrap();
    assert_ne!(
        &spc_head[..],
        &payload[..16],
        "no misroute into SPC RAM at the transfer shape's first dest"
    );
}

/// DATA_SND ($0F) is no longer a no-op: the packet's data lands at its target
/// SNES-work-RAM address of the 65C816 plugin (fullsnes `dest_lo, dest_hi, len,
/// data…`).
#[test]
fn data_snd_writes_to_snes_work_ram() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut cmds = TestCmds::default();
    // Write [0xDE, 0xAD] at $0300.
    cmds.data_snd.push(vec![0x00, 0x03, 0x02, 0xDE, 0xAD]);
    cop.poll(&mut cmds);
    assert_eq!(cpu_ram(&cop, 0x0300, 2), vec![0xDE, 0xAD]);
}

/// JUMP ($12) is no longer a no-op: it redirects the 65C816 plugin's program
/// counter, and the CPU then executes at the target. Verified functionally — a
/// sentinel program pre-loaded at the (bank-aliased) target writes a marker byte
/// only reachable if the CPU actually jumped there and ran.
#[test]
fn jump_redirects_the_snes_cpu() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    // At the jump target (bank 1 $9000, aliased to bank-0 RAM): LDA #$5A / STA
    // $0400 / STP — a marker the resident shim never writes.
    cop.cpu
        .borrow_mut()
        .write_ram(0x9000, &[0xA9, 0x5A, 0x8D, 0x00, 0x04, 0xDB])
        .unwrap();
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
    // throttle); loop until the window opens and JUMP redirects the PC.
    for _ in 0..64 {
        cop.poll(&mut cmds);
    }
    cop.clock(70_224); // the redirected CPU runs the sentinel program
    assert_eq!(
        cpu_ram(&cop, 0x0400, 1),
        vec![0x5A],
        "the CPU jumped to the target and executed there"
    );
}

/// The headline: a bare SGB SOUND ($08) command produces audio through the whole
/// clean-room chain — the mailbox is set in the 65C816 plugin's RAM, its shim
/// forwards it to the SPC700 comm port (mediated by the host between the two
/// loaded plugins), the SPC700 driver wakes and keys the S-DSP, and the S-DSP
/// synthesizes a tone. No game driver, no BIOS. A non-zero peak proves both
/// chips executed their firmware in wasm.
#[test]
fn sound_command_drives_the_firmware_chain_to_audio() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut cmds = TestCmds::default();
    cmds.sounds.push(SgbSound {
        effect_a: 0x40,
        effect_b: 0x00,
        attenuation: 0x00,
        effect_bank: 0x00,
    });
    cop.poll(&mut cmds); // mailbox note = 0x40, trigger = 1

    for _ in 0..8 {
        cop.clock(70_224);
    }
    assert!(
        peak(&cop.out) > 0.0,
        "SOUND drove 65C816 -> SPC700 -> S-DSP to audible output",
    );
    // The 65C816 really forwarded the mailbox to the SPC700 comm ports (the
    // host-mediated values that crossed into the SPC plugin).
    assert_eq!(cop.to_spc[0], 0x40, "shim forwarded the note to APUIO0");
    assert_ne!(cop.to_spc[1], 0x00, "shim forwarded the trigger to APUIO1");
}

/// A game that ships its own SPC700 driver via SOU_TRN still plays — the upload
/// replaces the resident driver and starts it (the path the built-in also has).
#[test]
fn sou_trn_game_driver_still_plays() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
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
    for _ in 0..8 {
        cop.clock(70_224);
    }
    assert!(
        peak(&cop.out) > 0.0,
        "the uploaded SOU_TRN driver synthesized audio"
    );
}

/// The coprocessor round-trips through a save state (both plugins' chip state +
/// the host accumulators), so an injected machine can still save/load.
#[test]
fn save_state_round_trips() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut cmds = TestCmds::default();
    cmds.sounds.push(SgbSound {
        effect_a: 0x22,
        effect_b: 0,
        attenuation: 0,
        effect_bank: 0,
    });
    // Two teed packets: the resident shim never reads the mailbox, so the
    // first stays unconsumed in the guest and the second stays queued — the
    // queued one (and the nonzero gb frame position) must ride the state.
    cmds.packets = vec![[0x11; 16], [0x22; 16]];
    cop.poll(&mut cmds);
    cop.clock(12_345);
    assert_eq!(cop.pending_packets.len(), 1, "one packet awaiting deposit");

    let mut w = Writer::new();
    cop.write_state(&mut w);
    let bytes = w.into_vec();

    let Some(mut restored) = build_cop(48_000) else {
        return;
    };
    let mut r = Reader::new(&bytes);
    restored.read_state(&mut r).unwrap();

    let mut w2 = Writer::new();
    restored.write_state(&mut w2);
    assert_eq!(bytes, w2.into_vec(), "state re-serializes identically");
}

/// Cloning is independent (fresh plugin instances with the state copied in), like
/// the built-in.
#[test]
fn clone_is_independent() {
    let Some(cop) = build_cop(48_000) else {
        return;
    };
    cop.cpu.borrow_mut().write_ram(0x0500, &[0x42]).unwrap();

    let cloned = cop.deep_clone().expect("re-instantiate for clone");
    cloned.cpu.borrow_mut().write_ram(0x0500, &[0x00]).unwrap();

    assert_eq!(
        cpu_ram(&cop, 0x0500, 1),
        vec![0x42],
        "SNES RAM is not shared"
    );
    assert_eq!(cpu_ram(&cloned, 0x0500, 1), vec![0x00]);
}

#[test]
fn inert_state_reloads_through_the_real_coprocessor() {
    // A machine saved while the coprocessor fell back to inert (a failed clone)
    // must still reload through the real coprocessor — the inert layout matches
    // `SgbCoprocessor::write_state`.
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut w = Writer::new();
    AudioCoprocessor::write_state(&InertCoprocessor, &mut w);
    let bytes = w.into_vec();
    let mut r = Reader::new(&bytes);
    cop.read_state(&mut r)
        .expect("inert-written state reads into the real coprocessor");
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

/// The full stack: inject the plugin-backed coprocessor into an SGB `GameBoy` via
/// the public `set_audio_coprocessor` seam, send a real SOUND packet through the
/// joypad, and drain audio. The GameBoy drives the injected coprocessor through
/// the `SgbCommandSource` seam and the clean-room firmware — running in two loaded
/// wasm plugins — turns the SOUND command into PCM, end to end.
#[test]
fn injected_coprocessor_makes_a_gameboy_sound_command_audible() {
    let Some(cop) = build_cop(slopgb_core::DEFAULT_SAMPLE_RATE) else {
        return;
    };
    let mut gb = GameBoy::new(Model::Sgb, sgb_rom()).unwrap();
    gb.set_audio_coprocessor(Box::new(cop));

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
        "an injected plugin-backed coprocessor made a real SOUND command audible",
    );
}

/// Injecting off an SGB model is impossible (no slot); a plain DMG is unaffected
/// and the coprocessor is never driven. Guards golden-safety at the seam.
#[test]
fn dmg_ignores_injection() {
    let Some(cop) = build_cop(48_000) else {
        return;
    };
    let mut dmg = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    dmg.set_audio_coprocessor(Box::new(cop));
    // Press/run/drain must not panic and produce a normal (silent-SGB) DMG.
    dmg.press(Button::A);
    let mut out = Vec::new();
    dmg.run_frame();
    dmg.drain_audio(&mut out);
    // No SGB mix was added (the box was dropped); the DMG simply ran.
    assert!(dmg.frame_count() >= 1);
}
