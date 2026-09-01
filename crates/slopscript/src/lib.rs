// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! # slopscript
//!
//! The dependency-free scripting language behind `slopgb --headless`
//! (`docs/ui-state/headless.md` in the slopgb tree documents the language as
//! built; `docs/headless-plan.md` records why it was designed this way. This
//! crate implements it and knows nothing about Game Boys). A
//! script drives a host through a `register_builtin`/`register_const`
//! registry — `slopscript` never links the emulator, the frontend links
//! `slopscript` and registers every `gb:` method into it.
//!
//! Every parse-time rule (unbound identifiers, the `within` requirement on
//! `while`/`repeat`, the no-leading-`(`/`[` statement rule, unknown call
//! targets) is enforced in [`parser`] before a single statement runs, so a
//! broken script fails before frame 1 rather than mid-run.

#![forbid(unsafe_code)]

use std::collections::HashMap;

mod ast;
mod eval;
mod lexer;
mod parser;

/// A slopscript runtime value. `Nil` is what a host builtin that produces no
/// meaningful result (e.g. `gb:press`) returns.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Nil,
}

/// What a finished run reports. The frontend maps this to a process exit
/// code (`docs/headless-plan.md`: 0 = every check held, 1 = a check/assert
/// failed, 2 = a script bug).
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Ran to the end, every check held. `checks` may be 0 (a valid silent
    /// run — driving a ROM and writing a battery for external comparison is
    /// a legitimate use with no assertions at all).
    Ok { checks: usize },
    /// A `check` or `assert` failed: the script under test did the wrong
    /// thing.
    Failed {
        checks: usize,
        failures: Vec<String>,
    },
    /// A parse error or a runtime error: the script itself is wrong.
    Error(String),
}

impl Value {
    /// This value's runtime type name, as it appears in every type-error
    /// message. Public so a host can word its own builtin's argument errors
    /// the same way the language words its own.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Int(_) => "Int",
            Value::Str(_) => "Str",
            Value::Bytes(_) => "Bytes",
            Value::Bool(_) => "Bool",
            Value::Nil => "Nil",
        }
    }
}

/// A boxed host builtin, keyed by its (possibly namespaced) registration name.
type Builtin<'a> = Box<dyn FnMut(&[Value]) -> Result<Value, String> + 'a>;

/// A slopscript interpreter. Construct one, register every builtin/constant
/// the host wants visible, then [`Interp::run`] a script — registration must
/// happen first, since an unknown call name is a parse error (rule 4).
pub struct Interp<'a> {
    builtins: HashMap<String, Builtin<'a>>,
    consts: HashMap<String, Value>,
    argv: Vec<String>,
    ticks_per_unit: u64,
    clock: Box<dyn FnMut() -> u64 + 'a>,
    program: Option<ast::Program>,
}

impl<'a> Interp<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            builtins: HashMap::new(),
            consts: HashMap::new(),
            argv: Vec::new(),
            ticks_per_unit: 1,
            clock: Box::new(|| 0),
            program: None,
        }
    }

    /// Register a host function callable as `name(...)`. `name` may carry a
    /// namespace prefix (`"gb:read"`), matching the `gb:read(...)` call
    /// syntax. Call this (and [`Interp::register_const`]) before
    /// [`Interp::parse_only`]/[`Interp::run`] — an unregistered call name is
    /// a parse error.
    pub fn register_builtin(
        &mut self,
        name: &str,
        f: impl FnMut(&[Value]) -> Result<Value, String> + 'a,
    ) {
        self.builtins.insert(name.to_string(), Box::new(f));
    }

    /// Register a named constant (buttons, registers, models — see
    /// `docs/headless-plan.md`).
    pub fn register_const(&mut self, name: &str, v: Value) {
        self.consts.insert(name.to_string(), v);
    }

    /// The script's `argv` list (1-based indexing: `argv[1]` is the first).
    pub fn set_argv(&mut self, argv: Vec<String>) {
        self.argv = argv;
    }

    /// The `within` budget clock. `ticks_per_unit` converts a `within N`
    /// count into clock ticks (the frontend passes the cycles in one
    /// frame); `f` reads the host's monotonic tick counter. Default:
    /// `(1, || 0)`, a clock that never advances, so `within` never expires
    /// in standalone tests.
    pub fn set_clock(&mut self, ticks_per_unit: u64, f: impl FnMut() -> u64 + 'a) {
        self.ticks_per_unit = ticks_per_unit;
        self.clock = Box::new(f);
    }

    /// Parse `src` only — every parse-time rule checked, nothing executed.
    /// On success the parsed program is stashed for a following
    /// [`Interp::run`]; on failure any previously parsed program is
    /// dropped, so a stale parse can never silently run.
    pub fn parse_only(&mut self, src: &str) -> Result<(), String> {
        self.program = None;
        let tokens = lexer::lex(src)?;
        let consts: std::collections::HashSet<String> = self.consts.keys().cloned().collect();
        let builtins: std::collections::HashSet<String> = self.builtins.keys().cloned().collect();
        let program = parser::parse(&tokens, &consts, &builtins)?;
        self.program = Some(program);
        Ok(())
    }

    /// Parse and run `src`: a tree-walking evaluator executes the parsed
    /// [`ast::Program`] against the registered `builtins`/`consts`, `argv`,
    /// and the `within` clock, returning the [`Outcome`] a caller maps to a
    /// process exit code (`docs/headless-plan.md`: 0 every check held, 1 a
    /// check/assert failed, 2 the script itself is wrong).
    pub fn run(&mut self, src: &str) -> Outcome {
        match self.parse_only(src) {
            Err(e) => Outcome::Error(e),
            Ok(()) => eval::run(self),
        }
    }
}

impl Default for Interp<'_> {
    fn default() -> Self {
        Self::new()
    }
}
