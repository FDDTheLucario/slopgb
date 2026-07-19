use super::*;

#[test]
fn tile_pixels_decodes_2bpp_planar() {
    // One tile: row 0 lo=0b1010_0000, hi=0b1100_0000 -> indices 3,1,2,0,0,0,0,0
    // (col0: hi=1,lo=1 ->3; col1: hi=1,lo=0 ->2; col2: hi=0,lo=1 ->1).
    let mut vram = vec![0u8; 0x4000];
    vram[0] = 0b1010_0000; // plane 0 (lo)
    vram[1] = 0b1100_0000; // plane 1 (hi)
    let t = tile_pixels(&vram, 0, 0);
    assert_eq!(t[0][0], 3);
    assert_eq!(t[0][1], 2);
    assert_eq!(t[0][2], 1);
    assert_eq!(t[0][3], 0);
    assert_eq!(t[1], [0; 8]); // untouched row
}

#[test]
fn tile_pixels_addresses_bank_and_index() {
    let mut vram = vec![0u8; 0x4000];
    // tile 1 in bank 1 starts at 0x2000 + 16.
    let base = 0x2000 + 16;
    vram[base] = 0xFF; // every plane-0 bit set
    vram[base + 1] = 0xFF; // every plane-1 bit set -> all index 3
    let t = tile_pixels(&vram, 1, 1);
    assert_eq!(t[0], [3; 8]);
    // Same offset in bank 0 is untouched.
    assert_eq!(tile_pixels(&vram, 0, 1)[0], [0; 8]);
}

#[test]
fn tile_pixels_out_of_range_is_zero() {
    let vram = [0u8; 16]; // far too short for tile 5
    assert_eq!(tile_pixels(&vram, 0, 5), [[0u8; 8]; 8]);
}

#[test]
fn oam_sprites_reads_all_forty_entries() {
    let mut oam = vec![0u8; 0xA0];
    // entry 0
    oam[0..4].copy_from_slice(&[16, 8, 0x42, 0xA0]);
    // entry 39 (last)
    oam[39 * 4..39 * 4 + 4].copy_from_slice(&[150, 80, 0x10, 0x07]);
    let s = oam_sprites(&oam);
    assert_eq!(s.len(), 40);
    assert_eq!(
        s[0],
        Sprite {
            y: 16,
            x: 8,
            tile: 0x42,
            attr: 0xA0
        }
    );
    assert_eq!(
        s[39],
        Sprite {
            y: 150,
            x: 80,
            tile: 0x10,
            attr: 0x07
        }
    );
}

#[test]
fn oam_sprites_short_slice_pads_zero() {
    let s = oam_sprites(&[5, 6]); // only entry 0's y,x present
    assert_eq!(s[0].y, 5);
    assert_eq!(s[0].x, 6);
    assert_eq!(s[0].tile, 0);
    assert_eq!(
        s[39],
        Sprite {
            y: 0,
            x: 0,
            tile: 0,
            attr: 0
        }
    );
}

#[test]
fn bg_tile_index_resolves_signed_and_unsigned_addressing() {
    assert_eq!(bg_tile_index(0, false), 0);
    assert_eq!(bg_tile_index(255, false), 255);
    // Signed (0x8800): byte is i8 relative to tile 256.
    assert_eq!(bg_tile_index(0, true), 256);
    assert_eq!(bg_tile_index(127, true), 383);
    assert_eq!(bg_tile_index(0x80, true), 128); // -128
    assert_eq!(bg_tile_index(0xFF, true), 255); // -1
}

#[test]
fn tile_guess_from_bg_cell_uses_cgb_palette_and_bank() {
    let mut vram = vec![0u8; 0x4000];
    // 0x9800 cell 0 → tile 5, attr palette 3 + VRAM bank 1 (bit 3).
    vram[0x1800] = 5;
    vram[0x2000 + 0x1800] = 0x0B; // bits2-0 = 3, bit3 = 1
    let g = tile_palette_guess(&vram, &[0u8; 0xA0], false, false, true);
    assert_eq!(
        g[1][5],
        Some(PaletteRef {
            obj: false,
            index: 3
        })
    );
    assert_eq!(g[0][5], None, "different bank untouched");
    // Tile 0 is referenced by every zero-filled map cell, so it is guessed BG 0;
    // a tile no cell names stays grey.
    assert_eq!(g[0][200], None, "unreferenced tile stays grey");
}

#[test]
fn tile_guess_signed_addressing_maps_byte_to_tile_256_block() {
    let mut vram = vec![0u8; 0x4000];
    vram[0x1800] = 0; // signed byte 0 → tile 256
    let g = tile_palette_guess(&vram, &[0u8; 0xA0], true, false, true);
    assert_eq!(g[0][256].map(|p| p.obj), Some(false));
    assert_eq!(g[0][0], None);
}

#[test]
fn tile_guess_obj_fills_unreferenced_and_bg_wins() {
    let mut vram = vec![0u8; 0x4000];
    // BG cell → tile 7, palette 1.
    vram[0x1800] = 7;
    vram[0x2000 + 0x1800] = 0x01;
    let mut oam = vec![0u8; 0xA0];
    oam[0..4].copy_from_slice(&[16, 8, 7, 0x05]); // sprite also uses tile 7, OBJ pal 5
    oam[4..8].copy_from_slice(&[16, 8, 20, 0x04]); // sprite uses tile 20, OBJ pal 4
    let g = tile_palette_guess(&vram, &oam, false, false, true);
    // Tile 7 referenced by both → BG wins.
    assert_eq!(
        g[0][7],
        Some(PaletteRef {
            obj: false,
            index: 1
        })
    );
    // Tile 20 only by a sprite → OBJ palette.
    assert_eq!(
        g[0][20],
        Some(PaletteRef {
            obj: true,
            index: 4
        })
    );
}

#[test]
fn tile_guess_dmg_obj_uses_obp_bit_and_tall_covers_two_tiles() {
    let mut oam = vec![0u8; 0xA0];
    // 8×16 sprite, tile 0x11 → covers 0x10 and 0x11; OBP bit4 set → index 1.
    oam[0..4].copy_from_slice(&[16, 8, 0x11, 0x10]);
    let g = tile_palette_guess(&[0u8; 0x4000], &oam, false, true, false);
    let want = Some(PaletteRef {
        obj: true,
        index: 1,
    });
    assert_eq!(g[0][0x10], want);
    assert_eq!(g[0][0x11], want);
}

#[test]
fn bg_map_reads_tile_from_bank0_and_attr_from_bank1() {
    let mut vram = vec![0u8; 0x4000];
    // 0x9800 map: offset 0x1800. cell (row 1, col 2) = 1*32 + 2 = index 34.
    let cell = 34;
    vram[0x1800 + cell] = 0x7F; // tile index, bank 0
    vram[0x2000 + 0x1800 + cell] = 0x68; // attribute, bank 1
    let map = bg_map(&vram, 0x9800);
    assert_eq!(
        map[cell],
        MapCell {
            tile: 0x7F,
            attr: 0x68
        }
    );
    assert_eq!(map.len(), 1024);
    // 0x9C00 map (offset 0x1C00) is a separate region.
    vram[0x1C00] = 0x11;
    assert_eq!(bg_map(&vram, 0x9C00)[0].tile, 0x11);
    assert_eq!(bg_map(&vram, 0x9800)[0].tile, 0x00);
}
