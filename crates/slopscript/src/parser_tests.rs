// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

use super::*;
use crate::lexer::lex;
use std::collections::HashSet;

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn parse_src(src: &str, consts: &[&str], builtins: &[&str]) -> Result<Program, String> {
    let toks = lex(src).unwrap();
    parse(&toks, &set(consts), &set(builtins))
}

// Rule 1: unbound identifier is a parse error.
#[test]
fn unbound_identifier_is_a_parse_error() {
    let err = parse_src("let x = y", &[], &[]).unwrap_err();
    assert!(err.contains("line 1"), "{err}");
    assert!(err.contains("unbound identifier"), "{err}");
    assert!(err.contains("'y'"), "{err}");
}

#[test]
fn bound_identifiers_are_accepted() {
    // a let, argv, and a registered const are all in scope.
    parse_src(
        "let x = 1\nlet y = x\nlet z = argv[1]\nlet w = CGB",
        &["CGB"],
        &[],
    )
    .unwrap();
}

// Rule 2: while/repeat need a lexically enclosing within.
#[test]
fn while_outside_within_is_a_parse_error() {
    let err = parse_src("while true { }", &[], &[]).unwrap_err();
    assert!(err.contains("line 1"), "{err}");
    assert!(err.contains("within"), "{err}");
}

#[test]
fn repeat_outside_within_is_a_parse_error() {
    let err = parse_src("repeat { } until true", &[], &[]).unwrap_err();
    assert!(err.contains("line 1"), "{err}");
    assert!(err.contains("within"), "{err}");
}

#[test]
fn repeat_until_inside_within_parses() {
    parse_src("within 10 { repeat { } until true }", &[], &[]).unwrap();
}

#[test]
fn while_in_proc_needs_its_own_within() {
    // the outer within does not reach across the proc-body scope barrier.
    let err = parse_src("within 10 { }\nproc p() { while true { } }", &[], &[]).unwrap_err();
    assert!(err.contains("within"), "{err}");
}

#[test]
fn while_in_proc_with_its_own_within_parses() {
    parse_src("proc p() { within 10 { while true { } } }", &[], &[]).unwrap();
}

#[test]
fn for_loop_needs_no_within() {
    parse_src("for i = 1, 10 { }", &[], &[]).unwrap();
}

#[test]
fn nested_within_parses() {
    parse_src("within 100 { within 10 { } }", &[], &[]).unwrap();
}

// Rule 3: a statement may not begin with '(' or '['.
#[test]
fn statement_cannot_begin_with_paren() {
    let err = parse_src("let x = 1\n(x)", &[], &[]).unwrap_err();
    assert!(err.contains("line 2"), "{err}");
    assert!(err.contains('('), "{err}");
}

#[test]
fn statement_cannot_begin_with_bracket() {
    let err = parse_src("let x = 1\n[x]", &[], &[]).unwrap_err();
    assert!(err.contains("line 2"), "{err}");
    assert!(err.contains('['), "{err}");
}

// Rule 4: an unknown call target is a parse error.
#[test]
fn unknown_bare_call_is_a_parse_error() {
    let err = parse_src("foo(1)", &[], &[]).unwrap_err();
    assert!(err.contains("line 1"), "{err}");
    assert!(err.contains("'foo'"), "{err}");
}

#[test]
fn unknown_namespaced_call_is_a_parse_error() {
    let err = parse_src("gb:reed(1)", &[], &["gb:read"]).unwrap_err();
    assert!(err.contains("line 1"), "{err}");
    assert!(err.contains("gb:reed"), "{err}");
}

#[test]
fn namespaced_call_parses() {
    parse_src("gb:tap(BTN_A, 3)", &["BTN_A"], &["gb:tap"]).unwrap();
}

#[test]
fn proc_call_may_precede_its_definition() {
    parse_src("p()\nproc p() { }", &[], &[]).unwrap();
}

// Rule 5: a proc is a statement, never an expression.
#[test]
fn proc_call_as_expression_is_a_parse_error() {
    let err = parse_src("proc p() { }\nlet x = p()", &[], &[]).unwrap_err();
    assert!(err.contains("line 2"), "{err}");
    assert!(err.contains("'p'"), "{err}");
    assert!(err.contains("statements only"), "{err}");
}

#[test]
fn proc_call_as_statement_still_parses() {
    parse_src("proc p() { }\np()", &[], &[]).unwrap();
}

#[test]
fn intrinsic_runner_calls_need_no_registration() {
    // print / check / assert / hex ship with the language (headless-plan.md,
    // "gb: / global split" table: "runner-side"), not via register_builtin.
    parse_src(r#"print("x")"#, &[], &[]).unwrap();
    parse_src("check(true, \"ok\")", &[], &[]).unwrap();
    parse_src("assert(true, \"ok\")", &[], &[]).unwrap();
    parse_src("let h = hex([1, 2])", &[], &[]).unwrap();
}

#[test]
fn else_if_chains_parse() {
    let prog = parse_src("if true { } else if false { } else { }", &[], &[]).unwrap();
    match &prog.items[0] {
        Item::Stmt(Stmt::If { arms, else_body }) => {
            assert_eq!(arms.len(), 2);
            assert!(else_body.is_some());
        }
        other => panic!("expected an if statement, got {other:?}"),
    }
}

#[test]
fn byte_list_literal_can_be_indexed_directly() {
    // docs/headless-plan.md, "Indexing": "[10,20,30][2] is Int(20)".
    let prog = parse_src("let x = [10, 20, 30][2]", &[], &[]).unwrap();
    match &prog.items[0] {
        Item::Stmt(Stmt::Let(_, Expr::Index(base, _))) => {
            assert!(matches!(**base, Expr::ByteList(_)));
        }
        other => panic!("expected a Let of an Index over a ByteList, got {other:?}"),
    }
}

#[test]
fn empty_byte_list_parses() {
    let prog = parse_src("let x = []", &[], &[]).unwrap();
    match &prog.items[0] {
        Item::Stmt(Stmt::Let(_, Expr::ByteList(items))) => assert!(items.is_empty()),
        other => panic!("expected an empty byte-list let, got {other:?}"),
    }
}

#[test]
fn whitespace_is_insignificant() {
    let allman = "within 10\n{\n  let x = 1\n  if x == 1\n  {\n    print(x)\n  }\n}\n";
    let kr = "within 10 {\n  let x = 1\n  if x == 1 {\n    print(x)\n  }\n}\n";
    let one_line = "within 10 { let x = 1 if x == 1 { print(x) } }";
    let a = parse_src(allman, &[], &[]).unwrap();
    let b = parse_src(kr, &[], &[]).unwrap();
    let c = parse_src(one_line, &[], &[]).unwrap();
    assert_eq!(a, b);
    assert_eq!(b, c);
}

// The plan's entire Worked example (docs/headless-plan.md, "## Worked
// example"), copied verbatim, once the frontend registers the `gb:`
// builtins and constants it names.
#[test]
fn worked_example_from_the_plan_parses() {
    const SRC: &str = r#"-- Usage: slopgb --headless case.slp <rom.gbc> <in.sav> <out.sav>

gb:load_rom(argv[1], CGB)
gb:load_battery(argv[2])
gb:set_rtc(1767225600)   -- pin the clock: a host-clock-dependent run is flaky by design
gb:load_symbols("game.sym")

-- Drive the title screen until the game reports it has processed the save.
within 6000 {
  repeat {
    gb:tap(BTN_START, 3); gb:wait_frames(17)
    gb:tap(BTN_A, 3);     gb:wait_frames(17)
  } until gb:read("sSaveVersion") == 2
} else {
  check(false, "save was never processed")
}

-- Through the confirmation prompt, then let the map load.
for i = 1, 8 {
  gb:tap(BTN_A, 3); gb:wait_frames(37)
}
gb:wait_frames(600)

let table = gb:read("wResultTable", 4)
print("table=" .. hex(table) .. " state=" .. gb:read("wGameState"))

check(table == [27, 2, 10, 4], "result table")
check(gb:read("wGameState") == 0, "game state after load")

gb:save_battery(argv[3])
"#;

    let mut interp = crate::Interp::new();
    for name in [
        "gb:load_rom",
        "gb:load_battery",
        "gb:set_rtc",
        "gb:load_symbols",
        "gb:tap",
        "gb:wait_frames",
        "gb:read",
        "gb:save_battery",
    ] {
        interp.register_builtin(name, |_args| Ok(crate::Value::Nil));
    }
    interp.register_const("CGB", crate::Value::Int(1));
    interp.register_const("BTN_START", crate::Value::Int(1));
    interp.register_const("BTN_A", crate::Value::Int(1));

    interp.parse_only(SRC).unwrap();
}
