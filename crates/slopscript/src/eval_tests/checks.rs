// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `check`/`assert` bookkeeping and the `Outcome` mapping: `checks` counts
//! every `check()` call, `check` accumulates failures and keeps running,
//! `assert` aborts immediately, and a runtime error always wins over an
//! already-failed check.

use crate::{Interp, Outcome};

#[test]
fn zero_checks_is_a_valid_silent_ok() {
    match Interp::new().run("let x = 1") {
        Outcome::Ok { checks: 0 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn check_accumulates_every_failure_and_keeps_running() {
    let src = "
check(false, \"first broken field\")
check(true, \"this one holds\")
check(false, \"second broken field\")
";
    match Interp::new().run(src) {
        Outcome::Failed { checks, failures } => {
            assert_eq!(checks, 3);
            assert_eq!(
                failures,
                vec![
                    "first broken field".to_string(),
                    "second broken field".to_string(),
                ]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn assert_aborts_immediately_statements_after_it_do_not_run() {
    let src = "
assert(false, \"stop here\")
check(false, \"must never run\")
";
    match Interp::new().run(src) {
        Outcome::Failed { checks, failures } => {
            // Only assert's own message is recorded — the check() after it
            // never executed, so `checks` (which only counts check() calls)
            // stays 0 and its failure message never appears.
            assert_eq!(checks, 0);
            assert_eq!(failures, vec!["stop here".to_string()]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn assert_that_holds_does_not_abort() {
    let src = "
assert(true, \"fine\")
check(true, \"reached\")
";
    match Interp::new().run(src) {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_runtime_error_after_a_failed_check_is_still_error_not_failed() {
    let src = "
check(false, \"broken\")
print(1 / 0)
";
    match Interp::new().run(src) {
        Outcome::Error(e) => assert!(e.contains("division by zero"), "{e}"),
        other => panic!("expected Outcome::Error, got {other:?}"),
    }
}

#[test]
fn check_wrong_arity_or_types_is_a_runtime_error() {
    match Interp::new().run("check(true)") {
        Outcome::Error(e) => assert!(e.contains("expects 2 arguments"), "{e}"),
        other => panic!("{other:?}"),
    }
    match Interp::new().run("check(1, \"msg\")") {
        Outcome::Error(e) => assert!(e.contains("condition must be a Bool"), "{e}"),
        other => panic!("{other:?}"),
    }
    match Interp::new().run("check(true, 1)") {
        Outcome::Error(e) => assert!(e.contains("message must be a Str"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn host_builtin_error_is_prefixed_with_the_builtin_name() {
    let mut interp = Interp::new();
    interp.register_builtin("gb:read", |_| Err("unknown symbol 'wFoo'".to_string()));
    match interp.run("print(gb:read(\"wFoo\"))") {
        Outcome::Error(e) => {
            assert!(e.starts_with("gb:read:"), "{e}");
            assert!(e.contains("unknown symbol 'wFoo'"), "{e}");
        }
        other => panic!("{other:?}"),
    }
}
