use super::*;

#[test]
fn candidates_are_the_three_clipboard_tools_in_priority_order() {
    let c = clipboard_candidates();
    assert_eq!(c[0].0, "wl-copy", "Wayland first");
    assert_eq!(c[0].1, &[] as &[&str], "wl-copy reads stdin with no args");
    assert_eq!(c[1], ("xclip", &["-selection", "clipboard"][..]));
    assert_eq!(c[2], ("xsel", &["-ib"][..]));
}

#[test]
fn copy_never_panics_and_returns_a_bool() {
    // On a headless CI host none of the tools exist → false; on a desktop one
    // may succeed → true. Either way it must not panic (every spawn/IO error
    // falls through, never unwraps).
    let _ = copy("slopgb clipboard test");
    let _ = copy(""); // empty text is also fine
}
