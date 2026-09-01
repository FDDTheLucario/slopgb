// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `if`/`else if`/`else`, `for` (inclusive, zero-iteration), and `while`/
//! `repeat…until` (both only legal inside a `within`, per the parser).

use crate::{Interp, Outcome};

fn checks_of(src: &str) -> usize {
    match Interp::new().run(src) {
        Outcome::Ok { checks } => checks,
        other => panic!("expected Outcome::Ok, got {other:?} for: {src}"),
    }
}

#[test]
fn if_else_if_else_picks_the_first_true_arm() {
    let src = "
let x = 2
if x == 1 { check(false, \"first\") }
else if x == 2 { check(true, \"second\") }
else { check(false, \"else\") }
";
    assert_eq!(checks_of(src), 1);
}

#[test]
fn if_with_no_matching_arm_and_no_else_runs_nothing() {
    assert_eq!(checks_of("if false { check(false, \"never\") }"), 0);
}

#[test]
fn if_condition_must_be_bool() {
    match Interp::new().run("if 1 { }") {
        Outcome::Error(e) => assert!(e.contains("Bool"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn for_loop_is_inclusive() {
    // 1..=5 is 5 iterations.
    assert_eq!(
        checks_of("for i = 1, 5 { check(i >= 1 and i <= 5, \"range\") }"),
        5
    );
}

#[test]
fn for_loop_runs_zero_times_when_to_is_less_than_from() {
    assert_eq!(checks_of("for i = 5, 1 { check(false, \"never\") }"), 0);
}

#[test]
fn for_var_is_not_bound_outside_the_body() {
    match Interp::new().run("for i = 1, 1 { }\nprint(i)") {
        Outcome::Error(e) => assert!(e.contains("unbound identifier 'i'"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn while_runs_zero_or_more_times_pre_test() {
    let src = "
within 100 {
  let n = 0
  while n < 3 { n = n + 1; check(true, \"iter\") }
}
";
    assert_eq!(checks_of(src), 3);
}

#[test]
fn while_false_never_runs_body() {
    let src = "within 100 { while false { check(false, \"never\") } }";
    assert_eq!(checks_of(src), 0);
}

#[test]
fn repeat_until_runs_at_least_once_post_test() {
    let src = "
within 100 {
  let n = 0
  repeat { n = n + 1; check(true, \"iter\") } until n >= 3
}
";
    assert_eq!(checks_of(src), 3);
}

#[test]
fn repeat_runs_exactly_once_even_when_until_is_true_immediately() {
    let src = "within 100 { repeat { check(true, \"once\") } until true }";
    assert_eq!(checks_of(src), 1);
}
