use super::*;

const W: usize = 8;
const H: usize = 6;
const BG: u32 = 0x00_00_00_00;
const FG: u32 = 0x00_FF_00_00;

fn blank() -> Vec<u32> {
    vec![BG; W * H]
}

fn at(buf: &[u32], x: i32, y: i32) -> u32 {
    buf[(y * W as i32 + x) as usize]
}

#[test]
fn fill_rect_sets_exactly_the_covered_pixels() {
    let mut buf = blank();
    let mut c = Canvas::new(&mut buf, W, H);
    c.fill_rect(Rect::new(2, 1, 3, 2), FG);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let inside = (2..5).contains(&x) && (1..3).contains(&y);
            assert_eq!(at(&buf, x, y), if inside { FG } else { BG }, "({x},{y})");
        }
    }
}

#[test]
fn blend_px_composites_coverage_over_the_destination() {
    let mut buf = blank();
    let mut c = Canvas::new(&mut buf, W, H);
    let white = 0x00FF_FFFF;
    // cov 0 leaves dst untouched; 255 is full fg; 128 is ~halfway. Out-of-bounds
    // is a silent no-op. Do every mutation before reading pixels back.
    c.blend_px(0, 0, white, 0);
    c.blend_px(1, 0, white, 255);
    c.blend_px(2, 0, white, 128);
    c.blend_px(-1, 0, white, 255);
    c.blend_px(W as i32, 0, white, 255);
    assert_eq!(at(&buf, 0, 0), BG, "cov 0 keeps dst");
    assert_eq!(at(&buf, 1, 0), white, "cov 255 is full fg");
    let mid = at(&buf, 2, 0);
    for shift in [16, 8, 0] {
        let ch = (mid >> shift) & 0xFF;
        assert!(
            (0x7E..=0x81).contains(&ch),
            "half coverage ~0x7F, got {ch:#x}"
        );
    }

    // Blend over a non-zero dst blends between the two colours (0x40 -> ~0xA0).
    let mut buf2 = vec![0x0040_4040u32; W * H];
    let mut c2 = Canvas::new(&mut buf2, W, H);
    c2.blend_px(0, 0, white, 128);
    let g = (buf2[0] >> 8) & 0xFF;
    assert!(
        (0x9E..=0xA1).contains(&g),
        "0x40 -> ~0xA0 at half, got {g:#x}"
    );
}

#[test]
fn drawing_clips_to_the_buffer_without_panicking() {
    let mut buf = blank();
    let mut c = Canvas::new(&mut buf, W, H);
    // Straddles every edge and extends far past — only the in-bounds part lands.
    c.fill_rect(Rect::new(-3, -3, 100, 100), FG);
    for px in &buf {
        assert_eq!(*px, FG);
    }
    // Wholly off-screen writes nothing.
    let mut buf2 = blank();
    let mut c2 = Canvas::new(&mut buf2, W, H);
    c2.fill_rect(Rect::new(50, 50, 10, 10), FG);
    c2.put(-1, 0, FG);
    c2.put(0, -1, FG);
    c2.put(W as i32, 0, FG);
    assert!(buf2.iter().all(|&p| p == BG));
}

#[test]
fn round_outline_chamfers_the_corners() {
    const BW: usize = 12;
    let mut buf = vec![BG; BW * BW];
    let atb = |b: &[u32], x: i32, y: i32| b[(y * BW as i32 + x) as usize];
    let r = Rect::new(1, 1, 10, 10); // x 1..11, y 1..11
    {
        let mut c = Canvas::new(&mut buf, BW, BW);
        c.round_outline(r, FG);
    }
    // The 2px chamfer cuts the outer corner triangle (3 px per corner)...
    for (cx, cy) in [(1, 1), (2, 1), (1, 2)] {
        assert_eq!(atb(&buf, cx, cy), BG, "chamfer cut ({cx},{cy})");
    }
    // ...bridged by one diagonal pixel, with the edges inset by 2.
    assert_eq!(atb(&buf, 2, 2), FG, "top-left chamfer bridge");
    assert_eq!(atb(&buf, 3, 1), FG, "top edge starts inset by 2");
    assert_eq!(atb(&buf, 1, 3), FG, "left edge starts inset by 2");
    // `chamfer_cut_pixels` reports exactly those cut coordinates.
    let cut = Canvas::chamfer_cut_pixels(r);
    assert_eq!(cut.len(), 12, "3 px * 4 corners");
    assert!(cut.contains(&(1, 1)) && cut.contains(&(2, 1)) && cut.contains(&(1, 2)));

    // A too-small rect falls back to a hard outline (corner set, no cut list).
    let mut buf2 = vec![BG; BW * BW];
    {
        let mut c = Canvas::new(&mut buf2, BW, BW);
        c.round_outline(Rect::new(0, 0, 4, 4), FG); // 4 < 2*2+1
    }
    assert_eq!(buf2[0], FG, "too-small falls back to a hard corner");
    assert!(Canvas::chamfer_cut_pixels(Rect::new(0, 0, 4, 4)).is_empty());
}

#[test]
fn push_clip_confines_drawing() {
    let mut buf = blank();
    {
        let mut c = Canvas::new(&mut buf, W, H);
        c.push_clip(Rect::new(1, 1, 2, 2));
        // Fill the whole surface, but only the 2×2 clip window is written.
        let full = c.bounds();
        c.fill_rect(full, FG);
    }
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let inside = (1..3).contains(&x) && (1..3).contains(&y);
            assert_eq!(at(&buf, x, y), if inside { FG } else { BG });
        }
    }
}

#[test]
fn set_clip_restores_the_previous_region() {
    let mut buf = blank();
    {
        let mut c = Canvas::new(&mut buf, W, H);
        let saved = c.push_clip(Rect::new(1, 1, 2, 2));
        let full = c.bounds();
        c.fill_rect(full, FG); // confined to the 2×2 window
        c.set_clip(saved); // restore the full clip
        c.fill_rect(full, FG); // now the whole surface
    }
    // Corners filled only if the restore actually widened the clip again.
    assert!(buf.iter().all(|&p| p == FG));
}

#[test]
fn outline_rect_draws_only_the_border() {
    let mut buf = blank();
    let mut c = Canvas::new(&mut buf, W, H);
    let r = Rect::new(1, 1, 4, 4); // covers x 1..5, y 1..5
    c.outline_rect(r, FG);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let on_border = r.contains(x, y)
                && (x == r.x || x == r.right() - 1 || y == r.y || y == r.bottom() - 1);
            assert_eq!(at(&buf, x, y), if on_border { FG } else { BG }, "({x},{y})");
        }
    }
    // Interior stayed background.
    assert_eq!(at(&buf, 2, 2), BG);
}

#[test]
fn blit_tile_scales_each_pixel_through_the_palette() {
    let pal = [0x00, 0x11, 0x22, 0x33];
    let mut pix = [[0u8; 8]; 8];
    pix[0][0] = 3; // top-left -> pal[3]
    pix[0][1] = 1; // next col -> pal[1]
    pix[1][0] = 2; // next row -> pal[2]
    let scale = 2i32;
    let cw = 8 * scale as usize;
    let ch = 8 * scale as usize;
    let mut buf = vec![0xDEAD_u32; cw * ch];
    {
        let mut c = Canvas::new(&mut buf, cw, ch);
        c.blit_tile(0, 0, &pix, &pal, scale);
    }
    let at = |x: usize, y: usize| buf[y * cw + x];
    // (0,0) source fills the 2×2 block (0..2, 0..2) with pal[3].
    for dy in 0..2 {
        for dx in 0..2 {
            assert_eq!(at(dx, dy), 0x33, "block (0,0) px ({dx},{dy})");
        }
    }
    assert_eq!(at(2, 0), 0x11, "source (1,0) -> x block 2..4");
    assert_eq!(at(0, 2), 0x22, "source (0,1) -> y block 2..4");
    assert_eq!(at(4, 0), 0x00, "source (2,0) is index 0 -> pal[0]");
}

#[test]
fn rect_intersect_and_contains() {
    let a = Rect::new(0, 0, 4, 4);
    let b = Rect::new(2, 2, 4, 4);
    assert_eq!(a.intersect(&b), Rect::new(2, 2, 2, 2));
    // Disjoint -> zero area.
    assert_eq!(a.intersect(&Rect::new(10, 10, 2, 2)).w, 0);
    assert!(a.contains(0, 0));
    assert!(!a.contains(4, 0)); // right edge exclusive
}
