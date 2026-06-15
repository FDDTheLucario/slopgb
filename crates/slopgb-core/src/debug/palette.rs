//! Palette decoders for the bgb VRAM viewer's Palettes tab. CGB palette RAM
//! is the exact 15-bit BGR555 words bgb prints (e.g. `7FFF`); DMG palettes are
//! the BGP/OBP shade-index mappings. The swatch *colour* bgb draws applies a
//! CGB colour-correction curve on top of the raw word — that is a rendering
//! concern for the viewer (see plan task C13), so the naive expansion here is
//! documented as such, not presented as bgb-accurate.

/// The four raw 15-bit BGR555 colour words of CGB palette `pal` (0..=7) from a
/// 64-byte palette-RAM half ([`crate::GameBoy::cgb_palette_ram`]). Each colour
/// is two little-endian bytes. Out-of-range reads yield 0.
#[must_use]
pub fn cgb_palette_words(cram: &[u8], pal: usize) -> [u16; 4] {
    let mut out = [0u16; 4];
    for (c, word) in out.iter_mut().enumerate() {
        let i = pal * 8 + c * 2;
        let lo = cram.get(i).copied().unwrap_or(0);
        let hi = cram.get(i + 1).copied().unwrap_or(0);
        *word = u16::from(lo) | (u16::from(hi) << 8);
    }
    out
}

/// The four shade indices (0..=3) a DMG palette register (BGP/OBP0/OBP1) maps
/// colour IDs 0..=3 to: bits 1-0 = colour 0's shade, 3-2 = colour 1's, etc.
#[must_use]
pub fn dmg_palette_shades(reg: u8) -> [u8; 4] {
    [reg & 3, (reg >> 2) & 3, (reg >> 4) & 3, (reg >> 6) & 3]
}

/// Naive 15-bit BGR555 word → 8-bit RGB (`channel * 255 / 31`), for a quick
/// swatch. **Not** bgb's CGB colour correction — match that in the viewer if
/// pixel parity is needed (plan C13).
#[must_use]
pub fn rgb555_to_rgb888(word: u16) -> (u8, u8, u8) {
    let expand = |c5: u16| ((c5 * 255 + 15) / 31) as u8;
    let r = expand(word & 0x1F);
    let g = expand((word >> 5) & 0x1F);
    let b = expand((word >> 10) & 0x1F);
    (r, g, b)
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
