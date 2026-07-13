use super::*;

#[test]
fn candidates_are_the_three_clipboard_tools_in_priority_order() {
    let c = clipboard_candidates();
    assert_eq!(c[0].0, "wl-copy", "Wayland first");
    assert_eq!(c[0].1, &[] as &[&str], "wl-copy reads stdin with no args");
    assert_eq!(c[1], ("xclip", &["-selection", "clipboard"][..]));
    assert_eq!(c[2], ("xsel", &["-ib"][..]));
}

// Exercise the real `try_copy` logic (write to the child's stdin, require a
// clean exit) against STUB commands, so the test never spawns a real clipboard
// tool or touches the developer's clipboard.
#[cfg(unix)]
#[test]
fn try_copy_needs_a_written_stdin_and_a_clean_exit() {
    // `cat` drains stdin and exits 0 → the write-succeeded + exited-cleanly path.
    assert!(try_copy("cat", &[], "slopgb clipboard payload"));
    assert!(try_copy("cat", &[], ""), "empty text still succeeds");
    // `false` ignores stdin and exits nonzero → the clean-exit check fails.
    assert!(!try_copy("false", &[], "anything"));
    // A tool that isn't installed can't spawn → falls through to false.
    assert!(!try_copy("slopgb-no-such-clipboard-tool", &[], "x"));
}
