// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `expr[expr]`: 1-based, `argv` and byte-list indexing, out-of-range and
//! wrong-type-index runtime errors.

use crate::{Interp, Outcome};

fn error_containing(src: &str, argv: &[&str], needle: &str) {
    let mut interp = Interp::new();
    interp.set_argv(argv.iter().map(|s| s.to_string()).collect());
    match interp.run(src) {
        Outcome::Error(e) => assert!(e.contains(needle), "error {e:?} did not contain {needle:?}"),
        other => panic!("expected Outcome::Error containing {needle:?}, got {other:?}"),
    }
}

#[test]
fn byte_list_indexing_is_one_based() {
    let mut interp = Interp::new();
    match interp.run("check([10, 20, 30][2] == 20, \"1-based\")") {
        Outcome::Ok { checks: 1 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn argv_indexing_is_one_based() {
    let mut interp = Interp::new();
    interp.set_argv(vec!["rom.gbc".to_string(), "in.sav".to_string()]);
    match interp
        .run("check(argv[1] == \"rom.gbc\", \"first\")\ncheck(argv[2] == \"in.sav\", \"second\")")
    {
        Outcome::Ok { checks: 2 } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn argv_out_of_range_with_no_set_argv_is_a_runtime_error() {
    match Interp::new().run("print(argv[1])") {
        Outcome::Error(e) => assert!(e.contains("out of range"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn byte_list_out_of_range_names_index_and_length() {
    match Interp::new().run("print([1, 2, 3][4])") {
        Outcome::Error(e) => {
            assert!(e.contains('4'), "{e}");
            assert!(e.contains("length 3"), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn zero_and_negative_index_are_out_of_range() {
    error_containing("print([1, 2, 3][0])", &[], "out of range");
    error_containing("print([1, 2, 3][-1])", &[], "out of range");
}

#[test]
fn non_int_index_is_a_runtime_error() {
    error_containing("print([1, 2, 3][\"x\"])", &[], "index must be an Int");
}

#[test]
fn indexing_a_non_indexable_value_is_a_runtime_error() {
    // Indexing chains only ever attach to an identifier syntactically
    // (`argv[1]`, never `5[1]`), so a non-Bytes base has to arrive through a
    // `let` — the type mismatch is only visible once it runs.
    error_containing("let x = 5\nprint(x[1])", &[], "cannot index a Int value");
}

#[test]
fn bare_argv_without_indexing_is_a_runtime_error_not_a_panic() {
    error_containing("print(argv)", &[], "must be indexed");
}
