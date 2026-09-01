// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `slopgb --headless <script.slp>`: run a slopscript case non-interactively
//! against one machine and exit with a code a CI step can check
//! (`docs/ui-state/headless.md` documents the language as built;
//! `docs/headless-plan.md` records why it was designed that way — 0 every
//! check held, 1 a check/assert failed, 2 the script itself is wrong).
//!
//! `slopscript` (`crates/slopscript`) knows nothing about Game Boys: this
//! module is the frontend half of the split the plan describes, registering
//! every `gb:` builtin/constant into an [`Interp`] against one shared
//! [`Machine`]. Split by concern, one submodule each: [`consts`] (the tagged
//! `BTN_*`/register/model constants), [`io`] (ROM/battery/state/symbol-file
//! I/O), [`mem`] (`gb:read`/`gb:poke`), [`input`] (buttons and frame/cycle
//! waits), [`regs`] (`gb:reg`/`gb:set_reg`), [`rtc`] (`gb:set_rtc`).

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slopgb_core::RamInit;
use slopgb_plugin_host::PluginRegistry;
use slopscript::{Interp, Outcome};

use crate::session::Session;
use crate::symbols::SymbolTable;

mod consts;
mod input;
mod io;
mod mem;
mod regs;
mod rtc;

/// The one machine a headless run drives, plus the ROM-load-time context
/// `main` already resolved (boot ROM / SGB BIOS / plugins dir / RAM init),
/// kept here so a `gb:load_rom`-loaded ROM gets the same treatment as an
/// interactive load. Wrapped in `Rc<RefCell<..>>` and cloned into every
/// builtin closure — a builtin never re-enters the interpreter (no `gb:`
/// call runs slopscript itself), so the `RefCell` can never be doubly
/// borrowed.
pub(crate) struct Machine {
    pub(crate) session: Session,
    pub(crate) syms: SymbolTable,
    pub(crate) boot_rom: Option<Vec<u8>>,
    pub(crate) sgb_bios: Option<Vec<u8>>,
    pub(crate) plugins_dir: Option<PathBuf>,
    pub(crate) ram_init: Option<RamInit>,
}

/// Run `machine` against the script at `script_path` (`-` = read from
/// stdin) with `argv`, and return the process exit code
/// (`docs/headless-plan.md`: 0 every check held, 1 a check/assert failed, 2
/// the script itself is wrong).
pub(crate) fn run(
    machine: Machine,
    script_path: &Path,
    argv: Vec<String>,
    registry: &PluginRegistry,
) -> i32 {
    let src = match read_script(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("slopgb: {e}");
            return 2;
        }
    };
    exit_code(run_outcome(machine, &src, argv, registry))
}

/// `-` reads the script from stdin (at EOF); any other path is read whole.
fn read_script(path: &Path) -> Result<String, String> {
    if path == Path::new("-") {
        std::io::read_to_string(std::io::stdin())
            .map_err(|e| format!("cannot read script from stdin: {e}"))
    } else {
        fs::read_to_string(path)
            .map_err(|e| format!("cannot read script '{}': {e}", path.display()))
    }
}

/// Build the interpreter and run `src`, returning the raw [`Outcome`] — the
/// core of [`run`], split out so tests can inspect `checks`/`failures`
/// directly instead of only the collapsed exit code.
fn run_outcome(
    machine: Machine,
    src: &str,
    argv: Vec<String>,
    registry: &PluginRegistry,
) -> Outcome {
    let mc = Rc::new(RefCell::new(machine));
    let mut interp = build_interp(&mc, registry);
    interp.set_argv(argv);
    interp.run(src)
}

/// Register every constant and `gb:` builtin (see the module doc) plus the
/// `within` frame-budget clock (`docs/headless-plan.md`, "Bounds": `within
/// N` is in frames, tracked in cycles so `gb:wait_cycles` also consumes the
/// budget). Shared by [`run_outcome`] and the parse-only test in
/// `headless_tests.rs`, which needs the exact table the real runner uses.
fn build_interp<'a>(mc: &Rc<RefCell<Machine>>, registry: &'a PluginRegistry) -> Interp<'a> {
    let mut interp = Interp::new();
    consts::register(&mut interp);
    io::register(&mut interp, mc, registry);
    mem::register(&mut interp, mc);
    input::register(&mut interp, mc);
    regs::register(&mut interp, mc);
    rtc::register(&mut interp, mc);
    let clock_mc = Rc::clone(mc);
    interp.set_clock(u64::from(slopgb_core::CYCLES_PER_FRAME), move || {
        clock_mc.borrow().session.gb.cycles()
    });
    interp
}

/// Map an [`Outcome`] to the process exit code, printing the plan's one-line
/// summary. slopscript itself already prints a `check failed: <msg>` /
/// `assert failed: <msg>` line per failure as it happens; this is only the
/// trailing tally, and it is scored against `checks` (the number of
/// `check()` calls that ran — an aborting `assert` doesn't add to it, and
/// prints its own line above instead of being counted here). So a run whose
/// only failure is an assert has `checks == 0` and prints no summary line.
fn exit_code(outcome: Outcome) -> i32 {
    match outcome {
        Outcome::Ok { checks: 0 } => 0,
        Outcome::Ok { checks: 1 } => {
            println!("1 check passed");
            0
        }
        Outcome::Ok { checks } => {
            println!("{checks} checks passed");
            0
        }
        Outcome::Failed { checks, failures } => {
            if let Some(s) = failure_summary(checks, failures.len()) {
                eprintln!("{s}");
            }
            1
        }
        Outcome::Error(msg) => {
            eprintln!("slopgb: {msg}");
            2
        }
    }
}

/// The `exit_code` summary line for a failed run, or `None` when `checks ==
/// 0` — there is nothing to summarise when the only failure was an aborting
/// `assert` (which is not a `check()` and prints its own line already).
fn failure_summary(checks: usize, failed: usize) -> Option<String> {
    if checks == 0 {
        None
    } else {
        Some(format!("{failed} of {checks} checks failed"))
    }
}

#[cfg(test)]
#[path = "headless_tests.rs"]
mod tests;
