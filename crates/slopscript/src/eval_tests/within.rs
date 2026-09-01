// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `within N { } else { }`: the frame budget, its clamp under nesting and
//! across a proc call, and expiry breaking out and continuing rather than
//! failing the run.

use super::helpers::{clock_with_tick_builtin, fake_clock};
use crate::Outcome;

#[test]
fn within_that_never_expires_runs_body_and_skips_else() {
    // Default clock never advances (`Interp::new()`), so a generous budget
    // never trips: `else`'s check(false, ...) must never run.
    let mut interp = crate::Interp::new();
    let src = "
within 100 {
  check(true, \"body ran\")
} else {
  check(false, \"else ran\")
}
";
    match interp.run(src) {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn expiry_breaks_out_runs_else_and_continues_with_the_next_statement() {
    let (mut interp, _clock) = clock_with_tick_builtin();
    let src = "
within 5 {
  tick(10)
  check(false, \"never runs, tick already spent the budget\")
} else {
  check(true, \"handler ran\")
}
check(true, \"continued after the within\")
";
    match interp.run(src) {
        Outcome::Ok { checks: 2 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn expiry_with_no_else_just_continues() {
    let (mut interp, _clock) = clock_with_tick_builtin();
    let src = "
within 5 {
  tick(10)
}
check(true, \"continued\")
";
    match interp.run(src) {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn wait_inside_repeat_trips_the_bound() {
    let (mut interp, clock) = clock_with_tick_builtin();
    let src = "
within 5 {
  repeat { tick(1) } until false
} else {
  check(true, \"bounded\")
}
";
    match interp.run(src) {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
    // The repeat must have stopped at (or just past) the budget, not run
    // away — proves the per-iteration check actually fired.
    assert!(
        clock.get() <= 6,
        "clock ran to {}, budget was 5",
        clock.get()
    );
}

#[test]
fn nested_within_clamps_to_the_tighter_outer_bound() {
    let (mut interp, clock) = clock_with_tick_builtin();
    let src = "
within 10 {
  within 1000000 {
    while true { tick(1) }
  }
  check(false, \"should not be reached — the outer bound is already spent too\")
} else {
  check(true, \"outer caught the clamped expiry\")
}
";
    match interp.run(src) {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
    // This is the property most likely to be got wrong: the inner `within`
    // asked for a million-tick budget, but the outer's 10-tick bound must
    // still be what actually stops the loop.
    assert!(
        clock.get() <= 11,
        "inner within was not clamped: clock ran to {}",
        clock.get()
    );
}

#[test]
fn a_within_inside_a_proc_cannot_defeat_its_caller_bound() {
    let (mut interp, clock) = clock_with_tick_builtin();
    let src = "
proc runs_forever() {
  within 1000000 {
    while true { tick(1) }
  }
}
within 10 {
  runs_forever()
} else {
  check(true, \"caller's bound still won\")
}
";
    match interp.run(src) {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
    assert!(
        clock.get() <= 11,
        "the proc's own within defeated its caller's bound: clock ran to {}",
        clock.get()
    );
}

#[test]
fn within_bound_must_be_a_non_negative_int() {
    let (mut interp, _clock) = fake_clock();
    match interp.run("within -1 { }") {
        Outcome::Error(e) => assert!(e.contains("non-negative"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_body_that_completes_on_its_last_budgeted_cycle_has_not_expired() {
    // `within 1` at ticks_per_unit 1 budgets exactly one tick. The body
    // spends it and *finishes* — nothing was cut short, so this is not an
    // expiry: `else` must not run and no expiry line is printed. Landing
    // exactly on the deadline is the boundary a real script hits whenever
    // its last `wait_frames` consumes the whole bound.
    let (mut interp, _clock) = clock_with_tick_builtin();
    let src = "
within 1 {
  repeat { tick(1) } until true
} else {
  check(false, \"spurious expiry\")
}
";
    match interp.run(src) {
        Outcome::Ok { checks: 0 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_loop_that_advances_no_cycles_is_a_script_error_not_a_hang() {
    // A `within` deadline is an *emulated-cycle* deadline, so a loop whose
    // body never advances the clock can never reach it. Without the spin
    // guard this is an infinite loop — the one way a script that parsed
    // clean can hang CI. It reports as a script error (exit 2), not as a
    // check failure: the script is wrong, the game did nothing.
    let (mut interp, _clock) = clock_with_tick_builtin();
    let src = "
let n = 0
within 10 {
  while true { n = n + 1 }
}
";
    match interp.run(src) {
        Outcome::Error(e) => {
            assert!(e.contains("without advancing"), "{e}");
            assert!(e.contains("line 3"), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_long_loop_that_does_advance_cycles_is_not_a_spin() {
    // The guard counts *consecutive* non-advancing back-edges, so a loop
    // that waits on the machine resets it every iteration and can run far
    // past the cap. Budget 40 ticks, one per iteration: 40 back-edges that
    // each advance, then expiry ends it normally.
    let (mut interp, _clock) = clock_with_tick_builtin();
    let src = "
within 40 {
  while true { tick(1) }
}
check(true, \"survived a long advancing loop\")
";
    match interp.run(src) {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
}
