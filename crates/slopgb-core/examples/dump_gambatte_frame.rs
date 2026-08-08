// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Dev tool: run a gambatte pixel-reference ROM under the suite's 16-LCD-frame
//! protocol (the same run length as `tests/gbtr/gambatte.rs`, unlike
//! `dump_frame`, which waits for an `LD B,B`) and write the 160x144 frame as
//! raw little-endian XRGB u32 — the input for a pixel diff against the sibling
//! reference PNG or SameBoy's tester BMP.
//!
//! `SLOPGB_DUMP_PAL=1` also prints the CGB palette RAM (8 BG + 8 OBJ palettes,
//! as 15-bit words) to stderr — a pixel that disagrees with a reference on
//! COLOUR rather than position is usually an unwritten palette entry, not a
//! pipeline fault.
//!
//! ```sh
//! cargo run -p slopgb-core --example dump_gambatte_frame -- <rom> <dmg|cgb> <out.raw>
//! ```

use slopgb_core::{CYCLES_PER_FRAME, GameBoy, Model};

fn main() {
    let mut args = std::env::args().skip(1);
    let rom_path = args
        .next()
        .expect("usage: dump_gambatte_frame <rom> <dmg|cgb> <out.raw>");
    let model = match args.next().as_deref() {
        Some("dmg") => Model::Dmg,
        _ => Model::Cgb,
    };
    let out = args.next().expect("out path");
    let rom = std::fs::read(&rom_path).expect("read rom");
    let mut gb = GameBoy::new(model, rom).expect("load rom");
    let target = 16 * u64::from(CYCLES_PER_FRAME);
    while gb.cycles() < target {
        gb.step();
    }
    if std::env::var("SLOPGB_DUMP_PAL").is_ok() {
        let (bg, obj) = gb.cgb_palette_ram();
        for (name, ram) in [("BG", bg), ("OBJ", obj)] {
            for pal in 0..8 {
                let w: Vec<String> = (0..4)
                    .map(|c| {
                        let i = pal * 8 + c * 2;
                        format!("{:04X}", u16::from(ram[i]) | u16::from(ram[i + 1]) << 8)
                    })
                    .collect();
                eprintln!("{name}{pal} {}", w.join(" "));
            }
        }
    }
    let bytes: Vec<u8> = gb.frame().iter().flat_map(|p| p.to_le_bytes()).collect();
    std::fs::write(out, bytes).expect("write frame");
}
