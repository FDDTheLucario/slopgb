// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `gb:press` / `gb:release` / `gb:tap` / `gb:wait_frames` /
//! `gb:wait_cycles`.

use std::cell::RefCell;
use std::rc::Rc;

use slopgb_core::{Button, CYCLES_PER_FRAME};
use slopscript::{Interp, Value};

use super::{Machine, consts};

pub(super) fn register(interp: &mut Interp<'_>, mc: &Rc<RefCell<Machine>>) {
    let m = Rc::clone(mc);
    interp.register_builtin("gb:press", move |args| press(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:release", move |args| release(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:tap", move |args| tap(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:wait_frames", move |args| wait_frames(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:wait_cycles", move |args| wait_cycles(&m, args));
}

fn expect_button(args: &[Value]) -> Result<Button, String> {
    match args {
        [b] => consts::decode_button(b),
        _ => Err(format!("expects (BTN), got {} argument(s)", args.len())),
    }
}

fn expect_nonneg(args: &[Value]) -> Result<i64, String> {
    match args {
        [Value::Int(n)] if *n >= 0 => Ok(*n),
        [Value::Int(n)] => Err(format!("expects a non-negative count, got {n}")),
        _ => Err("expects (n)".to_string()),
    }
}

fn press(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let b = expect_button(args)?;
    mc.borrow_mut().session.gb.press(b);
    Ok(Value::Nil)
}

fn release(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let b = expect_button(args)?;
    mc.borrow_mut().session.gb.release(b);
    Ok(Value::Nil)
}

/// Press, run `n` frames, release. The script supplies its own release gap
/// afterwards (`docs/headless-plan.md`'s worked example does).
fn tap(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let (b, n) = match args {
        [btn, Value::Int(n)] if *n >= 0 => (consts::decode_button(btn)?, *n),
        [_, Value::Int(n)] => return Err(format!("frame count must be non-negative, got {n}")),
        _ => return Err("expects (BTN, n)".to_string()),
    };
    let mut m = mc.borrow_mut();
    m.session.gb.press(b);
    for _ in 0..n {
        m.session.gb.run_frame();
    }
    m.session.gb.release(b);
    Ok(Value::Nil)
}

fn wait_frames(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let n = expect_nonneg(args)?;
    let mut m = mc.borrow_mut();
    for _ in 0..n {
        m.session.gb.run_frame();
    }
    Ok(Value::Nil)
}

/// Run whole-frame slices (or less, for the final one) until at least `n`
/// cycles have elapsed. `GameBoy::run_slice` always advances at least one
/// CPU step (>= 4 cycles), so this terminates but may overshoot `n` by a
/// few cycles. Consequence worth knowing (per the plan): a `gb:wait_frames`
/// right after this starts mid-frame, so frame boundaries are no longer
/// multiples of `CYCLES_PER_FRAME` once the two are mixed — fully
/// deterministic either way.
fn wait_cycles(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let n = expect_nonneg(args)?;
    let mut m = mc.borrow_mut();
    let target = m.session.gb.cycles().saturating_add(n as u64);
    while m.session.gb.cycles() < target {
        let remaining = target - m.session.gb.cycles();
        let slice = remaining.min(u64::from(CYCLES_PER_FRAME)) as u32;
        m.session.gb.run_slice(slice);
    }
    Ok(Value::Nil)
}
