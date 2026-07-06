use super::*;

/// A unique temp path per (process, test name) so parallel tests don't collide.
fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("slopgb_settest_{}_{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn save_then_load_round_trips_settings_and_recents() {
    let path = tmp("rt.ini");
    let s = Settings {
        mono: true,
        lowercase_hex: true,
        tile_hex_8bit: true,
        ff_speed: 7,
        volume: 0.4,
        ..Settings::default()
    };
    let recent = vec![PathBuf::from("/roms/a.gb"), PathBuf::from("/roms/b.gbc")];
    save_to(&path, &s, &recent);
    let loaded = load_from(&path);
    assert_eq!(loaded.settings, s);
    assert_eq!(loaded.recent, recent, "recent ROMs round-trip");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn missing_file_yields_defaults_and_no_recents() {
    let loaded = load_from(&tmp("nope.ini"));
    assert_eq!(loaded.settings, Settings::default());
    assert!(loaded.recent.is_empty());
}

#[test]
fn garbage_file_yields_defaults_without_panic() {
    let path = tmp("garbage.ini");
    std::fs::write(&path, "not an ini\r\nNoEquals here\r\n[weird section]\r\n").unwrap();
    assert_eq!(load_from(&path).settings, Settings::default(), "no known keys -> default");
    std::fs::write(&path, [0x00u8, 0xFF, 0xFE, b'x']).unwrap(); // non-UTF8 -> read err
    assert_eq!(load_from(&path).settings, Settings::default());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_preserves_an_existing_unknown_key() {
    let path = tmp("merge.ini");
    std::fs::write(&path, "SoundBufSize=57\r\nVolume=50\r\n").unwrap();
    save_to(&path, &Settings::default(), &[]); // default volume 1.0 -> "100"
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("SoundBufSize=57"), "unknown key preserved");
    assert!(text.contains("Volume=100"), "mapped key updated in place");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn recents_translate_wine_and_posix_paths() {
    // Read: a wine Z:\ path -> POSIX; a drive-less path passes through.
    assert_eq!(bgb_path_to_posix(r"Z:\home\me\a.gb"), PathBuf::from("/home/me/a.gb"));
    assert_eq!(bgb_path_to_posix("/already/posix.gb"), PathBuf::from("/already/posix.gb"));
    assert_eq!(bgb_path_to_posix("rom.gb"), PathBuf::from("rom.gb"), "no drive, no strip");
    // Write: POSIX -> wine Z:\, and the pair round-trips.
    assert_eq!(posix_to_bgb_path(Path::new("/home/me/a.gb")), r"Z:\home\me\a.gb");
    let p = Path::new("/x/y z/game (u).gbc");
    assert_eq!(bgb_path_to_posix(&posix_to_bgb_path(p)), p);
}

#[test]
fn blank_recent_slots_are_skipped_and_padded() {
    let path = tmp("recents.ini");
    save_to(&path, &Settings::default(), &[PathBuf::from("/a.gb")]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(r"Recent0=Z:\a.gb"));
    assert!(text.contains("Recent9="), "unfilled slots written blank (bgb shape)");
    assert_eq!(load_from(&path).recent, vec![PathBuf::from("/a.gb")], "blanks skipped on read");
    let _ = std::fs::remove_file(&path);
}
