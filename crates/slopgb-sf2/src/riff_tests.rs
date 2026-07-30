use super::*;

#[test]
fn round_trips_a_simple_chunk() {
    let chunk = write_chunk(b"TEST", &[1, 2, 3]);
    // id(4) + len(4) + 3 bytes body + 1 pad byte = 12
    assert_eq!(chunk.len(), 12);
    let parsed = parse_chunks(&chunk).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(&parsed[0].id, b"TEST");
    assert_eq!(parsed[0].data, &[1, 2, 3]);
}

#[test]
fn round_trips_an_even_length_chunk_with_no_pad() {
    let chunk = write_chunk(b"TEST", &[1, 2, 3, 4]);
    assert_eq!(chunk.len(), 12); // no pad byte needed
    let parsed = parse_chunks(&chunk).unwrap();
    assert_eq!(parsed[0].data, &[1, 2, 3, 4]);
}

#[test]
fn parses_sibling_chunks_in_sequence() {
    let mut buf = write_chunk(b"AAAA", &[1]);
    buf.extend(write_chunk(b"BBBB", &[2, 3]));
    let parsed = parse_chunks(&buf).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(&parsed[0].id, b"AAAA");
    assert_eq!(&parsed[1].id, b"BBBB");
    assert_eq!(parsed[1].data, &[2, 3]);
}

#[test]
fn list_chunk_splits_form_and_body() {
    let list = write_list(b"INFO", &write_chunk(b"INAM", b"hi"));
    let parsed = parse_chunks(&list).unwrap();
    assert_eq!(&parsed[0].id, b"LIST");
    let (form, body) = form_and_body(parsed[0].data).unwrap();
    assert_eq!(&form, b"INFO");
    let sub = parse_chunks(body).unwrap();
    assert_eq!(&sub[0].id, b"INAM");
    assert_eq!(read_cstr(sub[0].data), "hi");
}

#[test]
fn truncated_header_errors() {
    assert!(parse_chunks(&[1, 2, 3]).is_err());
}

#[test]
fn overrunning_length_errors() {
    let mut buf = b"TEST".to_vec();
    buf.extend_from_slice(&100u32.to_le_bytes()); // claims 100 bytes, has 0
    assert!(parse_chunks(&buf).is_err());
}

#[test]
fn fixed_str_truncates_and_pads() {
    let mut out = Vec::new();
    write_fixed_str(&mut out, "hi", 5);
    assert_eq!(out, [b'h', b'i', 0, 0, 0]);
    let mut out2 = Vec::new();
    write_fixed_str(&mut out2, "toolongname", 5);
    assert_eq!(out2, b"toolo");
}
