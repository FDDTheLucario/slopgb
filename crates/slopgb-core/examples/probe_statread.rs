// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Report what a gambatte test ROM's result read actually latched: run the
//! ROM under the suite's 16-frame protocol and print `cc` + the value left in
//! A every time the instruction at `read_pc` executes. A is the read's own
//! result — a later `debug_read` of the same register is a DIFFERENT read one
//! or two M-cycles on, and the two disagree on exactly the rows that straddle
//! a mode edge.
//!
//! The marker goes to stderr so it interleaves in order with a temporary
//! `eprintln!` in the register read being studied: the trace line printed
//! FIRST in a marker's group is the cc+0 leading-edge sample
//! (`Interconnect::leading_edge_sample`) the CPU actually latched; the one
//! right before the marker is the post-tick trailing read.
//!
//! The counterpart on the reference side is SameBoy's `SB_TRACE` tracer
//! (`docs/sameboy-port/tools/build_sameboy_tracers.sh`): comparing A here
//! against its `SBREAD ff41` line is what pins a read-frame law
//! (`docs/hardware-state/ppu-timing.md` § "The FF41 read frame").
//!
//! ```sh
//! cargo run -p slopgb-core --example probe_statread -- <rom> <read_pc_hex> [dmg] [frames]
//! ```

use slopgb_core::{CYCLES_PER_FRAME, GameBoy, Model};

fn main() {
    let mut args = std::env::args().skip(1);
    let rom_path = args
        .next()
        .expect("usage: probe_statread <rom> <pc_hex> [dmg]");
    let read_pc =
        u16::from_str_radix(args.next().expect("pc").trim_start_matches("0x"), 16).expect("hex pc");
    let model = match args.next().as_deref() {
        Some("dmg") => Model::Dmg,
        _ => Model::Cgb,
    };
    let rom = std::fs::read(&rom_path).expect("read rom");
    let mut gb = GameBoy::new(model, rom).expect("load rom");

    // Default to the gambatte suite's 16 frames; a fourth argument extends it
    // for the longer protocols (an age ladder needs ~85).
    let frames: u64 = args.next().and_then(|f| f.parse().ok()).unwrap_or(16);
    let target = frames * u64::from(CYCLES_PER_FRAME);
    // `SLOPGB_PCWINDOW=lo-hi` also traces every instruction executed inside
    // that PC range — for kernels whose observable is the instruction stream
    // (a DMA trigger at a bank boundary, say) rather than a register read.
    let window = std::env::var("SLOPGB_PCWINDOW").ok().and_then(|w| {
        let (a, b) = w.split_once('-')?;
        Some((
            u16::from_str_radix(a.trim_start_matches("0x"), 16).ok()?,
            u16::from_str_radix(b.trim_start_matches("0x"), 16).ok()?,
        ))
    });
    while gb.cycles() < target {
        let pc = gb.cpu_regs().pc;
        if let Some((lo, hi)) = window {
            if (lo..=hi).contains(&pc) {
                let r = gb.cpu_regs();
                eprintln!("PC {pc:04X} a={:02X} cc={}", r.a, gb.cycles());
            }
        }
        gb.step();
        if pc == read_pc {
            let r = gb.cpu_regs();
            eprintln!(
                "READ cc={} a={:02X} bc={:02X}{:02X} de={:02X}{:02X} hl={:02X}{:02X}",
                gb.cycles(),
                r.a,
                r.b,
                r.c,
                r.d,
                r.e,
                r.h,
                r.l,
            );
        }
    }
}
