//! Evaluate-expression + user-clock wiring tests (RM14): the Debug-menu rows are
//! live, and the eval modal stores the expression + signals the scan on accept.

use super::*;
use crate::input::Action;
use crate::ui::canvas::Rect;
use crate::ui::dialog::DialogKey;

const AREA: Rect = Rect::new(0, 0, 760, 560);

#[test]
fn debug_menu_evaluate_and_clocks_are_live() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let st = DebuggerState::default();
    let m = menubar_menu(3, l.menu, &st, 0x0100); // index 3 = Debug
    // Toggle bp / Evaluate / Set user clocks / Breakpoints / Watchpoints /
    // Load symbols.
    assert_eq!(m.items.len(), 6);
    assert!(
        m.items.iter().all(|it| it.enabled),
        "every Debug row is live"
    );
    assert_eq!(m.choices[1], MenuChoice::Command(Action::DbgEvaluate));
    assert_eq!(m.choices[2], MenuChoice::Command(Action::DbgSetUserClocks));
}

#[test]
fn eval_modal_stores_expression_and_signals_the_run() {
    let mut st = DebuggerState::default();
    open_eval(&mut st);
    assert!(matches!(
        st.dialog.as_ref().map(|d| d.kind),
        Some(DialogKind::EvalExpr)
    ));
    for ch in "bc+1".chars() {
        feed_dialog(&mut st, DialogKey::Char(ch));
    }
    let (consumed, outcome) = feed_dialog(&mut st, DialogKey::Enter);
    assert!(consumed);
    assert_eq!(outcome, Some(MenuOutcome::Command(Action::DbgEvalRun)));
    assert_eq!(st.eval_input, "bc+1");
    assert!(st.dialog.is_none());
}

#[test]
fn eval_result_box_is_display_only() {
    let mut st = DebuggerState::default();
    show_eval_result(&mut st, "bc+1 = 1235 (4661)".to_owned());
    assert!(matches!(
        st.dialog.as_ref().map(|d| d.kind),
        Some(DialogKind::EvalResult)
    ));
    // Any accept/cancel just closes it, with no machine effect.
    let (consumed, outcome) = feed_dialog(&mut st, DialogKey::Enter);
    assert!(consumed);
    assert_eq!(outcome, None);
    assert!(st.dialog.is_none());
}

#[test]
fn empty_eval_expression_is_a_no_op() {
    let mut st = DebuggerState::default();
    open_eval(&mut st);
    let (_, outcome) = feed_dialog(&mut st, DialogKey::Enter);
    assert_eq!(outcome, None);
}
