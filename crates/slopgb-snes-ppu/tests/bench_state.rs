//! Section timing over a captured PPU state (run by hand):
//! `SLOPGB_PPU_STATE=<blob> cargo test -p slopgb-snes-ppu --test bench_state -- --ignored --nocapture`
//! The blob is a `slopgb-snes-ppu-plugin` save_state image; only the leading
//! `PPU_STATE_LEN` bytes are used.

use slopgb_snes_ppu::{PPU_STATE_LEN, SnesPpu};

#[test]
#[ignore]
fn time_render_sections_on_a_captured_state() {
    let Some(path) = std::env::var_os("SLOPGB_PPU_STATE") else {
        eprintln!("SLOPGB_PPU_STATE not set; skipping");
        return;
    };
    let blob = std::fs::read(path).unwrap();
    let mut ppu = SnesPpu::new();
    ppu.load_state(&blob[..PPU_STATE_LEN]);

    let mut out = [0u16; 256];
    let frames = 400u32;
    let t0 = std::time::Instant::now();
    for _ in 0..frames {
        for y in 0..224u16 {
            ppu.render_line(y, &mut out);
        }
    }
    let full = t0.elapsed();

    let mut layer = [None; 256];
    let t0 = std::time::Instant::now();
    for _ in 0..frames {
        for y in 0..224u16 {
            for bg in 0..4 {
                ppu.bg_line(bg, y, &mut layer);
            }
        }
    }
    let bgs = t0.elapsed();

    let mut objs = [None; 256];
    let t0 = std::time::Instant::now();
    for _ in 0..frames {
        for y in 0..224u16 {
            ppu.obj_line(y, &mut objs);
        }
    }
    let obj = t0.elapsed();

    println!(
        "per frame: full={:?} bg_all4={:?} obj={:?} (merge+etc = full - enabled slices)",
        full / frames,
        bgs / frames,
        obj / frames
    );
}
