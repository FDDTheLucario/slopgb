//! PROBE — drop into `crates/slopgb-core/examples/mapdump.rs`, REVERT before commit.
//! Runs the gambatte 15+1-frame protocol, then dumps the BG map, its attributes,
//! tile data (both banks) and BG palette RAM, so `tools/colreq.py` can render each
//! map column's 8-pixel signature and match it against a reference PNG.
//!
//!   cargo run --release -p slopgb-core --example mapdump -- <rom> <dmg|cgb> <out.raw>
use slopgb_core::{CYCLES_PER_FRAME, GameBoy, Model};

fn main() {
    let mut a = std::env::args().skip(1);
    let (rom_p, model_s, out_p) = (a.next().unwrap(), a.next().unwrap(), a.next().unwrap());
    let model = if model_s == "cgb" { Model::Cgb } else { Model::Dmg };
    let mut gb = GameBoy::new(model, std::fs::read(&rom_p).expect("rom")).expect("cart");
    while gb.cycles() < 16 * u64::from(CYCLES_PER_FRAME) {
        gb.step();
    }
    let lcdc = gb.debug_read(0xFF40);
    let base = if lcdc & 0x08 != 0 { 0x9C00u16 } else { 0x9800 };
    let row: Vec<String> = (0..32)
        .map(|c| format!("{:02X}", gb.debug_read_banked(0, base + c)))
        .collect();
    println!("MAP {}", row.join(" "));
    let att: Vec<String> = (0..32)
        .map(|c| format!("{:02X}", gb.debug_read_banked(1, base + c)))
        .collect();
    println!("ATTR {}", att.join(" "));
    for t in 0..8u16 {
        let a0 = if lcdc & 0x10 != 0 {
            0x8000 + t * 16
        } else {
            0x9000u16.wrapping_add(t * 16)
        };
        for bank in 0..2u16 {
            let px: Vec<String> = (0..8u16)
                .map(|r| {
                    format!(
                        "{:02X}{:02X}",
                        gb.debug_read_banked(bank, a0 + r * 2),
                        gb.debug_read_banked(bank, a0 + r * 2 + 1)
                    )
                })
                .collect();
            println!("TILE {t} b{bank} {}", px.join(" "));
        }
    }
    let mut pal = Vec::new();
    for i in 0..64u8 {
        gb.debug_write(0xFF68, 0x80 | i);
        pal.push(format!("{:02X}", gb.debug_read(0xFF69)));
    }
    println!("BGPAL {}", pal.join(" "));
    let mut bytes = Vec::with_capacity(160 * 144 * 4);
    for &px in gb.frame().iter() {
        bytes.extend_from_slice(&px.to_le_bytes());
    }
    std::fs::write(&out_p, &bytes).expect("write");
}
