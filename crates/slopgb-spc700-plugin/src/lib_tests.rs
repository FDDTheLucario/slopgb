//! Native tests for the SPC700 + S-DSP coprocessor wrapper. These drive the
//! Coprocessor logic directly (target-independent); the wasm-crossing proof is
//! `slopgb-plugin-host`'s `spc700_roundtrip`.

use super::*;

/// Clocking the coprocessor runs the real SPC700 IPL ROM, which emits the
/// documented `$AA`/`$BB` boot handshake on comm ports 0/1 — proof the CPU
/// executes real code through the Coprocessor interface.
#[test]
fn ipl_boot_emits_aa_bb() {
    let mut cop = Spc700Cop::new();
    let reached = cop.run_until(60_000);
    assert!(reached >= 60_000, "run_until clocks to the target");
    assert_eq!(cop.port_read(0), 0xAA, "IPL handshake byte 0");
    assert_eq!(cop.port_read(1), 0xBB, "IPL handshake byte 1");
}

/// The S-DSP is clocked one sample per 32 SPC cycles; the observability ports
/// report a sample count consistent with the cycles run.
#[test]
fn dsp_synthesizes_while_clocked() {
    let mut cop = Spc700Cop::new();
    let reached = cop.run_until(60_000);
    let samples = u64::from(cop.port_read(4)) | (u64::from(cop.port_read(5)) << 8);
    assert!(samples > 0, "the DSP produced samples");
    assert!(
        (reached / u64::from(DSP_PERIOD)).abs_diff(samples) <= 2,
        "sample count ~= cycles/32 (reached={reached}, samples={samples})"
    );
}

/// Reset returns to the power-on IPL state and clears the counters.
#[test]
fn reset_returns_to_power_on() {
    let mut cop = Spc700Cop::new();
    cop.run_until(60_000);
    cop.reset();
    assert_eq!(cop.cycles, 0);
    assert_eq!(cop.port_read(4), 0, "sample count cleared");
    // The IPL comes back up on a fresh clock window.
    cop.run_until(60_000);
    assert_eq!(cop.port_read(0), 0xAA);
}

/// A SNES-side port write lands in the APU input latch the SPC700 reads.
#[test]
fn snes_port_write_reaches_the_apu() {
    let mut cop = Spc700Cop::new();
    cop.port_write(2, 0x5A);
    // The APU-side input latch for port 2 now holds the written value.
    assert_eq!(cop.spc.apu_port_in(2), 0x5A);
    // Out-of-range writes/reads are inert.
    cop.port_write(9, 0xFF);
    assert_eq!(cop.port_read(9), 0);
}
