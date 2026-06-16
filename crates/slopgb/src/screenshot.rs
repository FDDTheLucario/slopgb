//! Frame → 24-bit BMP, std-only (no PNG encoder, which would need a dep — the
//! frontend stays winit/softbuffer/cpal-only). bgb's "Save screenshot" writes a
//! viewable image; an uncompressed BMP opens everywhere. The encoder is pure so
//! it tests headless; `main` picks the path and writes the bytes.

/// Encode an XRGB8888 `frame` (`w × h`, row-major, top-down) as an uncompressed
/// 24-bit BMP. Rows are written bottom-up (BMP's default) as BGR.
#[must_use]
pub fn to_bmp(frame: &[u32], w: usize, h: usize) -> Vec<u8> {
    // 160×3 = 480 is already a multiple of 4, but pad defensively for any size.
    let row_bytes = w * 3;
    let pad = (4 - row_bytes % 4) % 4;
    let stride = row_bytes + pad;
    let pixels = stride * h;
    const HEADER: usize = 54; // 14-byte file header + 40-byte info header
    let mut out = Vec::with_capacity(HEADER + pixels);

    // BITMAPFILEHEADER.
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((HEADER + pixels) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&(HEADER as u32).to_le_bytes()); // pixel data offset
    // BITMAPINFOHEADER.
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // positive ⇒ bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB (no compression)
    out.extend_from_slice(&(pixels as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 DPI x
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 DPI y
    out.extend_from_slice(&0u32.to_le_bytes()); // colors used
    out.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // Pixel data, bottom row first, each pixel as B, G, R. Rows missing from a
    // short `frame` are emitted black, so a mismatched length yields a valid
    // (if partly blank) BMP instead of panicking.
    for y in (0..h).rev() {
        let row = frame.get(y * w..y * w + w);
        for x in 0..w {
            let px = row.map_or(0, |r| r[x]);
            out.push((px & 0xFF) as u8); // B
            out.push(((px >> 8) & 0xFF) as u8); // G
            out.push(((px >> 16) & 0xFF) as u8); // R
        }
        out.extend(std::iter::repeat_n(0u8, pad));
    }
    out
}

#[cfg(test)]
#[path = "screenshot_tests.rs"]
mod tests;
