use super::*;

#[test]
fn encode_wav_writes_a_valid_header_and_pcm() {
    let frames = [(0.0f32, 0.0f32), (1.0, -1.0)];
    let wav = encode_wav(&frames, 32768);
    // RIFF/WAVE magic + fmt/data chunk ids.
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(&wav[36..40], b"data");
    // 44-byte header + 2 frames × 4 bytes.
    assert_eq!(wav.len(), 44 + 2 * 4);
    // Sample rate is stored little-endian at offset 24.
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 32768);
    // Second frame: L=+1.0 -> i16::MAX, R=-1.0 -> -i16::MAX (clamped).
    let l = i16::from_le_bytes(wav[48..50].try_into().unwrap());
    let r = i16::from_le_bytes(wav[50..52].try_into().unwrap());
    assert_eq!(l, i16::MAX);
    assert_eq!(r, -i16::MAX);
}

#[test]
fn encode_wav_clamps_out_of_range_samples() {
    let wav = encode_wav(&[(2.0, -2.0)], 44100);
    let l = i16::from_le_bytes(wav[44..46].try_into().unwrap());
    let r = i16::from_le_bytes(wav[46..48].try_into().unwrap());
    assert_eq!(l, i16::MAX, "clamps to +full scale");
    assert_eq!(r, -i16::MAX, "clamps to -full scale");
}
