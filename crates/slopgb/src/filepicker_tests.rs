use super::*;

#[test]
fn open_candidates_are_the_dialog_tools_in_priority_order() {
    let c = open_candidates();
    assert_eq!(c[0], ("zenity", &["--file-selection"][..]), "zenity first");
    assert_eq!(c[1], ("kdialog", &["--getopenfilename", "."][..]));
    assert_eq!(c[2], ("yad", &["--file"][..]));
    assert_eq!(c[3].0, "qarma");
}

#[test]
fn save_candidates_use_the_save_flags() {
    let c = save_candidates();
    assert_eq!(
        c[0],
        (
            "zenity",
            &["--file-selection", "--save", "--confirm-overwrite"][..]
        )
    );
    assert_eq!(c[1], ("kdialog", &["--getsavefilename", "."][..]));
}

#[test]
fn parse_pick_output_trims_and_rejects_failures() {
    assert_eq!(
        parse_pick_output("/tmp/a.gb\n", true).as_deref(),
        Some("/tmp/a.gb")
    );
    assert_eq!(
        parse_pick_output("", true),
        None,
        "empty output → cancelled"
    );
    assert_eq!(parse_pick_output("/x", false), None, "non-zero exit → None");
    assert_eq!(
        parse_pick_output("  \n", true),
        None,
        "whitespace-only → None"
    );
    // Paths with spaces survive (only the trailing newline is trimmed).
    assert_eq!(
        parse_pick_output("/a/b c.gb\n", true).as_deref(),
        Some("/a/b c.gb")
    );
}

#[test]
fn try_pick_distinguishes_missing_tool_from_cancel() {
    // Missing binary → spawn error → NoSpawn (try the next tool); never unwraps.
    assert_eq!(
        try_pick("slopgb-no-such-picker-xyz", &[]),
        TryOutcome::NoSpawn
    );
    // `true` exists, exits 0, prints nothing → a tool ran but yielded no path.
    assert_eq!(try_pick("true", &[]), TryOutcome::Cancelled);
}
