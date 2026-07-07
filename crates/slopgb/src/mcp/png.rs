//! Std-only PNG encoder: an XRGB8888 `w×h` framebuffer → an 8-bit RGB PNG.
//! bgb screenshots are BMP (`screenshot::to_bmp`), but MCP image content wants a
//! format every client renders, so the `vram` tool emits PNG. The DEFLATE stream
//! uses *stored* (uncompressed) blocks — the images are small and a real
//! compressor would be a lot of code for no gain here — wrapped in the required
//! zlib + Adler-32 framing with a per-chunk CRC-32. No dep (frontend stays
//! winit/softbuffer/cpal-only).
//
// ponytail: stored (level-0) DEFLATE — swap in a real compressor only if VRAM
// PNGs ever get big enough that transfer size matters (they don't: max ~256×256).

/// Encode `pixels` (XRGB8888, top-down, row-major, `w×h`) as an RGB PNG. Pixels
/// missing from a short slice are emitted black, so a length mismatch yields a
/// valid (if partly blank) image instead of panicking.
#[must_use]
pub fn encode(pixels: &[u32], w: usize, h: usize) -> Vec<u8> {
    // Raw filtered scanlines: each row is a 0x00 (no-filter) byte then RGB.
    let mut raw = Vec::with_capacity(h.saturating_mul(1 + w * 3));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let px = pixels.get(y * w + x).copied().unwrap_or(0);
            raw.push((px >> 16) as u8);
            raw.push((px >> 8) as u8);
            raw.push(px as u8);
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, color type 2 (RGB), no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Append a PNG chunk: `len` (BE) + type + data + CRC-32(type+data) (BE).
fn chunk(out: &mut Vec<u8>, ctype: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ctype);
    out.extend_from_slice(data);
    let mut crc = Crc::new();
    crc.update(ctype);
    crc.update(data);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

/// Wrap `raw` in a zlib stream of stored DEFLATE blocks (≤0xFFFF each).
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    // 0x78,0x01: 32 KiB window, deflate, no preset dict; 0x7801 % 31 == 0.
    let mut out = vec![0x78, 0x01];
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]); // one empty final block
    } else {
        let mut chunks = raw.chunks(0xFFFF).peekable();
        while let Some(c) = chunks.next() {
            out.push(u8::from(chunks.peek().is_none())); // BFINAL on the last, BTYPE=00
            let len = c.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(c);
        }
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

struct Crc(u32);

impl Crc {
    fn new() -> Self {
        Crc(0xFFFF_FFFF)
    }
    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let idx = ((self.0 ^ u32::from(byte)) & 0xFF) as usize;
            self.0 = (self.0 >> 8) ^ CRC_TABLE[idx];
        }
    }
    fn finish(self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

const CRC_TABLE: [u32; 256] = make_crc_table();

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

#[cfg(test)]
#[path = "png_tests.rs"]
mod tests;
