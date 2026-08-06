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
//! cargo run -p slopgb-core --example probe_statread -- <rom> <read_pc_hex> [dmg]
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

    let target = 16 * u64::from(CYCLES_PER_FRAME);
    while gb.cycles() < target {
        let pc = gb.cpu_regs().pc;
        gb.step();
        if pc == read_pc {
            eprintln!("READ cc={} a={:02X}", gb.cycles(), gb.cpu_regs().a);
        }
    }
}
