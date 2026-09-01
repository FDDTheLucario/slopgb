// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Proc calls: params, the scope barrier, arity, and the recursion cap.

use crate::{Interp, Outcome};

#[test]
fn proc_runs_with_its_arguments() {
    let src = "
proc add_check(a, b, expected) {
  check(a + b == expected, \"sum\")
}
add_check(2, 3, 5)
add_check(10, -1, 9)
";
    match Interp::new().run(src) {
        Outcome::Ok { checks: 2 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn proc_body_cannot_see_a_top_level_let() {
    // The scope barrier is enforced at parse time (no closures) — `run`
    // surfaces it as Outcome::Error, same as any other parse error.
    let src = "
let secret = 1
proc p() { check(secret == 1, \"leaked\") }
p()
";
    match Interp::new().run(src) {
        Outcome::Error(e) => assert!(e.contains("unbound identifier 'secret'"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn proc_call_with_wrong_arity_is_a_runtime_error() {
    let src = "
proc p(a, b) { }
p(1)
";
    match Interp::new().run(src) {
        Outcome::Error(e) => {
            assert!(e.contains("p"), "{e}");
            assert!(e.contains('2'), "{e}");
            assert!(e.contains('1'), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn recursion_is_allowed_but_capped() {
    let src = "
proc down(n) {
  check(n >= 0, \"n\")
  down(n - 1)
}
down(1000000)
";
    match Interp::new().run(src) {
        Outcome::Error(e) => assert!(e.contains("depth"), "{e}"),
        other => panic!("expected a call-depth error, got {other:?}"),
    }
}

#[test]
fn finite_recursion_runs_to_completion() {
    let src = "
proc countdown(n) {
  check(n >= 0, \"n\")
  if n > 0 { countdown(n - 1) }
}
countdown(20)
";
    match Interp::new().run(src) {
        Outcome::Ok { checks: 21 } => {}
        other => panic!("{other:?}"),
    }
}
