//! Native tests for the 65C816 coprocessor wrapper: the Coprocessor logic is
//! target-independent, so these drive it directly (no wasm boundary). The
//! wasm-crossing proof is `slopgb-plugin-host`'s `w65c816_roundtrip`.

use super::*;

/// The built-in demo loop transforms host input (`port 1`) to `input + 7` on the
/// output (`port 0`), and a real reset clears the output latch.
#[test]
fn demo_echoes_input_plus_seven() {
    let mut cop = W65816Cop::new();
    cop.port_write(1, 0x10);
    // One loop is 15 cycles; 200 guarantees several completed iterations.
    let reached = cop.run_until(200);
    assert!(reached >= 200, "run_until reaches the target cycle");
    assert_eq!(cop.port_read(0), 0x17, "0x10 + 7 crossed back out");

    cop.port_write(1, 0x20);
    cop.run_until(cop.cycles + 200);
    assert_eq!(cop.port_read(0), 0x27, "tracks a new input");

    cop.reset();
    assert_eq!(cop.port_read(0), 0, "reset clears the output latch");
    assert_eq!(cop.cycles, 0, "reset clears the cycle counter");
}

/// The CPU's RAM read + write path (not just code fetch): store a byte to zero
/// page, read it back, publish it, then STP so `run_until` terminates.
#[test]
fn ram_round_trip_through_cpu() {
    let mut cop = W65816Cop::new();
    // LDA #$AB; STA $10; LDA #$00; LDA $10; STA $2140; STP
    let prog = [
        0xA9, 0xAB, 0x85, 0x10, 0xA9, 0x00, 0xA5, 0x10, 0x8D, 0x40, 0x21, 0xDB,
    ];
    let org = PROG_ORG as usize;
    cop.bus.ram[org..org + prog.len()].copy_from_slice(&prog);
    cop.cpu = Cpu::new();
    cop.cpu.regs.pc = PROG_ORG;
    cop.cycles = 0;

    let reached = cop.run_until(1000);
    assert!(cop.cpu.stopped, "STP halted the CPU");
    assert_eq!(
        reached, 1000,
        "idle span after STP still reaches the target"
    );
    assert_eq!(cop.port_read(0), 0xAB, "value survived the RAM round trip");
}

/// Out-of-range ports are ignored, not a panic (the ABI passes a raw `u8`).
#[test]
fn out_of_range_ports_are_inert() {
    let mut cop = W65816Cop::new();
    cop.port_write(9, 0xFF); // ignored
    assert_eq!(cop.port_read(9), 0);
    assert_eq!(cop.port_read(200), 0);
}

/// A comm port maps only at `$2140-$2143`; neighbouring addresses are RAM.
#[test]
fn port_window_is_exact() {
    assert_eq!(SnesBus::port_index(0x2140), Some(0));
    assert_eq!(SnesBus::port_index(0x2143), Some(3));
    assert_eq!(SnesBus::port_index(0x2144), None);
    assert_eq!(SnesBus::port_index(0x213F), None);
    // Banks alias, so the window is present in a non-zero bank too.
    assert_eq!(SnesBus::port_index(0x7E_2141), Some(1));
}

// ---- ICD2: bus routing + the host window ----

/// End to end through the real CPU: the host deposits a packet through the
/// window, the program reads the mailbox (clearing the flag) and answers on
/// a pad latch, and the host reads the latch + sticky flag back.
#[test]
fn icd2_bus_and_host_window_round_trip() {
    let mut cop = W65816Cop::new();
    let packet: [u8; 16] = core::array::from_fn(|i| 0xE0 + i as u8);
    cop.write_ram(HW_PACKET, &packet);
    assert_eq!(cop.read_ram(HW_PACKET, 1), vec![1], "flag raised");

    // LDA $7000; STA $6004; STP — read the packet header, publish it as the
    // player-1 pad latch, halt.
    let prog = [0xAD, 0x00, 0x70, 0x8D, 0x04, 0x60, 0xDB];
    let org = PROG_ORG as usize;
    cop.bus.ram[org..org + prog.len()].copy_from_slice(&prog);
    cop.cpu = Cpu::new();
    cop.cpu.regs.pc = PROG_ORG;
    cop.cycles = 0;
    cop.run_until(1000);
    assert!(cop.cpu.stopped, "STP halted the CPU");

    assert_eq!(
        cop.read_ram(HW_PACKET, 1),
        vec![0],
        "the CPU's $7000 read cleared the flag"
    );
    assert_eq!(
        cop.read_ram(HW_PADS, 5),
        vec![0xE0, 0xFF, 0xFF, 0xFF, 1],
        "pad latch + sticky written flag visible to the host"
    );
}

/// Below the host window, write_ram/read_ram keep their raw-memory meaning —
/// even inside the CPU-visible ICD2 address range (a raw install is not a
/// bus access).
#[test]
fn ram_install_below_window_stays_raw() {
    let mut cop = W65816Cop::new();
    cop.write_ram(0x6100, &[0xAA, 0xBB]);
    assert_eq!(cop.read_ram(0x6100, 2), vec![0xAA, 0xBB]);
}

/// The ICD2 block rides the plugin save state (mailbox, latches, control).
#[test]
fn icd2_state_round_trips() {
    let mut cop = W65816Cop::new();
    let packet = [0x5A; 16];
    cop.write_ram(HW_PACKET, &packet);
    cop.bus.icd2.cpu_write(0x6004, 0xEF);
    cop.bus.icd2.cpu_write(0x6003, 0x8A);
    let state = cop.save_state();
    assert_eq!(state.len(), STATE_LEN);

    let mut fresh = W65816Cop::new();
    fresh.load_state(&state);
    assert_eq!(fresh.read_ram(HW_PACKET, 1), vec![1]);
    assert_eq!(fresh.read_ram(HW_PADS, 5), vec![0xEF, 0xFF, 0xFF, 0xFF, 1]);
    assert_eq!(fresh.read_ram(HW_CONTROL, 1), vec![0x8A]);
    assert_eq!(fresh.bus.icd2.cpu_read(0x7003), 0x5A);
}
