// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Report what a gambatte test ROM's result read actually latched: run the
//! ROM under the suite's 16-frame protocol and, every time the instruction at
//! `read_pc` executes, print the dot, LY, the value left in A (the read's own
//! result) and the register state one instruction later.
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
            println!(
                "READ cc={} ly={:02X} a={:02X} stat_after={:02X}",
                gb.cycles(),
                gb.debug_read(0xFF44),
                gb.cpu_regs().a,
                gb.debug_read(0xFF41),
            );
        }
    }
}
