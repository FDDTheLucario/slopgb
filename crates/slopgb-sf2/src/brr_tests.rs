use super::*;

/// The clean-room coprocessor's built-in square-wave BRR block
/// (`crates/slopgb-sgb-coprocessor/src/lib.rs`, `spc_firmware`): header
/// `0x93` = shift 9, filter 0, loop + end set; eight `+7` nibbles then eight
/// `-8` nibbles. Filter 0 / shift 9 decodes each nibble to `(nib << 9) >> 1 =
/// nib << 8`, doubled at the end: `+7 -> 3584`, `-8 -> -4096`.
#[test]
fn decodes_known_square_block() {
    let brr = [0x93u8, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88];
    let decoded = decode(&brr, 0).expect("well-formed block decodes");
    let mut expected = [3584i16; 16];
    expected[8..].fill(-4096);
    assert_eq!(decoded.pcm.as_slice(), &expected[..]);
    assert!(decoded.loops);
}

/// A silent (all-zero) block round-trips exactly with zero error, and the
/// encoder must pick shift 0 / filter 0 (the only combination that can
/// reproduce silence exactly at any shift, and the cheapest).
#[test]
fn silent_block_round_trips_exactly() {
    let pcm = [0i16; 16];
    let enc = encode(&pcm, None);
    assert_eq!(enc.bytes.len(), 9);
    let header = enc.bytes[0];
    assert_eq!(header >> 4, 0, "silence should pick shift 0");
    assert_eq!((header >> 2) & 3, 0, "first block is always filter 0");
    let decoded = decode(&enc.bytes, 0).unwrap();
    assert_eq!(decoded.pcm, pcm);
}

/// A constant DC block (not just zero) also round-trips exactly, for a
/// value the 4-bit-mantissa BRR format can represent exactly (a power of
/// two — `nib=1` at some shift decodes to exactly `2^shift`). An arbitrary
/// DC value (e.g. 1000) is NOT guaranteed exact: BRR's 16 discrete mantissa
/// steps per shift cannot hit every integer.
#[test]
fn dc_block_round_trips_exactly() {
    let pcm = [1024i16; 16];
    let enc = encode(&pcm, None);
    let decoded = decode(&enc.bytes, 0).unwrap();
    assert_eq!(decoded.pcm, pcm);
}

/// RMS round-trip on a synthetic tone (sine + ramp). Threshold: RMS error
/// must be below 4% of full scale (`32768`) for a smooth, band-limited
/// signal — BRR's 4-bit-per-sample ADPCM is lossy by design; 4% is a loose
/// but meaningful bound (a broken encoder/decoder pairing produces errors an
/// order of magnitude larger).
#[test]
fn sine_ramp_round_trip_rms_under_threshold() {
    const N: usize = 16 * 40; // 40 BRR blocks
    let mut pcm = [0i16; N];
    for (i, s) in pcm.iter_mut().enumerate() {
        let t = i as f64;
        let sine = (t * 0.09).sin() * 12000.0;
        let ramp = (i as f64 / N as f64 - 0.5) * 4000.0;
        *s = (sine + ramp) as i16;
    }
    let enc = encode(&pcm, None);
    let decoded = decode(&enc.bytes, 0).unwrap();
    assert_eq!(decoded.pcm.len(), pcm.len());

    let sum_sq: f64 = pcm
        .iter()
        .zip(decoded.pcm.iter())
        .map(|(&a, &b)| {
            let d = f64::from(a) - f64::from(b);
            d * d
        })
        .sum();
    let rms = (sum_sq / pcm.len() as f64).sqrt();
    let threshold = 0.04 * 32768.0;
    println!("BRR round-trip RMS error: {rms:.2} (threshold {threshold:.2}, {:.3}% of full scale)", rms / 32768.0 * 100.0);
    assert!(
        rms < threshold,
        "RMS error {rms} exceeded 4% of full scale ({threshold})"
    );
}

/// Loop point rounds to the nearest 16-sample block boundary and is clamped
/// into range.
#[test]
fn loop_point_rounds_to_block_boundary() {
    let pcm = vec![0i16; 48]; // 3 blocks
    let enc = encode(&pcm, Some(20)); // nearest boundary: (20+8)/16 = 1
    assert_eq!(enc.loop_block, Some(1));
    let enc = encode(&pcm, Some(1000)); // clamped to last block (2)
    assert_eq!(enc.loop_block, Some(2));
}

/// [`decode`] errors (does not panic) on a truncated chain.
#[test]
fn decode_errors_on_truncated_buffer() {
    let buf = [0x00u8; 5]; // header claims 9 bytes/block but buffer is shorter
    assert!(decode(&buf, 0).is_err());
}
