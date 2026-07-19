//! Frame-assembly tests: the priority chart, TM, backdrop, INIDISP.

use super::*;

/// Paint BG `bg` (bases spread per BG so layers don't collide): map entry
/// (0,0) with `prio`, tile pixel 0 = color 1, palette 0. The caller sets
/// the mode; bases are poked directly (the port paths are pinned by the
/// other test files) so one BG's NBA nibble never clobbers another's.
fn solid_bg(ppu: &mut SnesPpu, bg: usize, prio: bool, color: u16) {
    let map = 0x400 + bg * 0x400;
    let chars = 0x2000 + bg * 0x800;
    ppu.bgsc[bg] = (map >> 10 << 2) as u8;
    ppu.nba[bg / 2] |= ((chars >> 12) as u8) << (bg % 2 * 4);
    ppu.vram[map] = 2 | u16::from(prio) << 13;
    let bpp_words = if bg == 2 { 8 } else { 16 };
    ppu.vram[chars + 2 * bpp_words] = 0x0080; // pixel 0 = color 1
    ppu.cgram[1] = color; // palette 0 color 1 (these BGs share it)
}

/// An OBJ at (0, 0) with `prio`, colored via OBJ palette 0 color 1.
fn solid_obj(ppu: &mut SnesPpu, prio: u8, color: u16) {
    ppu.write(0x01, 0x02); // OBJ tiles at word $4000
    ppu.oam[..4].copy_from_slice(&[0, 0, 2, prio << 4]);
    ppu.vram[0x4000 + 2 * 16] = 0x0080; // pixel 0 = color 1
    ppu.cgram[0x81] = color;
}

fn px0(ppu: &SnesPpu) -> u16 {
    let mut out = [0u16; 256];
    ppu.render_line(0, &mut out);
    out[0]
}

/// Forced blank and brightness: bit 7 or N=0 blacks the screen, N=7 scales
/// channels by 8/16, N=15 is identity (fullsnes 2100h).
#[test]
fn inidisp_blank_and_brightness() {
    let mut ppu = SnesPpu::new();
    ppu.cgram[0] = 0x7FFF; // white backdrop
    ppu.write(0x00, 0x0F);
    assert_eq!(px0(&ppu), 0x7FFF, "full brightness is identity");
    ppu.write(0x00, 0x07);
    assert_eq!(px0(&ppu), 0x3DEF, "each 31-channel scaled by 8/16 -> 15");
    ppu.write(0x00, 0x00);
    assert_eq!(px0(&ppu), 0, "brightness 0 is black");
    ppu.write(0x00, 0x8F);
    assert_eq!(px0(&ppu), 0, "forced blank is black");
}

/// The mode-1 chart: BG2.1 covers BG1.0; dropping BG2's priority bit
/// flips the order (BG1.0 covers BG2.0); OBJ.2 sits between them.
#[test]
fn mode1_priority_chart_order() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x00, 0x0F);
    ppu.write(0x05, 0x01); // mode 1
    ppu.write(0x2C, 0x17); // TM: BG1+BG2+BG3+OBJ
    solid_bg(&mut ppu, 0, false, 0);
    solid_bg(&mut ppu, 1, true, 0);
    // Distinct colors: BG1/BG2/BG3 share cgram[1]; recolor via palettes
    // instead — give BG2 palette 1.
    ppu.vram[0x400 + 0x400] |= 1 << 10;
    ppu.cgram[1] = 0x0001; // BG1 color
    ppu.cgram[16 + 1] = 0x0002; // BG2 (palette 1) color

    assert_eq!(px0(&ppu), 0x0002, "BG2.1 above BG1.0");
    ppu.vram[0x400 + 0x400] &= !(1 << 13); // BG2 priority off
    assert_eq!(px0(&ppu), 0x0001, "BG1.0 above BG2.0");

    solid_obj(&mut ppu, 2, 0x0004);
    assert_eq!(px0(&ppu), 0x0004, "OBJ.2 above BG1.0");
    ppu.vram[0x400] |= 1 << 13; // BG1 priority on
    assert_eq!(px0(&ppu), 0x0001, "BG1.1 above OBJ.2");
}

/// BGMODE bit 3 hoists BG3.1 above OBJ.3; without it BG3.1 sinks below
/// OBJ.1 (the a/b rows of the mode-1 chart).
#[test]
fn mode1_bg3_priority_bit() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x00, 0x0F);
    ppu.write(0x05, 0x09); // mode 1 + BG3 priority
    ppu.write(0x2C, 0x14); // TM: BG3 + OBJ
    solid_bg(&mut ppu, 2, true, 0x0003);
    solid_obj(&mut ppu, 3, 0x0004);
    assert_eq!(px0(&ppu), 0x0003, "BG3.1a above OBJ.3");
    ppu.write(0x05, 0x01);
    assert_eq!(px0(&ppu), 0x0004, "without the bit, OBJ.3 wins");
}

/// TM masks layers off the main screen; with everything masked (or
/// transparent) the CGRAM-0 backdrop shows.
#[test]
fn tm_masks_and_backdrop_shows() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x00, 0x0F);
    ppu.write(0x05, 0x01);
    ppu.cgram[0] = 0x2222;
    solid_bg(&mut ppu, 0, true, 0x0001);
    ppu.write(0x2C, 0x01); // TM: BG1 only
    assert_eq!(px0(&ppu), 0x0001);
    ppu.write(0x2C, 0x00); // everything masked
    assert_eq!(px0(&ppu), 0x2222, "backdrop");
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

/// A frozen copy of the per-pixel priority merge (the pre-rung-outer
/// shape), calling the pinned `bg_line`/`obj_line` for layer content: the
/// production assembler must stay pixel-identical to it.
fn render_line_ref(ppu: &SnesPpu, y: u16, out: &mut [u16; 256]) {
    #[derive(Clone, Copy)]
    enum R {
        Bg(usize, bool),
        Obj(u8),
    }
    use R::*;
    const M0: [R; 12] = [
        Obj(3),
        Bg(0, true),
        Bg(1, true),
        Obj(2),
        Bg(0, false),
        Bg(1, false),
        Obj(1),
        Bg(2, true),
        Bg(3, true),
        Obj(0),
        Bg(2, false),
        Bg(3, false),
    ];
    const M1A: [R; 10] = [
        Bg(2, true),
        Obj(3),
        Bg(0, true),
        Bg(1, true),
        Obj(2),
        Bg(0, false),
        Bg(1, false),
        Obj(1),
        Obj(0),
        Bg(2, false),
    ];
    const M1B: [R; 10] = [
        Obj(3),
        Bg(0, true),
        Bg(1, true),
        Obj(2),
        Bg(0, false),
        Bg(1, false),
        Obj(1),
        Bg(2, true),
        Obj(0),
        Bg(2, false),
    ];
    let brightness = u16::from(ppu.inidisp & 0x0F);
    if ppu.inidisp & 0x80 != 0 || brightness == 0 {
        out.fill(0);
        return;
    }
    let mut bg = [[None; 256]; 4];
    for (i, buf) in bg.iter_mut().enumerate() {
        if ppu.tm & 1 << i != 0 {
            ppu.bg_line(i, y, buf);
        }
    }
    let mut obj = [None; 256];
    if ppu.tm & 0x10 != 0 {
        ppu.obj_line(y, &mut obj);
    }
    let rungs: &[R] = match ppu.bgmode & 7 {
        0 => &M0,
        _ if ppu.bgmode & 8 != 0 => &M1A,
        _ => &M1B,
    };
    let backdrop = ppu.cgram[0] & 0x7FFF;
    for (x, px) in out.iter_mut().enumerate() {
        let mut color = backdrop;
        for rung in rungs {
            let hit = match *rung {
                Bg(b, want) => bg[b][x].filter(|&(_, p)| p == want).map(|(c, _)| c),
                Obj(want) => obj[x].filter(|&(_, p)| p == want).map(|(c, _)| c),
            };
            if let Some(c) = hit {
                color = c;
                break;
            }
        }
        *px = if brightness == 15 {
            color
        } else {
            let f = brightness + 1;
            let r = (color & 0x1F) * f / 16;
            let g = (color >> 5 & 0x1F) * f / 16;
            let b = (color >> 10 & 0x1F) * f / 16;
            b << 10 | g << 5 | r
        };
    }
}

/// The production assembler stays pixel-identical to the frozen per-pixel
/// merge over fuzzed states: both modes, BG3-hoist bit, random TM masks,
/// random OAM/OBSEL, brightness levels, forced blank.
#[test]
fn render_line_matches_reference_over_fuzzed_states() {
    let mut s = 0x9E37_79B9u32;
    for case in 0..120 {
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
        ppu.bgmode = (xs(&mut s) as u8 & 1) | (xs(&mut s) as u8 & 0xF8);
        for i in 0..4 {
            ppu.bgsc[i] = xs(&mut s) as u8;
            ppu.hofs[i] = xs(&mut s) as u16 & 0x3FF;
            ppu.vofs[i] = xs(&mut s) as u16 & 0x3FF;
        }
        ppu.nba = [xs(&mut s) as u8, xs(&mut s) as u8];
        ppu.tm = xs(&mut s) as u8 & 0x1F;
        ppu.obsel = xs(&mut s) as u8;
        ppu.oam_reload = xs(&mut s) as u16;
        ppu.oam_priority = xs(&mut s) & 1 != 0;
        // Bias toward visible: mostly bright, occasional dim/blank.
        ppu.inidisp = match xs(&mut s) & 7 {
            0 => xs(&mut s) as u8, // anything incl. blank
            _ => 0x0F,
        };
        for &y in &[0u16, 55, 120, 223] {
            let mut want = [0u16; 256];
            render_line_ref(&ppu, y, &mut want);
            let mut got = [0u16; 256];
            ppu.render_line(y, &mut got);
            assert_eq!(got[..], want[..], "case {case} y {y}");
        }
    }
}
