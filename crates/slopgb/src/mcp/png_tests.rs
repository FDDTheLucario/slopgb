use super::*;

const MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Walk the chunk list, verifying each chunk's CRC-32, and return the
/// concatenated IDAT payload + the (w,h) from IHDR.
fn chunks(png: &[u8]) -> (u32, u32, Vec<u8>) {
    assert_eq!(png[..8], MAGIC, "PNG signature");
    let mut i = 8;
    let (mut w, mut h) = (0, 0);
    let mut idat = Vec::new();
    while i < png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        let ctype = &png[i + 4..i + 8];
        let data = &png[i + 8..i + 8 + len];
        // Recompute the chunk CRC over type+data.
        let mut crc = Crc::new();
        crc.update(ctype);
        crc.update(data);
        let stored = u32::from_be_bytes(png[i + 8 + len..i + 12 + len].try_into().unwrap());
        assert_eq!(crc.finish(), stored, "CRC of {:?}", std::str::from_utf8(ctype));
        match ctype {
            b"IHDR" => {
                w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                assert_eq!(&data[8..], &[8, 2, 0, 0, 0], "8-bit RGB, no interlace");
            }
            b"IDAT" => idat.extend_from_slice(data),
            _ => {}
        }
        i += 12 + len;
    }
    (w, h, idat)
}

/// Reverse `zlib_stored`: strip the 2-byte header + trailing Adler-32, walk the
/// stored DEFLATE blocks, and return the raw (filtered) scanline bytes.
fn inflate_stored(zlib: &[u8]) -> Vec<u8> {
    assert_eq!(zlib[0], 0x78);
    assert_eq!(zlib[1], 0x01);
    assert_eq!(u16::from(zlib[0]) << 8 | u16::from(zlib[1]), 0x7801);
    assert_eq!((u16::from(zlib[0]) << 8 | u16::from(zlib[1])) % 31, 0, "FCHECK");
    let body = &zlib[2..zlib.len() - 4];
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let bfinal = body[i] & 1;
        assert_eq!(body[i] & 6, 0, "BTYPE=00 (stored)");
        i += 1;
        let len = u16::from_le_bytes([body[i], body[i + 1]]) as usize;
        let nlen = u16::from_le_bytes([body[i + 2], body[i + 3]]);
        assert_eq!(nlen, !(len as u16), "LEN/NLEN complement");
        i += 4;
        out.extend_from_slice(&body[i..i + len]);
        i += len;
        if bfinal == 1 {
            break;
        }
    }
    // Adler-32 of the reconstructed raw must match the trailer.
    let trailer = u32::from_be_bytes(zlib[zlib.len() - 4..].try_into().unwrap());
    assert_eq!(adler32(&out), trailer, "Adler-32");
    out
}

fn decode(png: &[u8]) -> (usize, usize, Vec<u32>) {
    let (w, h, idat) = chunks(png);
    let raw = inflate_stored(&idat);
    let (w, h) = (w as usize, h as usize);
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = &raw[y * (1 + w * 3)..];
        assert_eq!(row[0], 0, "no-filter scanline");
        for x in 0..w {
            let o = 1 + x * 3;
            px.push((u32::from(row[o]) << 16) | (u32::from(row[o + 1]) << 8) | u32::from(row[o + 2]));
        }
    }
    (w, h, px)
}

#[test]
fn encodes_a_decodable_png() {
    let pixels = vec![0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0x00FF_FFFF];
    let png = encode(&pixels, 2, 2);
    let (w, h, back) = decode(&png);
    assert_eq!((w, h), (2, 2));
    assert_eq!(back, pixels, "pixels survive the round-trip (XRGB, top byte dropped)");
}

#[test]
fn one_by_one_and_larger_are_valid() {
    let (w, h, back) = decode(&encode(&[0x0012_3456], 1, 1));
    assert_eq!((w, h), (1, 1));
    assert_eq!(back, vec![0x0012_3456]);

    // A block bigger than one stored-block cap (0xFFFF) exercises multi-block.
    let big: Vec<u32> = (0..256 * 256).map(|i| (i as u32) & 0x00FF_FFFF).collect();
    let (w, h, back) = decode(&encode(&big, 256, 256));
    assert_eq!((w, h), (256, 256));
    assert_eq!(back, big);
}

#[test]
fn short_slice_pads_black_no_panic() {
    let png = encode(&[0x00AB_CDEF], 2, 2); // only 1 of 4 pixels
    let (_, _, back) = decode(&png);
    assert_eq!(back, vec![0x00AB_CDEF, 0, 0, 0]);
}

#[test]
fn adler_of_empty_is_one() {
    assert_eq!(adler32(&[]), 1);
}
