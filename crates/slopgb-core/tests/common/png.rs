//! Minimal PNG decoder for test reference images.
//!
//! The core crate forbids dependencies — including dev-dependencies — so the
//! reference screenshots shipped with the c-sp game-boy-test-roms collection
//! are decoded by this hand-rolled reader instead of a PNG crate. Scope is
//! exactly the formats that occur in that collection (all non-interlaced,
//! bit depth <= 8): greyscale (color type 0), RGB8 (type 2), indexed
//! (type 3, with PLTE) and RGBA8 (type 6, alpha dropped). Anything else —
//! 16-bit depth, Adam7 interlacing, greyscale+alpha, unknown critical
//! chunks — is a clean `Err`.
//!
//! Integrity footers are intentionally NOT verified, neither zlib's Adler-32
//! nor the per-chunk CRC-32: the inputs are vendored reference assets, and
//! corruption surfaces as a pixel mismatch in the consuming test anyway.

/// A decoded image: `rgb` holds `w * h` pixels, row-major.
#[derive(Debug)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<[u8; 3]>,
}

pub fn load_png(path: &std::path::Path) -> Result<Image, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    decode_png(&bytes)
}

pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let (ihdr, palette, idat) = parse_chunks(bytes)?;
    let raw = defilter(&ihdr, &zlib_decompress(&idat)?)?;
    unpack(&ihdr, palette.as_deref(), &raw)
}

/// IHDR fields after validation against the supported scope.
struct Ihdr {
    w: usize,
    h: usize,
    depth: u32,
    color_type: u8,
}

impl Ihdr {
    /// Samples per pixel (PNG spec §11.2.2; types 4 and 16-bit depths are
    /// rejected by `parse`).
    fn channels(&self) -> u32 {
        match self.color_type {
            2 => 3,
            6 => 4,
            _ => 1, // 0 = greyscale, 3 = palette index
        }
    }

    /// Filter unit in bytes: the byte distance to the pixel to the left,
    /// rounded up to one for sub-byte depths (PNG spec §9.2, "bpp is
    /// defined as ... rounding up to one").
    fn filter_bpp(&self) -> usize {
        ((self.channels() * self.depth) as usize).div_ceil(8)
    }

    /// Bytes per defiltered scanline (sub-byte rows pad the last byte).
    fn row_bytes(&self) -> usize {
        (self.w * (self.channels() * self.depth) as usize).div_ceil(8)
    }

    fn parse(data: &[u8]) -> Result<Ihdr, String> {
        let Ok([wh @ .., d, ct, comp, filt, interlace]) = <&[u8; 13]>::try_from(data) else {
            return Err(format!("IHDR: {} bytes, want 13", data.len()));
        };
        let be32 = |b: &[u8]| u32::from_be_bytes(b.try_into().unwrap());
        let (w, h) = (be32(&wh[..4]) as usize, be32(&wh[4..]) as usize);
        if w == 0 || h == 0 {
            return Err("IHDR: zero width or height".into());
        }
        // Scope guard (see module docs): the c-sp collection is 8-bit at
        // most, never interlaced, and never greyscale+alpha.
        let depth_ok = match ct {
            0 | 3 => matches!(d, 1 | 2 | 4 | 8),
            2 | 6 => *d == 8,
            _ => return Err(format!("IHDR: unsupported color type {ct}")),
        };
        if !depth_ok {
            return Err(format!(
                "IHDR: unsupported bit depth {d} for color type {ct}"
            ));
        }
        if *comp != 0 || *filt != 0 {
            return Err("IHDR: nonstandard compression/filter method".into());
        }
        if *interlace != 0 {
            return Err("IHDR: interlaced (Adam7) images unsupported".into());
        }
        Ok(Ihdr {
            w,
            h,
            depth: u32::from(*d),
            color_type: *ct,
        })
    }
}

/// Walk the chunk sequence (PNG spec §5.3: 4-byte big-endian length, 4-byte
/// type, data, 4-byte CRC — the CRC is skipped, see module docs) and return
/// the parsed IHDR, the PLTE palette if any, and all IDAT data concatenated.
#[allow(clippy::type_complexity)]
fn parse_chunks(bytes: &[u8]) -> Result<(Ihdr, Option<Vec<[u8; 3]>>, Vec<u8>), String> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut rest = bytes
        .strip_prefix(&SIGNATURE)
        .ok_or("not a PNG: bad signature")?;
    let mut ihdr: Option<Ihdr> = None;
    let mut palette: Option<Vec<[u8; 3]>> = None;
    let mut idat = Vec::new();
    loop {
        let (header, body) = rest.split_at_checked(8).ok_or("truncated chunk header")?;
        let len = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let typ: [u8; 4] = header[4..].try_into().unwrap();
        // `len` data bytes plus the 4-byte CRC.
        let (data, after) = body
            .split_at_checked(len.checked_add(4).ok_or("chunk length overflow")?)
            .ok_or_else(|| format!("truncated {} chunk", String::from_utf8_lossy(&typ)))?;
        let data = &data[..len];
        rest = after;
        if ihdr.is_none() && typ != *b"IHDR" {
            return Err("first chunk is not IHDR".into());
        }
        match &typ {
            b"IHDR" => {
                if ihdr.is_some() {
                    return Err("duplicate IHDR".into());
                }
                ihdr = Some(Ihdr::parse(data)?);
            }
            b"PLTE" => {
                if data.is_empty() || data.len() % 3 != 0 || data.len() > 256 * 3 {
                    return Err(format!("PLTE: bad length {}", data.len()));
                }
                palette = Some(data.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect());
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            // Ancillary chunks (lowercase first letter, PNG spec §5.4) are
            // safely skippable; an unknown *critical* chunk is not.
            _ if typ[0].is_ascii_lowercase() => {}
            _ => {
                return Err(format!(
                    "unknown critical chunk {}",
                    String::from_utf8_lossy(&typ)
                ));
            }
        }
    }
    let ihdr = ihdr.ok_or("no IHDR chunk")?;
    if idat.is_empty() {
        return Err("no IDAT chunk".into());
    }
    Ok((ihdr, palette, idat))
}

/// Reverse the per-scanline filters (PNG spec §9.2: each scanline is one
/// filter-type byte followed by `row_bytes` filtered bytes) and return the
/// concatenated raw scanlines.
fn defilter(ihdr: &Ihdr, inflated: &[u8]) -> Result<Vec<u8>, String> {
    let row_bytes = ihdr.row_bytes();
    let bpp = ihdr.filter_bpp();
    let expected = ihdr
        .h
        .checked_mul(row_bytes + 1)
        .ok_or("image dimensions overflow")?;
    if inflated.len() != expected {
        return Err(format!(
            "pixel data is {} bytes, want {expected} ({} rows of 1+{row_bytes})",
            inflated.len(),
            ihdr.h
        ));
    }
    let mut raw = Vec::with_capacity(ihdr.h * row_bytes);
    let mut prev = vec![0u8; row_bytes];
    for line in inflated.chunks_exact(row_bytes + 1) {
        let mut cur = line[1..].to_vec();
        for i in 0..row_bytes {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let predictor = match line[0] {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                f => return Err(format!("unknown filter type {f}")),
            };
            cur[i] = cur[i].wrapping_add(predictor);
        }
        raw.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(raw)
}

/// Paeth predictor (PNG spec §9.4): whichever of left/above/upper-left is
/// closest to `a + b - c`, ties broken in that order.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i32::from(a) + i32::from(b) - i32::from(c);
    let (pa, pb, pc) = (
        (p - i32::from(a)).abs(),
        (p - i32::from(b)).abs(),
        (p - i32::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Extract sample `x` of a raw scanline: samples are packed left to right,
/// MSB first within each byte (PNG spec §7.2).
fn sample(row: &[u8], x: usize, depth: u32) -> u8 {
    if depth == 8 {
        return row[x];
    }
    let bit = x * depth as usize;
    (row[bit / 8] >> (8 - depth as usize - bit % 8)) & ((1 << depth) - 1)
}

/// Expand raw scanlines to 8-bit RGB.
fn unpack(ihdr: &Ihdr, palette: Option<&[[u8; 3]]>, raw: &[u8]) -> Result<Image, String> {
    // Greyscale samples scale to 8 bits by bit replication, which for these
    // depths is multiplication: 1-bit x255, 2-bit x85, 4-bit x17 (PNG spec
    // §13.12, sample depth scaling).
    let scale = (255 / ((1u16 << ihdr.depth) - 1)) as u8;
    let mut rgb = Vec::with_capacity(ihdr.w * ihdr.h);
    for row in raw.chunks_exact(ihdr.row_bytes()) {
        for x in 0..ihdr.w {
            rgb.push(match ihdr.color_type {
                0 => [sample(row, x, ihdr.depth) * scale; 3],
                2 => [row[x * 3], row[x * 3 + 1], row[x * 3 + 2]],
                3 => {
                    let palette = palette.ok_or("indexed image without a PLTE chunk")?;
                    let idx = usize::from(sample(row, x, ihdr.depth));
                    *palette.get(idx).ok_or_else(|| {
                        format!(
                            "palette index {idx} out of range ({} entries)",
                            palette.len()
                        )
                    })?
                }
                6 => [row[x * 4], row[x * 4 + 1], row[x * 4 + 2]],
                _ => unreachable!("Ihdr::parse rejects other color types"),
            });
        }
    }
    Ok(Image {
        w: ihdr.w,
        h: ihdr.h,
        rgb,
    })
}

/// Decompress a zlib stream (RFC 1950): 2-byte header, raw DEFLATE body.
/// The 4-byte Adler-32 trailer is not checked (see module docs).
fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let [cmf, flg, body @ ..] = data else {
        return Err("zlib: stream shorter than the 2-byte header".into());
    };
    if cmf & 0x0F != 8 {
        return Err(format!("zlib: compression method {} != 8", cmf & 0x0F));
    }
    if (u16::from(*cmf) * 256 + u16::from(*flg)) % 31 != 0 {
        return Err("zlib: FCHECK failed (corrupt header)".into());
    }
    if flg & 0x20 != 0 {
        return Err("zlib: preset dictionary (FDICT) unsupported".into());
    }
    inflate(body)
}

/// LSB-first bit reader over a byte slice (RFC 1951 §3.1.1: deflate packs
/// bits starting from the least significant bit of each byte).
struct BitReader<'a> {
    data: &'a [u8],
    /// Total bits consumed so far.
    pos: usize,
}

impl<'a> BitReader<'a> {
    fn bit(&mut self) -> Result<u32, String> {
        let byte = self
            .data
            .get(self.pos / 8)
            .ok_or("deflate: unexpected end of stream")?;
        let bit = u32::from(byte >> (self.pos % 8)) & 1;
        self.pos += 1;
        Ok(bit)
    }

    /// Read `n` bits as an integer, LSB first.
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        let mut v = 0;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Ok(v)
    }

    /// Skip to the next byte boundary and take `n` whole bytes (stored
    /// blocks, RFC 1951 §3.2.4).
    fn take_aligned_bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        self.pos = self.pos.div_ceil(8) * 8;
        let start = self.pos / 8;
        let bytes = self
            .data
            .get(start..start + n)
            .ok_or("deflate: stored block longer than the remaining stream")?;
        self.pos += n * 8;
        Ok(bytes)
    }
}

/// Canonical Huffman decoder built from per-symbol code lengths
/// (RFC 1951 §3.2.2), decoding one bit at a time.
struct Huffman {
    /// `count[len]` = number of codes of bit length `len`.
    count: [u16; 16],
    /// Symbols ordered by (code length, symbol value) — canonical order.
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let mut count = [0u16; 16];
        for &len in lengths {
            if len > 15 {
                return Err(format!("deflate: code length {len} > 15"));
            }
            count[usize::from(len)] += 1;
        }
        // Kraft inequality: an over-subscribed code is unusable. Incomplete
        // codes are permitted — dynamic blocks commonly declare a single
        // distance code (RFC 1951 §3.2.7 allows it) — and decode() errors
        // out if a gap code actually appears in the stream.
        let mut left = 1i32;
        for &c in &count[1..] {
            left = left * 2 - i32::from(c);
            if left < 0 {
                return Err("deflate: over-subscribed Huffman code".into());
            }
        }
        let mut offsets = [0usize; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + usize::from(count[len]);
        }
        let mut symbol = vec![0u16; lengths.len() - usize::from(count[0])];
        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                symbol[offsets[usize::from(len)]] = sym as u16;
                offsets[usize::from(len)] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }

    fn decode(&self, br: &mut BitReader) -> Result<u16, String> {
        // Huffman codes are packed MSB-of-code first (RFC 1951 §3.1.1), so
        // grow the code one bit at a time and test it against each length's
        // canonical [first, first+count) range.
        let mut code = 0u32;
        let mut first = 0u32;
        let mut index = 0u32;
        for len in 1..=15 {
            code |= br.bit()?;
            let count = u32::from(self.count[len]);
            if code < first + count {
                return Ok(self.symbol[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("deflate: invalid Huffman code".into())
    }
}

/// Length-code bases/extra bits for symbols 257..=285 (RFC 1951 §3.2.5).
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// Distance-code bases/extra bits for symbols 0..=29 (RFC 1951 §3.2.5).
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Decompress a raw DEFLATE stream (RFC 1951): stored, fixed-Huffman and
/// dynamic-Huffman blocks.
fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut br = BitReader { data, pos: 0 };
    let mut out = Vec::new();
    loop {
        let bfinal = br.bits(1)?;
        match br.bits(2)? {
            0 => {
                // Stored (§3.2.4): byte-aligned LEN, one's-complement NLEN.
                let header = br.take_aligned_bytes(4)?;
                let len = usize::from(header[0]) | usize::from(header[1]) << 8;
                let nlen = usize::from(header[2]) | usize::from(header[3]) << 8;
                if len ^ nlen != 0xFFFF {
                    return Err("deflate: stored block NLEN is not !LEN".into());
                }
                out.extend_from_slice(br.take_aligned_bytes(len)?);
            }
            1 => {
                // Fixed code lengths (§3.2.6).
                let mut litlen = [8u8; 288];
                litlen[144..256].fill(9);
                litlen[256..280].fill(7);
                let litlen = Huffman::new(&litlen)?;
                let dist = Huffman::new(&[5u8; 30])?;
                inflate_huffman_block(&mut br, &litlen, &dist, &mut out)?;
            }
            2 => {
                let (litlen, dist) = read_dynamic_tables(&mut br)?;
                inflate_huffman_block(&mut br, &litlen, &dist, &mut out)?;
            }
            _ => return Err("deflate: reserved block type 3".into()),
        }
        if bfinal == 1 {
            return Ok(out);
        }
    }
}

/// Parse a dynamic block's code-length code and the lit/len + distance
/// tables it encodes (RFC 1951 §3.2.7).
fn read_dynamic_tables(br: &mut BitReader) -> Result<(Huffman, Huffman), String> {
    /// Permuted order in which code-length-code lengths are stored.
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;
    let mut cl_lengths = [0u8; 19];
    for &slot in &ORDER[..hclen] {
        cl_lengths[slot] = br.bits(3)? as u8;
    }
    let cl_code = Huffman::new(&cl_lengths)?;
    let mut lengths = Vec::with_capacity(hlit + hdist);
    while lengths.len() < hlit + hdist {
        let sym = cl_code.decode(br)?;
        let (repeat, len) = match sym {
            0..=15 => (1, sym as u8),
            16 => {
                let &prev = lengths
                    .last()
                    .ok_or("deflate: repeat code 16 with no previous length")?;
                (3 + br.bits(2)?, prev)
            }
            17 => (3 + br.bits(3)?, 0),
            18 => (11 + br.bits(7)?, 0),
            _ => unreachable!("code-length alphabet is 0..=18"),
        };
        if lengths.len() + repeat as usize > hlit + hdist {
            return Err("deflate: code-length repeat overflows the table".into());
        }
        lengths.extend(std::iter::repeat_n(len, repeat as usize));
    }
    if lengths[256] == 0 {
        return Err("deflate: dynamic block has no end-of-block code".into());
    }
    Ok((
        Huffman::new(&lengths[..hlit])?,
        Huffman::new(&lengths[hlit..])?,
    ))
}

/// Decode one Huffman-coded block body: literals and length/distance
/// back-references, until the end-of-block symbol 256 (RFC 1951 §3.2.3).
fn inflate_huffman_block(
    br: &mut BitReader,
    litlen: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    loop {
        let sym = litlen.decode(br)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let i = usize::from(sym - 257);
                let len = usize::from(LEN_BASE[i]) + br.bits(LEN_EXTRA[i])? as usize;
                let dsym = usize::from(dist.decode(br)?);
                if dsym >= 30 {
                    return Err(format!("deflate: invalid distance symbol {dsym}"));
                }
                let distance = usize::from(DIST_BASE[dsym]) + br.bits(DIST_EXTRA[dsym])? as usize;
                if distance > out.len() {
                    return Err("deflate: back-reference before output start".into());
                }
                // Byte-by-byte so overlapping copies (distance < len)
                // repeat the just-written bytes, as deflate requires.
                for _ in 0..len {
                    out.push(out[out.len() - distance]);
                }
            }
            _ => return Err(format!("deflate: invalid literal/length symbol {sym}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- inflate / zlib ----
    //
    // Compressed vectors generated offline with CPython's zlib; the block
    // type of each was verified offline from the first deflate byte
    // (BFINAL = bit 0, BTYPE = bits 1-2, RFC 1951 §3.2.3).

    /// `zlib.compress(b"hello world", 0)` — one stored (BTYPE=0) block.
    const Z_STORED: &[u8] = &[
        0x78, 0x01, 0x01, 0x0B, 0x00, 0xF4, 0xFF, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x77, 0x6F,
        0x72, 0x6C, 0x64, 0x1A, 0x0B, 0x04, 0x5D,
    ];

    /// `zlib.compress(b"abcabcabcabc")` — one fixed-Huffman (BTYPE=1) block
    /// containing a length/distance back-reference.
    const Z_FIXED: &[u8] = &[
        0x78, 0x9C, 0x4B, 0x4C, 0x4A, 0x4E, 0x84, 0x21, 0x00, 0x1D, 0xE0, 0x04, 0x99,
    ];

    /// `zlib.compress(data, 9)` of 400 pseudorandom skewed-alphabet bytes
    /// (mostly literals, so the dynamic table pays off) — one
    /// dynamic-Huffman (BTYPE=2) block.
    const Z_DYNAMIC: &[u8] = &[
        0x78, 0xDA, 0x1D, 0x90, 0x89, 0x0D, 0xC0, 0x30, 0x08, 0x03, 0x57, 0xC9, 0x6A, 0x36, 0xEF,
        0xFE, 0x13, 0xF4, 0xA8, 0x54, 0x45, 0x94, 0x38, 0xF6, 0x81, 0x64, 0x5B, 0x1A, 0x6D, 0x4A,
        0xD4, 0xFD, 0x34, 0x9E, 0x74, 0xAD, 0xE5, 0x0C, 0x8E, 0x90, 0x52, 0x11, 0xCF, 0xAA, 0xA1,
        0x9A, 0x78, 0xC1, 0xDD, 0xF5, 0x2D, 0x74, 0xE1, 0x08, 0x1C, 0x9E, 0xC7, 0xAE, 0xE4, 0xBC,
        0x7E, 0x24, 0x0F, 0x37, 0x68, 0x14, 0x7F, 0x9E, 0xF1, 0xE6, 0x9E, 0x5B, 0x97, 0x2E, 0x26,
        0x9F, 0x32, 0x5F, 0xB5, 0x87, 0x68, 0x5E, 0xA0, 0x47, 0xD8, 0x6E, 0x45, 0x13, 0x14, 0x9E,
        0xD6, 0x0B, 0xB4, 0x71, 0x04, 0x2B, 0x15, 0x84, 0xE4, 0x1F, 0xDF, 0x9F, 0x91, 0x51, 0x58,
        0x91, 0x1B, 0x5A, 0xCC, 0x7D, 0xA6, 0xA8, 0xD5, 0x40, 0x60, 0xBA, 0x5B, 0x07, 0x03, 0x79,
        0x64, 0x5E, 0x64, 0xC6, 0xCD, 0x39, 0x05, 0x31, 0x82, 0x9B, 0xE4, 0x0C, 0xED, 0xAC, 0x72,
        0xC2, 0xF3, 0x4F, 0x92, 0xDC, 0x9B, 0xC0, 0x1D, 0x9C, 0xB4, 0xD5, 0x0D, 0x30, 0xBE, 0xEF,
        0x04, 0xEB, 0x7D, 0x54, 0xB9, 0xDB, 0xCC, 0xCB, 0xC7, 0x52, 0x0A, 0x59, 0xBD, 0xBD, 0x19,
        0xFA, 0x50, 0x6E, 0x15, 0xC5, 0x82, 0x9A, 0x2D, 0xFD, 0x40, 0x43, 0xEB, 0x9D, 0xB8, 0xEB,
        0x60, 0x7E, 0x44, 0x00, 0x26, 0xE7, 0xD6, 0x40, 0xCC, 0xDE, 0x4A, 0x99, 0x52, 0xB7, 0xC3,
        0x98, 0x7C, 0x1F, 0x0D, 0xAD, 0x96, 0xB4,
    ];

    /// The 400 bytes Z_DYNAMIC decompresses to.
    const Z_DYNAMIC_RAW: &[u8] = &[
        0x61, 0x61, 0x62, 0x62, 0x62, 0x61, 0x61, 0x67, 0x61, 0x68, 0x64, 0x61, 0x61, 0x61, 0x62,
        0x62, 0x66, 0x20, 0x61, 0x67, 0x62, 0x67, 0x64, 0x62, 0x65, 0x68, 0x62, 0x61, 0x62, 0x64,
        0x63, 0x62, 0x61, 0x62, 0x63, 0x61, 0x61, 0x64, 0x61, 0x63, 0x63, 0x20, 0x62, 0x61, 0x65,
        0x67, 0x61, 0x64, 0x61, 0x67, 0x63, 0x20, 0x63, 0x68, 0x62, 0x61, 0x61, 0x62, 0x63, 0x61,
        0x62, 0x61, 0x64, 0x62, 0x65, 0x63, 0x62, 0x63, 0x63, 0x62, 0x62, 0x61, 0x20, 0x62, 0x67,
        0x62, 0x62, 0x65, 0x64, 0x62, 0x67, 0x62, 0x63, 0x61, 0x62, 0x61, 0x63, 0x64, 0x62, 0x61,
        0x62, 0x68, 0x63, 0x62, 0x65, 0x64, 0x65, 0x61, 0x62, 0x61, 0x62, 0x67, 0x67, 0x62, 0x68,
        0x64, 0x68, 0x64, 0x63, 0x62, 0x61, 0x66, 0x65, 0x61, 0x61, 0x61, 0x61, 0x62, 0x64, 0x20,
        0x61, 0x64, 0x64, 0x20, 0x65, 0x66, 0x62, 0x67, 0x61, 0x61, 0x67, 0x62, 0x63, 0x61, 0x63,
        0x64, 0x62, 0x65, 0x61, 0x62, 0x66, 0x62, 0x66, 0x61, 0x63, 0x66, 0x20, 0x62, 0x61, 0x63,
        0x62, 0x67, 0x66, 0x61, 0x20, 0x63, 0x65, 0x61, 0x61, 0x63, 0x63, 0x62, 0x61, 0x62, 0x68,
        0x61, 0x61, 0x65, 0x61, 0x67, 0x61, 0x61, 0x65, 0x67, 0x62, 0x62, 0x66, 0x20, 0x64, 0x62,
        0x67, 0x62, 0x63, 0x64, 0x63, 0x65, 0x66, 0x65, 0x61, 0x62, 0x62, 0x61, 0x63, 0x61, 0x68,
        0x67, 0x62, 0x68, 0x62, 0x61, 0x61, 0x61, 0x62, 0x61, 0x61, 0x63, 0x61, 0x66, 0x62, 0x62,
        0x65, 0x62, 0x67, 0x61, 0x68, 0x68, 0x65, 0x62, 0x65, 0x64, 0x62, 0x61, 0x61, 0x64, 0x63,
        0x64, 0x64, 0x65, 0x61, 0x61, 0x61, 0x64, 0x63, 0x61, 0x62, 0x62, 0x62, 0x67, 0x65, 0x61,
        0x64, 0x62, 0x62, 0x65, 0x62, 0x61, 0x65, 0x67, 0x61, 0x61, 0x67, 0x61, 0x61, 0x62, 0x62,
        0x64, 0x65, 0x65, 0x62, 0x64, 0x61, 0x62, 0x64, 0x61, 0x64, 0x62, 0x65, 0x63, 0x64, 0x67,
        0x65, 0x61, 0x62, 0x63, 0x62, 0x61, 0x68, 0x67, 0x61, 0x63, 0x61, 0x61, 0x68, 0x65, 0x66,
        0x66, 0x62, 0x61, 0x66, 0x61, 0x62, 0x61, 0x20, 0x61, 0x62, 0x64, 0x61, 0x68, 0x62, 0x68,
        0x20, 0x61, 0x20, 0x61, 0x64, 0x68, 0x68, 0x66, 0x63, 0x62, 0x62, 0x63, 0x62, 0x62, 0x64,
        0x61, 0x63, 0x65, 0x63, 0x61, 0x61, 0x65, 0x20, 0x68, 0x61, 0x61, 0x67, 0x62, 0x66, 0x62,
        0x61, 0x63, 0x61, 0x62, 0x63, 0x63, 0x62, 0x65, 0x67, 0x63, 0x20, 0x66, 0x61, 0x67, 0x63,
        0x61, 0x61, 0x62, 0x61, 0x61, 0x67, 0x61, 0x62, 0x63, 0x20, 0x62, 0x63, 0x62, 0x62, 0x66,
        0x65, 0x62, 0x61, 0x61, 0x64, 0x62, 0x61, 0x61, 0x63, 0x61, 0x62, 0x62, 0x65, 0x67, 0x64,
        0x67, 0x61, 0x61, 0x61, 0x61, 0x67, 0x61, 0x63, 0x68, 0x67, 0x61, 0x64, 0x61, 0x61, 0x63,
        0x63, 0x61, 0x63, 0x62, 0x62, 0x61, 0x63, 0x67, 0x64, 0x20,
    ];

    #[test]
    fn inflate_stored_block() {
        assert_eq!(zlib_decompress(Z_STORED).unwrap(), b"hello world");
    }

    #[test]
    fn inflate_fixed_huffman_with_backreference() {
        assert_eq!(zlib_decompress(Z_FIXED).unwrap(), b"abcabcabcabc");
    }

    #[test]
    fn inflate_dynamic_huffman_block() {
        assert_eq!(zlib_decompress(Z_DYNAMIC).unwrap(), Z_DYNAMIC_RAW);
    }

    #[test]
    fn inflate_truncated_stream_is_err() {
        // Cuts into the deflate body (not merely the Adler-32 trailer).
        assert!(zlib_decompress(&Z_FIXED[..7]).is_err());
        assert!(zlib_decompress(&Z_DYNAMIC[..20]).is_err());
        assert!(zlib_decompress(&[]).is_err());
        assert!(zlib_decompress(&[0x78]).is_err());
    }

    #[test]
    fn inflate_reserved_block_type_is_err() {
        // BFINAL=1, BTYPE=3 (reserved, RFC 1951 §3.2.3).
        assert!(inflate(&[0x07]).is_err());
    }

    #[test]
    fn inflate_stored_length_complement_mismatch_is_err() {
        // Stored block whose NLEN is not the one's complement of LEN.
        assert!(inflate(&[0x01, 0x02, 0x00, 0x00, 0x00, 0xAA, 0xBB]).is_err());
    }

    #[test]
    fn zlib_rejects_bad_header() {
        // CM=7 is not DEFLATE (RFC 1950 §2.2).
        assert!(zlib_decompress(&[0x77, 0x01, 0x03, 0x00]).is_err());
        // FCHECK failure: 0x78 0x00 is not a multiple of 31.
        assert!(zlib_decompress(&[0x78, 0x00, 0x03, 0x00]).is_err());
        // FDICT set (0x7820 = 31 * 992 passes FCHECK): preset dictionaries
        // are unsupported.
        assert!(zlib_decompress(&[0x78, 0x20, 0x03, 0x00]).is_err());
    }

    // ---- decode_png ----
    //
    // Synthetic PNGs hand-built offline (python3 struct + zlib), each
    // verified against Pillow before embedding. Together they cover every
    // format in the c-sp collection census and all five filter types.

    /// 2x2 RGBA8 (color type 6); row filters None, Sub. Pixels
    /// red/green/blue/yellow with alphas 128/255/7/0 — alpha is dropped.
    const PNG_RGBA8: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72,
        0xB6, 0x0D, 0x24, 0x00, 0x00, 0x00, 0x1B, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0xD0, 0xC0, 0xF0, 0x9F, 0xE1, 0x3F, 0x23, 0x03, 0xC3, 0x7F, 0xF6, 0xFF, 0xFF,
        0x19, 0x7F, 0x02, 0x00, 0x3C, 0x05, 0x07, 0x7D, 0x28, 0xFB, 0xEA, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 3x3 RGB8 (color type 2); row filters Up, Average, Paeth.
    const PNG_RGB8: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0xD9,
        0x4A, 0x22, 0xE8, 0x00, 0x00, 0x00, 0x22, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xE2,
        0x12, 0x91, 0xD3, 0x30, 0xB2, 0x71, 0x0B, 0x88, 0x62, 0xE6, 0xE2, 0x17, 0x11, 0x02, 0x03,
        0x96, 0x9D, 0xDE, 0x77, 0x2D, 0x52, 0x1C, 0xCD, 0x2D, 0x52, 0x00, 0x47, 0xCF, 0x05, 0xF6,
        0x3C, 0x14, 0xD7, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
        0x82,
    ];

    /// 5x2 indexed 2-bit (color type 3) with a 4-entry PLTE; row filters
    /// None, Up. 10 bits per row pack into 2 bytes (6 padding bits).
    /// Indices row 0: 0,1,2,3,0; row 1: 3,2,1,0,2.
    const PNG_IDX2: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x02, 0x02, 0x03, 0x00, 0x00, 0x00, 0xED,
        0x04, 0xFE, 0xCE, 0x00, 0x00, 0x00, 0x0C, 0x50, 0x4C, 0x54, 0x45, 0xFF, 0x00, 0x00, 0x00,
        0xFF, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFB, 0x00, 0x60, 0xF6, 0x00, 0x00, 0x00,
        0x0E, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x90, 0x66, 0x60, 0x3A, 0xD9, 0x00, 0x00,
        0x02, 0xA5, 0x01, 0x67, 0x4B, 0xA9, 0xB5, 0x22, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
        0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 9x2 greyscale 1-bit (color type 0): bits 101010101 / 010101010,
    /// MSB-first, 9 bits per row pack into 2 bytes.
    const PNG_GRAY1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0xA2,
        0x2D, 0xCB, 0x7E, 0x00, 0x00, 0x00, 0x0E, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x58,
        0xD5, 0xC0, 0x10, 0xCA, 0x00, 0x00, 0x06, 0x02, 0x01, 0x80, 0x82, 0x1D, 0x99, 0x65, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 4x1 greyscale 2-bit: samples 0,1,2,3.
    const PNG_GRAY2: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x96,
        0xE7, 0x48, 0xB0, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x90,
        0x06, 0x00, 0x00, 0x1D, 0x00, 0x1C, 0x8E, 0xF4, 0xF5, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 2x2 greyscale 8-bit; row filters None, Sub. Samples 0,255 / 100,50.
    const PNG_GRAY8: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x00, 0x00, 0x00, 0x00, 0x57,
        0xDD, 0x52, 0xF8, 0x00, 0x00, 0x00, 0x0E, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60,
        0xF8, 0xCF, 0x98, 0x72, 0x0E, 0x00, 0x05, 0x9B, 0x02, 0x33, 0x45, 0xD2, 0x17, 0x31, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 3x1 indexed 4-bit with a 3-entry PLTE; indices 2,0,1 (MSB nibble
    /// first).
    const PNG_IDX4: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x04, 0x03, 0x00, 0x00, 0x00, 0xE9,
        0xCE, 0x09, 0x87, 0x00, 0x00, 0x00, 0x09, 0x50, 0x4C, 0x54, 0x45, 0x0A, 0x14, 0x1E, 0x28,
        0x32, 0x3C, 0x46, 0x50, 0x5A, 0x16, 0xAC, 0x84, 0x74, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9C, 0x63, 0x50, 0x10, 0x00, 0x00, 0x00, 0x53, 0x00, 0x31, 0x9B, 0x6D,
        0xA7, 0x29, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 2x1 indexed 1-bit with a 2-entry PLTE; indices 1,0.
    const PNG_IDX1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03, 0x00, 0x00, 0x00, 0xCE,
        0xEC, 0xED, 0xC9, 0x00, 0x00, 0x00, 0x06, 0x50, 0x4C, 0x54, 0x45, 0x09, 0x08, 0x07, 0xC8,
        0xC9, 0xCA, 0xD1, 0xFA, 0x1B, 0x2E, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
        0x9C, 0x63, 0x68, 0x00, 0x00, 0x00, 0x82, 0x00, 0x81, 0x77, 0xCD, 0x72, 0xB6, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 2x1 indexed 4-bit whose second pixel uses index 5 against a 3-entry
    /// PLTE — must be rejected, not decoded as garbage.
    const PNG_IDX_OOR: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x04, 0x03, 0x00, 0x00, 0x00, 0x06,
        0x0C, 0x62, 0xB9, 0x00, 0x00, 0x00, 0x09, 0x50, 0x4C, 0x54, 0x45, 0x0A, 0x14, 0x1E, 0x28,
        0x32, 0x3C, 0x46, 0x50, 0x5A, 0x16, 0xAC, 0x84, 0x74, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x05, 0x00, 0x00, 0x07, 0x00, 0x06, 0x80, 0xCD, 0x62,
        0x8A, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// The PNG_GRAY2 image with its IDAT split in two and a tEXt chunk
    /// inserted between IHDR and the first IDAT.
    const PNG_MULTI_IDAT: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x96,
        0xE7, 0x48, 0xB0, 0x00, 0x00, 0x00, 0x13, 0x74, 0x45, 0x58, 0x74, 0x6B, 0x00, 0x61, 0x6E,
        0x63, 0x69, 0x6C, 0x6C, 0x61, 0x72, 0x79, 0x20, 0x73, 0x6B, 0x69, 0x70, 0x70, 0x65, 0x64,
        0x8A, 0x06, 0x06, 0x69, 0x00, 0x00, 0x00, 0x05, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63,
        0x90, 0x06, 0xA0, 0x0B, 0xF6, 0x8D, 0x00, 0x00, 0x00, 0x05, 0x49, 0x44, 0x41, 0x54, 0x00,
        0x00, 0x1D, 0x00, 0x1C, 0x4C, 0x73, 0x35, 0xA5, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
        0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn pixels(bytes: &[u8]) -> (usize, usize, Vec<[u8; 3]>) {
        let img = decode_png(bytes).unwrap();
        assert_eq!(img.rgb.len(), img.w * img.h);
        (img.w, img.h, img.rgb)
    }

    #[test]
    fn decode_rgba8_drops_alpha() {
        let (w, h, rgb) = pixels(PNG_RGBA8);
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgb, [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]]);
    }

    #[test]
    fn decode_rgb8_up_average_paeth_filters() {
        let (w, h, rgb) = pixels(PNG_RGB8);
        assert_eq!((w, h), (3, 3));
        assert_eq!(
            rgb,
            [
                [10, 20, 30],
                [40, 50, 60],
                [70, 80, 90],
                [15, 25, 35],
                [45, 55, 65],
                [75, 85, 95],
                [200, 100, 0],
                [0, 200, 100],
                [100, 0, 200],
            ]
        );
    }

    #[test]
    fn decode_indexed_2bit_with_plte() {
        let (w, h, rgb) = pixels(PNG_IDX2);
        assert_eq!((w, h), (5, 2));
        let (r, g, b, white) = ([255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]);
        assert_eq!(rgb, [r, g, b, white, r, white, b, g, r, b]);
    }

    #[test]
    fn decode_gray1_msb_first_packing() {
        let (w, h, rgb) = pixels(PNG_GRAY1);
        assert_eq!((w, h), (9, 2));
        let (wh, bl) = ([255u8; 3], [0u8; 3]);
        assert_eq!(
            rgb,
            [
                wh, bl, wh, bl, wh, bl, wh, bl, wh, // row 0
                bl, wh, bl, wh, bl, wh, bl, wh, bl, // row 1
            ]
        );
    }

    #[test]
    fn decode_gray2_scales_by_85() {
        let (w, h, rgb) = pixels(PNG_GRAY2);
        assert_eq!((w, h), (4, 1));
        assert_eq!(rgb, [[0; 3], [85; 3], [170; 3], [255; 3]]);
    }

    #[test]
    fn decode_gray8() {
        let (w, h, rgb) = pixels(PNG_GRAY8);
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgb, [[0; 3], [255; 3], [100; 3], [50; 3]]);
    }

    #[test]
    fn decode_indexed_4bit() {
        let (w, h, rgb) = pixels(PNG_IDX4);
        assert_eq!((w, h), (3, 1));
        assert_eq!(rgb, [[70, 80, 90], [10, 20, 30], [40, 50, 60]]);
    }

    #[test]
    fn decode_indexed_1bit() {
        let (w, h, rgb) = pixels(PNG_IDX1);
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgb, [[200, 201, 202], [9, 8, 7]]);
    }

    #[test]
    fn decode_concatenates_idat_chunks_and_skips_ancillary() {
        let (w, h, rgb) = pixels(PNG_MULTI_IDAT);
        assert_eq!((w, h), (4, 1));
        assert_eq!(rgb, [[0; 3], [85; 3], [170; 3], [255; 3]]);
    }

    #[test]
    fn decode_rejects_palette_index_out_of_range() {
        let err = decode_png(PNG_IDX_OOR).unwrap_err();
        assert!(err.contains("palette"), "{err}");
    }

    #[test]
    fn decode_rejects_bad_signature_and_truncation() {
        assert!(decode_png(b"not a png").is_err());
        assert!(decode_png(&[]).is_err());
        // Cut mid-chunk: the IHDR length field promises more than remains.
        assert!(decode_png(&PNG_RGB8[..20]).is_err());
    }

    /// Patch one IHDR byte of an embedded vector. Chunk CRCs are not
    /// verified by this decoder (see module docs), so no re-checksum is
    /// needed. IHDR data layout after the 8-byte signature + 8-byte chunk
    /// header: width (16..20), height (20..24), depth (24), color type
    /// (25), compression (26), filter (27), interlace (28).
    fn patched(png: &[u8], offset: usize, value: u8) -> Vec<u8> {
        let mut v = png.to_vec();
        v[offset] = value;
        v
    }

    #[test]
    fn decode_rejects_16bit_depth() {
        let err = decode_png(&patched(PNG_GRAY8, 24, 16)).unwrap_err();
        assert!(err.contains("depth"), "{err}");
    }

    #[test]
    fn decode_rejects_interlaced() {
        let err = decode_png(&patched(PNG_GRAY8, 28, 1)).unwrap_err();
        assert!(err.contains("interlace"), "{err}");
    }

    #[test]
    fn decode_rejects_grey_alpha_color_type() {
        // Color type 4 (greyscale+alpha) does not occur in the collection.
        let err = decode_png(&patched(PNG_GRAY8, 25, 4)).unwrap_err();
        assert!(err.contains("color type"), "{err}");
    }

    #[test]
    fn decode_rejects_pixel_data_size_mismatch() {
        // Lie about the height: the inflated stream no longer matches
        // h * (1 + row_bytes).
        assert!(decode_png(&patched(PNG_GRAY8, 23, 3)).is_err());
    }

    #[test]
    fn decode_rejects_unknown_critical_chunk() {
        // Uppercase the first letter of PNG_MULTI_IDAT's tEXt chunk type —
        // it starts at offset 37, after the IHDR chunk (8 signature + 25)
        // and the 4-byte length. "TEXt" is unknown-critical and must not be
        // skipped.
        let err = decode_png(&patched(PNG_MULTI_IDAT, 37, b'T')).unwrap_err();
        assert!(err.contains("critical"), "{err}");
    }

    #[test]
    fn decode_rejects_indexed_without_plte() {
        // Splice the 18-byte PLTE chunk (offsets 33..51) out of PNG_IDX1.
        let stripped = [&PNG_IDX1[..33], &PNG_IDX1[51..]].concat();
        let err = decode_png(&stripped).unwrap_err();
        assert!(err.contains("PLTE"), "{err}");
    }

    // ---- real reference image from the c-sp collection ----

    #[test]
    fn decode_real_dmg_acid2_reference_if_present() {
        // Opportunistic end-to-end check against a real collection file
        // (160x144, 2-bit greyscale, non-interlaced). The collection is
        // gitignored like the mooneye ROMs, so a missing checkout skips
        // silently, in the same spirit as `skip_or_fail`.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-roms/game-boy-test-roms-v7.0/dmg-acid2/dmg-acid2-dmg.png");
        if !path.is_file() {
            println!(
                "skipping dmg-acid2 PNG decode: {} not present",
                path.display()
            );
            return;
        }
        let img = load_png(&path).unwrap();
        assert_eq!((img.w, img.h), (160, 144));
        // Spot values and the full grey-level histogram, both derived
        // offline with Pillow from the same file.
        let px = |x: usize, y: usize| img.rgb[y * img.w + x];
        assert_eq!(px(0, 0), [255; 3]);
        assert_eq!(px(80, 72), [0; 3]);
        assert_eq!(px(159, 143), [255; 3]);
        assert_eq!(px(52, 28), [0; 3]);
        assert_eq!(px(112, 68), [170; 3]);
        assert_eq!(px(54, 40), [85; 3]);
        let mut histogram = std::collections::BTreeMap::new();
        for p in &img.rgb {
            assert_eq!(p[0], p[1]);
            assert_eq!(p[1], p[2]);
            *histogram.entry(p[0]).or_insert(0u32) += 1;
        }
        let expected = [(0u8, 3749u32), (85, 188), (170, 6254), (255, 12849)];
        assert_eq!(histogram.into_iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn decode_every_collection_png_if_present() {
        // The supported-format scope was fixed by a census of all 542 PNGs
        // in the v7.0 collection; this guards that every one of them keeps
        // decoding (e.g. against a future scope-narrowing refactor).
        // Silently skipped when the collection is not checked out.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-roms/game-boy-test-roms-v7.0");
        if !root.is_dir() {
            println!("skipping collection sweep: {} not present", root.display());
            return;
        }
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "png") {
                    out.push(p);
                }
            }
        }
        let mut pngs = Vec::new();
        walk(&root, &mut pngs);
        assert!(!pngs.is_empty(), "collection present but holds no PNGs");
        for path in &pngs {
            let img = load_png(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(img.rgb.len(), img.w * img.h, "{}", path.display());
        }
        println!("decoded {} collection PNGs", pngs.len());
    }
}
