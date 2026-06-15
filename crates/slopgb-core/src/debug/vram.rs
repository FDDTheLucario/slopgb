//! Pure decoders over the raw VRAM/OAM bytes ([`crate::GameBoy::vram`] /
//! [`crate::GameBoy::oam`]) for the bgb VRAM viewer's Tiles and OAM tabs.
//! Kept out of the PPU (which only hands over raw bytes) so they unit-test
//! without a running machine.

/// Decode one 8×8 tile into a row-major grid of 2-bit colour indices (0..=3).
///
/// `vram` is the whole 16 KiB ([`crate::GameBoy::vram`]); `bank` is 0 or 1
/// (CGB second bank); `tile` is 0..=383, the 16-byte tiles counting from
/// 0x8000 within that bank. Classic 2bpp planar format: two bytes per row,
/// bit 7 = leftmost pixel, the low byte carries plane-0 (bit 0 of each index)
/// and the high byte plane-1 (bit 1). Out-of-range reads decode as 0.
#[must_use]
pub fn tile_pixels(vram: &[u8], bank: usize, tile: usize) -> [[u8; 8]; 8] {
    let base = bank * 0x2000 + tile * 16;
    let mut out = [[0u8; 8]; 8];
    for (row, cells) in out.iter_mut().enumerate() {
        let lo = vram.get(base + row * 2).copied().unwrap_or(0);
        let hi = vram.get(base + row * 2 + 1).copied().unwrap_or(0);
        for (col, cell) in cells.iter_mut().enumerate() {
            let bit = 7 - col;
            *cell = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1);
        }
    }
    out
}

/// One OAM sprite entry, as bgb's OAM tab lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprite {
    /// OAM byte 0: screen Y position + 16 (so 16 = top edge of the screen).
    pub y: u8,
    /// OAM byte 1: screen X position + 8 (so 8 = left edge of the screen).
    pub x: u8,
    /// OAM byte 2: tile index.
    pub tile: u8,
    /// OAM byte 3: attributes — bit7 BG-priority, bit6 Y-flip, bit5 X-flip,
    /// bit4 DMG palette (OBP0/1), bit3 CGB VRAM bank, bits2-0 CGB OBJ palette.
    pub attr: u8,
}

/// The 40 OAM sprite entries in OAM order ([`crate::GameBoy::oam`]). A short
/// slice decodes its missing entries as all-zero.
#[must_use]
pub fn oam_sprites(oam: &[u8]) -> [Sprite; 40] {
    let mut out = [Sprite {
        y: 0,
        x: 0,
        tile: 0,
        attr: 0,
    }; 40];
    for (i, s) in out.iter_mut().enumerate() {
        let b = i * 4;
        let at = |o: usize| oam.get(b + o).copied().unwrap_or(0);
        *s = Sprite {
            y: at(0),
            x: at(1),
            tile: at(2),
            attr: at(3),
        };
    }
    out
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
