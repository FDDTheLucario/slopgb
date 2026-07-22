//! Dev tool: run a screenshot test ROM to its `LD B,B` breakpoint +1 frame and
//! write the 160x144 framebuffer as raw little-endian u32 (XRGB) to a file, for
//! pixel-diffing against a suite reference PNG. Usage: dump_frame <rom> <model> <out.raw>

use std::process::ExitCode;

use slopgb_core::{GameBoy, Model};

/// 120 emulated seconds — the mealybug/mooneye hang timeout.
const TIMEOUT_TCYCLES: u64 = 120 * 4_194_304;

fn parse_model(s: &str) -> Option<Model> {
    match s.to_ascii_lowercase().as_str() {
        "dmg" => Some(Model::Dmg),
        "cgb" => Some(Model::Cgb),
        _ => None,
    }
}

fn main() -> ExitCode {
    let mut a = std::env::args().skip(1);
    let (rom_p, model_s, out_p) = match (a.next(), a.next(), a.next()) {
        (Some(r), Some(m), Some(o)) => (r, m, o),
        _ => {
            eprintln!("usage: dump_frame <rom> <dmg|cgb> <out.raw>");
            return ExitCode::from(2);
        }
    };
    let rom = std::fs::read(&rom_p).expect("read rom");
    let model = parse_model(&model_s).expect("model dmg|cgb");
    let mut gb = GameBoy::new(model, rom).expect("cart");
    while !gb.debug_breakpoint_hit() {
        if gb.cycles() > TIMEOUT_TCYCLES {
            eprintln!("timeout");
            return ExitCode::from(1);
        }
        gb.step();
    }
    gb.run_frame(); // mealybug harness compares the frame after the breakpoint
    let mut bytes = Vec::with_capacity(160 * 144 * 4);
    for &px in gb.frame().iter() {
        bytes.extend_from_slice(&px.to_le_bytes());
    }
    std::fs::write(&out_p, &bytes).expect("write raw");
    eprintln!("wrote {} px to {out_p}", gb.frame().len());
    ExitCode::SUCCESS
}
