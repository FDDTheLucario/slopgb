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
