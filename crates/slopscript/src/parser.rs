// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Recursive-descent parser: tokens in, [`Program`] out, with every
//! parse-time rule from `docs/headless-plan.md` enforced here rather than
//! deferred to a runtime error:
//!
//! 1. An unbound identifier is a parse error (`let` is the only binder).
//! 2. `while` and `repeat` must be lexically inside a `within` (tracked as a
//!    depth counter, reset to 0 on entry to a proc body — a loop in a proc
//!    needs its own `within`).
//! 3. A statement may not begin with `(` or `[` (it would glue onto the
//!    previous statement now that newlines are insignificant).
//! 4. A call to an unregistered, non-proc name is a parse error.
//! 5. A proc is called as a statement only — using one in an expression
//!    position (`let x = my_proc()`) is a parse error, since procs have no
//!    `return` and produce no value.
//!
//! A **proc body does not see outer variables**: there are no closures, so
//! entering a proc body replaces the whole lexical scope stack with a fresh
//! one seeded only by that proc's own parameters — a scope barrier, not just
//! a new nested frame.
//!
//! Because a call may textually precede the `proc` it names, proc names are
//! collected in a cheap pre-pass over the raw token stream before the real,
//! validating parse begins.

use crate::ast::{BinOp, Expr, Item, Proc, Program, Stmt};
use crate::lexer::{Token, TokenKind};
use std::collections::HashSet;

/// Runner-side globals that ship with the language itself (headless-plan.md,
/// the "`gb:` / global split" table: "`argv print check assert hex` —
/// runner-side"). Unlike `gb:` methods, these need no `register_builtin`
/// call — they are always valid call targets.
pub(crate) const INTRINSIC_CALLS: [&str; 4] = ["print", "check", "assert", "hex"];

/// Parse a full program. `consts` and `builtins` are the exact name sets the
/// host has registered (via `Interp::register_const` / `register_builtin`)
/// at the time of the call — registration order relative to parsing matters,
/// per rule 4.
pub(crate) fn parse(
    tokens: &[Token],
    consts: &HashSet<String>,
    builtins: &HashSet<String>,
) -> Result<Program, String> {
    let procs = collect_proc_names(tokens);
    let mut bare_builtins: HashSet<String> = builtins
        .iter()
        .filter(|n| !n.contains(':'))
        .cloned()
        .collect();
    let ns_builtins: HashSet<String> = builtins
        .iter()
        .filter(|n| n.contains(':'))
        .cloned()
        .collect();
    for intrinsic in INTRINSIC_CALLS {
        bare_builtins.insert(intrinsic.to_string());
    }
    let mut p = Parser {
        toks: tokens,
        pos: 0,
        consts: consts.clone(),
        bare_builtins,
        ns_builtins,
        procs,
        scopes: vec![HashSet::new()],
        within_depth: 0,
    };
    p.parse_program()
}

/// A cheap scan for every `proc <ident>` pair in the raw token stream, so a
/// call can be validated (rule 4) against a proc defined later in the file.
fn collect_proc_names(tokens: &[Token]) -> HashSet<String> {
    let mut names = HashSet::new();
    for w in tokens.windows(2) {
        if w[0].kind == TokenKind::Proc {
            if let TokenKind::Ident(name) = &w[1].kind {
                names.insert(name.clone());
            }
        }
    }
    names
}

struct Parser<'t> {
    toks: &'t [Token],
    pos: usize,
    consts: HashSet<String>,
    /// Registered bare builtins plus [`INTRINSIC_CALLS`].
    bare_builtins: HashSet<String>,
    /// Registered namespaced builtins, keyed by their full `"ns:name"`.
    ns_builtins: HashSet<String>,
    procs: HashSet<String>,
    /// The lexical `let`/`for`-var/param scope stack, innermost frame last.
    /// A proc body replaces this whole stack (the scope barrier); every
    /// other block pushes one frame on top and pops it on exit.
    scopes: Vec<HashSet<String>>,
    /// How many `within` blocks lexically enclose the current position.
    /// `while`/`repeat` require this to be nonzero (rule 2).
    within_depth: u32,
}

impl Parser<'_> {
    fn parse_program(&mut self) -> Result<Program, String> {
        let mut items = Vec::new();
        loop {
            self.skip_semicolons();
            if self.check(&TokenKind::Eof) {
                break;
            }
            if self.check(&TokenKind::Proc) {
                items.push(Item::Proc(self.parse_proc()?));
            } else {
                items.push(Item::Stmt(self.parse_stmt()?));
            }
        }
        Ok(Program { items })
    }

    fn parse_proc(&mut self) -> Result<Proc, String> {
        self.advance(); // 'proc'
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                params.push(self.expect_ident()?);
                if self.check(&TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RParen)?;

        // The scope barrier: a proc body sees only its own params, lets,
        // constants, builtins, procs and argv — never the caller's scope.
        let saved_scopes =
            std::mem::replace(&mut self.scopes, vec![params.iter().cloned().collect()]);
        let saved_within = std::mem::replace(&mut self.within_depth, 0);
        let body = self.parse_block();
        self.scopes = saved_scopes;
        self.within_depth = saved_within;

        Ok(Proc {
            name,
            params,
            body: body?,
        })
    }

    /// One statement. Also where rule 3 (no leading `(`/`[`) and the
    /// nested-`proc` restriction are enforced, since both are checked before
    /// any statement-specific parsing begins.
    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.cur_line();
        match self.peek() {
            TokenKind::LParen => Err(format!(
                "line {line}: a statement cannot begin with '(' — it would glue onto the previous statement as a call"
            )),
            TokenKind::LBracket => Err(format!(
                "line {line}: a statement cannot begin with '[' — it would glue onto the previous statement as an index"
            )),
            TokenKind::Proc => Err(format!(
                "line {line}: 'proc' may only be defined at the top level, not nested in a block"
            )),
            TokenKind::Let => self.parse_let(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Repeat => self.parse_repeat(),
            TokenKind::For => self.parse_for(),
            TokenKind::Within => self.parse_within(),
            TokenKind::Ident(_) => self.parse_ident_stmt(),
            other => Err(format!(
                "line {line}: expected a statement, found {}",
                describe(other)
            )),
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, String> {
        self.advance(); // 'let'
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Assign)?;
        let expr = self.parse_expr()?;
        self.scopes
            .last_mut()
            .expect("at least one scope frame always exists")
            .insert(name.clone());
        Ok(Stmt::Let(name, expr))
    }

    /// A bare `NAME`, `NAME(...)`, `NAME = ...`, or `NS:NAME(...)` statement
    /// — the four shapes that can start with an identifier.
    fn parse_ident_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.cur_line();
        let name = self.expect_ident()?;
        match self.peek() {
            TokenKind::Colon => {
                self.advance();
                let member_line = self.cur_line();
                let member = self.expect_ident()?;
                self.expect(&TokenKind::LParen)?;
                let args = self.parse_args()?;
                self.expect(&TokenKind::RParen)?;
                self.check_ns_call(&name, &member, member_line)?;
                Ok(Stmt::Call {
                    ns: Some(name),
                    name: member,
                    args,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let args = self.parse_args()?;
                self.expect(&TokenKind::RParen)?;
                self.check_bare_call(&name, line)?;
                Ok(Stmt::Call {
                    ns: None,
                    name,
                    args,
                })
            }
            TokenKind::Assign => {
                self.advance();
                if !self.is_bound(&name) {
                    return Err(format!("line {line}: assignment to unbound name '{name}'"));
                }
                let expr = self.parse_expr()?;
                Ok(Stmt::Assign(name, expr))
            }
            other => Err(format!(
                "line {line}: expected '(', ':' or '=' after '{name}', found {}",
                describe(other)
            )),
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.advance(); // 'if'
        let mut arms = Vec::new();
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        arms.push((cond, body));

        let mut else_body = None;
        while self.check(&TokenKind::Else) {
            self.advance();
            if self.check(&TokenKind::If) {
                self.advance();
                let cond = self.parse_expr()?;
                let body = self.parse_block()?;
                arms.push((cond, body));
            } else {
                else_body = Some(self.parse_block()?);
                break;
            }
        }
        Ok(Stmt::If { arms, else_body })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        let line = self.cur_line();
        if self.within_depth == 0 {
            return Err(format!(
                "line {line}: 'while' must be lexically inside a 'within' block — an unbounded loop is a parse error"
            ));
        }
        self.advance(); // 'while'
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While(cond, body))
    }

    fn parse_repeat(&mut self) -> Result<Stmt, String> {
        let line = self.cur_line();
        if self.within_depth == 0 {
            return Err(format!(
                "line {line}: 'repeat' must be lexically inside a 'within' block — an unbounded loop is a parse error"
            ));
        }
        self.advance(); // 'repeat'
        let body = self.parse_block()?;
        self.expect(&TokenKind::Until)?;
        let cond = self.parse_expr()?;
        Ok(Stmt::Repeat(body, cond))
    }

    /// `for` is exempt from the `within` rule (a counted loop cannot hang)
    /// and does not affect `within_depth`.
    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.advance(); // 'for'
        let var = self.expect_ident()?;
        self.expect(&TokenKind::Assign)?;
        let from = self.parse_expr()?;
        self.expect(&TokenKind::Comma)?;
        let to = self.parse_expr()?;
        let body = self.parse_block_seeded(std::slice::from_ref(&var))?;
        Ok(Stmt::For {
            var,
            from,
            to,
            body,
        })
    }

    fn parse_within(&mut self) -> Result<Stmt, String> {
        let line = self.cur_line();
        self.advance(); // 'within'
        let limit = self.parse_expr()?;
        self.within_depth += 1;
        let body = self.parse_block();
        self.within_depth -= 1;
        let body = body?;

        // The `else` branch is a sibling of the counted body, not nested in
        // it, so it runs at the enclosing depth (already restored above).
        let else_body = if self.check(&TokenKind::Else) {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };
        Ok(Stmt::Within {
            limit,
            body,
            else_body,
            line,
        })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.parse_block_seeded(&[])
    }

    /// Parse a `{ ... }` block whose scope frame starts pre-populated with
    /// `seed` (a `for` loop's own variable).
    fn parse_block_seeded(&mut self, seed: &[String]) -> Result<Vec<Stmt>, String> {
        self.expect(&TokenKind::LBrace)?;
        self.scopes.push(seed.iter().cloned().collect());
        let mut stmts = Vec::new();
        loop {
            self.skip_semicolons();
            if self.check(&TokenKind::RBrace) {
                break;
            }
            if self.check(&TokenKind::Eof) {
                self.scopes.pop();
                return Err(format!(
                    "line {}: unterminated block, expected '}}'",
                    self.cur_line()
                ));
            }
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace)?;
        self.scopes.pop();
        Ok(stmts)
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        while self.check(&TokenKind::Or) {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_not()?;
        while self.check(&TokenKind::And) {
            self.advance();
            let rhs = self.parse_not()?;
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if self.check(&TokenKind::Not) {
            self.advance();
            let inner = self.parse_not()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_cmp()
    }

    /// Comparison is non-associative: at most one `== != < <= > >=` per
    /// expression. A second one simply is not consumed here, so it surfaces
    /// as an "unexpected token" error from whatever expected the expression
    /// to have ended.
    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_concat()?;
        let op = match self.peek() {
            TokenKind::Eq => Some(BinOp::Eq),
            TokenKind::Ne => Some(BinOp::Ne),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Le => Some(BinOp::Le),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::Ge => Some(BinOp::Ge),
            _ => None,
        };
        let Some(op) = op else { return Ok(lhs) };
        self.advance();
        let rhs = self.parse_concat()?;
        Ok(Expr::Binary(op, Box::new(lhs), Box::new(rhs)))
    }

    fn parse_concat(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_add()?;
        while self.check(&TokenKind::DotDot) {
            self.advance();
            let rhs = self.parse_add()?;
            lhs = Expr::Binary(BinOp::Concat, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_mul()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if self.check(&TokenKind::Minus) {
            self.advance();
            let inner = self.parse_unary()?;
            return Ok(Expr::Neg(Box::new(inner)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let line = self.cur_line();
        match self.peek().clone() {
            TokenKind::Int(n) => {
                self.advance();
                Ok(Expr::Int(n))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr()?);
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                // A byte-list literal can itself be indexed (`[10, 20,
                // 30][2]`) — this is safe from the same-line-vs-next-line
                // ambiguity a bare trailing `[` would otherwise risk (see
                // the identifier case below) only because it's still inside
                // the same `parse_expr` call that opened with `[`; the
                // parser is never at a statement boundary here.
                Ok(self.parse_index_chain(Expr::ByteList(items))?)
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(inner)
            }
            TokenKind::Ident(name) => {
                self.advance();
                match self.peek() {
                    TokenKind::Colon => {
                        self.advance();
                        let member_line = self.cur_line();
                        let member = self.expect_ident()?;
                        self.expect(&TokenKind::LParen)?;
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        self.check_ns_call(&name, &member, member_line)?;
                        Ok(Expr::Call {
                            ns: Some(name),
                            name: member,
                            args,
                        })
                    }
                    TokenKind::LParen => {
                        self.advance();
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        self.check_bare_call(&name, line)?;
                        if self.procs.contains(&name) {
                            return Err(format!(
                                "line {line}: '{name}' is a proc — procs are called as statements only, not used as an expression"
                            ));
                        }
                        Ok(Expr::Call {
                            ns: None,
                            name,
                            args,
                        })
                    }
                    _ => {
                        if !self.is_bound(&name) {
                            return Err(format!("line {line}: unbound identifier '{name}'"));
                        }
                        // Indexing chains only directly onto an identifier
                        // or a byte-list literal, never onto an arbitrary
                        // trailing expression — chaining after e.g. an int
                        // literal would silently glue a *following*
                        // statement's leading `[` onto this one instead of
                        // hitting the rule-3 error, which is exactly the
                        // ambiguity that rule exists to remove.
                        self.parse_index_chain(Expr::Ident(name))
                    }
                }
            }
            other => Err(format!(
                "line {line}: unexpected token in expression: {}",
                describe(&other)
            )),
        }
    }

    /// Consume zero or more trailing `[index]` groups onto `base`
    /// (`argv[1]`, `argv[1][2]`, `[10, 20, 30][2]`).
    fn parse_index_chain(&mut self, base: Expr) -> Result<Expr, String> {
        let mut e = base;
        while self.check(&TokenKind::LBracket) {
            self.advance();
            let idx = self.parse_expr()?;
            self.expect(&TokenKind::RBracket)?;
            e = Expr::Index(Box::new(e), Box::new(idx));
        }
        Ok(e)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if self.check(&TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        Ok(args)
    }

    /// Rule 1, applied to any bare name used as a value: a `let`/`for`/proc
    /// param in the current scope stack, a registered constant, a bare
    /// builtin (registered or intrinsic), a proc name, or `argv`.
    fn is_bound(&self, name: &str) -> bool {
        name == "argv"
            || self.consts.contains(name)
            || self.bare_builtins.contains(name)
            || self.procs.contains(name)
            || self.scopes.iter().any(|s| s.contains(name))
    }

    /// Rule 4 for a bare `NAME(...)` call: it must be a proc or a bare
    /// builtin (registered or intrinsic).
    fn check_bare_call(&self, name: &str, line: u32) -> Result<(), String> {
        if self.procs.contains(name) || self.bare_builtins.contains(name) {
            Ok(())
        } else {
            Err(format!(
                "line {line}: call to unknown '{name}' — not a registered builtin or a proc"
            ))
        }
    }

    /// Rule 4 for a `NS:NAME(...)` call: it must be a registered namespaced
    /// builtin. Namespaced calls are never procs (proc names are never
    /// colon-qualified).
    fn check_ns_call(&self, ns: &str, name: &str, line: u32) -> Result<(), String> {
        let full = format!("{ns}:{name}");
        if self.ns_builtins.contains(&full) {
            Ok(())
        } else {
            Err(format!(
                "line {line}: call to unknown '{full}' — not a registered builtin"
            ))
        }
    }

    fn peek(&self) -> &TokenKind {
        &self.toks[self.pos].kind
    }

    fn cur_line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn check(&self, k: &TokenKind) -> bool {
        self.peek() == k
    }

    /// Returns the current token and moves past it. Never advances past the
    /// trailing `Eof` token, so this never runs off the end of `toks`.
    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, k: &TokenKind) -> Result<Token, String> {
        if self.check(k) {
            Ok(self.advance())
        } else {
            Err(format!(
                "line {}: expected {}, found {}",
                self.cur_line(),
                describe(k),
                describe(self.peek())
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(format!(
                "line {}: expected an identifier, found {}",
                self.cur_line(),
                describe(&other)
            )),
        }
    }

    fn skip_semicolons(&mut self) {
        while self.check(&TokenKind::Semicolon) {
            self.advance();
        }
    }
}

/// A short, human-readable name for a token kind, for error messages.
fn describe(k: &TokenKind) -> String {
    match k {
        TokenKind::Int(n) => format!("integer '{n}'"),
        TokenKind::Str(s) => format!("string {s:?}"),
        TokenKind::Ident(n) => format!("identifier '{n}'"),
        TokenKind::Let => "'let'".into(),
        TokenKind::Proc => "'proc'".into(),
        TokenKind::If => "'if'".into(),
        TokenKind::Else => "'else'".into(),
        TokenKind::While => "'while'".into(),
        TokenKind::Repeat => "'repeat'".into(),
        TokenKind::Until => "'until'".into(),
        TokenKind::For => "'for'".into(),
        TokenKind::Within => "'within'".into(),
        TokenKind::And => "'and'".into(),
        TokenKind::Or => "'or'".into(),
        TokenKind::Not => "'not'".into(),
        TokenKind::True => "'true'".into(),
        TokenKind::False => "'false'".into(),
        TokenKind::LBrace => "'{'".into(),
        TokenKind::RBrace => "'}'".into(),
        TokenKind::LParen => "'('".into(),
        TokenKind::RParen => "')'".into(),
        TokenKind::LBracket => "'['".into(),
        TokenKind::RBracket => "']'".into(),
        TokenKind::Comma => "','".into(),
        TokenKind::Semicolon => "';'".into(),
        TokenKind::Colon => "':'".into(),
        TokenKind::Assign => "'='".into(),
        TokenKind::Eq => "'=='".into(),
        TokenKind::Ne => "'!='".into(),
        TokenKind::Lt => "'<'".into(),
        TokenKind::Le => "'<='".into(),
        TokenKind::Gt => "'>'".into(),
        TokenKind::Ge => "'>='".into(),
        TokenKind::Plus => "'+'".into(),
        TokenKind::Minus => "'-'".into(),
        TokenKind::Star => "'*'".into(),
        TokenKind::Slash => "'/'".into(),
        TokenKind::Percent => "'%'".into(),
        TokenKind::DotDot => "'..'".into(),
        TokenKind::Eof => "end of input".into(),
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
