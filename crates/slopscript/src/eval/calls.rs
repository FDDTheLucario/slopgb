// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Call dispatch: proc invocation (the scope barrier + recursion cap), the
//! four runner intrinsics (`print`/`hex`/`check`/`assert`), and handing
//! everything else to a registered host builtin.

use super::format::{format_value, hex_bytes, hex_int};
use super::*;
use crate::ast::Proc;

impl Ctx<'_, '_> {
    /// `Stmt::Call`: a bare call is a proc if one by that name exists
    /// (procs are called as statements only — the parser already rejects
    /// one used as an expression), otherwise it's a value-producing call
    /// whose result is simply discarded.
    pub(super) fn exec_call(
        &mut self,
        ns: Option<&str>,
        name: &str,
        args: Vec<Value>,
    ) -> EResult<Flow> {
        if ns.is_none() {
            if let Some(proc) = self.procs.get(name).cloned() {
                return self.call_proc(&proc, args);
            }
        }
        self.call_value(ns, name, &args)?;
        Ok(Flow::Normal)
    }

    /// `proc(...)`: arity-checked, depth-capped recursion, and the scope
    /// barrier — the callee's scope stack starts fresh with only its own
    /// params (no closures). The `within` budget stack is untouched: it
    /// persists across the call (see the `within_stack` doc on [`Ctx`]).
    fn call_proc(&mut self, proc: &Proc, args: Vec<Value>) -> EResult<Flow> {
        if args.len() != proc.params.len() {
            return Err(Signal::Runtime(format!(
                "proc '{}' expects {} argument(s), got {}",
                proc.name,
                proc.params.len(),
                args.len()
            )));
        }
        self.call_depth += 1;
        if self.call_depth > MAX_CALL_DEPTH {
            self.call_depth -= 1;
            return Err(Signal::Runtime(format!(
                "call depth exceeded {MAX_CALL_DEPTH} (recursion in proc '{}')",
                proc.name
            )));
        }
        let mut frame = HashMap::new();
        for (p, v) in proc.params.iter().zip(args) {
            frame.insert(p.clone(), v);
        }
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![frame]);
        let result = self.exec_block(&proc.body);
        self.scopes = saved_scopes;
        self.call_depth -= 1;
        result
    }

    /// A value-producing call: one of the four runner intrinsics, or a
    /// registered host builtin (`"ns:name"` when namespaced, else just
    /// `"name"`).
    pub(super) fn call_value(
        &mut self,
        ns: Option<&str>,
        name: &str,
        args: &[Value],
    ) -> EResult<Value> {
        if ns.is_none() {
            match name {
                "print" => return self.intrinsic_print(args),
                "hex" => return self.intrinsic_hex(args),
                "check" => return self.intrinsic_check(args),
                "assert" => return self.intrinsic_assert(args),
                _ => {}
            }
        }
        let key = match ns {
            Some(ns) => format!("{ns}:{name}"),
            None => name.to_string(),
        };
        let f = self
            .interp
            .builtins
            .get_mut(&key)
            .ok_or_else(|| Signal::Runtime(format!("call to unregistered builtin '{key}'")))?;
        f(args).map_err(|e| Signal::Runtime(format!("{key}: {e}")))
    }

    fn intrinsic_print(&mut self, args: &[Value]) -> EResult<Value> {
        let [v] = require_args(args, "print")?;
        println!("{}", format_value(v));
        Ok(Value::Nil)
    }

    fn intrinsic_hex(&mut self, args: &[Value]) -> EResult<Value> {
        let [v] = require_args(args, "hex")?;
        match v {
            Value::Bytes(b) => Ok(Value::Str(hex_bytes(b))),
            Value::Int(n) => Ok(Value::Str(hex_int(*n))),
            other => Err(Signal::Runtime(format!(
                "hex() requires a Bytes or an Int, got {}",
                type_name(other)
            ))),
        }
    }

    /// Records the result and keeps running, so one run reports every
    /// broken field rather than only the first.
    fn intrinsic_check(&mut self, args: &[Value]) -> EResult<Value> {
        let [cond, msg] = require_args2(args, "check")?;
        let (b, m) = bool_and_str(cond, msg, "check")?;
        self.checks += 1;
        if !b {
            eprintln!("check failed: {m}");
            self.failures.push(m.to_string());
        }
        Ok(Value::Bool(b))
    }

    /// Aborts the run at once on failure (`Signal::Aborted`) — still an
    /// `Outcome::Failed` (the game did the wrong thing), never
    /// `Outcome::Error`.
    fn intrinsic_assert(&mut self, args: &[Value]) -> EResult<Value> {
        let [cond, msg] = require_args2(args, "assert")?;
        let (b, m) = bool_and_str(cond, msg, "assert")?;
        if !b {
            eprintln!("assert failed: {m}");
            self.failures.push(m.to_string());
            return Err(Signal::Aborted);
        }
        Ok(Value::Bool(b))
    }
}

fn require_args<'v>(args: &'v [Value], who: &str) -> EResult<[&'v Value; 1]> {
    match args {
        [a] => Ok([a]),
        _ => Err(Signal::Runtime(format!(
            "{who}() expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn require_args2<'v>(args: &'v [Value], who: &str) -> EResult<[&'v Value; 2]> {
    match args {
        [a, b] => Ok([a, b]),
        _ => Err(Signal::Runtime(format!(
            "{who}() expects 2 arguments, got {}",
            args.len()
        ))),
    }
}

fn bool_and_str<'v>(cond: &'v Value, msg: &'v Value, who: &str) -> EResult<(bool, &'v str)> {
    let Value::Bool(b) = cond else {
        return Err(Signal::Runtime(format!(
            "{who}() condition must be a Bool, got {}",
            type_name(cond)
        )));
    };
    let Value::Str(m) = msg else {
        return Err(Signal::Runtime(format!(
            "{who}() message must be a Str, got {}",
            type_name(msg)
        )));
    };
    Ok((*b, m.as_str()))
}
