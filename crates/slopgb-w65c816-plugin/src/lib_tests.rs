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
    cop.write_ram(u32::from(PROG_ORG), &prog);
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

/// A comm port maps only at `$2140-$2143` in the system banks; `$7E:2141`
/// is WRAM, not a port (A22=1 — the I/O windows don't decode there).
#[test]
fn port_window_is_exact() {
    assert_eq!(SnesBus::port_index(0x2140), Some(0));
    assert_eq!(SnesBus::port_index(0x2143), Some(3));
    assert_eq!(SnesBus::port_index(0x2144), None);
    assert_eq!(SnesBus::port_index(0x213F), None);
    assert_eq!(SnesBus::port_index(0x80_2140), Some(0), "WS2 system bank");
    assert_eq!(SnesBus::port_index(0x7E_2141), None, "WRAM bank: no ports");
    assert_eq!(SnesBus::port_index(0x40_2140), None, "HiROM bank: no ports");
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
    cop.write_ram(u32::from(PROG_ORG), &prog);
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

/// Below the host window, write_ram/read_ram keep their raw-memory meaning:
/// installs land in real memory (WRAM / the program area), and a raw install
/// never triggers a bus side effect (I/O space is simply dropped).
#[test]
fn ram_install_below_window_stays_raw() {
    let mut cop = W65816Cop::new();
    cop.write_ram(0x0200, &[0xAA, 0xBB]);
    assert_eq!(cop.read_ram(0x0200, 2), vec![0xAA, 0xBB]);
    cop.write_ram(0x9100, &[0xCC]);
    assert_eq!(cop.read_ram(0x9100, 1), vec![0xCC]);
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

// ---- The real memory map (fullsnes "SNES Memory Map") ----

/// 128 KB WRAM at $7E-$7F with the bank-0/$80 low-8K mirror; $7F does not
/// alias bank 0 (the pilot's DATA_TRN target depends on it).
#[test]
fn wram_banks_are_distinct_and_bank0_mirrors_7e() {
    let mut cop = W65816Cop::new();
    cop.write_ram(0x7F_0100, &[0xAA]);
    cop.write_ram(0x7E_0100, &[0xBB]);
    assert_eq!(cop.read_ram(0x7F_0100, 1), vec![0xAA], "7F unaliased");
    assert_eq!(cop.read_ram(0x7E_0100, 1), vec![0xBB]);
    assert_eq!(cop.read_ram(0x0100, 1), vec![0xBB], "bank-0 mirror of 7E");
    assert_eq!(
        cop.read_ram(0x80_0100, 1),
        vec![0xBB],
        "bank-80 mirror of 7E"
    );
    cop.write_ram(0x1FFF, &[0xCC]);
    assert_eq!(
        cop.read_ram(0x7E_1FFF, 1),
        vec![0xCC],
        "mirror writes through"
    );
    cop.write_ram(0x7E_2100, &[0xDD]);
    assert_eq!(
        cop.read_ram(0x2100, 1),
        vec![0x00],
        "the mirror covers only the first 8 KB"
    );
}

/// The RAM-backed program area at $8000-$FFFF aliases across the system
/// banks (one 32 KB image — the SGB BIOS area slopgb never ships, writable
/// so the host installs firmware there); HiROM banks are open bus.
#[test]
fn program_area_aliases_across_system_banks_only() {
    let mut cop = W65816Cop::new();
    cop.write_ram(0x9000, &[0x42]);
    assert_eq!(
        cop.read_ram(0x80_9000, 1),
        vec![0x42],
        "WS2 system-bank alias"
    );
    assert_eq!(cop.read_ram(0x3F_9000, 1), vec![0x42]);
    assert_eq!(
        cop.read_ram(0x40_9000, 1),
        vec![0x00],
        "HiROM bank: open bus"
    );
    cop.write_ram(0x40_9000, &[0x55]);
    assert_eq!(
        cop.read_ram(0x40_9000, 1),
        vec![0x00],
        "open-bus write dropped"
    );
}

/// I/O space is not memory: a raw install into $2000-$7FFF (ports, ICD2,
/// expansion) is dropped, never silently landed in some backing store.
#[test]
fn io_space_raw_installs_are_dropped() {
    let mut cop = W65816Cop::new();
    cop.write_ram(0x4200, &[0x99]);
    assert_eq!(cop.read_ram(0x4200, 1), vec![0x00]);
    cop.write_ram(0x6100, &[0x99]);
    assert_eq!(cop.read_ram(0x6100, 1), vec![0x00]);
}

/// The pilot's upload: 4 KB at $7F:0100 lands whole and does not stomp the
/// bank-0 mirror (which is $7E) — the exact corruption the flat map had.
#[test]
fn data_trn_block_lands_unaliased_at_7f0100() {
    let mut cop = W65816Cop::new();
    let block: Vec<u8> = (0..4096).map(|i| (i % 253) as u8).collect();
    cop.write_ram(0x7F_0100, &block);
    assert_eq!(cop.read_ram(0x7F_0100, 4096), block);
    assert_eq!(
        cop.read_ram(0x0100, 1),
        vec![0x00],
        "bank-0 mirror untouched"
    );
}

/// The CPU reaches high WRAM through the bus (24-bit long addressing).
#[test]
fn cpu_reads_wram_banks_via_bus() {
    let mut cop = W65816Cop::new();
    cop.write_ram(0x7F_0180, &[0x5D]);
    // LDA $7F0180 (long); STA $2140; STP
    let prog = [0xAF, 0x80, 0x01, 0x7F, 0x8D, 0x40, 0x21, 0xDB];
    cop.write_ram(0x9000, &prog);
    cop.set_pc(0x9000);
    cop.run_until(500);
    assert!(cop.cpu.stopped);
    assert_eq!(cop.port_read(0), 0x5D, "high-WRAM byte crossed out");
}
