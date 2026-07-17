//! Pure model tests — no filesystem access anywhere in this file. Every
//! `Picker` here is built with `with_entries`; `navigate_to`'s real-disk path
//! (via `crate::source::read_dir`) is exercised separately by the `nav.rs`
//! integration test against a temp dir, not here.
use super::*;

fn file(name: &str, size: u64, mtime: u64) -> Entry {
    Entry::new(name, false, Some(size), Some(mtime))
}

fn dir(name: &str) -> Entry {
    Entry::new(name, true, None, None)
}

fn names(v: &[&Entry]) -> Vec<String> {
    v.iter().map(|e| e.name.clone()).collect()
}

// ---- visible(): filter + sort ---------------------------------------------

#[test]
fn sort_dirs_before_files_then_name() {
    let p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![
            file("banana.txt", 1, 1),
            dir("Zed"),
            file("Apple.txt", 1, 1),
        ],
    );
    assert_eq!(names(&p.visible()), vec!["Zed", "Apple.txt", "banana.txt"]);
}

#[test]
fn sort_by_size_desc_keeps_dirs_first() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("small", 10, 1), dir("D1"), file("big", 1000, 1)],
    );
    p.sort_key = SortKey::Size;
    p.sort_dir = SortDir::Desc;
    assert_eq!(names(&p.visible()), vec!["D1", "big", "small"]);
}

#[test]
fn sort_key_name_orders_case_insensitively() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![
            file("banana.txt", 1, 1),
            file("apple.txt", 1, 1),
            file("Cherry.txt", 1, 1),
        ],
    );
    p.sort_key = SortKey::Name;
    assert_eq!(
        names(&p.visible()),
        vec!["apple.txt", "banana.txt", "Cherry.txt"]
    );
}

#[test]
fn sort_key_size_orders_ascending() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("b", 300, 1), file("a", 100, 1), file("c", 200, 1)],
    );
    p.sort_key = SortKey::Size;
    assert_eq!(names(&p.visible()), vec!["a", "c", "b"]);
}

#[test]
fn sort_key_mtime_orders_ascending() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("b", 1, 300), file("a", 1, 100), file("c", 1, 200)],
    );
    p.sort_key = SortKey::Mtime;
    assert_eq!(names(&p.visible()), vec!["a", "c", "b"]);
}

#[test]
fn sort_key_kind_orders_by_extension_then_name() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("b.txt", 1, 1), file("a.md", 1, 1), file("c.txt", 1, 1)],
    );
    p.sort_key = SortKey::Kind;
    assert_eq!(names(&p.visible()), vec!["a.md", "b.txt", "c.txt"]);
}

#[test]
fn filter_hides_nonmatching_files_keeps_dirs() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![dir("Games"), file("rom.gb", 1, 1), file("readme.txt", 1, 1)],
    );
    p.all_files = false;
    p.filters = vec!["gb".to_string()];
    assert_eq!(names(&p.visible()), vec!["Games", "rom.gb"]);

    p.all_files = true;
    assert_eq!(names(&p.visible()), vec!["Games", "readme.txt", "rom.gb"]);
}

#[test]
fn hidden_files_excluded_until_toggle() {
    let p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![dir(".git"), file("visible.txt", 1, 1)],
    );
    assert_eq!(names(&p.visible()), vec!["visible.txt"]);

    let mut p = p;
    p.show_hidden = true;
    assert_eq!(names(&p.visible()), vec![".git", "visible.txt"]);
}

// ---- selection + scroll ----------------------------------------------------

#[test]
fn selection_clamps_at_bounds() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("a", 1, 1), file("b", 1, 1), file("c", 1, 1)],
    );
    p.search = "xyz".to_string();
    p.move_sel(-5);
    assert_eq!(p.sel, 0);
    assert!(p.search.is_empty());

    p.move_sel(100);
    assert_eq!(p.sel, 2);
}

#[test]
fn scroll_follows_selection() {
    let entries: Vec<Entry> = (0..20)
        .map(|i| file(&format!("f{i:02}", i = i), 1, 1))
        .collect();
    let mut p = Picker::with_entries(Mode::Open, "/x", entries);
    p.viewport = 5;

    p.move_sel(6);
    assert_eq!(p.sel, 6);
    assert_eq!(p.offset, 2);

    assert_eq!(p.on_key(Key::Home), Outcome::None);
    assert_eq!(p.sel, 0);
    assert_eq!(p.offset, 0);

    assert_eq!(p.on_key(Key::End), Outcome::None);
    assert_eq!(p.sel, 19);
    assert_eq!(p.offset, 15); // max_offset = 20 - 5

    assert_eq!(p.on_key(Key::Home), Outcome::None);
    assert_eq!(p.on_key(Key::PageDown), Outcome::None);
    assert_eq!(p.sel, 5);
    assert_eq!(p.offset, 1);

    assert_eq!(p.on_key(Key::PageUp), Outcome::None);
    assert_eq!(p.sel, 0);
    assert_eq!(p.offset, 0);
}

// ---- type-ahead -------------------------------------------------------------

#[test]
fn typeahead_jumps_to_prefix() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![
            file("apple.txt", 1, 1),
            file("date.txt", 1, 1),
            file("date2.txt", 1, 1),
        ],
    );
    assert_eq!(p.sel, 0);

    p.typeahead('d');
    assert_eq!(p.sel, 1); // jump to first match: date.txt

    p.typeahead('d');
    assert_eq!(p.sel, 2); // repeat same char while on a match: cycle to date2.txt

    p.typeahead('d');
    assert_eq!(p.sel, 1); // cycle wraps back to date.txt
}

#[test]
fn clear_search_empties_the_buffer_without_moving_selection() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("apple.txt", 1, 1), file("date.txt", 1, 1)],
    );
    p.typeahead('d');
    assert_eq!(p.sel, 1);
    assert_eq!(p.search, "d");

    p.clear_search();
    assert_eq!(p.search, "");
    assert_eq!(
        p.sel, 1,
        "clearing the search buffer alone doesn't move selection"
    );

    // A fresh 'd' after clearing starts a new query rather than concatenating
    // onto the old one (the pause-then-new-letter case the buffer no longer
    // silently mishandles once the host calls clear_search on its timeout).
    p.typeahead('d');
    assert_eq!(p.search, "d");
}

// ---- enter / navigation intent (pure) --------------------------------------

#[test]
fn resolve_enter_dir_navigates_file_picks_in_open_mode() {
    let mut p = Picker::with_entries(Mode::Open, "/x", vec![dir("sub"), file("a.txt", 1, 1)]);
    // selected() picks index 0 (dirs sort first) = "sub"
    assert_eq!(
        p.resolve_enter(),
        EnterAction::Navigate(PathBuf::from("/x/sub"))
    );

    p.sel = 1; // "a.txt"
    assert_eq!(
        p.resolve_enter(),
        EnterAction::Pick(PathBuf::from("/x/a.txt"))
    );
}

#[test]
fn resolve_enter_file_in_save_mode_is_none() {
    let mut p = Picker::with_entries(Mode::Save, "/x", vec![file("a.txt", 1, 1)]);
    p.sel = 0;
    assert_eq!(p.resolve_enter(), EnterAction::None);
}

#[test]
fn on_key_enter_picks_file_in_open_mode() {
    // Only exercises the Pick branch — never selects a dir, so navigate_to
    // (and the disk-touching source::read_dir it calls) is never invoked here.
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("a.txt", 1, 1), file("b.txt", 1, 1)],
    );
    p.sel = 1;
    assert_eq!(
        p.on_key(Key::Enter),
        Outcome::Picked(PathBuf::from("/x/b.txt"))
    );
}

// ---- path bar ---------------------------------------------------------------

#[test]
fn path_bar_tab_complete_lcp() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/home/user",
        vec![
            file("document.txt", 1, 1),
            file("documentation.md", 1, 1),
            dir("downloads"),
        ],
    );
    p.focus = Focus::PathBar;
    p.path_edit = "/home/user/doc".to_string();
    assert_eq!(p.path_completion(), Some("document".to_string()));

    assert_eq!(p.on_key(Key::Tab), Outcome::None);
    assert_eq!(p.path_edit, "/home/user/document");

    // Typed component already equals a full name -> no further completion.
    p.path_edit = "/home/user/downloads".to_string();
    assert_eq!(p.path_completion(), None);
}

#[test]
fn path_completion_none_when_typed_dir_differs_from_cwd() {
    // cwd is "/home" (its listing has "passwd"), but the typed path bar text
    // points into "/etc" — completing against the loaded "/home" listing
    // would be wrong, so no completion is offered at all.
    let mut p = Picker::with_entries(Mode::Open, "/home", vec![file("passwd", 1, 1)]);
    p.focus = Focus::PathBar;
    p.path_edit = "/etc/pas".to_string();
    assert_eq!(p.path_completion(), None);
}

#[test]
fn path_completion_bare_root_does_not_complete_against_cwd() {
    // `Path::new("/").parent()` is `None`, which used to skip the dir-gate
    // entirely and complete a bare "/" against whatever cwd happened to be
    // loaded. The typed dir "/" must equal cwd, and here it doesn't.
    let mut p = Picker::with_entries(Mode::Open, "/home/user", vec![file("document.txt", 1, 1)]);
    p.focus = Focus::PathBar;
    p.path_edit = "/".to_string();
    assert_eq!(p.path_completion(), None);
}

#[test]
fn path_completion_trailing_slash_equal_to_cwd_completes() {
    // `Path::new("/x/").parent()` is `Some("/")`, one component short of cwd
    // "/x" — the trailing separator must not strip an extra component.
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("doc1.txt", 1, 1), file("doc2.txt", 1, 1)],
    );
    p.focus = Focus::PathBar;
    p.path_edit = "/x/".to_string();
    assert_eq!(p.path_completion(), Some("doc".to_string()));
}

#[test]
fn path_completion_root_cwd() {
    let mut p = Picker::with_entries(Mode::Open, "/", vec![dir("bin"), dir("boot")]);
    p.focus = Focus::PathBar;
    p.path_edit = "/".to_string();
    assert_eq!(p.path_completion(), Some("b".to_string()));
}

// ---- esc two-stage + save two-stage -----------------------------------------

#[test]
fn esc_backs_out_then_cancels() {
    let mut p = Picker::with_entries(Mode::Open, "/x", vec![file("a.txt", 1, 1)]);
    assert_eq!(p.on_key(Key::FocusPath), Outcome::None);
    assert_eq!(p.focus, Focus::PathBar);

    assert_eq!(p.on_key(Key::Cancel), Outcome::None);
    assert_eq!(p.focus, Focus::Browse);

    assert_eq!(p.on_key(Key::Cancel), Outcome::Cancelled);
}

#[test]
fn save_overwrite_two_stage() {
    let mut p = Picker::with_entries(Mode::Save, "/tmp", vec![file("out.gbc", 10, 10)]);

    for c in "out.gbc".chars() {
        assert_eq!(p.on_key(Key::Char(c)), Outcome::None);
    }
    assert_eq!(p.focus, Focus::SaveName);
    assert_eq!(p.save_name, "out.gbc");

    assert_eq!(p.on_key(Key::Enter), Outcome::None);
    assert!(p.overwrite_pending);

    assert_eq!(
        p.on_key(Key::Enter),
        Outcome::Picked(PathBuf::from("/tmp/out.gbc"))
    );
}

#[test]
fn save_empty_name_enter_is_noop() {
    let mut p = Picker::with_entries(Mode::Save, "/tmp", vec![]);
    p.focus = Focus::SaveName;
    assert_eq!(p.on_key(Key::Enter), Outcome::None);
    assert!(!p.overwrite_pending);
}

#[test]
fn save_name_starts_fresh_after_cancel() {
    let mut p = Picker::with_entries(Mode::Save, "/tmp", vec![]);
    for c in "abc".chars() {
        assert_eq!(p.on_key(Key::Char(c)), Outcome::None);
    }
    assert_eq!(p.save_name, "abc");

    assert_eq!(p.on_key(Key::Cancel), Outcome::None);
    assert_eq!(p.focus, Focus::Browse);

    // Re-entering via a fresh Char must not concatenate onto the old buffer.
    assert_eq!(p.on_key(Key::Char('x')), Outcome::None);
    assert_eq!(p.save_name, "x");
}

// ---- toggles / sort / focus (Browse) ----------------------------------------

#[test]
fn on_key_toggles_hidden_all_files_sort_and_focus_path() {
    let mut p = Picker::with_entries(Mode::Open, "/x", vec![dir(".hidden"), file("a.txt", 1, 1)]);

    assert_eq!(p.on_key(Key::ToggleHidden), Outcome::None);
    assert!(p.show_hidden);
    assert_eq!(p.visible().len(), 2);

    assert_eq!(p.on_key(Key::ToggleAllFiles), Outcome::None);
    assert!(!p.all_files);

    assert_eq!(p.sort_key, SortKey::Name);
    assert_eq!(p.on_key(Key::CycleSort), Outcome::None);
    assert_eq!(p.sort_key, SortKey::Size);
    assert_eq!(p.on_key(Key::CycleSort), Outcome::None);
    assert_eq!(p.sort_key, SortKey::Mtime);
    assert_eq!(p.on_key(Key::CycleSort), Outcome::None);
    assert_eq!(p.sort_key, SortKey::Kind);
    assert_eq!(p.on_key(Key::CycleSort), Outcome::None);
    assert_eq!(p.sort_key, SortKey::Name);

    assert_eq!(p.sort_dir, SortDir::Asc);
    assert_eq!(p.on_key(Key::ToggleSortDir), Outcome::None);
    assert_eq!(p.sort_dir, SortDir::Desc);
    assert_eq!(p.on_key(Key::ToggleSortDir), Outcome::None);
    assert_eq!(p.sort_dir, SortDir::Asc);

    assert_eq!(p.on_key(Key::FocusPath), Outcome::None);
    assert_eq!(p.focus, Focus::PathBar);
    assert_eq!(p.path_edit, p.cwd.display().to_string());
}

// ---- mouse ------------------------------------------------------------------

#[test]
fn on_click_selects_row() {
    let mut p = Picker::with_entries(
        Mode::Open,
        "/x",
        vec![file("a", 1, 1), file("b", 1, 1), file("c", 1, 1)],
    );
    p.search = "b".to_string();
    p.on_click(2);
    assert_eq!(p.sel, 2);
    assert!(p.search.is_empty());

    p.on_click(50); // out of range: no-op
    assert_eq!(p.sel, 2);
}

#[test]
fn on_activate_picks_file() {
    let mut p = Picker::with_entries(Mode::Open, "/x", vec![file("a", 1, 1), file("b", 1, 1)]);
    assert_eq!(p.on_activate(1), Outcome::Picked(PathBuf::from("/x/b")));
}

#[test]
fn on_activate_out_of_range_is_none() {
    let mut p = Picker::with_entries(Mode::Open, "/x", vec![file("a", 1, 1)]);
    assert_eq!(p.on_activate(50), Outcome::None);
    assert_eq!(
        p.sel, 0,
        "stale selection must not move on an out-of-range activate"
    );
}

// ---- view model ---------------------------------------------------------------

#[test]
fn view_reflects_state() {
    let entries: Vec<Entry> = (0..8).map(|i| file(&format!("f{i}"), 1, 1)).collect();
    let mut p = Picker::with_entries(Mode::Open, "/x", entries);

    let v = p.view(3);
    assert_eq!(v.total, 8);
    assert!(v.rows.len() <= 3);
    assert_eq!(v.highlight.map(|h| h + v.offset), Some(p.sel));
    assert!(v.status.contains('8'));
    assert!(!v.path_focused);
    assert_eq!(v.path_bar, "/x");
    assert_eq!(v.save_name, None);

    p.move_sel(5);
    let v = p.view(3);
    assert_eq!(v.highlight.map(|h| h + v.offset), Some(p.sel));
}

// ---- extension() free helper -------------------------------------------------

#[test]
fn extension_of_dotfile_is_empty() {
    assert_eq!(extension(".gitignore"), "");
    assert_eq!(extension("rom.gb"), "gb");
    assert_eq!(extension("Makefile"), "");
}

// ---- formatting helpers ------------------------------------------------------

#[test]
fn fmt_size_units() {
    assert_eq!(fmt_size(None), "");
    assert_eq!(fmt_size(Some(0)), "0 B");
    assert_eq!(fmt_size(Some(512)), "512 B");
    assert_eq!(fmt_size(Some(1024)), "1.0 KB");
    assert_eq!(fmt_size(Some(1229)), "1.2 KB");
    assert_eq!(fmt_size(Some(1024 * 1024)), "1.0 MB");
    assert_eq!(fmt_size(Some(1024 * 1024 * 1024)), "1.0 GB");
}

#[test]
fn fmt_mtime_known_dates() {
    assert_eq!(fmt_mtime(None), "");
    assert_eq!(fmt_mtime(Some(0)), "1970-01-01");
    assert_eq!(fmt_mtime(Some(1_700_000_000)), "2023-11-14");
}

// ---- Mode::Directory ------------------------------------------------------

#[test]
fn directory_mode_picks_cwd_and_never_picks_a_file() {
    use std::path::PathBuf;
    let mut p = Picker::with_entries(
        Mode::Directory,
        "/base",
        vec![file("a.txt", 1, 1), dir("sub")],
    );
    // "select this folder" returns the current dir.
    assert_eq!(p.pick_cwd(), Outcome::Picked(PathBuf::from("/base")));
    // A file highlighted + Enter does NOT pick it (unlike Open mode) — files are
    // shown but not selectable in directory mode.
    p.on_click(0); // highlight the file row (a.txt sorts after the dir)
    assert_eq!(
        p.on_key(Key::Enter),
        Outcome::None,
        "a file is not pickable"
    );
    // Contrast: the same file IS picked in Open mode.
    let mut o = Picker::with_entries(Mode::Open, "/base", vec![file("a.txt", 1, 1)]);
    o.on_click(0);
    assert_eq!(
        o.on_key(Key::Enter),
        Outcome::Picked(PathBuf::from("/base/a.txt"))
    );
}
