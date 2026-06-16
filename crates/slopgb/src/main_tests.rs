use super::*;

#[test]
fn frame_duration_matches_hardware_rate() {
    // 70224 / 4194304 s = 16.742706... ms
    assert_eq!(FRAME_DURATION.as_nanos(), 16_742_706);
}

#[test]
fn recent_list_dedups_to_front_and_caps_at_ten() {
    let mut recent: Vec<PathBuf> = Vec::new();
    push_recent_into(&mut recent, Path::new("a.gb"));
    push_recent_into(&mut recent, Path::new("b.gb"));
    assert_eq!(recent, vec![PathBuf::from("b.gb"), PathBuf::from("a.gb")]);
    // Re-loading A moves it to the front (deduped, no duplicate entry).
    push_recent_into(&mut recent, Path::new("a.gb"));
    assert_eq!(recent, vec![PathBuf::from("a.gb"), PathBuf::from("b.gb")]);
    // Capped at 10 most-recent.
    for i in 0..15 {
        push_recent_into(&mut recent, Path::new(&format!("rom{i}.gb")));
    }
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0], PathBuf::from("rom14.gb"), "most-recent first");
}
