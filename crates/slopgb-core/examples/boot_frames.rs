//! Dump boot-animation frames as PPM so we can study the exact reference timing.
//! `cargo run -p slopgb-core --example boot_frames -- <boot.bin> <cart> <outdir> <step> <count>`
use slopgb_core::{GameBoy, Model, SCREEN_H, SCREEN_W};

fn dump(gb: &GameBoy, path: &str) {
    let f = gb.frame();
    let mut out = format!("P6\n{SCREEN_W} {SCREEN_H}\n255\n").into_bytes();
    for &px in f.iter() {
        out.push((px >> 16) as u8);
        out.push((px >> 8) as u8);
        out.push(px as u8);
    }
    std::fs::write(path, out).unwrap();
}

fn main() {
    let mut a = std::env::args().skip(1);
    let boot = std::fs::read(a.next().expect("boot")).expect("read boot");
    let cart = std::fs::read(a.next().expect("cart")).expect("read cart");
    let outdir = a.next().expect("outdir");
    let step: u32 = a.next().map_or(4, |s| s.parse().unwrap());
    let count: u32 = a.next().map_or(40, |s| s.parse().unwrap());

    let mut gb = GameBoy::new_with_boot(Model::Cgb, cart, boot).expect("build");
    let mut handoff = None;
    for i in 0..count {
        for _ in 0..step {
            gb.run_frame();
            if !gb.boot_active() && handoff.is_none() {
                handoff = Some(i * step);
            }
        }
        dump(&gb, &format!("{outdir}/f{:04}.ppm", i * step));
    }
    println!("handoff at ~frame {handoff:?}");
    if std::env::var("DUMP_TILES").is_ok() {
        // Decode VRAM tiles 0..56 into a 8-wide strip (shade>0 = black).
        let v = gb.vram();
        let n = 56usize;
        let mut img = vec![255u8; 8 * (n * 8) * 3];
        for t in 0..n {
            for y in 0..8 {
                let lo = v[t * 16 + y * 2];
                let hi = v[t * 16 + y * 2 + 1];
                for x in 0..8 {
                    let bit = 7 - x;
                    let s = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1);
                    if s != 0 {
                        let px = (y * (n * 8) + t * 8 + x) * 3;
                        img[px] = 0;
                        img[px + 1] = 0;
                        img[px + 2] = 0;
                    }
                }
            }
        }
        let mut out = format!("P6\n{} 8\n255\n", n * 8).into_bytes();
        out.extend_from_slice(&img);
        std::fs::write("/tmp/tiles.ppm", out).unwrap();
        println!("wrote /tmp/tiles.ppm (tiles 0..{n})");
    }
    if std::env::var("DUMP_MAP").is_ok() {
        // BG map $9800, rows 4..14, cols 2..18 — tile index per cell.
        for row in 4..14 {
            let mut line = format!("r{row:2}: ");
            for col in 2..18 {
                let t = gb.debug_read(0x9800 + row * 32 + col);
                line += &format!("{t:3} ");
            }
            println!("{line}");
        }
    }
}
