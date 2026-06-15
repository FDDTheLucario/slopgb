use super::*;

const T: Theme = Theme::BGB;

#[test]
fn tab_labels_and_render() {
    assert_eq!(
        VramTab::ALL.map(VramTab::label),
        ["BG map", "Tiles", "OAM", "Palettes"]
    );
    let (w, h) = (260usize, 20usize);
    let mut buf = vec![0u32; w * h];
    let mut c = Canvas::new(&mut buf, w, h);
    let rects = render_tabs(&mut c, 0, 0, VramTab::Oam, &T);
    assert_eq!(rects.len(), 4);
}

#[test]
fn render_tiles_blits_tile_zero_into_the_top_left_cell() {
    let mut vram = vec![0u8; 0x4000];
    for b in &mut vram[0..16] {
        *b = 0xFF; // tile 0: every pixel index 3
    }
    let scale = 2;
    let (w, h) = (16 * 8 * scale as usize, 24 * 8 * scale as usize);
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_tiles(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            &vram,
            0,
            &GREYS,
            scale,
        );
    }
    // Tile 0's top-left scaled block is palette index 3 = GREYS[3] (black).
    assert_eq!(buf[0], GREYS[3]);
    // Tile 1 (next 16 bytes are zero -> index 0 = white) sits at x = 8*scale.
    assert_eq!(buf[8 * scale as usize], GREYS[0]);
}

#[test]
fn render_oam_draws_present_sprites_and_skips_empty() {
    let mut vram = vec![0u8; 0x4000];
    // tile 5 = all index 3.
    for b in &mut vram[5 * 16..5 * 16 + 16] {
        *b = 0xFF;
    }
    let mut oam = vec![0u8; 0xA0];
    oam[0..4].copy_from_slice(&[16, 8, 5, 0]); // sprite 0: present, tile 5
    // sprite 1 left all-zero -> empty.
    let scale = 1;
    let (w, h) = (8 * (8 * scale as usize + 4), 5 * (8 * scale as usize + 4));
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_oam(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            &oam,
            &vram,
            &GREYS,
            scale,
        );
    }
    // Cell 0 shows tile 5 (black); cell 1 (empty sprite) stays untouched.
    assert_eq!(buf[0], GREYS[3], "sprite 0 cell drawn");
    let cell = 8 * scale as usize + 4;
    assert_eq!(buf[cell], 0x0012_3456, "empty sprite 1 cell blank");
}

#[test]
fn render_palettes_expands_cgb_colour_words() {
    let mut bg = vec![0u8; 64];
    bg[0] = 0xFF; // palette 0, colour 0 = 0x7FFF -> white
    bg[1] = 0x7F;
    let obj = vec![0u8; 64];
    let (w, h) = (200usize, 160usize);
    let mut buf = vec![0x0012_3456_u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render_palettes(&mut c, Rect::new(0, 0, w as i32, h as i32), &bg, &obj, &T);
    }
    // First swatch's interior is white (0x7FFF expanded).
    assert_eq!(buf[2 * w + 2], 0x00FF_FFFF);
}
