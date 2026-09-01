// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Tree-walking evaluator: walks the [`ast::Program`] `Interp::parse_only`
//! stashed and produces an [`Outcome`]. Every parse-time rule in [`parser`]
//! already ruled out unbound names, unregistered call targets and unbounded
//! loops, so this module only has to enforce the rules that can't be checked
//! without running the program: strict value typing, 1-based indexing
//! bounds, the recursion-depth cap, and the `within` frame budget.
//!
//! Split by concern, each a second `impl Ctx` block via `use super::*`:
//! [`exec`] (statements, blocks, loops, `within`), [`expr`] (expression
//! evaluation, operators, indexing), [`calls`] (proc calls, the `print` /
//! `hex` / `check` / `assert` intrinsics, and dispatch to host builtins),
//! [`format`] (the `hex()`/`print()` rendering, matching the format
//! `mcp/tools.rs`'s `peek` dumps use — free functions, not `Ctx` methods;
//! nothing is shared, this crate is dep-free and never links the frontend).

use crate::ast::{Item, Proc, Stmt};
use crate::{Interp, Outcome, Value};
use std::collections::HashMap;
use std::rc::Rc;

mod calls;
mod exec;
mod expr;
mod format;

#[cfg(test)]
#[path = "eval_tests.rs"]
mod tests;

/// A recursion cap on proc calls: a runtime error names it rather than a
/// real stack overflow aborting the process on a fuzzed/garbage script.
const MAX_CALL_DEPTH: u32 = 256;

/// How many `while`/`repeat` back-edges may pass with the host clock frozen
/// before the loop is called a spin. A `within` deadline is an emulated-cycle
/// deadline, so a loop whose body advances no cycles never reaches it — the
/// one way a script that parsed clean can still hang CI. Only *consecutive*
/// non-advancing back-edges count (see [`Ctx::spin_steps`]), so any loop that
/// waits on the machine resets it and is unaffected however long it runs.
///
/// ponytail: a fixed cap, not configurable. slopscript has no maps, closures
/// or string library, so a genuine machine-idle loop can only be counting;
/// raise the constant if a real script ever needs more than a million turns.
const MAX_SPIN_STEPS: u64 = 1_000_000;

/// One open `within`: the absolute clock-tick deadline it expires at, and the
/// source line of its `within` keyword for the expiry / spin messages.
#[derive(Clone, Copy)]
struct Bound {
    deadline: u64,
    line: u32,
}

/// A control-flow signal that unwinds past the normal `Ok` path.
pub(crate) enum Signal {
    /// A script or host-builtin error: becomes `Outcome::Error`.
    Runtime(String),
    /// A failed `assert`: unwinds straight to `run`, becomes
    /// `Outcome::Failed` (the game did the wrong thing, not the script).
    Aborted,
}

pub(crate) type EResult<T> = Result<T, Signal>;

/// What executing one statement (or a whole block) produced, on the
/// non-error path. `WithinExpired` propagates up through every enclosing
/// block, loop and proc call until the `within` statement that owns the
/// expired budget catches it and resumes with the statement after it.
pub(crate) enum Flow {
    Normal,
    WithinExpired,
}

/// The mutable state threaded through one script run.
struct Ctx<'c, 'a> {
    interp: &'c mut Interp<'a>,
    /// Every top-level `proc`, keyed by name. `Rc` so a call can hold its
    /// own reference to the body while recursing without borrowing `self`.
    procs: HashMap<String, Rc<Proc>>,
    /// The `let`/`for`-var/param scope stack, innermost frame last. A block
    /// pushes and pops one frame (block scoping); a proc call replaces the
    /// whole stack (the scope barrier — no closures, matching the parser).
    scopes: Vec<HashMap<String, Value>>,
    call_depth: u32,
    /// Every `within` currently open, outermost first. Each [`Bound`]'s
    /// deadline is already clamped against the one below it at push time, so
    /// only the top needs checking — nested `within`
    /// clamps: an inner bound can only tighten, never extend, and this
    /// stack is NOT reset on a proc call, so a caller's bound still applies
    /// inside the callee (`docs/headless-plan.md`, "Bounds").
    within_stack: Vec<Bound>,
    /// `while`/`repeat` back-edges taken since the host clock last moved,
    /// against [`MAX_SPIN_STEPS`]. Reset to 0 the moment the clock advances,
    /// so it measures a stalled loop rather than a long one.
    spin_steps: u64,
    /// The clock reading at the last back-edge, to notice that advance.
    spin_clock: u64,
    checks: usize,
    failures: Vec<String>,
}

/// Parse-then-walk entry point: `Interp::run` calls this after a successful
/// `parse_only`. Takes the stashed program (leaving `Interp::program` empty,
/// matching `parse_only`'s "a stale parse can never silently run" rule).
pub(crate) fn run(interp: &mut Interp) -> Outcome {
    let program = interp
        .program
        .take()
        .expect("run is only reached after Interp::parse_only succeeded");

    let mut procs = HashMap::new();
    let mut top = Vec::new();
    for item in program.items {
        match item {
            Item::Proc(p) => {
                procs.insert(p.name.clone(), Rc::new(p));
            }
            Item::Stmt(s) => top.push(s),
        }
    }

    let mut ctx = Ctx {
        interp,
        procs,
        scopes: vec![HashMap::new()],
        call_depth: 0,
        within_stack: Vec::new(),
        spin_steps: 0,
        spin_clock: 0,
        checks: 0,
        failures: Vec::new(),
    };

    match ctx.exec_block(&top) {
        Ok(_) if ctx.failures.is_empty() => Outcome::Ok { checks: ctx.checks },
        Ok(_) => Outcome::Failed {
            checks: ctx.checks,
            failures: ctx.failures,
        },
        Err(Signal::Aborted) => Outcome::Failed {
            checks: ctx.checks,
            failures: ctx.failures,
        },
        Err(Signal::Runtime(msg)) => Outcome::Error(msg),
    }
}

/// The runtime type name used in every strict-typing error message.
pub(crate) fn type_name(v: &Value) -> &'static str {
    v.kind()
}
