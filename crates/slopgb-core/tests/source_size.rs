//! Repo hygiene guard: enforce the project's "every `.rs` under 1000 lines"
//! rule (CLAUDE.md "No god files"). Walks the whole workspace source + test
//! tree and fails listing every file over the cap, so a silently-growing god
//! file blocks the test run instead of waiting for a human to notice. Mirrors
//! the mooneye coverage-guard philosophy: a rule the repo states should be
//! machine-checked, not review-checked.

use std::path::{Path, PathBuf};

/// The god-file ceiling. A file may sit at the cap; exceeding it fails.
const LIMIT: usize = 1000;

/// Collect every `.rs` under `dir`, skipping `target/` build dirs.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn every_rs_file_under_the_god_file_cap() {
    // CARGO_MANIFEST_DIR = <repo>/crates/slopgb-core → up two → <repo>.
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("crates");
    let mut files = Vec::new();
    rs_files(&crates, &mut files);
    assert!(
        !files.is_empty(),
        "walked no .rs files under {} — wrong root?",
        crates.display()
    );

    let mut over: Vec<(usize, PathBuf)> = files
        .into_iter()
        .filter_map(|p| {
            let n = std::fs::read_to_string(&p).ok()?.lines().count();
            (n > LIMIT).then_some((n, p))
        })
        .collect();
    over.sort_by(|a, b| b.0.cmp(&a.0));

    assert!(
        over.is_empty(),
        "{} file(s) exceed the {LIMIT}-line god-file cap (CLAUDE.md \"No god \
         files\" — split into cohesive submodules / externalize tests):\n{}",
        over.len(),
        over.iter()
            .map(|(n, p)| format!("  {n:>5}  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
