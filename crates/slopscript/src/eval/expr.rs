// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Expression evaluation: literals, identifiers, indexing and every
//! operator. Strict typing throughout — an operator applied to the wrong
//! type is always a runtime error, never a silent coercion
//! (`docs/headless-plan.md`, "Types"/"Operators").

use super::*;
use crate::ast::{BinOp, Expr};

impl Ctx<'_, '_> {
    pub(super) fn eval_expr(&mut self, e: &Expr) -> EResult<Value> {
        match e {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::ByteList(items) => self.eval_byte_list(items),
            Expr::Ident(name) => self.lookup(name),
            Expr::Index(base, idx) => self.eval_index(base, idx),
            Expr::Call { ns, name, args } => {
                let vals = self.eval_args(args)?;
                self.call_value(ns.as_deref(), name, &vals)
            }
            Expr::Not(inner) => match self.eval_expr(inner)? {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                other => Err(Signal::Runtime(format!(
                    "'not' requires a Bool, got {}",
                    type_name(&other)
                ))),
            },
            Expr::Neg(inner) => match self.eval_expr(inner)? {
                Value::Int(n) => n
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or_else(|| Signal::Runtime(format!("integer overflow negating {n}"))),
                other => Err(Signal::Runtime(format!(
                    "unary '-' requires an Int, got {}",
                    type_name(&other)
                ))),
            },
            Expr::Binary(op, l, r) => self.eval_binary(op, l, r),
        }
    }

    pub(super) fn eval_args(&mut self, args: &[Expr]) -> EResult<Vec<Value>> {
        args.iter().map(|a| self.eval_expr(a)).collect()
    }

    fn eval_byte_list(&mut self, items: &[Expr]) -> EResult<Value> {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            match self.eval_expr(item)? {
                Value::Int(n) if (0..=255).contains(&n) => out.push(n as u8),
                Value::Int(n) => {
                    return Err(Signal::Runtime(format!(
                        "byte-list element {n} out of range 0..=255"
                    )));
                }
                other => {
                    return Err(Signal::Runtime(format!(
                        "byte-list element must be an Int 0..=255, got {}",
                        type_name(&other)
                    )));
                }
            }
        }
        Ok(Value::Bytes(out))
    }

    /// A `let`/`for`-var/param (innermost scope wins), a registered
    /// constant, or a bare reference to a builtin/proc name (which the
    /// parser accepts as "bound" but which has no value in this language —
    /// only `name(...)` is meaningful, so that's a runtime error here, not
    /// a panic).
    fn lookup(&self, name: &str) -> EResult<Value> {
        if name == "argv" {
            // `argv` is a list of `Str`, and `Value` has no list variant —
            // only `argv[n]` (handled in `eval_index`) produces a `Value`.
            return Err(Signal::Runtime(
                "'argv' must be indexed (argv[n]) — it has no value on its own".to_string(),
            ));
        }
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Ok(v.clone());
            }
        }
        if let Some(v) = self.interp.consts.get(name) {
            return Ok(v.clone());
        }
        if self.procs.contains_key(name)
            || crate::parser::INTRINSIC_CALLS.contains(&name)
            || self.interp.builtins.contains_key(name)
        {
            return Err(Signal::Runtime(format!(
                "'{name}' is a proc or builtin — it can only be called (as '{name}(...)'), not used as a value"
            )));
        }
        Err(Signal::Runtime(format!("unbound identifier '{name}'")))
    }

    /// `base[index]`, 1-based. `argv` is special-cased here (see `lookup`)
    /// since it isn't a `Value`; everything else evaluates `base` normally
    /// — today that's only `Bytes`, which indexes to an `Int`.
    fn eval_index(&mut self, base: &Expr, idx: &Expr) -> EResult<Value> {
        if let Expr::Ident(name) = base {
            if name == "argv" {
                let i = self.eval_index_value(idx)?;
                let pos = check_index(i, self.interp.argv.len(), "argv")?;
                return Ok(Value::Str(self.interp.argv[pos].clone()));
            }
        }
        let bv = self.eval_expr(base)?;
        let i = self.eval_index_value(idx)?;
        match bv {
            Value::Bytes(bytes) => {
                let pos = check_index(i, bytes.len(), "byte-list")?;
                Ok(Value::Int(i64::from(bytes[pos])))
            }
            other => Err(Signal::Runtime(format!(
                "cannot index a {} value",
                type_name(&other)
            ))),
        }
    }

    fn eval_index_value(&mut self, idx: &Expr) -> EResult<i64> {
        match self.eval_expr(idx)? {
            Value::Int(n) => Ok(n),
            other => Err(Signal::Runtime(format!(
                "index must be an Int, got {}",
                type_name(&other)
            ))),
        }
    }

    fn eval_binary(&mut self, op: &BinOp, l: &Expr, r: &Expr) -> EResult<Value> {
        // `and`/`or` short-circuit, so the right side is only ever
        // evaluated (and only ever needs to be a Bool) once it matters.
        match op {
            BinOp::And => {
                let lb = self.eval_bool(l, "'and' left operand")?;
                if !lb {
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(self.eval_bool(r, "'and' right operand")?))
            }
            BinOp::Or => {
                let lb = self.eval_bool(l, "'or' left operand")?;
                if lb {
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(self.eval_bool(r, "'or' right operand")?))
            }
            _ => {
                let lv = self.eval_expr(l)?;
                let rv = self.eval_expr(r)?;
                apply_binop(op, lv, rv)
            }
        }
    }
}

fn check_index(i: i64, len: usize, what: &str) -> EResult<usize> {
    if i < 1 || i as u64 > len as u64 {
        return Err(Signal::Runtime(format!(
            "{what} index {i} out of range (length {len}, 1-based)"
        )));
    }
    Ok((i - 1) as usize)
}

fn apply_binop(op: &BinOp, lv: Value, rv: Value) -> EResult<Value> {
    use BinOp::{Add, Concat, Div, Eq, Ge, Gt, Le, Lt, Mod, Mul, Ne, Sub};
    match op {
        Eq => Ok(Value::Bool(values_eq(&lv, &rv)?)),
        Ne => Ok(Value::Bool(!values_eq(&lv, &rv)?)),
        Lt | Le | Gt | Ge => {
            let (a, b) = int_pair(op_symbol(op), lv, rv)?;
            Ok(Value::Bool(match op {
                Lt => a < b,
                Le => a <= b,
                Gt => a > b,
                Ge => a >= b,
                _ => unreachable!("guarded by the outer match"),
            }))
        }
        Concat => concat_values(lv, rv),
        Add | Sub | Mul | Div | Mod => {
            let (a, b) = int_pair(op_symbol(op), lv, rv)?;
            arith(op, a, b)
        }
        BinOp::And | BinOp::Or => unreachable!("short-circuited in eval_binary before this call"),
    }
}

/// `==`/`!=`: same-type value comparison for every `Value` variant.
/// Comparing two different types is always a runtime error, never `false` —
/// the plan calls this out explicitly as "always a script bug".
fn values_eq(lv: &Value, rv: &Value) -> EResult<bool> {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Ok(a == b),
        (Value::Str(a), Value::Str(b)) => Ok(a == b),
        (Value::Bool(a), Value::Bool(b)) => Ok(a == b),
        (Value::Bytes(a), Value::Bytes(b)) => Ok(a == b),
        (Value::Nil, Value::Nil) => Ok(true),
        (a, b) => Err(Signal::Runtime(format!(
            "cannot compare {} with {} ('==' requires matching types)",
            type_name(a),
            type_name(b)
        ))),
    }
}

fn int_pair(op_sym: &str, lv: Value, rv: Value) -> EResult<(i64, i64)> {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Ok((a, b)),
        (a, b) => Err(Signal::Runtime(format!(
            "'{op_sym}' requires two Ints, got {} and {}",
            type_name(&a),
            type_name(&b)
        ))),
    }
}

/// `..`: `Str`/`Int` operands render to a `Str` (`Int` in decimal); `Bytes`
/// names `hex()` as the fix, `Bool`/`Nil` are simply not concatenable.
fn concat_values(lv: Value, rv: Value) -> EResult<Value> {
    Ok(Value::Str(concat_operand(lv)? + &concat_operand(rv)?))
}

fn concat_operand(v: Value) -> EResult<String> {
    match v {
        Value::Str(s) => Ok(s),
        Value::Int(n) => Ok(n.to_string()),
        Value::Bytes(_) => Err(Signal::Runtime(
            "cannot '..' a Bytes value directly — format it with hex() first".to_string(),
        )),
        Value::Bool(b) => Err(Signal::Runtime(format!("cannot '..' a Bool value ({b})"))),
        Value::Nil => Err(Signal::Runtime("cannot '..' a Nil value".to_string())),
    }
}

fn arith(op: &BinOp, a: i64, b: i64) -> EResult<Value> {
    let r = match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        BinOp::Mul => a.checked_mul(b),
        BinOp::Div if b == 0 => return Err(Signal::Runtime("division by zero".to_string())),
        BinOp::Div => a.checked_div(b),
        BinOp::Mod if b == 0 => return Err(Signal::Runtime("modulo by zero".to_string())),
        BinOp::Mod => a.checked_rem(b),
        _ => unreachable!("guarded by apply_binop's match"),
    };
    r.map(Value::Int).ok_or_else(|| {
        Signal::Runtime(format!(
            "integer overflow evaluating {a} {} {b}",
            op_symbol(op)
        ))
    })
}

fn op_symbol(op: &BinOp) -> &'static str {
    match op {
        BinOp::Or => "or",
        BinOp::And => "and",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Concat => "..",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Mod => "%",
        BinOp::Div => "/",
    }
}
