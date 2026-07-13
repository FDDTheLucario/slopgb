use super::*;
use slopfp::Entry;
use std::path::PathBuf;
use winit::keyboard::ModifiersState;

/// A `FallbackPicker` over a pure in-memory listing (no fs), rooted at `/x`,
/// for the pure hit-test math below — `open()` always reads a real directory,
/// which these tests don't need.
fn picker_with(entries: Vec<Entry>) -> FallbackPicker {
    // `last_rowcount` defaults to the full entry count, as if `render` had just
    // drawn every one of them (the common case these hit-test fixtures want);
    // tests of the "undrawn sliver" guard override it explicitly.
    let last_rowcount = entries.len();
    FallbackPicker {
        picker: Picker::with_entries(Mode::Open, "/x", entries),
        purpose: PathPurpose::LoadRom,
        last_offset: 0,
        list_rect: Rect::new(0, 0, 0, 0),
        last_rowcount,
    }
}

// ---- winit_key_to_picker mapping table (task 20) ------------------------

#[test]
fn winit_key_to_picker_maps_named_keys() {
    let none = ModifiersState::empty();
    assert_eq!(
        winit_key_to_picker(KeyCode::ArrowUp, None, none),
        Some(Key::Up)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::ArrowDown, None, none),
        Some(Key::Down)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::PageUp, None, none),
        Some(Key::PageUp)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::PageDown, None, none),
        Some(Key::PageDown)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::Home, None, none),
        Some(Key::Home)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::End, None, none),
        Some(Key::End)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::Enter, None, none),
        Some(Key::Enter)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::NumpadEnter, None, none),
        Some(Key::Enter)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::Backspace, None, none),
        Some(Key::Backspace)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::Escape, None, none),
        Some(Key::Cancel)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::Tab, None, none),
        Some(Key::Tab)
    );
}

#[test]
fn winit_key_to_picker_printable_char_from_text() {
    let none = ModifiersState::empty();
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyA, Some("a"), none),
        Some(Key::Char('a'))
    );
    // A control character in `text` never maps (mirrors `dialog_key_from`).
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyA, Some("\u{7}"), none),
        None
    );
    // No named key and no text -> nothing to send.
    assert_eq!(winit_key_to_picker(KeyCode::KeyA, None, none), None);
}

#[test]
fn ctrl_h_toggles_hidden_else_falls_through_to_char() {
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyH, Some("h"), ModifiersState::CONTROL),
        Some(Key::ToggleHidden)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyH, Some("h"), ModifiersState::empty()),
        Some(Key::Char('h'))
    );
}

#[test]
fn ctrl_hotkeys_reach_the_path_bar_and_sort_toggles() {
    // These have no other affordance with no native dialog open (no menu bar
    // in the fallback picker), so they must map through; each falls back to
    // its plain `Char` with no modifier, same shape as Ctrl+H above.
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyL, Some("l"), ModifiersState::CONTROL),
        Some(Key::FocusPath)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyL, Some("l"), ModifiersState::empty()),
        Some(Key::Char('l'))
    );

    assert_eq!(
        winit_key_to_picker(KeyCode::KeyK, Some("k"), ModifiersState::CONTROL),
        Some(Key::CycleSort)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyK, Some("k"), ModifiersState::empty()),
        Some(Key::Char('k'))
    );

    assert_eq!(
        winit_key_to_picker(KeyCode::KeyR, Some("r"), ModifiersState::CONTROL),
        Some(Key::ToggleSortDir)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyR, Some("r"), ModifiersState::empty()),
        Some(Key::Char('r'))
    );

    assert_eq!(
        winit_key_to_picker(KeyCode::KeyA, Some("a"), ModifiersState::CONTROL),
        Some(Key::ToggleAllFiles)
    );
    assert_eq!(
        winit_key_to_picker(KeyCode::KeyA, Some("a"), ModifiersState::empty()),
        Some(Key::Char('a'))
    );
}

// ---- hit-test row math (task 22) -----------------------------------------

#[test]
fn hit_test_maps_click_to_absolute_row_index() {
    let entries = vec![
        Entry::new("a.txt", false, Some(1), Some(1)),
        Entry::new("b.txt", false, Some(1), Some(1)),
        Entry::new("c.txt", false, Some(1), Some(1)),
    ];
    let mut fp = picker_with(entries);
    fp.list_rect = Rect::new(10, 20, 200, 100);
    fp.last_offset = 1; // the view was scrolled down by one row

    // A click landing on row-in-view 1 -> abs index = last_offset + 1 = 2 ("c.txt").
    let py = fp.list_rect.y + line_height() + 1;
    assert_eq!(
        fp.on_click(10, py, true),
        Outcome::Picked(PathBuf::from("/x/c.txt"))
    );
}

#[test]
fn hit_test_outside_list_rect_is_none() {
    let mut fp = picker_with(vec![Entry::new("a.txt", false, Some(1), Some(1))]);
    fp.list_rect = Rect::new(10, 20, 200, 100);
    assert_eq!(
        fp.on_click(0, 0, true),
        Outcome::None,
        "above/left of the rect"
    );
    assert_eq!(fp.on_click(10, 200, true), Outcome::None, "below the rect");
}

#[test]
fn hit_test_below_last_drawn_row_is_none() {
    // Only 2 entries were ever drawn (`last_rowcount = 2`), but the list rect
    // (as a real `render` would leave it) is taller than exactly 2 lines —
    // e.g. `list_rect.h` isn't a multiple of `line_height()`. A click landing
    // in that sub-row sliver, one row-in-view past the last real one, must not
    // resolve to a row.
    let entries = vec![
        Entry::new("a.txt", false, Some(1), Some(1)),
        Entry::new("b.txt", false, Some(1), Some(1)),
    ];
    let mut fp = picker_with(entries);
    fp.list_rect = Rect::new(10, 20, 200, 100);
    fp.last_offset = 0;
    fp.last_rowcount = 2;

    let py = fp.list_rect.y + line_height() * 2 + 1; // row-in-view 2: past the drawn rows
    assert_eq!(fp.on_click(10, py, true), Outcome::None);
    assert_eq!(fp.on_click(10, py, false), Outcome::None);
}

#[test]
fn single_click_selects_without_picking_then_enter_picks_it() {
    let entries = vec![
        Entry::new("a.txt", false, Some(1), Some(1)),
        Entry::new("b.txt", false, Some(1), Some(1)),
    ];
    let mut fp = picker_with(entries);
    fp.list_rect = Rect::new(10, 20, 200, 100);

    let py = fp.list_rect.y + line_height(); // row-in-view 1 -> "b.txt"
    assert_eq!(fp.on_click(10, py, false), Outcome::None);
    assert_eq!(
        fp.feed_key(Key::Enter),
        Outcome::Picked(PathBuf::from("/x/b.txt"))
    );
}
