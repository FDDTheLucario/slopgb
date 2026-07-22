//! Performance floor: the emulator must run each model **faster than real time**
//! headless, or it can't hold 60 fps for a player. Deliberately a loose bound
//! (~realtime, not the true ~hundreds-of-fps headroom) so it never flakes on a
//! slow CI runner while still catching a catastrophic (order-of-magnitude) speed
//! regression. The measured rate is printed for the record. Dep-free timing.

use std::time::Instant;

use slopgb_core::{GameBoy, Model};

/// A GB frame is 1/59.7275 s. "Real time" = emulating a frame in less wall-clock
/// than that; we require a comfortable margin below it.
const FRAME_WALL_BUDGET_MS: f64 = 16.7;

fn rom_only(cgb: bool) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x00;
    rom[0x143] = if cgb { 0xC0 } else { 0x00 };
    rom
}

fn measure(model: Model, cgb: bool, frames: u32) -> f64 {
    let mut gb = GameBoy::new(model, rom_only(cgb)).expect("cart builds");
    // Warm up (JIT-free interpreter, but let caches settle) then time.
    for _ in 0..60 {
        gb.run_frame();
    }
    let t0 = Instant::now();
    for _ in 0..frames {
        gb.run_frame();
    }
    let ms_per_frame = t0.elapsed().as_secs_f64() * 1000.0 / f64::from(frames);
    let fps = 1000.0 / ms_per_frame;
    println!(
        "{model:?}: {ms_per_frame:.3} ms/frame  (~{fps:.0} fps, {:.1}x realtime)",
        1000.0 / (ms_per_frame * 59.7275)
    );
    ms_per_frame
}

#[test]
fn dmg_runs_faster_than_realtime() {
    let ms = measure(Model::Dmg, false, 600);
    assert!(
        ms < FRAME_WALL_BUDGET_MS,
        "DMG emulates a frame in {ms:.3} ms, slower than the {FRAME_WALL_BUDGET_MS} ms realtime budget"
    );
}

#[test]
fn cgb_runs_faster_than_realtime() {
    let ms = measure(Model::Cgb, true, 600);
    assert!(
        ms < FRAME_WALL_BUDGET_MS,
        "CGB emulates a frame in {ms:.3} ms, slower than the {FRAME_WALL_BUDGET_MS} ms realtime budget"
    );
}
