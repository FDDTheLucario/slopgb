// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Statement execution: blocks, loops, `if`, proc calls and the `within`
//! frame budget (`docs/headless-plan.md`, "Bounds").

use super::*;
use crate::ast::Expr;

impl Ctx<'_, '_> {
    /// Execute `stmts` in order. Checks the `within` budget before each one
    /// ("Check it before each statement inside a within body") — a nested
    /// block, loop or proc call signalling `WithinExpired` stops this block
    /// too and bubbles further, until the owning `within` statement catches
    /// it and resumes after itself.
    pub(super) fn exec_block(&mut self, stmts: &[Stmt]) -> EResult<Flow> {
        for stmt in stmts {
            if self.within_expired() {
                return Ok(Flow::WithinExpired);
            }
            match self.exec_stmt(stmt)? {
                Flow::Normal => {}
                Flow::WithinExpired => return Ok(Flow::WithinExpired),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> EResult<Flow> {
        match stmt {
            Stmt::Let(name, expr) => {
                let v = self.eval_expr(expr)?;
                self.bind(name.clone(), v);
                Ok(Flow::Normal)
            }
            Stmt::Assign(name, expr) => {
                let v = self.eval_expr(expr)?;
                self.assign(name, v)
            }
            Stmt::Call { ns, name, args } => {
                let vals = self.eval_args(args)?;
                self.exec_call(ns.as_deref(), name, vals)
            }
            Stmt::If { arms, else_body } => self.exec_if(arms, else_body),
            Stmt::While(cond, body) => self.exec_while(cond, body),
            Stmt::Repeat(body, cond) => self.exec_repeat(body, cond),
            Stmt::For {
                var,
                from,
                to,
                body,
            } => self.exec_for(var, from, to, body),
            Stmt::Within {
                limit,
                body,
                else_body,
                line,
            } => self.exec_within(limit, body, else_body, *line),
        }
    }

    fn exec_if(
        &mut self,
        arms: &[(Expr, Vec<Stmt>)],
        else_body: &Option<Vec<Stmt>>,
    ) -> EResult<Flow> {
        for (cond, body) in arms {
            if self.eval_bool(cond, "if")? {
                return self.exec_scoped_block(body);
            }
        }
        match else_body {
            Some(body) => self.exec_scoped_block(body),
            None => Ok(Flow::Normal),
        }
    }

    fn exec_while(&mut self, cond: &Expr, body: &[Stmt]) -> EResult<Flow> {
        loop {
            if self.within_expired() {
                return Ok(Flow::WithinExpired);
            }
            self.note_back_edge()?;
            if !self.eval_bool(cond, "while")? {
                return Ok(Flow::Normal);
            }
            match self.exec_scoped_block(body)? {
                Flow::Normal => {}
                Flow::WithinExpired => return Ok(Flow::WithinExpired),
            }
        }
    }

    /// Post-test: `repeat { body } until cond` runs `body` at least once —
    /// except a `within` that has already expired before the first
    /// iteration cuts it short too, the same as it would mid-loop.
    fn exec_repeat(&mut self, body: &[Stmt], cond: &Expr) -> EResult<Flow> {
        loop {
            if self.within_expired() {
                return Ok(Flow::WithinExpired);
            }
            self.note_back_edge()?;
            match self.exec_scoped_block(body)? {
                Flow::Normal => {}
                Flow::WithinExpired => return Ok(Flow::WithinExpired),
            }
            if self.eval_bool(cond, "until")? {
                return Ok(Flow::Normal);
            }
        }
    }

    /// `for var = from, to { body }`: inclusive, step 1, zero iterations
    /// when `to < from`. Both bounds are evaluated once, before the loop;
    /// `var` is bound only for the duration of `body`. Exempt from the
    /// `within` requirement (a counted loop can't hang), but still honours
    /// one if it happens to be nested inside one.
    fn exec_for(&mut self, var: &str, from: &Expr, to: &Expr, body: &[Stmt]) -> EResult<Flow> {
        let from_v = self.eval_int(from, "for")?;
        let to_v = self.eval_int(to, "for")?;
        let mut i = from_v;
        while i <= to_v {
            if self.within_expired() {
                return Ok(Flow::WithinExpired);
            }
            self.scopes
                .push(HashMap::from([(var.to_string(), Value::Int(i))]));
            let r = self.exec_block(body);
            self.scopes.pop();
            match r? {
                Flow::Normal => {}
                Flow::WithinExpired => return Ok(Flow::WithinExpired),
            }
            i = i
                .checked_add(1)
                .ok_or_else(|| Signal::Runtime("'for' loop counter overflowed".to_string()))?;
        }
        Ok(Flow::Normal)
    }

    /// `within N { body } else { else_body }`. `N` is a frame count,
    /// converted to clock ticks via `Interp::set_clock`'s `ticks_per_unit`.
    /// A nested `within` can only tighten the deadline, never extend it, so
    /// the pushed value is already `min(own_deadline, enclosing)` — see the
    /// `within_stack` field doc on [`Ctx`].
    fn exec_within(
        &mut self,
        limit: &Expr,
        body: &[Stmt],
        else_body: &Option<Vec<Stmt>>,
        line: u32,
    ) -> EResult<Flow> {
        let n = self.eval_int(limit, "within")?;
        if n < 0 {
            return Err(Signal::Runtime(format!(
                "'within' bound must be non-negative, got {n}"
            )));
        }
        let now = (self.interp.clock)();
        let budget = (n as u64)
            .checked_mul(self.interp.ticks_per_unit)
            .ok_or_else(|| {
                Signal::Runtime(format!(
                    "'within {n}': budget overflowed converting to ticks"
                ))
            })?;
        let own_deadline = now
            .checked_add(budget)
            .ok_or_else(|| Signal::Runtime(format!("'within {n}': deadline overflowed")))?;
        let effective = self
            .within_stack
            .last()
            .map_or(own_deadline, |outer| own_deadline.min(outer.deadline));
        self.within_stack.push(Bound {
            deadline: effective,
            line,
        });

        let result = self.exec_block(body);
        self.within_stack.pop();

        // Only a body that was *cut short* expired. A body that ran to its
        // last statement did everything the script asked, even if that last
        // statement spent the final cycle of the budget — landing exactly on
        // the deadline is what a `wait_frames` sized to the bound always
        // does, and reporting it as an expiry would run `else` (typically a
        // `check(false, ...)`) and turn a passing run into a CI failure.
        let expired = matches!(result, Ok(Flow::WithinExpired));
        if !expired {
            return result;
        }

        eprintln!("slopgb: within {n} (line {line}) expired");
        let after = match else_body {
            // The `else` body runs unconditionally: our own (now-popped)
            // budget can't re-trigger it. It still answers to whatever
            // enclosing `within` remains on the stack, via the same
            // per-statement check every other block gets.
            Some(b) => self.exec_scoped_block(b)?,
            None => Flow::Normal,
        };
        match after {
            Flow::WithinExpired => Ok(Flow::WithinExpired),
            Flow::Normal => {
                // Nested clamp: an enclosing `within` may already be past
                // its own deadline too (it clamped ours down to reach it).
                // Re-check so that case still unwinds to it, instead of
                // this block quietly continuing as if nothing happened.
                if self.within_expired() {
                    Ok(Flow::WithinExpired)
                } else {
                    Ok(Flow::Normal)
                }
            }
        }
    }

    /// `true` once the clock reaches the tightest (innermost-clamped)
    /// deadline currently open. `false` with no `within` open at all.
    fn within_expired(&mut self) -> bool {
        match self.within_stack.last() {
            Some(&b) => (self.interp.clock)() >= b.deadline,
            None => false,
        }
    }

    /// Count one `while`/`repeat` back-edge against [`MAX_SPIN_STEPS`], and
    /// fail the run if the loop has turned that many times without the host
    /// clock moving. Such a loop can never reach its `within` deadline (that
    /// deadline is in emulated cycles), so it would otherwise spin forever.
    ///
    /// This is a `Signal::Runtime`, not a `within` expiry: expiry means the
    /// bound did its job and the run carries on, whereas a stalled loop means
    /// the *script* is wrong, which the frontend reports as exit 2. The
    /// parser guarantees a `while`/`repeat` is always inside a `within`, so
    /// there is always a bound to name.
    fn note_back_edge(&mut self) -> EResult<()> {
        let now = (self.interp.clock)();
        if now != self.spin_clock {
            self.spin_clock = now;
            self.spin_steps = 0;
            return Ok(());
        }
        self.spin_steps += 1;
        if self.spin_steps <= MAX_SPIN_STEPS {
            return Ok(());
        }
        let line = self.within_stack.last().map_or(0, |b| b.line);
        Err(Signal::Runtime(format!(
            "loop inside the 'within' on line {line} turned {MAX_SPIN_STEPS} times without advancing the clock — it can never reach its bound"
        )))
    }

    /// Run `body` in a fresh block-scope frame (popped on every exit path).
    fn exec_scoped_block(&mut self, body: &[Stmt]) -> EResult<Flow> {
        self.scopes.push(HashMap::new());
        let r = self.exec_block(body);
        self.scopes.pop();
        r
    }

    fn bind(&mut self, name: String, v: Value) {
        self.scopes
            .last_mut()
            .expect("at least one scope frame always exists")
            .insert(name, v);
    }

    /// Rebind `name` in the innermost scope frame it's already bound in.
    /// The parser only accepts `Stmt::Assign` for a name it proved was
    /// bound, so a miss here would mean an evaluator bug, not a script
    /// one — reported as a runtime error regardless, never a panic.
    fn assign(&mut self, name: &str, v: Value) -> EResult<Flow> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), v);
                return Ok(Flow::Normal);
            }
        }
        Err(Signal::Runtime(format!(
            "assignment to unbound name '{name}'"
        )))
    }

    pub(super) fn eval_bool(&mut self, e: &Expr, ctx: &str) -> EResult<bool> {
        match self.eval_expr(e)? {
            Value::Bool(b) => Ok(b),
            other => Err(Signal::Runtime(format!(
                "{ctx} condition must be a Bool, got {}",
                type_name(&other)
            ))),
        }
    }

    pub(super) fn eval_int(&mut self, e: &Expr, ctx: &str) -> EResult<i64> {
        match self.eval_expr(e)? {
            Value::Int(n) => Ok(n),
            other => Err(Signal::Runtime(format!(
                "{ctx} expects an Int, got {}",
                type_name(&other)
            ))),
        }
    }
}
