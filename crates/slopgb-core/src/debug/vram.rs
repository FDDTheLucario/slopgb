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

/// A guessed display palette for a raw tile: which palette set (BG vs OBJ) and
/// index, inferred from where the tile is referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteRef {
    /// `true` = OBJ palette (OBP/CGB OBJ CRAM); `false` = BG palette (BGP/CGB BG
    /// CRAM).
    pub obj: bool,
    /// Palette index: CGB 0-7. DMG BG is always 0 (BGP); DMG OBJ is 0/1
    /// (OBP0/OBP1, from OAM attr bit 4).
    pub index: u8,
}

/// A BG-map tile byte's absolute tile index (0..=383) under the current
/// tile-data addressing mode. Unsigned (LCDC.4=1, 0x8000 base): index = byte.
/// Signed (LCDC.4=0, 0x8800/0x9000 base): byte is `i8` around tile 256, so
/// 0..127 → 256..383 and 128..255 → 128..255.
#[must_use]
pub fn bg_tile_index(byte: u8, signed: bool) -> usize {
    if signed {
        (256 + i32::from(byte as i8)) as usize
    } else {
        byte as usize
    }
}

/// Guess a display palette for every tile in each VRAM bank by finding a
/// reference to it — bgb's Tiles "show paletted". A raw tile carries no palette,
/// so infer one from usage: scan both BG tilemaps (BG palette + CGB attr bank),
/// then OAM sprites (OBJ palette) for tiles no BG cell referenced. BG wins when a
/// tile is used by both. `signed` is BG tile-data addressing (LCDC.4=0 → 0x8800
/// signed); `tall` is 8×16 OBJ (LCDC.2, so a sprite covers `tile&!1` and
/// `tile|1`); `cgb` selects CGB attr palettes vs the DMG OBP bit. Unreferenced
/// tiles stay `None` (caller renders them neutral grey).
#[must_use]
pub fn tile_palette_guess(
    vram: &[u8],
    oam: &[u8],
    signed: bool,
    tall: bool,
    cgb: bool,
) -> [[Option<PaletteRef>; 384]; 2] {
    let mut guess = [[None; 384]; 2];
    // BG first (first reference wins, and BG wins over OBJ). Both tilemaps.
    for base in [0x9800u16, 0x9C00] {
        for cell in bg_map(vram, base) {
            let bank = if cgb { usize::from(cell.attr >> 3 & 1) } else { 0 };
            let slot = &mut guess[bank][bg_tile_index(cell.tile, signed)];
            if slot.is_none() {
                *slot = Some(PaletteRef {
                    obj: false,
                    index: if cgb { cell.attr & 7 } else { 0 },
                });
            }
        }
    }
    // OBJ fills only tiles no BG cell claimed.
    for s in oam_sprites(oam) {
        if s.y == 0 && s.x == 0 {
            continue; // unused OAM slot
        }
        let bank = if cgb { usize::from(s.attr >> 3 & 1) } else { 0 };
        let pal = PaletteRef {
            obj: true,
            index: if cgb { s.attr & 7 } else { s.attr >> 4 & 1 },
        };
        // Sprites use 0x8000 unsigned addressing; 8×16 covers two stacked tiles.
        let top = if tall { s.tile & 0xFE } else { s.tile } as usize;
        let tiles: &[usize] = if tall { &[top, top + 1] } else { &[top] };
        for &t in tiles {
            let slot = &mut guess[bank][t];
            if slot.is_none() {
                *slot = Some(pal);
            }
        }
    }
    guess
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
