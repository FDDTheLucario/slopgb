//! Run a single mooneye test ROM and report pass/fail, final registers and
//! elapsed cycles. Dev tool for the test fix loop.
//!
//! Usage: `run_mooneye <rom.gb> [dmg0|dmg|mgb|sgb|sgb2|cgb|agb]`
//!
//! Without a model argument the ROM's CGB-support header flag picks DMG or
//! CGB. Exit code 0 = pass, 1 = fail/timeout, 2 = usage or I/O error.
//!
//! Protocol (test-roms-src/README.markdown): the ROM executes `LD B,B` when
//! finished; it passed iff B/C/D/E/H/L are the Fibonacci numbers
//! 3/5/8/13/21/34. 120 emulated seconds without the breakpoint is a timeout.

use std::process::ExitCode;

use slopgb_core::{GameBoy, Model};

// Pass/fail protocol constants shared with the integration harness
// (tests/common/mod.rs) so the two cannot drift.
#[path = "../tests/common/protocol.rs"]
mod protocol;
use protocol::{FIB, TIMEOUT_TCYCLES};

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
    let Some(rom_path) = args.next() else {
        eprintln!("usage: run_mooneye <rom.gb> [dmg0|dmg|mgb|sgb|sgb2|cgb|agb]");
        return ExitCode::from(2);
    };
    let rom = match std::fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(e) => {
            eprintln!("error: cannot read {rom_path}: {e}");
            return ExitCode::from(2);
        }
    };
    let model = match args.next() {
        Some(name) => match parse_model(&name) {
            Some(model) => model,
            None => {
                eprintln!("error: unknown model {name:?}");
                return ExitCode::from(2);
            }
        },
        None => GameBoy::auto_model(&rom),
    };

    let mut gb = match GameBoy::new(model, rom) {
        Ok(gb) => gb,
        Err(e) => {
            eprintln!("error: cartridge rejected: {e}");
            return ExitCode::from(2);
        }
    };
    // gbmicrotest verdict mode: run ~0.7 emulated s then print
    // $FF80/$FF81/$FF82 ($FF82==0x01 is PASS). Checks gbmicrotest's cc+4
    // counter ROMs (e.g. int_hblank_halt) without the full gbtr matrix.
    if std::env::var("SLOPGB_GBMICRO").is_ok() {
        let deadline = gb.cycles().saturating_add(3_000_000);
        while gb.cycles() < deadline {
            gb.step();
        }
        println!(
            "GBMICRO {rom_path}: FF82={:02X} FF80(actual)={:02X} FF81(expected)={:02X}",
            gb.peek_no_io(0xFF82),
            gb.peek_no_io(0xFF80),
            gb.peek_no_io(0xFF81),
        );
        return ExitCode::from(0);
    }
    // The 2016 wilbertpol fork signals completion with the undefined opcode
    // 0xED (`debug_undefined_hit`), not `LD B,B` — set SLOPGB_WILBERT=1 to use
    // that exit condition (still a Fibonacci verdict).
    let wilbert = std::env::var("SLOPGB_WILBERT").is_ok();
    let mut timed_out = false;
    while !(gb.debug_breakpoint_hit() || (wilbert && gb.debug_undefined_hit())) {
        if gb.cycles() > TIMEOUT_TCYCLES {
            timed_out = true;
            break;
        }
        gb.step();
    }

    if std::env::var("SLOPGB_HRAMDUMP").is_ok() {
        print!("HRAM");
        for a in 0xFF80u16..=0xFF90 {
            print!(" {a:04X}={:02X}", gb.peek_no_io(a));
        }
        println!();
    }
    if std::env::var("SLOPGB_WRAMDUMP").is_ok() {
        print!("WRAM");
        for a in 0xC014u16..=0xC018 {
            print!(" {a:04X}={:02X}", gb.peek_no_io(a));
        }
        println!();
    }
    let r = gb.cpu_regs();
    let pass = !timed_out && [r.b, r.c, r.d, r.e, r.h, r.l] == FIB;
    println!(
        "{}: {} [{model:?}]",
        if pass { "PASS" } else { "FAIL" },
        rom_path
    );
    if timed_out {
        println!("  timeout: no LD B,B breakpoint within 120 emulated seconds");
    }
    println!(
        "  regs: A={:02X} F={:02X} B={:02X} C={:02X} D={:02X} E={:02X} H={:02X} L={:02X} \
         SP={:04X} PC={:04X}",
        r.a,
        r.f(),
        r.b,
        r.c,
        r.d,
        r.e,
        r.h,
        r.l,
        r.sp,
        r.pc
    );
    println!("  cycles: {} T-cycles", gb.cycles());
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
