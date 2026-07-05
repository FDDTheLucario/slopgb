use super::*;

#[test]
fn scalars_round_trip_in_order() {
    let mut w = Writer::new();
    w.u8(0x12);
    w.u16(0x3456);
    w.u32(0x789A_BCDE);
    w.u64(0x0102_0304_0506_0708);
    w.bool(true);
    w.bool(false);
    w.bytes(&[0xAA, 0xBB, 0xCC]);
    let bytes = w.into_vec();

    let mut r = Reader::new(&bytes);
    assert_eq!(r.u8().unwrap(), 0x12);
    assert_eq!(r.u16().unwrap(), 0x3456);
    assert_eq!(r.u32().unwrap(), 0x789A_BCDE);
    assert_eq!(r.u64().unwrap(), 0x0102_0304_0506_0708);
    assert!(r.bool().unwrap());
    assert!(!r.bool().unwrap());
    let mut buf = [0u8; 3];
    r.bytes_into(&mut buf).unwrap();
    assert_eq!(buf, [0xAA, 0xBB, 0xCC]);
}

#[test]
fn little_endian_layout() {
    let mut w = Writer::new();
    w.u16(0x1234);
    assert_eq!(w.into_vec(), vec![0x34, 0x12], "u16 is little-endian");
}

#[test]
fn reading_past_the_end_is_truncated_not_a_panic() {
    let bytes = [0x01u8];
    let mut r = Reader::new(&bytes);
    assert_eq!(r.u8().unwrap(), 0x01);
    assert_eq!(r.u8(), Err(StateError::Truncated));
    // A wider read on an empty remainder is also Truncated, never a slice panic.
    let mut r = Reader::new(&bytes);
    assert_eq!(r.u32(), Err(StateError::Truncated));
    let mut dst = [0u8; 4];
    let mut r = Reader::new(&bytes);
    assert_eq!(r.bytes_into(&mut dst), Err(StateError::Truncated));
}

#[test]
fn state_error_displays() {
    // Each variant has a human message (shown by the UI on a failed load).
    for e in [
        StateError::Truncated,
        StateError::BadMagic,
        StateError::BadVersion,
        StateError::RomMismatch,
    ] {
        assert!(!e.to_string().is_empty());
    }
}
