//! Voice tests: directory read + key-on, pitch advance, envelope gating, and
//! the end/loop → ENDX path.

use super::*;

/// Build APU RAM with a sample directory (entry 0 at `dir<<8`) pointing at a
/// BRR sample at `sample_addr`, and one block written there with `header` and
/// all nibbles = `nib`.
fn setup(dir: u8, sample_addr: u16, header: u8, nib: u8) -> Box<[u8; 0x1_0000]> {
    let mut ram = Box::new([0u8; 0x1_0000]);
    let d = (u32::from(dir) << 8) as usize;
    ram[d] = sample_addr as u8;
    ram[d + 1] = (sample_addr >> 8) as u8;
    ram[d + 2] = sample_addr as u8; // loop = start
    ram[d + 3] = (sample_addr >> 8) as u8;
    let s = sample_addr as usize;
    ram[s] = header;
    let byte = (nib << 4) | nib;
    for b in 1..9 {
        ram[s + b] = byte;
    }
    ram
}

#[test]
fn keyon_plays_a_constant_sample() {
    // filter 0, shift 4, loop+end; nibble 2 -> constant sample value 32.
    let ram = setup(0x02, 0x0210, 0x43, 2);
    let mut v = Voice::default();
    v.key_on(&ram, 0x02, 0);
    let mut endx = 0u8;
    let mut out = 0;
    for _ in 0..16 {
        // GAIN direct max (0x7F -> env 0x7F0), pitch 0x1000 = one sample/step.
        out = v.step(&ram, 0x1000, 0x00, 0x00, 0x7F, 0, 0, false, &mut endx, 0);
    }
    // Steady output ≈ 32 * 0x7F0 >> 11 = 31.
    assert!(out > 20 && out < 40, "steady output {out}");
}

#[test]
fn zero_envelope_is_silent() {
    let ram = setup(0x02, 0x0210, 0x43, 2);
    let mut v = Voice::default();
    v.key_on(&ram, 0x02, 0);
    let mut endx = 0u8;
    let mut out = 1;
    for _ in 0..16 {
        // GAIN direct 0 -> env 0.
        out = v.step(&ram, 0x1000, 0x00, 0x00, 0x00, 0, 0, false, &mut endx, 0);
    }
    assert_eq!(out, 0);
}

#[test]
fn key_on_startup_delay_mutes_first_samples() {
    let ram = setup(0x02, 0x0210, 0x43, 2);
    let mut v = Voice::default();
    v.key_on(&ram, 0x02, 0);
    let mut endx = 0u8;
    // The first sample after key-on is muted (startup pipeline).
    let first = v.step(&ram, 0x1000, 0x00, 0x00, 0x7F, 0, 0, false, &mut endx, 0);
    assert_eq!(first, 0);
}

#[test]
fn pitch_zero_does_not_advance_the_sample() {
    let ram = setup(0x02, 0x0210, 0x43, 2);
    let mut v = Voice::default();
    v.key_on(&ram, 0x02, 0);
    let mut endx = 0u8;
    // With pitch 0 the BRR read never advances, so the history stays all-zero
    // and the output is silence even past the startup delay.
    let mut out = 1;
    for _ in 0..16 {
        out = v.step(&ram, 0, 0x00, 0x00, 0x7F, 0, 0, false, &mut endx, 0);
    }
    assert_eq!(out, 0);
}

#[test]
fn end_without_loop_sets_endx_and_mutes() {
    // header 0x41: shift 4, filter 0, end, NO loop.
    let ram = setup(0x02, 0x0210, 0x41, 2);
    let mut v = Voice::default();
    v.key_on(&ram, 0x02, 0);
    let mut endx = 0u8;
    // Consume well past the 16-sample block so the end flag is hit.
    for _ in 0..40 {
        v.step(&ram, 0x1000, 0x00, 0x00, 0x7F, 0, 0, false, &mut endx, 5);
    }
    assert_eq!(endx & (1 << 5), 1 << 5, "ENDX bit 5 should be set");
    let out = v.step(&ram, 0x1000, 0x00, 0x00, 0x7F, 0, 0, false, &mut endx, 5);
    assert_eq!(out, 0, "muted after end-without-loop");
}

#[test]
fn noise_source_overrides_brr() {
    let ram = setup(0x02, 0x0210, 0x43, 0); // silent BRR (nibble 0)
    let mut v = Voice::default();
    v.key_on(&ram, 0x02, 0);
    let mut endx = 0u8;
    let mut out = 0;
    for _ in 0..8 {
        // use_noise = true, noise sample 10000 -> output tracks noise, not BRR.
        out = v.step(&ram, 0x1000, 0x00, 0x00, 0x7F, 0, 10000, true, &mut endx, 0);
    }
    assert!(
        out > 100,
        "noise-sourced output should be nonzero, got {out}"
    );
}
