// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Abstract syntax tree for slopscript. Pure data — no logic lives here, only
//! the shapes the parser produces and the evaluator (a later stage) consumes.
//! All types are `pub(crate)`: the AST is an implementation detail, not part
//! of the crate's public API (`Interp`/`Value`/`Outcome` in `lib.rs` are).
//!
//! Every variant derives `PartialEq` so the parser tests can assert that
//! differently-formatted spellings of the same program (Allman vs K&R vs one
//! line) produce the identical tree — whitespace is fully insignificant.

/// A binary operator, ordered here the same as the language's precedence
/// table (loosest to tightest): `or`, `and`, comparison, `..`, `+ -`, `* / %`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Add,
    Sub,
    Mul,
    Mod,
    Div,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    Int(i64),
    Str(String),
    Bool(bool),
    /// A byte-list literal, `[1, 2, 3]` (empty `[]` allowed).
    ByteList(Vec<Expr>),
    /// A bound identifier: a `let`, a `for` variable, a proc param, a
    /// registered constant, a bare (unnamespaced) builtin name, a proc name,
    /// or `argv`.
    Ident(String),
    /// `base[index]`, e.g. `argv[1]`.
    Index(Box<Expr>, Box<Expr>),
    /// `name(args)` or `ns:name(args)`. `ns` is `None` for a bare call.
    Call {
        ns: Option<String>,
        name: String,
        args: Vec<Expr>,
    },
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Stmt {
    Let(String, Expr),
    /// Assignment to a name that is already bound (`let` introduces a name;
    /// this only ever rebinds one).
    Assign(String, Expr),
    Call {
        ns: Option<String>,
        name: String,
        args: Vec<Expr>,
    },
    /// `if e { } else if e { } else { }`. `arms` holds every `(cond, body)`
    /// pair (the leading `if` and every `else if`); `else_body` is the
    /// trailing plain `else`, if any.
    If {
        arms: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    /// Pre-test loop; parses only inside a `within`.
    While(Expr, Vec<Stmt>),
    /// Post-test loop: `repeat { body } until cond`. Parses only inside a
    /// `within`.
    Repeat(Vec<Stmt>, Expr),
    /// `for var = from, to { body }` — counted, inclusive, terminates by
    /// construction. Exempt from the `within` requirement.
    For {
        var: String,
        from: Expr,
        to: Expr,
        body: Vec<Stmt>,
    },
    /// `within limit { body }` with an optional `else` run on expiry.
    /// `line` is the 1-based source line the `within` keyword started on, so
    /// an expiry report can name the block (`docs/headless-plan.md`, "Bounds").
    Within {
        limit: Expr,
        body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        line: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Proc {
    pub(crate) name: String,
    pub(crate) params: Vec<String>,
    pub(crate) body: Vec<Stmt>,
}

/// One top-level construct: a proc definition or an ordinary statement.
/// `proc` is top-level only, so this distinction only exists at this level —
/// nested blocks hold plain `Vec<Stmt>`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Item {
    Proc(Proc),
    Stmt(Stmt),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Program {
    pub(crate) items: Vec<Item>,
}
