//! Shared runners and result plumbing for the game-boy-test-roms suites.
//!
//! Per-suite pass protocols are documented in each suite's
//! `game-boy-test-roms-howto.md` inside the collection; the runners here are
//! the protocol-neutral building blocks (run-to-breakpoint, run-for-time,
//! serial/memory polling, frame-vs-PNG comparison, baseline ratchets).

use std::path::Path;

use slopgb_core::{CLOCK_HZ, GameBoy, Model};

use crate::common::framecmp::{self, CgbColorMap};
use crate::common::png;

/// Build a machine for a collection ROM; a rejected cartridge means a
/// corrupt checkout, not a test failure.
pub fn boot(rom: &[u8], model: Model) -> GameBoy {
    GameBoy::new(model, rom.to_vec())
        .unwrap_or_else(|e| panic!("cartridge rejected ({model:?}): {e:?}"))
}

/// Boot with the Stage-B Tier-2 reclock enabled at construction (before the
/// post-boot state lands), the way the production default behaves once the
/// port flips it. Required for boot-time-DIV-sensitive ROMs (`boot_div`,
/// `boot_sclk_align`) whose phase is decided during `apply_post_boot_state`;
/// the post-boot `set_tier2_reclock` toggle is too late for those.
pub fn boot_with_reclock(rom: &[u8], model: Model) -> GameBoy {
    GameBoy::new_with_reclock(model, rom.to_vec())
        .unwrap_or_else(|e| panic!("cartridge rejected ({model:?}): {e:?}"))
}

/// Step until `pred` is true, or `Err` after `timeout_tcycles` more T-cycles.
/// `pred` is checked every instruction.
pub fn run_until(
    gb: &mut GameBoy,
    timeout_tcycles: u64,
    mut pred: impl FnMut(&GameBoy) -> bool,
) -> Result<(), String> {
    let deadline = gb.cycles().saturating_add(timeout_tcycles);
    while !pred(gb) {
        if gb.cycles() >= deadline {
            return Err(format!(
                "condition not reached within {timeout_tcycles} T-cycles"
            ));
        }
        gb.step();
    }
    Ok(())
}

/// Run until the CPU executes `LD B,B` (mooneye-style completion signal).
pub fn run_until_breakpoint(gb: &mut GameBoy, timeout_tcycles: u64) -> Result<(), String> {
    run_until(gb, timeout_tcycles, |gb| gb.debug_breakpoint_hit())
        .map_err(|e| format!("no LD B,B breakpoint: {e}"))
}

/// Run until the CPU executes an undefined opcode (the 2016-era mooneye
/// fork's completion signal, opcode 0xED).
pub fn run_until_undefined(gb: &mut GameBoy, timeout_tcycles: u64) -> Result<(), String> {
    run_until(gb, timeout_tcycles, |gb| gb.debug_undefined_hit())
        .map_err(|e| format!("no undefined-opcode exit: {e}"))
}

/// Run for an emulated duration at the normal-speed T-cycle rate. The
/// howtos state exit conditions in emulated seconds measured the same way.
pub fn run_for_seconds(gb: &mut GameBoy, seconds: f64) {
    let target = gb
        .cycles()
        .saturating_add((seconds * f64::from(CLOCK_HZ)) as u64);
    while gb.cycles() < target {
        gb.step();
    }
}

/// Run for `frames` frame periods (vblank-to-vblank, or the equivalent
/// cycle count while the LCD is off).
pub fn run_for_frames(gb: &mut GameBoy, frames: u64) {
    for _ in 0..frames {
        gb.run_frame();
    }
}

/// Mooneye/same-suite/age pass check: B,C,D,E,H,L = Fibonacci.
pub fn check_fib(gb: &GameBoy) -> Result<(), String> {
    let r = gb.cpu_regs();
    crate::common::check_fib(r.b, r.c, r.d, r.e, r.h, r.l)
}

/// Blargg memory-signature protocol: `Some(status)` once the magic bytes
/// DE B0 61 sit at $A001-$A003 ($A000 = $80 while running, $00 = pass,
/// anything else = failure code; see blargg readmes).
pub fn blargg_signature_status(gb: &GameBoy) -> Option<u8> {
    (gb.peek_no_io(0xA001) == 0xDE && gb.peek_no_io(0xA002) == 0xB0 && gb.peek_no_io(0xA003) == 0x61)
        .then(|| gb.peek_no_io(0xA000))
}

/// The NUL-terminated result text blargg ROMs leave at $A004 (only
/// meaningful once [`blargg_signature_status`] is `Some`).
pub fn blargg_signature_text(gb: &GameBoy) -> String {
    let mut text = String::new();
    for addr in 0xA004..0xC000u16 {
        match gb.peek_no_io(addr) {
            0 => break,
            b => text.push(char::from(b)),
        }
    }
    text
}

/// Drain-and-accumulate serial output until `pred(&collected)` is true, or
/// `Err` with whatever was collected after `timeout_tcycles`.
pub fn run_until_serial(
    gb: &mut GameBoy,
    timeout_tcycles: u64,
    mut pred: impl FnMut(&[u8]) -> bool,
) -> Result<Vec<u8>, String> {
    let deadline = gb.cycles().saturating_add(timeout_tcycles);
    let mut out = Vec::new();
    loop {
        // Batched stepping: serial bytes arrive every few thousand cycles
        // at most, so polling per-instruction would only burn time.
        for _ in 0..10_000 {
            gb.step();
        }
        out.extend(gb.take_serial_output());
        if pred(&out) {
            return Ok(out);
        }
        if gb.cycles() >= deadline {
            return Err(format!(
                "timeout; serial output so far: {:?}",
                String::from_utf8_lossy(&out)
            ));
        }
    }
}

/// Compare the current frame against a reference PNG from the collection.
/// Failure messages carry the reference path and an ASCII rendering of the
/// emulator frame for triage.
pub fn expect_frame_png(gb: &GameBoy, png_path: &Path, map: CgbColorMap) -> Result<(), String> {
    let img = png::load_png(png_path)?;
    framecmp::compare_frame_image(gb.frame(), &img, map).map_err(|e| {
        format!(
            "{}: {e}\nemulator frame:\n{}",
            png_path.display(),
            framecmp::frame_ascii(gb.frame())
        )
    })
}

/// Run `f`, converting a panic into `Err(payload text)`. Panic-hook output
/// is suppressed for the duration (`common::quiet_catch_unwind`) so expected
/// per-case panics cannot bury the structured failure report a suite builds.
pub fn catch_panic<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    crate::common::quiet_catch_unwind(f).map_err(|p| crate::common::panic_message(p.as_ref()))
}

/// One case body through [`catch_panic`]: a panicking case becomes a
/// regular `Err("panicked: …")` [`CaseResult`] payload, so a core crash in
/// one rom×model case can never abort a whole suite matrix.
pub fn catch_case(f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    catch_panic(f).unwrap_or_else(|msg| Err(format!("panicked: {msg}")))
}

/// Parse a known-failure baseline file: one case key per line. Blank lines
/// and whole-line `#` comments are ignored; a trailing comment is introduced
/// by ` #` (space-hash, the key then trimmed) so a `#subtest` discriminator
/// *inside* a key — smallsuites' `rtc3test/rtc3test.gb#basic-tests [Dmg]`
/// style — survives if such a suite ever moves its baseline to a txt file.
pub fn parse_baseline(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let key = match line.find(" #") {
                Some(i) => line[..i].trim_end(),
                None => line,
            };
            (!key.is_empty()).then_some(key)
        })
        .collect()
}

/// `path` relative to `root` as a forward-slash string — the suites' case
/// key and inventory entry format, independent of the host path separator
/// (Windows CI yields backslash components otherwise). A path outside
/// `root` is a harness bug and panics; callers must never silently fall
/// back to an absolute path that no baseline or inventory entry can match.
pub fn rel_unix(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or_else(|_| panic!("{} not under {}", path.display(), root.display()))
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// One executed rom×model case: `key` identifies it (stable across runs,
/// e.g. `"dmg-acid2/dmg-acid2.gb [Cgb]"`), `result` is the protocol verdict.
pub struct CaseResult {
    pub key: String,
    pub result: Result<(), String>,
}

/// Stable case key for baselines: collection-relative path + model.
pub fn case_key(rel: &str, model: Model) -> String {
    format!("{rel} [{model:?}]")
}

/// Ratchet a suite's results against its known-failure baseline:
///
/// * a failing case **not** in `baseline` is a regression — panic;
/// * a passing case that **is** in `baseline` is a stale entry — panic
///   (shrink the list, that is the progress being tracked);
/// * an empty `baseline` therefore asserts the whole suite passes.
///
/// Failure output carries every offending case with its error detail.
pub fn assert_against_baseline(suite: &str, results: &[CaseResult], baseline: &[&str]) {
    let mut regressions = Vec::new();
    let mut stale: Vec<&str> = Vec::new();
    for case in results {
        let listed = baseline.contains(&case.key.as_str());
        match (&case.result, listed) {
            (Err(e), false) => regressions.push(format!("{}: {e}", case.key)),
            (Ok(()), true) => stale.push(&case.key),
            _ => {}
        }
    }
    // Baseline entries that no longer match any executed case are stale too
    // (renamed ROM, changed model routing) — they would otherwise mask a
    // future regression under the old name.
    let executed: Vec<&str> = results.iter().map(|c| c.key.as_str()).collect();
    let orphaned: Vec<&&str> = baseline.iter().filter(|b| !executed.contains(*b)).collect();
    if regressions.is_empty() && stale.is_empty() && orphaned.is_empty() {
        return;
    }
    let mut msg = format!("{suite}: baseline mismatch\n");
    if !regressions.is_empty() {
        msg.push_str(&format!(
            "\n{} case(s) failing but not in the known-failure baseline:\n  {}\n",
            regressions.len(),
            regressions.join("\n  ")
        ));
    }
    if !stale.is_empty() {
        msg.push_str(&format!(
            "\n{} baseline entr(ies) now passing — remove them:\n  {}\n",
            stale.len(),
            stale.join("\n  ")
        ));
    }
    if !orphaned.is_empty() {
        msg.push_str(&format!(
            "\n{} baseline entr(ies) match no executed case — remove or fix them:\n  {}\n",
            orphaned.len(),
            orphaned
                .iter()
                .map(|s| **s)
                .collect::<Vec<_>>()
                .join("\n  ")
        ));
    }
    panic!("{msg}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(key: &str, result: Result<(), String>) -> CaseResult {
        CaseResult {
            key: key.into(),
            result,
        }
    }

    #[test]
    fn baseline_accepts_listed_failures_and_unlisted_passes() {
        assert_against_baseline(
            "demo",
            &[case("a [Dmg]", Ok(())), case("b [Cgb]", Err("boom".into()))],
            &["b [Cgb]"],
        );
    }

    #[test]
    #[should_panic(expected = "not in the known-failure baseline")]
    fn baseline_panics_on_unlisted_failure() {
        assert_against_baseline("demo", &[case("a [Dmg]", Err("boom".into()))], &[]);
    }

    #[test]
    #[should_panic(expected = "now passing")]
    fn baseline_panics_on_stale_entry() {
        assert_against_baseline("demo", &[case("a [Dmg]", Ok(()))], &["a [Dmg]"]);
    }

    #[test]
    #[should_panic(expected = "match no executed case")]
    fn baseline_panics_on_orphaned_entry() {
        assert_against_baseline("demo", &[case("a [Dmg]", Ok(()))], &["zz [Cgb]"]);
    }

    #[test]
    fn case_key_format_is_stable() {
        assert_eq!(
            case_key("dmg-acid2/dmg-acid2.gb", slopgb_core::Model::Cgb),
            "dmg-acid2/dmg-acid2.gb [Cgb]"
        );
    }

    // --- panic isolation ---

    #[test]
    fn catch_case_converts_panics_and_passes_results_through() {
        assert_eq!(catch_case(|| Ok(())), Ok(()));
        assert_eq!(catch_case(|| Err("plain".into())), Err("plain".into()));
        let err = catch_case(|| panic!("boom {}", 42)).unwrap_err();
        assert_eq!(err, "panicked: boom 42");
        let err = catch_case(|| std::panic::panic_any(7u32)).unwrap_err();
        assert_eq!(err, "panicked: non-string panic payload");
    }

    #[test]
    fn catch_panic_preserves_the_return_value() {
        assert_eq!(catch_panic(|| vec![1, 2, 3]), Ok(vec![1, 2, 3]));
        assert_eq!(catch_panic(|| -> u8 { panic!("x") }), Err("x".into()));
    }

    // --- baseline file grammar ---

    #[test]
    fn parse_baseline_skips_blanks_and_whole_line_comments() {
        let text = "# header\n\n  a/b.gb [Dmg]  \n# note\nc/d.gbc [Cgb]\n";
        assert_eq!(parse_baseline(text), ["a/b.gb [Dmg]", "c/d.gbc [Cgb]"]);
        assert!(parse_baseline("").is_empty());
        assert!(parse_baseline("# only comments\n\n").is_empty());
    }

    #[test]
    fn parse_baseline_strips_trailing_space_hash_comments() {
        assert_eq!(
            parse_baseline("a [Dmg]\nb [Cgb] # trailing note\n  c [Dmg]   #x\n"),
            ["a [Dmg]", "b [Cgb]", "c [Dmg]"]
        );
        // A line that is only a comment after trimming yields no key.
        assert!(parse_baseline("   # indented comment\n").is_empty());
    }

    #[test]
    fn parse_baseline_keeps_hash_subtest_discriminators() {
        // The trailing-comment marker is ` #` (space-hash), so a bare `#`
        // inside a key — the rtc3test subtest convention — must survive,
        // even combined with a real trailing comment.
        let text = "rtc3test/rtc3test.gb#basic-tests [Dmg]\n\
                    rtc3test/rtc3test.gb#range-tests [Cgb] # flaky\n";
        assert_eq!(
            parse_baseline(text),
            [
                "rtc3test/rtc3test.gb#basic-tests [Dmg]",
                "rtc3test/rtc3test.gb#range-tests [Cgb]"
            ]
        );
    }

    // --- rel_unix ---

    #[test]
    fn rel_unix_joins_components_with_forward_slashes() {
        assert_eq!(
            rel_unix(Path::new("/a/b"), Path::new("/a/b/dmg-acid2/dmg-acid2.gb")),
            "dmg-acid2/dmg-acid2.gb"
        );
    }

    #[test]
    #[should_panic(expected = "not under")]
    fn rel_unix_panics_on_paths_outside_the_root() {
        rel_unix(Path::new("/a/b"), Path::new("/elsewhere/rom.gb"));
    }
}
