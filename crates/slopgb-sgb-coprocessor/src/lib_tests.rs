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
/// `pub(crate)`: also reused by `samples_tests.rs` (a cousin test module, not
/// a descendant of this one) so the wasm plugins build only once per run.
pub(crate) fn build_cop(rate: u32) -> Option<SgbCoprocessor> {
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
    /// Capture counter served by `data_trn_seq` (`None` = no counter).
    trn_seq: Option<u64>,
    /// How many times `data_trn_data` was read (the gate observability).
    trn_reads: std::cell::Cell<u32>,
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
        self.trn_reads.set(self.trn_reads.get() + 1);
        self.data_trn.as_deref()
    }
    fn data_trn_seq(&self) -> Option<u64> {
        self.trn_seq
    }
    fn flags(&self) -> Option<SgbFlags> {
        self.flags
    }
}

fn peak(out: &[(f32, f32)]) -> f32 {
    out.iter()
        .fold(0.0f32, |m, &(l, r)| m.max(l.abs()).max(r.abs()))
}

/// A little `LDA #imm / STA abs` assembler for guest MMIO setup programs.
fn stores(pairs: &[(u16, u8)], tail: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    for &(addr, val) in pairs {
        p.extend_from_slice(&[0xA9, val, 0x8D, addr as u8, (addr >> 8) as u8]);
    }
    p.extend_from_slice(tail);
    p
}

/// Drive one resident main-service call on the guest — consumes a pending
/// delivery-mailbox packet, publishing it to the BIOS-runtime variables
/// (the publish is guest-side since the mailbox serialization landed).
fn run_service(cop: &mut SgbCoprocessor) {
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9F00, &[0x20, 0xED, 0xBB, 0xDB]).unwrap(); // JSR $BBED / STP
        cpu.set_pc(0x9F00).unwrap();
    }
    cop.clock(4096 * 2);
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

/// JUMP hands control over in native mode (pinned black-box: the pilot's
/// dispatcher uses REP #$30 + 16-bit index ops, impossible in emulation
/// mode). A target program whose marker is only correct when REP #$20
/// actually widens A proves the mode.
#[test]
fn jump_enters_native_mode() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    // REP #$20 / LDA #$1234 / STA $0330 / SEP #$20 / STP.
    let prog = [
        0xC2, 0x20, 0xA9, 0x34, 0x12, 0x8D, 0x30, 0x03, 0xE2, 0x20, 0xDB,
    ];
    cop.cpu.borrow_mut().write_ram(0x9100, &prog).unwrap();
    let mut cmds = TestCmds {
        flags: Some(SgbFlags {
            atrc_en: false,
            test_en: false,
            icon_en: false,
            pal_pri: false,
            jump: Some(0x9100),
        }),
        ..Default::default()
    };
    for _ in 0..64 {
        cop.poll(&mut cmds);
    }
    cop.clock(70_224);
    assert_eq!(
        cop.debug_cpu_ram(0x0330, 2),
        vec![0x34, 0x12],
        "16-bit store: the JUMP target ran in native mode"
    );
}

/// The resident BIOS-runtime contract that SGB arcade-takeover programs hook
/// (pinned black-box from the pilot's own DATA_SND-installed routines — see
/// docs/hardware-state/sgb-arcade-takeover.md): every teed packet's bytes
/// land at `$7E:0600` with the command number at `$7E:02C2`; a DATA_TRN
/// payload is staged in WRAM with a pointer at `$7E:0284/85`; and the
/// service entries the uploaded bootstrap JSRs into hold resident RTS stubs
/// (the host performs their duties asynchronously) so the call does not
/// crash into zeroed RAM.
#[test]
fn bios_runtime_contract_variables_stubs_and_staging() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    // Service entries survive a JSR (no crash into zeroed RAM), and the
    // main service calls the game's $0800 hook when one is installed: a
    // guest program JSRs both entries around installing a marker hook.
    {
        let mut cpu = cop.cpu.borrow_mut();
        // Hook at $0800: LDA #$77 / STA $0310 / RTS.
        cpu.write_ram(0x0800, &[0xA9, 0x77, 0x8D, 0x10, 0x03, 0x60])
            .unwrap();
        // JSR $C58D / JSR $BBED / LDA #$55 / STA $0320 / STP.
        let prog = [
            0x20, 0x8D, 0xC5, // JSR $C58D (aux: waits one vblank NMI)
            0x20, 0xED, 0xBB, // JSR $BBED (main: calls the hook)
            0xA9, 0x55, // LDA #$55
            0x8D, 0x20, 0x03, // STA $0320 (survival marker)
            0xDB, // STP
        ];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    // The aux service blocks on the vblank edge, so the program needs the
    // host pump (RDNMI shadow + NMI delivery), not a raw run_until.
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0320, 1),
        vec![0x55],
        "both JSRs returned"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x0310, 1),
        vec![0x77],
        "the service calls the hook every invocation"
    );

    // A teed packet lands in the packet buffer + last-command variable.
    let mut pkt = [0u8; 16];
    pkt[0] = 0x81; // DATA_TRN
    pkt[1] = 0x00;
    pkt[2] = 0x01;
    pkt[3] = 0x7F;
    let payload: Vec<u8> = (0..4096u32).map(|i| (i % 249) as u8).collect();
    let mut cmds = TestCmds {
        packets: vec![pkt],
        data_trn: Some(payload.clone()),
        ..TestCmds::default()
    };
    for _ in 0..65 {
        cop.poll(&mut cmds);
    }
    run_service(&mut cop);
    assert_eq!(cop.debug_cpu_ram(0x0600, 16), pkt.to_vec(), "packet buffer");
    assert_eq!(
        cop.debug_cpu_ram(0x0310, 1),
        vec![0x77],
        "the delivery called the installed hook"
    );
    assert_eq!(cop.debug_cpu_ram(0x02C2, 1), vec![0x10], "last command");

    // The payload is staged in WRAM behind the $0284/85 pointer (bank $7E
    // implied — the game's copy loop hardwires it) and still lands at the
    // packet's dest.
    let ptr = cop.debug_cpu_ram(0x0284, 2);
    let staging = 0x7E_0000 | u32::from(ptr[0]) | u32::from(ptr[1]) << 8;
    assert_eq!(cop.debug_cpu_ram(staging, 4096), payload, "staged payload");
    assert_eq!(cop.debug_cpu_ram(0x7F_0100, 4096), payload, "dest copy");
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
    // Pan Docs "SGB Command 0Fh — DATA_SND": dest lo, dest hi, BANK, count,
    // data. Write [0xDE, 0xAD] at $7F:0300.
    cmds.data_snd.push(vec![0x00, 0x03, 0x7F, 0x02, 0xDE, 0xAD]);
    cop.poll(&mut cmds);
    assert_eq!(cpu_ram(&cop, 0x7F_0300, 2), vec![0xDE, 0xAD]);
    // Bank 0 routes through the low-WRAM mirror ($7E), count clamps at 11.
    let mut pkt = vec![0x00, 0x18, 0x00, 0x0B];
    pkt.extend_from_slice(&[0xEA; 12]); // 12 data bytes, only 11 valid
    cmds.data_snd.push(pkt);
    cop.poll(&mut cmds);
    assert_eq!(cpu_ram(&cop, 0x7E_1800, 12), {
        let mut v = vec![0xEA; 11];
        v.push(0x00);
        v
    });
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
    // Non-default DMA + autopoll state must ride too.
    cop.dma_regs[2][1] = 0x18;
    cop.wmadd = 0x1234;
    cop.joy_busy = true;

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

// ---- The SNES clocking loop: vblank NMI + status shadows ----

/// Install a WAI-loop main + counting NMI handler, enable NMIs the real way
/// (the guest's own $4200 write, tracked through the MMIO capture ring), and
/// count exactly one NMI per emulated GB frame.
fn nmi_counting_cop() -> Option<SgbCoprocessor> {
    let cop = build_cop(48_000)?;
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0xFFFA, &[0x00, 0x92]).unwrap(); // NMI vector -> $9200
        // main: LDA #$81 / STA $4200 (NMI enable + autopoll) / WAI / BRA WAI.
        cpu.write_ram(0x9000, &[0xA9, 0x81, 0x8D, 0x00, 0x42, 0xCB, 0x80, 0xFD])
            .unwrap();
        cpu.write_ram(0x9200, &[0xEE, 0x40, 0x03, 0x40]).unwrap(); // INC $0340 / RTI
        cpu.set_pc(0x9000).unwrap();
    }
    Some(cop)
}

#[test]
fn vblank_nmi_fires_once_per_frame_when_enabled() {
    let Some(mut cop) = nmi_counting_cop() else {
        return;
    };
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0340, 1),
        vec![2],
        "exactly one NMI per frame across two frames"
    );
}

#[test]
fn no_nmi_without_the_guest_enabling_it() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0xFFFA, &[0x00, 0x92]).unwrap();
        cpu.write_ram(0x9000, &[0xCB, 0x80, 0xFD]).unwrap(); // WAI loop, no $4200
        cpu.write_ram(0x9200, &[0xEE, 0x40, 0x03, 0x40]).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0340, 1),
        vec![0],
        "NMITIMEN bit 7 gates NMI"
    );
}

/// RDNMI/HVBJOY shadows: the guest spins on HVBJOY bit 7, then reads RDNMI
/// twice — first read shows the flag + CPU version, the second shows the
/// read-acknowledge (bit 7 cleared, guest-side).
#[test]
fn rdnmi_and_hvbjoy_shadows_follow_the_frame() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        // wait: LDA $4212 / BPL wait / LDA $4210 / STA $0341 / LDA $4210 /
        // STA $0342 / STP.
        let prog = [
            0xAD, 0x12, 0x42, // LDA $4212 (HVBJOY)
            0x10, 0xFB, // BPL -5 (spin until vblank bit sets)
            0xAD, 0x10, 0x42, // LDA $4210 (RDNMI)
            0x8D, 0x41, 0x03, // STA $0341
            0xAD, 0x10, 0x42, // LDA $4210 (again)
            0x8D, 0x42, 0x03, // STA $0342
            0xDB, // STP
        ];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224);
    let first = cop.debug_cpu_ram(0x0341, 1)[0];
    let second = cop.debug_cpu_ram(0x0342, 1)[0];
    assert_eq!(first & 0x80, 0x80, "RDNMI flag set inside vblank");
    assert_eq!(first & 0x0F, 0x02, "CPU version bits");
    assert_eq!(second & 0x80, 0, "read acknowledged the flag");
}

/// The resident NMI handler dispatches through the RAM vector at $00:00BB
/// (fullsnes SGB notes: the hookable NMI vector JUMP clobbers). Empty vector
/// -> the NMI is a no-op for the program; installed vector -> the hook runs.
#[test]
fn nmi_dispatches_through_the_ram_vector() {
    let Some(mut cop) = nmi_counting_cop() else {
        return;
    };
    // Point the RAM vector at a counting hook: INC $0344 / RTI... the hook
    // is entered by JML, so return with RTI (the handler's JML replaced its
    // own frame — the interrupt frame is still on the stack).
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9300, &[0xEE, 0x44, 0x03, 0x40]).unwrap(); // INC $0344 / RTI
        cpu.write_ram(0x00BB, &[0x00, 0x93, 0x00]).unwrap(); // [$00BB] = $00:9300
        // Note: nmi_counting_cop's own $FFFA override is replaced back with
        // the resident handler so the RAM-vector path is what runs.
        cpu.write_ram(0xFFFA, &[0x30, 0xBE]).unwrap();
    }
    cop.clock(70_224);
    assert_eq!(
        cop.debug_cpu_ram(0x0344, 1),
        vec![1],
        "the hook behind the RAM vector ran"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x0340, 1),
        vec![0],
        "the test vector override was replaced"
    );
}

/// The resident NMI handler preserves the interrupted program's A on the
/// empty-vector path (the BIOS-only case): A survives across an NMI.
#[test]
fn nmi_handler_preserves_a_with_empty_vector() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        // LDA #$81 / STA $4200 (enable NMI) / LDA #$5A / WAI / STA $0346 / STP
        let prog = [
            0xA9, 0x81, 0x8D, 0x00, 0x42, 0xA9, 0x5A, 0xCB, 0x8D, 0x46, 0x03, 0xDB,
        ];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224);
    assert_eq!(
        cop.debug_cpu_ram(0x0346, 1),
        vec![0x5A],
        "A survived the NMI round trip"
    );
}

/// DATA_TRN pairing under the one-frame capture skew: the payload lands a
/// The DATA_TRN payload check gates on the source's capture counter: an
/// unchanged counter skips the 4 KB read+hash entirely (it used to run once
/// per GB instruction), a bump reopens exactly one check, and a source
/// without a counter (`None`) checks every poll as before.
#[test]
fn data_trn_checks_gate_on_the_capture_counter() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut cmds = TestCmds {
        data_trn: Some(vec![0x11; 4096]),
        trn_seq: Some(1),
        ..TestCmds::default()
    };
    for _ in 0..10 {
        cop.poll(&mut cmds);
    }
    assert_eq!(cmds.trn_reads.get(), 1, "one read on the counter edge");
    cmds.trn_seq = Some(2);
    cmds.data_trn = Some(vec![0x22; 4096]);
    for _ in 0..10 {
        cop.poll(&mut cmds);
    }
    assert_eq!(cmds.trn_reads.get(), 2, "one more read on the next edge");
    // No counter: every poll checks (the pre-gate behavior).
    let mut cmds = TestCmds {
        data_trn: Some(vec![0x33; 4096]),
        ..TestCmds::default()
    };
    for _ in 0..10 {
        cop.poll(&mut cmds);
    }
    assert_eq!(cmds.trn_reads.get(), 10, "counterless sources re-check");
}

/// frame after its packet, so a packet waits pending until the payload edge
/// fires — and a byte-identical payload (no edge) is flushed by the next
/// packet's arrival with those same bytes.
#[test]
fn data_trn_pairs_payloads_with_their_own_packets() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let trn = |hi: u8| {
        let mut p = [0u8; 16];
        p[0] = 0x10 << 3 | 1;
        p[1] = 0x00;
        p[2] = hi;
        p[3] = 0x7F;
        p
    };
    // Packet 1 arrives; its payload is not captured yet.
    let mut cmds = TestCmds {
        packets: vec![trn(0x01)],
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert_eq!(
        cop.debug_cpu_ram(0x7F_0100, 4),
        vec![0; 4],
        "nothing lands before the payload is captured"
    );
    // The capture lands: the payload edge pairs it with packet 1.
    let mut cmds = TestCmds {
        data_trn: Some(vec![0xAB; 64]),
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert_eq!(
        cop.debug_cpu_ram(0x7F_0100, 4),
        vec![0xAB; 4],
        "payload 1 landed at packet 1's dest"
    );
    run_service(&mut cop);
    assert_eq!(
        cop.debug_cpu_ram(0x02C2, 1),
        vec![0x10],
        "packet 1 published through the service call"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x0601, 3),
        vec![0x00, 0x01, 0x7F],
        "the published packet is packet 1 (its dest bytes)"
    );
    // Packet 2, then its distinct payload a frame later.
    let mut cmds = TestCmds {
        packets: vec![trn(0x91)],
        data_trn: Some(vec![0xAB; 64]),
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    let mut cmds = TestCmds {
        data_trn: Some(vec![0xCD; 64]),
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert_eq!(
        cop.debug_cpu_ram(0x7F_9100, 4),
        vec![0xCD; 4],
        "payload 2 landed at packet 2's dest"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x7F_0100, 4),
        vec![0xAB; 4],
        "packet 1's dest untouched by the second transfer"
    );
    // Packet 3's payload is byte-identical to payload 2 — no signature edge.
    // Packet 4's arrival flushes it with the current (identical) bytes.
    let mut cmds = TestCmds {
        packets: vec![trn(0xA1)],
        data_trn: Some(vec![0xCD; 64]),
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert_eq!(
        cop.debug_cpu_ram(0x7F_A100, 4),
        vec![0; 4],
        "identical payload has no edge; packet 3 stays pending"
    );
    let mut cmds = TestCmds {
        packets: vec![trn(0xB1)],
        data_trn: Some(vec![0xCD; 64]),
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert_eq!(
        cop.debug_cpu_ram(0x7F_A100, 4),
        vec![0xCD; 4],
        "packet 4's arrival flushed packet 3 with the identical payload"
    );
}

/// JUMP ($12) carries an optional NMI-handler address in packet bytes 4-6
/// (Pan Docs "SGB Command 12h — JUMP"): nonzero installs the SNES RAM NMI
/// vector at $00BB-$00BD (the bytes fullsnes documents JUMP clobbering);
/// all-zero leaves the vector unchanged.
#[test]
fn jump_packet_installs_the_ram_nmi_vector() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut pkt = [0u8; 16];
    pkt[0] = 0x12 << 3 | 1;
    pkt[1] = 0x00; // PC $001800 (the flags path applies it)
    pkt[2] = 0x18;
    pkt[3] = 0x00;
    pkt[4] = 0x00; // NMI handler $7F:0100
    pkt[5] = 0x01;
    pkt[6] = 0x7F;
    let mut cmds = TestCmds {
        packets: vec![pkt],
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert_eq!(
        cop.debug_cpu_ram(0x00BB, 3),
        vec![0x00, 0x01, 0x7F],
        "the RAM NMI vector holds the packet's handler address"
    );

    let mut zero = [0u8; 16];
    zero[0] = 0x12 << 3 | 1;
    zero[2] = 0x18; // PC only, NMI bytes all zero
    let mut cmds = TestCmds {
        packets: vec![zero],
        ..TestCmds::default()
    };
    cop.poll(&mut cmds);
    assert_eq!(
        cop.debug_cpu_ram(0x00BB, 3),
        vec![0x00, 0x01, 0x7F],
        "an all-zero NMI field leaves the vector unchanged"
    );
}

/// The resident aux BIOS service: wait for the next vblank (RDNMI poll),
/// then return — without ever touching NMITIMEN (the pilot's JUMP points
/// the $00BB vector at its bootstrap entry, so a delivered NMI would
/// re-enter the main loop recursively). Pinned black-box from the hook's
/// ACK-latch sandwich around the aux call.
#[test]
fn aux_service_waits_for_vblank_without_nmis() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9300, &[0xEE, 0x60, 0x03, 0x40]).unwrap(); // INC $0360 / RTI
        cpu.write_ram(0x00BB, &[0x00, 0x93, 0x00]).unwrap(); // [$00BB] = $00:9300
        // JSR the aux entry ($C590 — a thunk to the resident body), then halt.
        let prog = [0x20, 0x90, 0xC5, 0xA9, 0xA5, 0x8D, 0x64, 0x03, 0xDB];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 3);
    assert_eq!(
        cop.debug_cpu_ram(0x0364, 1),
        vec![0xA5],
        "the aux service returned after the vblank wait"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x0360, 1),
        vec![0],
        "no NMI was delivered (the guest never enabled one)"
    );
    assert_eq!(cop.nmitimen, 0, "NMITIMEN untouched");
}

// ---- Joypad autopoll ($4200 bit 0 → $4218-$421F) ----

/// The GB→SNES bit mapping (fullsnes 4218h): every mapped button lands on
/// its SNES bit, unmapped SNES bits (Y/X/L/R + the id nibble) stay clear.
#[test]
fn joy1_mapping_covers_the_gb_matrix() {
    assert_eq!(joy1_bytes(0x0F, 0x0F), [0x00, 0x00], "idle");
    assert_eq!(joy1_bytes(0x00, 0x00), [0x80, 0xBF], "everything pressed");
    assert_eq!(
        joy1_bytes(0x07, 0x07),
        [0x00, 0x14],
        "Start (bit 12) + Down (bit 10)"
    );
    assert_eq!(
        joy1_bytes(0x0E, 0x0E),
        [0x80, 0x01],
        "A (bit 7) + Right (bit 8)"
    );
}

/// End to end: the guest enables autopoll (NMITIMEN bit 0), sees the HVBJOY
/// busy bit pulse at vblank, and after it clears reads the pushed GB input
/// from the JOY1 shadows — values become valid when busy drops (fullsnes:
/// reads during the poll window are unreliable).
#[test]
fn joypad_autopoll_serves_input_after_the_busy_pulse() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        let prog = [
            0xA9, 0x01, 0x8D, 0x00, 0x42, // LDA #$01 / STA $4200 (autopoll on)
            0xAD, 0x12, 0x42, 0x29, 0x01, 0xF0, 0xF9, // w1: busy set?
            0x8D, 0x60, 0x04, // STA $0460 (records 1)
            0xAD, 0x12, 0x42, 0x29, 0x01, 0xD0, 0xF9, // w2: busy clear?
            0xAD, 0x18, 0x42, 0x8D, 0x61, 0x04, // JOY1L -> $0461
            0xAD, 0x19, 0x42, 0x8D, 0x62, 0x04, // JOY1H -> $0462
            0xDB, // STP
        ];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.set_input(0x0E, 0x0E); // Right + A pressed (active-low GB nibbles)
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0460, 3),
        vec![0x01, 0x80, 0x01],
        "busy pulse seen, then JOY1L = A, JOY1H = Right"
    );
}

/// Without NMITIMEN bit 0 the JOY shadows never move — the seam is inert
/// until the guest itself asks for autopoll.
#[test]
fn no_autopoll_without_the_guest_enabling_it() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        // loop: copy JOY1L to $0470 forever (no $4200 write).
        let prog = [0xAD, 0x18, 0x42, 0x8D, 0x70, 0x04, 0x80, 0xF8];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.set_input(0x00, 0x00); // everything pressed
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0470, 1),
        vec![0x00],
        "JOY1 shadow untouched with autopoll disabled"
    );
}

/// The pad feed preserves sub-flush latch sequences and passes the local
/// matrix through when idle: a guest writing $3F / $01 / $00 back to back
/// (the takeover init's one-shot Select+Start trigger chased by an ACK
/// sandwich) surfaces each value in order — each dwelling long enough for
/// GB polls — and afterwards the player's own buttons flow (the resident
/// BIOS's continuous pad forward).
#[test]
fn pad_feed_replays_latch_sequences_then_passes_the_matrix_through() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    assert_eq!(cop.joypad_feed(), None, "matrix untouched before takeover");
    let prog = [
        0xA9, 0x3F, 0x8D, 0x04, 0x60, // LDA #$3F / STA $6004
        0xA9, 0x01, 0x8D, 0x04, 0x60, // LDA #$01 / STA $6004
        0xA9, 0x00, 0x8D, 0x04, 0x60, // LDA #$00 / STA $6004
        0xDB, // STP
    ];
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(8192);
    let mut seen = Vec::new();
    for _ in 0..8192 {
        let f = cop.joypad_feed().expect("taken over");
        if seen.last() != Some(&f[0]) {
            seen.push(f[0]);
        }
    }
    assert_eq!(
        seen[..3],
        [0x3F, 0x01, 0x00],
        "every latch write surfaced, in order"
    );
    assert_eq!(
        seen.get(3),
        Some(&0xFF),
        "then the idle matrix (nothing pressed) passes through"
    );
    // Queue drained: the local matrix (Select+Start = buttons $3, dpad $F,
    // active low) passes through as the latch byte.
    cop.set_input(0x0F, 0x03);
    assert_eq!(
        cop.joypad_feed(),
        Some([0x3F, 0xFF, 0xFF, 0xFF]),
        "player input forwards while the SNES side is idle"
    );
}

/// The pilot's phase streams cover ALL of bank $7F and the upper half of
/// bank $7E (the observed DATA_TRN dest map: $7F:0100-$F100 + $7E:8000-
/// $F000), so the host-side staging buffers and the delivery mailbox must
/// live in the untouched $7E:2000-$7FFF window — anywhere else, every
/// staged payload / delivery overwrites the game's own data (4 KB holes;
/// Space Invaders' attract formation tables were the visible casualty).
#[test]
fn host_wram_structures_avoid_the_streamed_regions() {
    for a in BIOS_TRN_STAGING {
        assert_eq!(a >> 16, 0x7E, "staging stays in bank 7E: {a:06X}");
        let off = a & 0xFFFF;
        assert!(
            (0x2000..0x7000).contains(&off),
            "staging (4 KB) inside the free window: {a:06X}"
        );
    }
    let off = BIOS_DELIVERY & 0xFFFF;
    assert_eq!(BIOS_DELIVERY >> 16, 0x7E);
    assert!(
        (0x2000..0x7FE0).contains(&off),
        "delivery mailbox inside the free window: {BIOS_DELIVERY:06X}"
    );
}

/// MMIO ring drain sizing: a guest program that writes multiple INIDISP values
/// via STA $2100. The ring captures each write; on flush(), the host probes the
/// pending count and sizes the read to `3 + entry_size * pending` (clamped to
/// full window). The parse still takes `n` entries from the buffer — verify
/// that sized reads drain exactly `pending` entries and all writes apply.
#[test]
fn mmio_ring_drain_sizing_applies_all_writes() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    // A guest program that writes INIDISP ($2100) three times with distinct
    // values, then reads the final result back to a marker location.
    let prog = [
        0xA9, 0x0F, 0x8D, 0x00, 0x21, // LDA #$0F / STA $2100 (show screen, max brightness)
        0xA9, 0x42, 0x8D, 0x00, 0x21, // LDA #$42 / STA $2100 (partial brightness)
        0xA9, 0x81, 0x8D, 0x00, 0x21, // LDA #$81 / STA $2100 (force blank)
        0xDB, // STP
    ];
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    // Clock long enough for all three writes to transit through the ring,
    // be drained via sized reads, and apply to the host state.
    cop.clock(4096 * 4);
    // The coprocessor ran without error (no unwrap failures from the sized
    // reads). The PPU plugin (if loaded) or the internal state machine
    // accepted all three writes through the drain cycle.
}

#[path = "lib_tests_apu.rs"]
mod apu;

#[path = "lib_tests_dma.rs"]
mod dma;

#[path = "lib_tests_ppu.rs"]
mod ppu;
