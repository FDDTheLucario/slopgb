use super::*;

#[test]
fn frame_duration_matches_hardware_rate() {
    // 70224 / 4194304 s = 16.742706... ms
    assert_eq!(FRAME_DURATION.as_nanos(), 16_742_706);
}
