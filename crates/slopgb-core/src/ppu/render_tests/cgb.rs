//! `render_tests` — cgb tests (split for file size).

use super::*;

#[test]
fn cgb_color_expansion() {
    let p = cgb_on(0x91);
    assert_eq!(p.cgb_color(&p.bg_pal_ram, 0, 0), CGB_WHITE);
    assert_eq!(p.cgb_color(&p.bg_pal_ram, 1, 1), RED);
    // 5->8 bit expansion: (c << 3) | (c >> 2).
    let mut q = cgb_on(0x91);
    q.bg_pal_ram[0] = 0x10; // red = 16
    q.bg_pal_ram[1] = 0x00;
    assert_eq!(q.cgb_color(&q.bg_pal_ram, 0, 0), 0x84_0000);
}

#[test]
fn cgb_bg_attributes_palette_bank_flips() {
    let mut p = cgb_on(0x91);
    // Tile 1 data in bank 1 only; bank 0 left zero.
    set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00); // leftmost pixel color 1
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x09; // palette 1, bank 1
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), RED, "bank 1 data, palette 1");
    assert_eq!(px(&p, 2, 1), CGB_WHITE);

    // X flip.
    let mut p = cgb_on(0x91);
    set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00);
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x29; // + X flip
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), CGB_WHITE);
    assert_eq!(px(&p, 2, 7), RED);

    // Y flip: line 2 fetches tile row 5.
    let mut p = cgb_on(0x91);
    set_tile_row(&mut p, 1, 1, 5, 0x80, 0x00);
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x49; // + Y flip
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), RED);
}

#[test]
fn cgb_sprite_priority_by_oam_index() {
    let mut p = cgb_on(0x93);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
    sprite(&mut p, 0, 18, 20, 4, 0x01); // idx 0, obj palette 1 (blue)
    sprite(&mut p, 1, 18, 18, 4, 0x00); // idx 1, palette 0, lower X
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 14), BLUE, "CGB: lower OAM index wins overlap");
    // OPRI bit 0 set: DMG-style X priority.
    let mut p = cgb_on(0x93);
    p.write(0xFF6C, 1);
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    sprite(&mut p, 0, 18, 20, 4, 0x01);
    sprite(&mut p, 1, 18, 18, 4, 0x00);
    render_line(&mut p, 2);
    assert_ne!(px(&p, 2, 14), BLUE, "OPRI=1: lower X wins");
}

#[test]
fn cgb_bg_priority_and_master_priority() {
    // BG attr bit 7 set, BG color nonzero: BG wins...
    let mut p = cgb_on(0x93);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00); // bg cols 0-3 color 1
    set_map(&mut p, 0x1800, 0, 0, 1);
    p.vram[0x2000 + 0x1800] = 0x81; // priority + palette 1
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    sprite(&mut p, 0, 18, 8, 4, 0x01); // obj palette 1 (blue)
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), RED, "BG attr priority beats sprite");
    assert_eq!(px(&p, 2, 4), BLUE, "BG color 0 always loses");

    // ...unless LCDC bit 0 is clear: master priority off.
    let mut p = cgb_on(0x92);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00);
    set_map(&mut p, 0x1800, 0, 0, 1);
    set_map(&mut p, 0x1800, 0, 2, 1);
    p.vram[0x2000 + 0x1800] = 0x81;
    p.vram[0x2000 + 0x1802] = 0x81;
    set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
    sprite(&mut p, 0, 18, 8, 4, 0x81); // even OAM bit 7 set
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), BLUE, "LCDC0=0 strips all BG priority");
    // And the BG itself still renders (not blanked like DMG).
    assert_eq!(px(&p, 2, 9), CGB_WHITE);
    assert_eq!(px(&p, 2, 16), RED, "BG drawn where no sprite covers it");
}

#[test]
fn cgb_vbk_banks() {
    let mut p = cgb_on(0x91);
    run_to(&mut p, 145, 0); // vblank: VRAM accessible
    assert_eq!(p.read(0xFF4F), 0xFE);
    p.write(0x8000, 0x11);
    p.write(0xFF4F, 1);
    assert_eq!(p.read(0xFF4F), 0xFF);
    assert_eq!(p.read(0x8000), 0);
    p.write(0x8000, 0x22);
    assert_eq!(p.read(0x8000), 0x22);
    assert_eq!(p.vram_read_raw(0x8000), 0x22);
    p.vram_write_raw(0x9FFF, 0x33);
    assert_eq!(p.vram[0x3FFF], 0x33);
    p.write(0xFF4F, 0xFE); // only bit 0 counts
    assert_eq!(p.read(0x8000), 0x11);
    assert_eq!(p.vram_read_raw(0x8000), 0x11);
}

#[test]
fn cgb_palette_registers() {
    let mut p = cgb_on(0x91);
    run_to(&mut p, 145, 0);
    p.write(0xFF68, 0x80); // index 0, auto-increment
    p.write(0xFF69, 0x1F);
    p.write(0xFF69, 0x00);
    assert_eq!(p.read(0xFF68), 0x40 | 0x82);
    assert_eq!(p.bg_pal_ram[0], 0x1F);
    assert_eq!(p.bg_pal_ram[1], 0x00);
    p.write(0xFF68, 0x00);
    assert_eq!(p.read(0xFF69), 0x1F, "read back without increment");
    assert_eq!(p.read(0xFF68), 0x40, "reads have bit 6 set");

    p.write(0xFF6A, 0x80 | 0x10);
    p.write(0xFF6B, 0xAA);
    assert_eq!(p.obj_pal_ram[0x10], 0xAA);
    assert_eq!(p.read(0xFF6A), 0x40 | 0x91);
}

#[test]
fn cgb_palette_ram_blocked_in_mode3() {
    let mut p = cgb_on(0x91);
    p.bg_pal_ram[0] = 0x12;
    run_to(&mut p, 1, 100); // mode 3
    assert_eq!(p.read(0xFF41) & 3, 3);
    p.write(0xFF68, 0x80);
    assert_eq!(p.read(0xFF69), 0xFF, "reads blocked during mode 3");
    p.write(0xFF69, 0x77);
    assert_eq!(p.bg_pal_ram[0], 0x12, "write dropped during mode 3");
    assert_eq!(
        p.read(0xFF68) & 0x3F,
        1,
        "auto-increment still happens on a blocked write (Pan Docs)"
    );
}

#[test]
fn dmg_cgb_registers_unmapped() {
    let mut p = dmg_on(0x91);
    assert_eq!(p.read(0xFF4F), 0xFF);
    assert_eq!(p.read(0xFF68), 0xFF);
    assert_eq!(p.read(0xFF69), 0xFF);
    assert_eq!(p.read(0xFF6C), 0xFF);
    p.write(0xFF4F, 1); // ignored
    p.write(0x9000, 0x55);
    run_to(&mut p, 150, 0);
    assert_eq!(p.read(0x9000), 0x55);
}

#[test]
fn set_dmg_palette_applies() {
    let mut p = dmg_on(0x91);
    p.set_dmg_palette([0x11, 0x22, 0x33, 0x44]);
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
    set_map(&mut p, 0x1800, 0, 0, 1);
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), 0x22);
    assert_eq!(px(&p, 2, 4), 0x33);
    assert_eq!(px(&p, 2, 8), 0x11);
}

/// End-to-end DMG-compat rendering through the CGB boot ROM's *default*
/// compatibility palettes (Pan Docs "Compatibility palettes"; SameBoy
/// cgb_boot.asm combination OBJ0=4, OBJ1=4, BG=29): BG pixels remap
/// through BGP into the BG table, OBJ pixels through OBP0/OBP1 into the
/// distinct OBJ table. Expected XRGB values follow the c-sp collection's
/// `(X << 3) | (X >> 2)` channel expansion (dmg-acid2 README).
#[test]
fn cgb_compat_default_palette_render() {
    let mut p = Ppu::new(Model::Cgb);
    p.set_dmg_compat(true);
    // Install the boot defaults through the palette ports (LCD off — no
    // mode-3 blocking), exactly as `apply_post_boot_state` does.
    p.write(0xFF68, 0x80);
    for c in [0x7FFFu16, 0x1BEF, 0x6180, 0x0000] {
        p.write(0xFF69, c as u8);
        p.write(0xFF69, (c >> 8) as u8);
    }
    p.write(0xFF6A, 0x80);
    for _ in 0..2 {
        for c in [0x7FFFu16, 0x421F, 0x1CF2, 0x0000] {
            p.write(0xFF6B, c as u8);
            p.write(0xFF6B, (c >> 8) as u8);
        }
    }
    p.write(0xFF47, 0xE4); // identity BGP
    p.write(0xFF48, 0xE4); // identity OBP0
    set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
    set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // shade 3
    set_map(&mut p, 0x1800, 0, 0, 1);
    set_map(&mut p, 0x1800, 0, 1, 2);
    set_tile_row(&mut p, 0, 3, 0, 0xF0, 0x0F); // sprite: 1s then 2s
    sprite(&mut p, 0, 18, 48, 3, 0); // line 2 row 0, screen x 40-47, OBP0
    p.write(0xFF40, 0x93); // LCD + BG + OBJ on
    render_line(&mut p, 2);
    assert_eq!(px(&p, 2, 0), 0x7BFF31, "BG shade 1");
    assert_eq!(px(&p, 2, 4), 0x0063C6, "BG shade 2");
    assert_eq!(px(&p, 2, 8), 0x00_0000, "BG shade 3");
    assert_eq!(px(&p, 2, 16), 0xFF_FFFF, "BG shade 0");
    assert_eq!(px(&p, 2, 40), 0xFF8484, "OBJ shade 1");
    assert_eq!(px(&p, 2, 44), 0x943939, "OBJ shade 2");
}

#[test]
fn frame_buffer_double_buffering() {
    let mut p = dmg_on(0x91);
    set_tile_row(&mut p, 0, 1, 0, 0xFF, 0xFF);
    set_map(&mut p, 0x1800, 0, 0, 1);
    // The frame right after the LCD enable is presented blank (see
    // `first_frame_after_lcd_enable_is_blank`); double buffering is
    // observable from the second frame on.
    run_to(&mut p, 144, 0);
    run_to(&mut p, 143, 455);
    assert_eq!(p.frame()[0], WHITE, "frame() is the completed frame");
    p.tick(); // 144:0 -> swap
    assert_eq!(p.frame()[0], BLACK);
}
