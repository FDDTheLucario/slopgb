use super::*;

/// A unique temp path per (process, test name) so parallel tests don't collide.
fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("slopgb_settest_{}_{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn native_save_then_load_round_trips_settings_and_recents() {
    let path = tmp("rt.conf");
    let s = Settings {
        mono: true,
        lowercase_hex: true,
        tile_hex_8bit: true,
        ff_speed: 7,
        volume: 0.4,
        model: crate::windows::options::ModelChoice::Cgb,
        ..Settings::default()
    };
    let recent = vec![PathBuf::from("/roms/a.gb"), PathBuf::from("/roms/b.gbc")];
    save_native(&path, &s, &recent);
    let loaded = load_native(&path);
    assert_eq!(loaded.settings, s);
    assert_eq!(loaded.recent, recent);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn native_wins_over_bgb_when_both_exist() {
    let np = tmp("both.conf");
    let bp = tmp("both.ini");
    save_native(&np, &Settings { mono: true, ..Settings::default() }, &[]);
    std::fs::write(&bp, "SoundMono=0\r\n").unwrap();
    let loaded = load_from_paths(Some(&np), Some(&bp));
    assert!(loaded.settings.mono, "native file wins the precedence");
    let _ = std::fs::remove_file(&np);
    let _ = std::fs::remove_file(&bp);
}

#[test]
fn migrates_bgb_to_native_when_only_bgb_exists() {
    let np = tmp("migrate.conf");
    let bp = tmp("migrate.ini");
    std::fs::write(&bp, "SoundMono=1\r\nRecent0=Z:\\r\\game.gb\r\n").unwrap();
    let loaded = load_from_paths(Some(&np), Some(&bp));
    assert!(loaded.settings.mono, "imported from bgb.ini");
    assert_eq!(loaded.recent, vec![PathBuf::from("/r/game.gb")]);
    assert!(np.exists(), "native file written by the migration");
    // The written native file re-loads to the same settings.
    assert!(load_native(&np).settings.mono);
    let _ = std::fs::remove_file(&np);
    let _ = std::fs::remove_file(&bp);
}

#[test]
fn missing_everything_yields_defaults() {
    let loaded = load_from_paths(Some(&tmp("none.conf")), Some(&tmp("none.ini")));
    assert_eq!(loaded.settings, Settings::default());
    assert!(loaded.recent.is_empty());
}

#[test]
fn native_save_preserves_an_unknown_key() {
    let path = tmp("merge.conf");
    std::fs::write(&path, "version = 1\n[future]\nfrobs = 3\n").unwrap();
    save_native(&path, &Settings::default(), &[]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[future]") && text.contains("frobs = 3"), "unknown section survives");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn export_then_import_bgb_round_trips() {
    let path = tmp("export.ini");
    let s = Settings { mono: true, tile_hex_8bit: true, ..Settings::default() };
    let recent = vec![PathBuf::from("/x/a.gb")];
    export_bgb(&path, &s, &recent);
    let back = import_bgb(&path);
    assert_eq!(back.settings, s);
    assert_eq!(back.recent, recent);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn recents_translate_wine_and_posix_paths() {
    assert_eq!(bgb_path_to_posix(r"Z:\home\me\a.gb"), PathBuf::from("/home/me/a.gb"));
    assert_eq!(bgb_path_to_posix("/already/posix.gb"), PathBuf::from("/already/posix.gb"));
    assert_eq!(bgb_path_to_posix("rom.gb"), PathBuf::from("rom.gb"), "no drive, no strip");
    assert_eq!(posix_to_bgb_path(Path::new("/home/me/a.gb")), r"Z:\home\me\a.gb");
    let p = Path::new("/x/y z/game (u).gbc");
    assert_eq!(bgb_path_to_posix(&posix_to_bgb_path(p)), p);
}
