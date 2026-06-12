//! Debug runner for gambatte `_out<HEX>` test ROMs: runs a ROM for the
//! suite's 16 LCD frames and OCRs the top tile row back into hex digits
//! (the same protocol as `tests/gbtr/gambatte.rs`; see the howto notes
//! there). Usage:
//!
//! ```sh
//! cargo run -p slopgb-core --example run_gambatte -- <rom> [dmg|cgb]
//! ```

use slopgb_core::{CYCLES_PER_FRAME, GameBoy, Model, SCREEN_W};

/// 8x8 hex glyphs as drawn by the gambatte test framework's print code.
const GLYPHS: [[u8; 8]; 16] = [
    [0x00, 0x7F, 0x41, 0x41, 0x41, 0x41, 0x41, 0x7F], // 0
    [0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08], // 1
    [0x00, 0x7F, 0x01, 0x01, 0x7F, 0x40, 0x40, 0x7F], // 2
    [0x00, 0x7F, 0x01, 0x01, 0x3F, 0x01, 0x01, 0x7F], // 3
    [0x00, 0x41, 0x41, 0x41, 0x7F, 0x01, 0x01, 0x01], // 4
    [0x00, 0x7F, 0x40, 0x40, 0x7E, 0x01, 0x01, 0x7E], // 5
    [0x00, 0x7F, 0x40, 0x40, 0x7F, 0x41, 0x41, 0x7F], // 6
    [0x00, 0x7F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10], // 7
    [0x00, 0x3E, 0x41, 0x41, 0x3E, 0x41, 0x41, 0x3E], // 8
    [0x00, 0x7F, 0x41, 0x41, 0x7F, 0x01, 0x01, 0x7F], // 9
    [0x00, 0x08, 0x22, 0x41, 0x7F, 0x41, 0x41, 0x41], // A
    [0x00, 0x7E, 0x41, 0x41, 0x7E, 0x41, 0x41, 0x7E], // B
    [0x00, 0x3E, 0x41, 0x40, 0x40, 0x40, 0x41, 0x3E], // C
    [0x00, 0x7E, 0x41, 0x41, 0x41, 0x41, 0x41, 0x7E], // D
    [0x00, 0x7F, 0x40, 0x40, 0x7F, 0x40, 0x40, 0x7F], // E
    [0x00, 0x7F, 0x40, 0x40, 0x7F, 0x40, 0x40, 0x40], // F
];

const WHITE: u32 = 0x00F8_F8F8;

/// gambatte's CGB-to-RGB conversion (tests/common/framecmp.rs).
fn gambatte_rgb(px: u32) -> u32 {
    let r5 = (px >> 19) & 0x1F;
    let g5 = (px >> 11) & 0x1F;
    let b5 = (px >> 3) & 0x1F;
    let r = (r5 * 13 + g5 * 2 + b5) / 2;
    let g = (g5 * 3 + b5) * 2;
    let b = (r5 * 3 + g5 * 2 + b5 * 11) / 2;
    (r << 16) | (g << 8) | b
}

fn masked_pixel(px: u32, cgb: bool) -> u32 {
    let v = if cgb { gambatte_rgb(px) } else { px };
    v & 0x00F8_F8F8
}

fn read_hex_screen(frame: &[u32], cgb: bool) -> String {
    let mut out = String::new();
    for i in 0..SCREEN_W / 8 {
        let mut tile = [0u32; 64];
        for (p, t) in tile.iter_mut().enumerate() {
            *t = masked_pixel(frame[(p / 8) * SCREEN_W + i * 8 + p % 8], cgb);
        }
        let glyph_of = |g: &[u8; 8]| {
            (0..64).all(|p| {
                let want = if g[p / 8] & (0x80 >> (p % 8)) != 0 {
                    0
                } else {
                    WHITE
                };
                tile[p] == want
            })
        };
        match GLYPHS.iter().position(glyph_of) {
            Some(v) => out.push(char::from_digit(v as u32, 16).unwrap().to_ascii_uppercase()),
            None if tile.iter().all(|&p| p == WHITE) => out.push(' '),
            None => out.push('?'),
        }
    }
    out.trim_end().to_string()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let rom_path = args.next().expect("usage: run_gambatte <rom> [dmg|cgb]");
    let model = match args.next().as_deref() {
        Some("dmg") => Model::Dmg,
        _ => Model::Cgb,
    };
    let rom = std::fs::read(&rom_path).expect("read rom");
    let mut gb = GameBoy::new(model, rom).expect("load rom");
    let target = 16 * u64::from(CYCLES_PER_FRAME);
    while gb.cycles() < target {
        gb.step();
    }
    println!("{}", read_hex_screen(gb.frame(), model.is_cgb()));
}
