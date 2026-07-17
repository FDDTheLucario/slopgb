//! OBJ renderer tests, pinned to the fullsnes OAM/OBSEL laws.

use super::*;

fn oline(ppu: &SnesPpu, y: u16) -> [Option<(u16, u8)>; 256] {
    let mut out = [None; 256];
    ppu.obj_line(y, &mut out);
    out
}

/// Write OAM entry `n`: X low byte, Y, tile low byte, attributes.
fn entry(ppu: &mut SnesPpu, n: usize, x: u8, y: u8, tile: u8, attr: u8) {
    ppu.oam[n * 4..n * 4 + 4].copy_from_slice(&[x, y, tile, attr]);
}

/// An 8x8 sprite renders its rows at framebuffer lines Y..Y+7, samples its
/// 4bpp tile from the OBSEL base, and colors from the OBJ half of CGRAM
/// (palettes at 80h+ — fullsnes CGRAM indices).
#[test]
fn obj_8x8_rows_palette_and_priority() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x01, 0x01); // OBSEL: tiles at word $2000
    entry(&mut ppu, 0, 4, 10, 2, 1 << 1 | 2 << 4); // X=4 Y=10 tile 2 pal 1 prio 2
    let t = 0x2000 + 2 * 16;
    ppu.vram[t] = 0x0080; // row 0 pixel 0 = color 1
    ppu.vram[t + 7] = 0x0001; // row 7 pixel 7 = color 1
    ppu.cgram[0x80 + 16 + 1] = 0x7FFF;

    assert_eq!(oline(&ppu, 10)[4], Some((0x7FFF, 2)), "top row at line Y");
    assert_eq!(oline(&ppu, 10)[5], None, "color 0 transparent");
    assert_eq!(oline(&ppu, 17)[11], Some((0x7FFF, 2)), "row 7 at Y+7");
    assert!(
        oline(&ppu, 9).iter().all(Option::is_none),
        "nothing above Y"
    );
    assert!(oline(&ppu, 18).iter().all(Option::is_none), "nothing below");
}

/// The high-table X bit makes X 9-bit signed: X=$1FC is -4, clipping the
/// left half off-screen; flips mirror the fetch across the whole sprite.
#[test]
fn obj_negative_x_and_flips() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x01, 0x01);
    entry(&mut ppu, 0, 0xFC, 0, 2, 0xC0); // X=-4, X-flip + Y-flip
    ppu.oam[0x200] = 0x01; // OBJ 0 X bit 8
    let t = 0x2000 + 2 * 16;
    ppu.vram[t + 7] = 0x0080; // tile row 7, pixel 0
    ppu.cgram[0x80 + 1] = 0x1234;

    // Y-flip: sprite row 0 samples tile row 7. X-flip: sprite col 7
    // samples tile pixel 0; col 7 lands at screen x = -4 + 7 = 3.
    let out = oline(&ppu, 0);
    assert_eq!(out[3], Some((0x1234, 0)), "flipped pixel, clipped X");
    assert!(out[..3].iter().all(Option::is_none));
    assert!(out[4..].iter().all(Option::is_none));
}

/// The fullsnes 16x16 OBJ example verbatim: tile $1FF's right half is
/// $1F0 (x wraps in bits 3-0) and its lower half is $10F (y wraps in bits
/// 7-4, bit 8 fixed) — no carries between the fields.
#[test]
fn obj_16x16_tile_number_wraps_per_field() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x01, 0x00); // tiles at word 0, size sel 0 (8x8 / 16x16)
    entry(&mut ppu, 0, 0, 0, 0xFF, 0x01); // tile $1FF (attr bit 0 = bit 8)
    ppu.oam[0x200] = 0x02; // OBJ 0 large -> 16x16
    ppu.vram[0x1F0 * 16] = 0x0080; // tile $1F0 row 0 pixel 0
    ppu.vram[0x10F * 16] = 0x0040; // tile $10F row 0 pixel 1
    ppu.cgram[0x80 + 1] = 0x2468;

    assert_eq!(oline(&ppu, 0)[8], Some((0x2468, 0)), "right half = $1F0");
    assert_eq!(oline(&ppu, 8)[1], Some((0x2468, 0)), "lower half = $10F");
}

/// The OBSEL gap (bits 4-3, 4K-word steps) offsets tiles $100-$1FF only.
#[test]
fn obj_name_gap_applies_above_ff() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x01, 0x08); // base 0, gap 1 -> +$1000 words for tiles >= $100
    entry(&mut ppu, 0, 0, 0, 0x00, 0x01); // tile $100
    ppu.vram[(0x100 * 16 + 0x1000) & 0x7FFF] = 0x0080;
    ppu.cgram[0x80 + 1] = 0x1357;

    assert_eq!(oline(&ppu, 0)[0], Some((0x1357, 0)));
}

/// Overlap: the sprite earliest in evaluation order wins even when a later
/// one has higher priority bits; priority rotation (OAMADD bit 15) moves
/// the evaluation start to OBJ #N so #0 can lose; and the 33rd sprite on a
/// line is dropped (range over).
#[test]
fn obj_order_rotation_and_range_limit() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x01, 0x01);
    let t = 0x2000 + 2 * 16;
    ppu.vram[t] = 0x0080; // shared tile: pixel 0 = color 1
    ppu.cgram[0x80 + 1] = 0x1111; // palette 0
    ppu.cgram[0x80 + 16 + 1] = 0x2222; // palette 1

    entry(&mut ppu, 0, 0, 0, 2, 0); // pal 0, prio 0
    entry(&mut ppu, 1, 0, 0, 2, 1 << 1 | 3 << 4); // pal 1, prio 3
    assert_eq!(
        oline(&ppu, 0)[0],
        Some((0x1111, 0)),
        "OBJ 0 in front despite lower priority bits"
    );

    // Priority rotation: start evaluation at OBJ 1 — #N sits in the
    // reload's bits 7-1 (fullsnes 2102h), so OBJ 1 needs reload = 2.
    ppu.write(0x02, 0x02);
    ppu.write(0x03, 0x80); // rotation on
    assert_eq!(oline(&ppu, 0)[0], Some((0x2222, 3)), "OBJ 1 first");
    ppu.write(0x03, 0x00);
    ppu.write(0x02, 0x00);

    // 33 sprites share line 0; the 33rd in order renders nothing.
    for n in 0..33 {
        entry(&mut ppu, n, (n * 7) as u8, 0, 2, 0);
    }
    let out = oline(&ppu, 0);
    assert_eq!(out[31 * 7], Some((0x1111, 0)), "sprite 31 kept");
    assert_eq!(out[32 * 7], None, "sprite 32 dropped: range over");
}
