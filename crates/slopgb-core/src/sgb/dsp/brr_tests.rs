//! BRR decode tests. Where a value is "known", it is computed by hand from the
//! Blargg decode rules this module implements; the filter tests additionally
//! check the decoded slope against the documented float coefficients.

use super::*;

fn ram_with(block: &[u8], at: u16) -> Box<[u8; 0x1_0000]> {
    let mut ram = Box::new([0u8; 0x1_0000]);
    for (i, &b) in block.iter().enumerate() {
        ram[at as usize + i] = b;
    }
    ram
}

#[test]
fn silent_block_decodes_to_silence() {
    // header shift=0 filter=0, all-zero nibbles → all-zero samples.
    let ram = ram_with(&[0x00; 9], 0x0200);
    let (mut p1, mut p2) = (0, 0);
    let blk = decode_block(&ram, 0x0200, &mut p1, &mut p2);
    assert_eq!(blk.samples, [0i16; 16]);
    assert_eq!(p1, 0);
    assert_eq!(p2, 0);
}

#[test]
fn filter0_applies_shift_only() {
    // shift=4, filter=0. Each nibble n decodes to (n<<4)>>1 then *2 == n<<4.
    // Nibbles: 1,2,3,4,... alternating high/low.
    let mut block = [0u8; 9];
    block[0] = 0x40; // shift 4, filter 0
    block[1] = 0x12; // nibbles 1, 2
    block[2] = 0x34; // 3, 4
    // remaining bytes 0 -> nibbles 0
    let ram = ram_with(&block, 0x0200);
    let (mut p1, mut p2) = (0, 0);
    let blk = decode_block(&ram, 0x0200, &mut p1, &mut p2);
    assert_eq!(blk.samples[0], 16); // 1 << 4
    assert_eq!(blk.samples[1], 32); // 2 << 4
    assert_eq!(blk.samples[2], 48); // 3 << 4
    assert_eq!(blk.samples[3], 64); // 4 << 4
    assert_eq!(blk.samples[4], 0);
}

#[test]
fn negative_nibbles_sign_extend() {
    // shift=4, filter=0, nibble 0xF = -1 -> (-1<<4)>>1 = -8, *2 = -16.
    let mut block = [0u8; 9];
    block[0] = 0x40;
    block[1] = 0xF8; // nibbles -1, -8
    let ram = ram_with(&block, 0x0200);
    let (mut p1, mut p2) = (0, 0);
    let blk = decode_block(&ram, 0x0200, &mut p1, &mut p2);
    assert_eq!(blk.samples[0], -16); // -1 << 4
    assert_eq!(blk.samples[1], -128); // -8 << 4
}

#[test]
fn filter1_predicts_from_previous_sample() {
    // filter 1 ≈ 0.9375 * p1. Prime p1 with a large value, decode a block of
    // zero deltas (filter 1) and check the output decays by ~15/16 each step.
    let mut block = [0u8; 9];
    block[0] = 0x04; // shift 0, filter 1
    let ram = ram_with(&block, 0x0200);
    let (mut p1, mut p2) = (0x2000, 0);
    let blk = decode_block(&ram, 0x0200, &mut p1, &mut p2);
    // First output ≈ 0.9375 * 0x2000 = 0x1E00 (7680), within rounding.
    let expected = (f64::from(0x2000) * 0.9375) as i16;
    assert!((blk.samples[0] - expected).abs() <= 4, "{} vs {expected}", blk.samples[0]);
    // Monotone decay toward zero.
    assert!(blk.samples[1].abs() < blk.samples[0].abs());
    assert!(blk.samples[15].abs() < blk.samples[0].abs());
}

#[test]
fn end_and_loop_flags_are_parsed() {
    let ram = ram_with(&[0x03, 0, 0, 0, 0, 0, 0, 0, 0], 0x0200); // loop|end
    let (mut p1, mut p2) = (0, 0);
    let blk = decode_block(&ram, 0x0200, &mut p1, &mut p2);
    assert!(blk.end_flag);
    assert!(blk.loop_flag);

    let ram = ram_with(&[0x00, 0, 0, 0, 0, 0, 0, 0, 0], 0x0200);
    let blk = decode_block(&ram, 0x0200, &mut p1, &mut p2);
    assert!(!blk.end_flag);
    assert!(!blk.loop_flag);
}

#[test]
fn predictor_history_threads_out() {
    // After decoding, p1/p2 hold the last two decoded samples.
    let mut block = [0u8; 9];
    block[0] = 0x40; // shift 4, filter 0
    block[8] = 0x0A; // last byte: nibbles 0, 0xA(-6) -> samples[14]=0, samples[15]=-6<<4
    let ram = ram_with(&block, 0x0200);
    let (mut p1, mut p2) = (0, 0);
    let blk = decode_block(&ram, 0x0200, &mut p1, &mut p2);
    assert_eq!(p1 as i16, blk.samples[15]);
    assert_eq!(p2 as i16, blk.samples[14]);
}
