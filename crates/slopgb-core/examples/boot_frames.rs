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
}
