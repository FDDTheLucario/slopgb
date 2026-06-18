//! Generate the SLOPGB boot wordmark in the chunky forward-italic style of the
//! classic Game Boy logo (original art — not the trademarked logo bitmap).
//! Upright slab glyphs are defined at half height, doubled vertically (the GB
//! logo's signature chunk), sheared into italic, and composed onto an 88x24
//! (11x3 tile) canvas, emitted as a 1-bit PBM on stdout. To rebuild the art:
//!   cargo run -p slopgb-core --example gen_boot_logo > /tmp/logo.pbm
//!   magick /tmp/logo.pbm -type bilevel boot/slopgb_logo.png
//!   make -C boot          # rgbgfx packs slopgb_logo.png into logo.2bpp

// Each glyph: upright, 12 design rows (doubled to 24px), '#' = ink.
const S: &[&str] = &[
    ".###########.",
    "#############",
    "#####....####",
    "#####........",
    ".######......",
    "..#########..",
    "....#######..",
    ".......######",
    "........#####",
    "####.....####",
    "#############",
    ".###########.",
];
const L: &[&str] = &[
    "#####........",
    "#####........",
    "#####........",
    "#####........",
    "#####........",
    "#####........",
    "#####........",
    "#####........",
    "#####........",
    "#####........",
    "############.",
    "#############",
];
const O: &[&str] = &[
    "..#########..",
    ".###########.",
    "####.....####",
    "####.....####",
    "####.....####",
    "####.....####",
    "####.....####",
    "####.....####",
    "####.....####",
    "####.....####",
    ".###########.",
    "..#########..",
];
const P: &[&str] = &[
    "###########..",
    "############.",
    "####.....####",
    "####.....####",
    "####.....####",
    "############.",
    "###########..",
    "####.........",
    "####.........",
    "####.........",
    "####.........",
    "####.........",
];
const G: &[&str] = &[
    "..#########..",
    ".###########.",
    "####.....####",
    "####.........",
    "####.........",
    "####..#######",
    "####..#######",
    "####.....####",
    "####.....####",
    "####.....####",
    ".###########.",
    "..#########..",
];
const B: &[&str] = &[
    "###########..",
    "############.",
    "####.....####",
    "####.....####",
    "####....#####",
    "###########..",
    "###########..",
    "####....#####",
    "####.....####",
    "####.....####",
    "############.",
    "###########..",
];

fn main() {
    let glyphs = [S, L, O, P, G, B];
    let canvas_w = 88usize; // 11 tiles
    let canvas_h = 24usize;
    let shear_top = 6i32; // top row shifted this many px right (forward italic)

    // advance per glyph: glyph width + 1px tracking (italic overlap tightens it).
    // The last glyph needs no trailing tracking, so drop one from the total.
    let advances: Vec<usize> = glyphs.iter().map(|g| g[0].len() + 1).collect();
    let total: usize = advances.iter().sum::<usize>() - 1 + shear_top as usize;
    let mut x0 = (canvas_w as i32 - total as i32) / 2;
    if x0 < 0 {
        x0 = 0;
    }

    let mut canvas = vec![vec![false; canvas_w]; canvas_h];
    let mut pen = x0;
    for (g, adv) in glyphs.iter().zip(&advances) {
        let h = g.len() as i32; // 12 design rows
        for (ry, row) in g.iter().enumerate() {
            // one shear offset per design row (both doubled rows share it) so the
            // doubled 2px blocks stay clean and stairstep every 2px, like the real
            // logo. Top design rows shifted furthest right (forward italic).
            let dx = ((h - 1 - ry as i32) * shear_top) / (h - 1);
            for dy in 0..2 {
                let py = ry * 2 + dy;
                for (cx, ch) in row.bytes().enumerate() {
                    if ch == b'#' {
                        let px = pen + dx + cx as i32;
                        if px >= 0 && (px as usize) < canvas_w && py < canvas_h {
                            canvas[py][px as usize] = true;
                        }
                    }
                }
            }
        }
        pen += *adv as i32;
    }

    // Emit PBM (P1): 1 = black ink.
    let mut out = format!("P1\n{canvas_w} {canvas_h}\n");
    for row in &canvas {
        for &p in row {
            out.push(if p { '1' } else { '0' });
            out.push(' ');
        }
        out.push('\n');
    }
    print!("{out}");
}
