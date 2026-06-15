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

/// One BG/Window tilemap cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapCell {
    /// Tile index (VRAM bank 0 at the map offset).
    pub tile: u8,
    /// CGB attribute byte (VRAM bank 1, same offset; 0 on DMG): bit7 BG
    /// priority, bit6 Y-flip, bit5 X-flip, bit3 tile VRAM bank, bits2-0 BG
    /// palette.
    pub attr: u8,
}

/// Decode a 32×32 BG/Window tilemap into 1024 cells, row-major (32 per row).
/// `base` is 0x9800 or 0x9C00 (only the VRAM offset bits matter). The tile
/// index comes from bank 0 and the CGB attribute from bank 1 at the same
/// offset. Note: mapping a cell's `tile` to pixels still needs the LCDC
/// tile-data area (0x8000 unsigned vs 0x8800 signed) — that's the caller's
/// choice, applied through [`tile_pixels`].
#[must_use]
pub fn bg_map(vram: &[u8], base: u16) -> [MapCell; 1024] {
    let off = (base & 0x1FFF) as usize; // 0x1800 or 0x1C00
    let mut out = [MapCell { tile: 0, attr: 0 }; 1024];
    for (i, cell) in out.iter_mut().enumerate() {
        cell.tile = vram.get(off + i).copied().unwrap_or(0);
        cell.attr = vram.get(0x2000 + off + i).copied().unwrap_or(0);
    }
    out
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
