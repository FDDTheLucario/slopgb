//! APU-chain tests: the SOUND command driving the 65C816 -> SPC700 ->
//! S-DSP firmware chain to audible output, the SOU_TRN game-driver upload,
//! and the SPC700 IPL upload protocol under both fair pacing and the
//! flooded-ring pacing the arcade takeover's loader produces.

use super::*;

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
/// The SPC700 boots its real IPL ROM, which speaks the documented upload
/// protocol (fullsnes "Uploader"): announce $AA/$BB, $CC kick with a dest
/// on ports 2/3 and a nonzero command on port 1, a per-byte index/ack
/// pump, then an entry command (port 1 = 0) jumping to the uploaded code
/// — whose own port write is visible after the jump. This is exactly the
/// protocol the pilot's arcade loader drives (its terminator header sends
/// command 0 with the entry address).
#[test]
fn spc_ipl_upload_protocol_round_trips() {
    let Some((spc_wasm, _)) = plugins() else {
        return;
    };
    let mut spc = slopgb_plugin_host::LoadedCoprocessor::load(&spc_wasm).unwrap();
    spc.reset().unwrap();
    let mut cyc = 0u64;
    let run = |spc: &mut slopgb_plugin_host::LoadedCoprocessor, cyc: &mut u64| {
        *cyc += 4_000;
        spc.run_until(*cyc).unwrap();
    };
    // Wait for the announce.
    run(&mut spc, &mut cyc);
    assert_eq!(spc.port_read(0).unwrap(), 0xAA, "ready low");
    assert_eq!(spc.port_read(1).unwrap(), 0xBB, "ready high");

    // Upload `MOV $F6,#$5A / BRA *` to $0300 per the documented sequence.
    let driver = [0x8F, 0x5A, 0xF6, 0x2F, 0xFE];
    spc.port_write(2, 0x00).unwrap(); // dest $0300
    spc.port_write(3, 0x03).unwrap();
    spc.port_write(1, 0x01).unwrap(); // command: transfer
    spc.port_write(0, 0xCC).unwrap(); // kick
    run(&mut spc, &mut cyc);
    assert_eq!(spc.port_read(0).unwrap(), 0xCC, "kick acknowledged");
    for (i, &b) in driver.iter().enumerate() {
        spc.port_write(1, b).unwrap();
        spc.port_write(0, i as u8).unwrap();
        run(&mut spc, &mut cyc);
        assert_eq!(spc.port_read(0).unwrap(), i as u8, "byte {i} acked");
    }
    // Entry command: kick = (index+2)|1, command 0, address = $0300.
    let kick = (driver.len() as u8 + 2) | 1;
    spc.port_write(2, 0x00).unwrap();
    spc.port_write(3, 0x03).unwrap();
    spc.port_write(1, 0x00).unwrap();
    spc.port_write(0, kick).unwrap();
    run(&mut spc, &mut cyc);
    assert_eq!(spc.port_read(0).unwrap(), kick, "entry acknowledged");
    run(&mut spc, &mut cyc);
    assert_eq!(
        spc.port_read(2).unwrap(),
        0x5A,
        "the uploaded driver runs (its port-2 marker visible)"
    );
}

/// The interleaved co-simulation must not let a flooded ring clobber
/// unconsumed port writes. The guest here blind-pumps a whole IPL upload
/// with no echo waits — the pacing a stale echo shadow lets the pilot's
/// loader reach — so hundreds of index writes land in one flush window.
/// Every byte must still arrive: each replayed event owes the SPC700 a
/// slice long enough to consume it (the IPL's per-byte pump is ~25 SPC
/// cycles), whatever the event rate.
#[test]
fn flooded_port_ring_uploads_without_clobbering() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    // Let the IPL reach its announce before the pump starts.
    cop.clock(70_224 * 2);
    {
        let mut spc = cop.spc.borrow_mut();
        assert_eq!(spc.port_read(0).unwrap(), 0xAA, "IPL announced");
    }
    // Kick a transfer to $0400, then blind-pump 600 bytes (data = index
    // XOR $A5) back to back — no echo waits anywhere.
    let mut prog = stores(
        &[
            (0x2142, 0x00),
            (0x2143, 0x04),
            (0x2141, 0x01),
            (0x2140, 0xCC),
        ],
        &[],
    );
    prog.extend_from_slice(&[
        0x18, 0xFB, // CLC / XCE -> native
        0xC2, 0x10, // REP #$10
        0xA2, 0x58, 0x02, // LDX #600
        0xA9, 0x00, // LDA #$00 (index)
        // loop:
        0x48, // PHA
        0x49, 0xA5, // EOR #$A5
        0x8D, 0x41, 0x21, // STA $2141 (data)
        0x68, // PLA
        0x8D, 0x40, 0x21, // STA $2140 (index)
        0x1A, // INC A
        0xCA, // DEX
        0xD0, 0xF2, // BNE loop
        0xDB, // STP
    ]);
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 4);
    let got = cop.spc.borrow_mut().read_ram(0x0400, 600).unwrap();
    let want: Vec<u8> = (0..600u32).map(|i| (i as u8) ^ 0xA5).collect();
    assert_eq!(got, want, "every blind-pumped byte reached APU RAM");
}
