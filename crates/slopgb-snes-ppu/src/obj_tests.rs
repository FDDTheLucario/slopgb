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

/// xorshift32 — deterministic in-test randomness (no deps).
fn xs(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// A frozen copy of the per-pixel `obj_line` (the pre-chunk-decode shape):
/// the production renderer must stay slot-identical to it.
fn obj_line_ref(ppu: &SnesPpu, y: u16, out: &mut [Option<(u16, u8)>; 256]) {
    const SIZES: [(u16, u16, u16, u16); 8] = [
        (8, 8, 16, 16),
        (8, 8, 32, 32),
        (8, 8, 64, 64),
        (16, 16, 32, 32),
        (16, 16, 64, 64),
        (32, 32, 64, 64),
        (16, 32, 32, 64),
        (16, 32, 32, 32),
    ];
    out.fill(None);
    let base = usize::from(ppu.obsel & 7) << 13;
    let gap = usize::from(ppu.obsel >> 3 & 3) << 12;
    let (sw, sh, lw, lh) = SIZES[usize::from(ppu.obsel >> 5)];
    let first = if ppu.oam_priority {
        usize::from(ppu.oam_reload >> 1) & 0x7F
    } else {
        0
    };
    let mut range = 0;
    let mut slots = 34;
    for i in 0..128 {
        let n = (first + i) & 0x7F;
        let e = &ppu.oam[n * 4..n * 4 + 4];
        let hi = ppu.oam[0x200 + n / 4] >> (n % 4 * 2);
        let (w, h) = if hi & 2 != 0 { (lw, lh) } else { (sw, sh) };
        let row = y.wrapping_sub(u16::from(e[1])) & 0xFF;
        if row >= h {
            continue;
        }
        if range == 32 {
            break;
        }
        range += 1;
        let x9 = u16::from(e[0]) | u16::from(hi & 1) << 8;
        let sx = if x9 >= 256 {
            i32::from(x9) - 512
        } else {
            i32::from(x9)
        };
        let attr = e[3];
        let tile = u16::from(e[2]) | u16::from(attr & 1) << 8;
        let pal = usize::from(attr >> 1 & 7);
        let prio = attr >> 4 & 3;
        let fy = if attr & 0x80 != 0 { h - 1 - row } else { row };
        let trow = tile & 0x100 | (tile >> 4).wrapping_add(fy / 8) << 4 & 0xF0 | tile & 0xF;
        for chunk in 0..w / 8 {
            let on_screen = (0..8).any(|p| {
                let x = sx + i32::from(chunk * 8 + p);
                (0..256).contains(&x)
            });
            if !on_screen {
                continue;
            }
            if slots == 0 {
                return;
            }
            slots -= 1;
            for p in 0..8u16 {
                let c = chunk * 8 + p;
                let x = sx + i32::from(c);
                if !(0..256).contains(&x) {
                    continue;
                }
                let slot = &mut out[x as usize];
                if slot.is_some() {
                    continue;
                }
                let src = if attr & 0x40 != 0 { w - 1 - c } else { c };
                let t = trow & 0x1F0 | trow.wrapping_add(src / 8) & 0xF;
                let word = base
                    + usize::from(t) * 16
                    + if t >= 0x100 { gap } else { 0 }
                    + usize::from(fy & 7);
                let bit = 7 - (src & 7);
                let w0 = ppu.vram[word & 0x7FFF];
                let w1 = ppu.vram[(word + 8) & 0x7FFF];
                let idx = usize::from(
                    w0 >> bit & 1
                        | (w0 >> 8 >> bit & 1) << 1
                        | (w1 >> bit & 1) << 2
                        | (w1 >> 8 >> bit & 1) << 3,
                );
                if idx != 0 {
                    *slot = Some((ppu.cgram[0x80 + pal * 16 + idx] & 0x7FFF, prio));
                }
            }
        }
    }
}

/// The production `obj_line` stays slot-identical to the frozen per-pixel
/// reference over fuzzed OAM/OBSEL/rotation states.
#[test]
fn obj_line_matches_reference_over_fuzzed_states() {
    let mut s = 0xDEAD_BEEFu32;
    for case in 0..200 {
        let mut ppu = SnesPpu::new();
        for _ in 0..4096 {
            let i = xs(&mut s) as usize & 0x7FFF;
            ppu.vram[i] = xs(&mut s) as u16;
        }
        for c in ppu.cgram.iter_mut() {
            *c = xs(&mut s) as u16;
        }
        for b in ppu.oam.iter_mut() {
            *b = xs(&mut s) as u8;
        }
        ppu.obsel = xs(&mut s) as u8;
        ppu.oam_reload = xs(&mut s) as u16;
        ppu.oam_priority = xs(&mut s) & 1 != 0;
        for &y in &[0u16, 31, 100, 223] {
            let mut want = [None; 256];
            obj_line_ref(&ppu, y, &mut want);
            let mut got = [None; 256];
            ppu.obj_line(y, &mut got);
            assert_eq!(got[..], want[..], "case {case} y {y}");
        }
    }
}
