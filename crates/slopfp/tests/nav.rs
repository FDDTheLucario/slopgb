//! Integration test: drive the `Picker` through the **public** API only,
//! against a real temp-dir tree on disk. Unit tests exercise `model.rs`
//! logic in isolation (`with_entries`, no fs); this test is the one place
//! `Picker::new` + `navigate_to`'s real disk read (via `on_key`/`on_activate`)
//! gets exercised end to end.

use slopfp::{Key, Mode, Outcome, Picker};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh, unique temp dir under `std::env::temp_dir()` (pid + atomic
/// counter, no rand/deps) — mirrors `source_tests.rs::unique_temp_dir`.
fn unique_temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "slopfp-nav-test-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

/// Row index of the visible entry named `name` in the current `view()`, or
/// panic (the test knows the fixture it built).
fn row_index(p: &mut Picker, name: &str) -> usize {
    let v = p.view(50);
    v.rows.iter().position(|r| r.name == name).unwrap_or_else(|| panic!("{name} not listed"))
}

#[test]
fn nav_into_subdir_reads_disk() {
    let parent = unique_temp_dir("subdir");
    let sub = parent.join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("known.txt"), b"hello").unwrap();

    let mut p = Picker::new(Mode::Open, &parent, &[]);
    let idx = row_index(&mut p, "sub");
    p.on_click(idx);
    let outcome = p.on_key(Key::Enter);
    assert_eq!(outcome, Outcome::None);

    assert_eq!(p.cwd(), sub.as_path());
    let v = p.view(50);
    assert!(v.rows.iter().any(|r| r.name == "known.txt"), "known.txt not listed after nav: {:?}", v.rows);

    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn entering_a_file_picks_it() {
    let parent = unique_temp_dir("pickfile");
    let file = parent.join("known.txt");
    std::fs::write(&file, b"hello").unwrap();

    let mut p = Picker::new(Mode::Open, &parent, &[]);
    let idx = row_index(&mut p, "known.txt");
    let outcome = p.on_activate(idx);
    assert_eq!(outcome, Outcome::Picked(file));

    let _ = std::fs::remove_dir_all(&parent);
}
