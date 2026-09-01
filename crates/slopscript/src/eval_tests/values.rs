// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Arithmetic, comparison, concatenation, equality and `hex()` — every
//! runtime-error case the plan's "Operators" section spells out.

use crate::{Interp, Outcome};

fn ok_checks(src: &str) -> usize {
    match Interp::new().run(src) {
        Outcome::Ok { checks } => checks,
        other => panic!("expected Outcome::Ok, got {other:?} for: {src}"),
    }
}

fn error_containing(src: &str, needle: &str) {
    match Interp::new().run(src) {
        Outcome::Error(e) => assert!(e.contains(needle), "error {e:?} did not contain {needle:?}"),
        other => panic!("expected Outcome::Error containing {needle:?}, got {other:?} for: {src}"),
    }
}

#[test]
fn integer_arithmetic() {
    assert_eq!(ok_checks("check(2 + 3 == 5, \"add\")"), 1);
    assert_eq!(ok_checks("check(2 - 3 == -1, \"sub\")"), 1);
    assert_eq!(ok_checks("check(4 * 3 == 12, \"mul\")"), 1);
    assert_eq!(ok_checks("check(7 / 2 == 3, \"div\")"), 1);
    assert_eq!(ok_checks("check(7 % 2 == 1, \"mod\")"), 1);
    assert_eq!(ok_checks("check(-5 == 0 - 5, \"unary neg\")"), 1);
}

#[test]
fn division_and_modulo_by_zero_are_runtime_errors() {
    error_containing("print(1 / 0)", "division by zero");
    error_containing("print(1 % 0)", "modulo by zero");
}

#[test]
fn integer_overflow_is_a_runtime_error_not_a_panic() {
    error_containing("print(9223372036854775807 + 1)", "overflow");
    error_containing("print(-9223372036854775807 - 2)", "overflow");
}

#[test]
fn comparisons_are_int_only() {
    assert_eq!(ok_checks("check(1 < 2, \"lt\")"), 1);
    assert_eq!(ok_checks("check(2 >= 2, \"ge\")"), 1);
    error_containing("print(1 < \"a\")", "requires two Ints");
}

#[test]
fn concat_renders_str_and_int() {
    match Interp::new().run("print(\"table=\" .. 5 .. \" more\")") {
        Outcome::Ok { checks: 0 } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(ok_checks("check(\"a\" .. 1 == \"a1\", \"concat\")"), 1);
    assert_eq!(ok_checks("check(1 .. \"a\" == \"1a\", \"concat rev\")"), 1);
}

#[test]
fn concat_rejects_bytes_and_bool() {
    error_containing("print([1, 2] .. \"x\")", "hex()");
    error_containing("print(true .. \"x\")", "Bool");
}

#[test]
fn concat_rejects_nil_via_builtin() {
    // `nil` has no literal in the grammar (only a builtin can produce it);
    // exercise the Nil branch through a Nil-returning builtin instead.
    let mut interp = Interp::new();
    interp.register_builtin("nilval", |_| Ok(crate::Value::Nil));
    match interp.run("print(nilval() .. \"x\")") {
        Outcome::Error(e) => assert!(e.contains("Nil"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn byte_list_equality_is_by_value() {
    assert_eq!(
        ok_checks("check([27, 2, 10, 4] == [27, 2, 10, 4], \"eq\")"),
        1
    );
    match Interp::new().run("check([1, 2] == [1, 3], \"neq\")") {
        Outcome::Failed {
            checks: 1,
            failures,
        } => assert_eq!(failures, vec!["neq".to_string()]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn comparing_mismatched_types_is_always_a_runtime_error() {
    error_containing("print(1 == \"1\")", "cannot compare");
    error_containing("print(true == 1)", "cannot compare");
    error_containing("print([1] == \"a\")", "cannot compare");
}

#[test]
fn byte_list_literal_rejects_out_of_range_and_wrong_type_elements() {
    error_containing("print([1, 256])", "out of range");
    error_containing("print([1, \"x\"])", "must be an Int");
}

#[test]
fn hex_formats_bytes_uppercase_space_joined_min_two_digits() {
    match Interp::new().run("print(hex([27, 2, 10, 4]))") {
        Outcome::Ok { .. } => {}
        other => panic!("{other:?}"),
    }
    // The exact string is what matters here — assert it directly via a
    // `check`, since `Outcome` doesn't carry `print`'s stdout.
    assert_eq!(
        ok_checks("check(hex([27, 2, 10, 4]) == \"1B 02 0A 04\", \"hex fmt\")"),
        1
    );
    assert_eq!(ok_checks("check(hex(5) == \"05\", \"hex int pad\")"), 1);
    assert_eq!(ok_checks("check(hex(255) == \"FF\", \"hex int wide\")"), 1);
}

#[test]
fn hex_rejects_non_bytes_non_int() {
    error_containing("print(hex(true))", "hex() requires");
}

#[test]
fn and_or_short_circuit() {
    // A builtin that errors if it's ever called: the run only succeeds
    // (proving the true/false result on its own would otherwise fail the
    // check) if short-circuiting skipped it entirely.
    let mut interp = Interp::new();
    interp.register_builtin("boom", |_| Err("must not be called".to_string()));
    match interp.run("check((false and boom()) == false, \"and short-circuits\")") {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
    let mut interp2 = Interp::new();
    interp2.register_builtin("boom", |_| Err("must not be called".to_string()));
    match interp2.run("check((true or boom()) == true, \"or short-circuits\")") {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn not_requires_bool() {
    assert_eq!(ok_checks("check(not false, \"not\")"), 1);
    error_containing("print(not 1)", "'not' requires a Bool");
}
