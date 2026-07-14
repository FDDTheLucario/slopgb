//! Echo unit tests: FIR passthrough, EVOL scaling, and the ECEN write-disable.

use super::*;

fn regs(fir: [i8; 8], write_disabled: bool) -> EchoRegs {
    EchoRegs {
        esa: 0x40, // echo buffer at 0x4000
        edl: 1,    // 2 KiB ring
        efb: 0,
        evol_l: 127,
        evol_r: 127,
        fir,
        write_disabled,
    }
}

#[test]
fn silent_buffer_produces_silence() {
    let mut ram = Box::new([0u8; 0x1_0000]);
    let mut echo = Echo::default();
    let out = echo.process(&mut ram, &regs([0; 8], false), (0, 0));
    assert_eq!(out, (0, 0));
}

#[test]
fn fir_passes_the_newest_tap_scaled_by_evol() {
    // Put a constant sample in the whole echo region; only FIR7 (newest) is
    // nonzero, so after one read fir_l = value * 64 >> 7 = value/2, and the
    // output is (value/2) * EVOL/128 ≈ value/2 (EVOL=127).
    let mut ram = Box::new([0u8; 0x1_0000]);
    for a in 0x4000..0x4800 {
        // 0x0400 = 1024 little-endian
        ram[a] = if a % 2 == 0 { 0x00 } else { 0x04 };
    }
    let mut echo = Echo::default();
    let out = echo.process(&mut ram, &regs([0, 0, 0, 0, 0, 0, 0, 64], false), (0, 0));
    // value = 0x0400 = 1024; fir = 1024*64>>7 = 512; out = 512*127>>7 = 508.
    assert_eq!(out.0, 508);
    assert_eq!(out.1, 508);
}

#[test]
fn write_disable_suppresses_buffer_writes() {
    let mut ram = Box::new([0u8; 0x1_0000]);
    let mut echo = Echo::default();
    // With writes enabled the echo bus is written into the buffer.
    echo.process(&mut ram, &regs([0; 8], false), (5000, -5000));
    let wrote = ram[0x4000] != 0 || ram[0x4001] != 0;
    assert!(wrote, "echo buffer should have been written");

    // With ECEN set the buffer stays untouched.
    let mut ram2 = Box::new([0u8; 0x1_0000]);
    let mut echo2 = Echo::default();
    echo2.process(&mut ram2, &regs([0; 8], true), (5000, -5000));
    assert_eq!(ram2[0x4000], 0);
    assert_eq!(ram2[0x4001], 0);
}

#[test]
fn buffer_wraps_at_the_edl_length() {
    // EDL 1 -> 2 KiB = 512 stereo samples. After 512 processes the offset
    // returns to 0, so sample 0 is revisited.
    let mut ram = Box::new([0u8; 0x1_0000]);
    let mut echo = Echo::default();
    let r = regs([0; 8], false);
    for i in 0..512i32 {
        // Distinct echo-bus value each step so we can detect the wrap write.
        echo.process(&mut ram, &r, (i, 0));
    }
    // The 513th process wraps back to offset 0 and overwrites it with 1000.
    echo.process(&mut ram, &r, (1000, 0));
    let v = i16::from_le_bytes([ram[0x4000], ram[0x4001]]);
    assert_eq!(v, 1000);
}
