//! Search-menu wiring tests (MB3): the Search dropdown is fully live, and the
//! Search-string modal stores the query + signals the scan on accept.

use super::*;
use crate::input::Action;
use crate::ui::canvas::Rect;
use crate::ui::dialog::DialogKey;

const AREA: Rect = Rect::new(0, 0, 760, 560);

#[test]
fn search_menu_rows_are_all_live_commands() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    let st = DebuggerState::default();
    let m = menubar_menu(1, l.menu, &st, 0x0100); // index 1 = Search
    assert_eq!(m.items.len(), 5);
    assert!(
        m.items.iter().all(|it| it.enabled),
        "every Search row is enabled now"
    );
    assert_eq!(m.choices[0], MenuChoice::Command(Action::DbgSearch));
    assert_eq!(m.choices[1], MenuChoice::Command(Action::DbgContinueSearch));
    assert_eq!(m.choices[2], MenuChoice::Command(Action::DbgNextBookmark));
    assert_eq!(m.choices[3], MenuChoice::Command(Action::DbgPrevBookmark));
    assert_eq!(m.choices[4], MenuChoice::Command(Action::DbgGoToPc));
}

#[test]
fn search_string_modal_stores_query_and_signals_the_scan() {
    let mut st = DebuggerState::default();
    open_search(&mut st);
    assert!(matches!(
        st.dialog.as_ref().map(|d| d.kind),
        Some(DialogKind::SearchString)
    ));
    // Type "ld a," then Enter.
    for ch in "ld a,".chars() {
        feed_dialog(&mut st, DialogKey::Char(ch));
    }
    let (consumed, outcome) = feed_dialog(&mut st, DialogKey::Enter);
    assert!(consumed);
    assert_eq!(
        outcome,
        Some(MenuOutcome::Command(Action::DbgContinueSearch))
    );
    assert_eq!(st.search_query, "ld a,");
    assert_eq!(
        st.search_hit, None,
        "a fresh search resets the resume point"
    );
    assert!(st.dialog.is_none(), "accepting closes the modal");
}

#[test]
fn empty_search_string_is_a_no_op() {
    let mut st = DebuggerState::default();
    open_search(&mut st);
    let (_, outcome) = feed_dialog(&mut st, DialogKey::Enter);
    assert_eq!(outcome, None, "an empty query triggers no scan");
}
