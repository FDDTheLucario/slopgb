// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Evaluator tests, split by concern: each of these is `#[path]`-included
//! rather than nested inline, per the crate's "no `.rs` over 1000 lines"
//! rule — an evaluator plus its tests doesn't fit one file.

#[path = "eval_tests/checks.rs"]
mod checks;
#[path = "eval_tests/control_flow.rs"]
mod control_flow;
#[path = "eval_tests/helpers.rs"]
mod helpers;
#[path = "eval_tests/indexing.rs"]
mod indexing;
#[path = "eval_tests/procs.rs"]
mod procs;
#[path = "eval_tests/values.rs"]
mod values;
#[path = "eval_tests/within.rs"]
mod within;
#[path = "eval_tests/worked_example.rs"]
mod worked_example;
