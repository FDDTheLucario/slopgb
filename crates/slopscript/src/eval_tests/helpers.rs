// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Shared test scaffolding: an `Interp` wired to a fake clock a test can
//! drive by hand, for deterministic `within` expiry (`docs/headless-plan.md`
//! says exactly this — `set_clock` is the hook for it).

use crate::Interp;
use std::cell::Cell;
use std::rc::Rc;

/// One shared tick counter, plugged into `Interp::set_clock` as the reader
/// and returned so a test can also drive it directly (`clock.set(...)`)
/// without going through a script-side `tick()` builtin.
pub(super) fn fake_clock() -> (Interp<'static>, Rc<Cell<u64>>) {
    let clock = Rc::new(Cell::new(0u64));
    let mut interp = Interp::new();
    let read = clock.clone();
    interp.set_clock(1, move || read.get());
    (interp, clock)
}

/// `fake_clock` plus a registered `tick(n)` builtin that advances the same
/// counter, so a script can consume its own budget (the `wait_cycles`
/// idiom `docs/headless-plan.md` describes).
pub(super) fn clock_with_tick_builtin() -> (Interp<'static>, Rc<Cell<u64>>) {
    let (mut interp, clock) = fake_clock();
    let advance = clock.clone();
    interp.register_builtin("tick", move |args| match args {
        [crate::Value::Int(n)] if *n >= 0 => {
            advance.set(advance.get() + *n as u64);
            Ok(crate::Value::Nil)
        }
        [crate::Value::Int(_)] => Err("tick(n) expects a non-negative n".to_string()),
        _ => Err("tick(n) expects one Int argument".to_string()),
    });
    (interp, clock)
}
