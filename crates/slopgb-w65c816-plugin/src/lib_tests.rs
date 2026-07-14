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
