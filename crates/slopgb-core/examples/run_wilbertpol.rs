//! Scratch dev tool: run one wilbertpol-fork ROM (0xED exit protocol) and
//! dump the result registers for round-by-round decoding against the asm.
//!
//! Usage: `run_wilbertpol <rom.gb> [model]`

use std::process::ExitCode;

use slopgb_core::{GameBoy, Model};

#[path = "../tests/common/protocol.rs"]
#[allow(dead_code)] // the module also exports the mooneye hang timeout
mod protocol;
use protocol::FIB;

fn parse_model(s: &str) -> Option<Model> {
    match s.to_ascii_lowercase().as_str() {
        "dmg0" => Some(Model::Dmg0),
        "dmg" => Some(Model::Dmg),
        "mgb" => Some(Model::Mgb),
        "sgb" => Some(Model::Sgb),
        "sgb2" => Some(Model::Sgb2),
        "cgb" => Some(Model::Cgb),
        "agb" => Some(Model::Agb),
        _ => None,
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let rom_path = args.next().expect("usage: run_wilbertpol <rom.gb> [model]");
    let rom = std::fs::read(&rom_path).expect("readable rom");
    let model = args
        .next()
        .map(|n| parse_model(&n).expect("known model"))
        .unwrap_or_else(|| GameBoy::auto_model(&rom));

    let mut gb = GameBoy::new(model, rom).expect("cartridge accepted");
    let mut timed_out = false;
    while !gb.debug_undefined_hit() && !gb.debug_breakpoint_hit() {
        if gb.cycles() > 200 * 4_194_304 {
            timed_out = true;
            break;
        }
        gb.step();
    }
    let r = gb.cpu_regs();
    let pass = !timed_out && [r.b, r.c, r.d, r.e, r.h, r.l] == FIB;
    println!(
        "{}: {rom_path} [{model:?}] A={:02X} F={:02X} B={:02X} C={:02X} D={:02X} E={:02X} \
         H={:02X} L={:02X}{}",
        if pass { "PASS" } else { "FAIL" },
        r.a,
        r.f(),
        r.b,
        r.c,
        r.d,
        r.e,
        r.h,
        r.l,
        if timed_out { " (timeout)" } else { "" },
    );
    // Dump the test framework's WRAM state: regs_save [F,A,C,B,E,D,L,H] at
    // $C000, regs_flags, regs_assert, then the Test-State rounds.
    print!("  wram:");
    for addr in 0xC000u16..0xC080 {
        if addr % 16 == 0 {
            print!("\n  {addr:04X}:");
        }
        print!(" {:02X}", gb.peek(addr));
    }
    println!();
    ExitCode::from(u8::from(!pass))
}
