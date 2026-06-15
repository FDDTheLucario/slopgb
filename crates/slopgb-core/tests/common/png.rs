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
    decode_png(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let (ihdr, palette, idat) = parse_chunks(bytes)?;
    // The exact decompressed size is known from the IHDR (`h` scanlines of
    // 1 filter byte + `row_bytes`); bounding inflate by it stops a crafted
    // IDAT (deflate expands up to ~1032:1) from allocating unbounded output
    // before `defilter`'s length check would reject it anyway.
    let expected = ihdr
        .h
        .checked_mul(ihdr.row_bytes() + 1)
        .ok_or("pixel data size overflows usize")?;
    let raw = defilter(&ihdr, &zlib_decompress(&idat, expected)?)?;
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
                if palette.is_some() {
                    return Err("duplicate PLTE".into());
                }
                if !idat.is_empty() {
                    return Err("PLTE after IDAT".into());
                }
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
/// `max_out` bounds the decompressed size (callers know it exactly from the
/// IHDR); exceeding it is an error, not an allocation.
fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
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
    inflate(body, max_out)
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
/// dynamic-Huffman blocks. Output beyond `max_out` bytes is an error.
fn inflate(data: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
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
                if out.len() + len > max_out {
                    return Err(format!("deflate: output exceeds expected {max_out} bytes"));
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
                inflate_huffman_block(&mut br, &litlen, &dist, &mut out, max_out)?;
            }
            2 => {
                let (litlen, dist) = read_dynamic_tables(&mut br)?;
                inflate_huffman_block(&mut br, &litlen, &dist, &mut out, max_out)?;
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
    max_out: usize,
) -> Result<(), String> {
    loop {
        let sym = litlen.decode(br)?;
        match sym {
            0..=255 => {
                if out.len() >= max_out {
                    return Err(format!("deflate: output exceeds expected {max_out} bytes"));
                }
                out.push(sym as u8);
            }
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
                if out.len() + len > max_out {
                    return Err(format!("deflate: output exceeds expected {max_out} bytes"));
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
#[path = "png_tests.rs"]
mod tests;
