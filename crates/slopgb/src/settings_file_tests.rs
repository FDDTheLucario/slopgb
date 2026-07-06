use super::*;

/// A unique temp path per (process, test name) so parallel tests don't collide.
fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("slopgb_settest_{}_{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn save_then_load_round_trips_on_disk() {
    let path = tmp("rt.ini");
    let s = Settings {
        mono: true,
        lowercase_hex: true,
        tile_hex_8bit: true,
        ff_speed: 7,
        volume: 0.4,
        ..Settings::default()
    };
    save_to(&path, &s);
    assert_eq!(load_from(&path), s);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn missing_file_yields_defaults() {
    assert_eq!(load_from(&tmp("nope.ini")), Settings::default());
}

#[test]
fn garbage_file_yields_defaults_without_panic() {
    let path = tmp("garbage.ini");
    std::fs::write(&path, "not an ini\r\nNoEquals here\r\n[weird section]\r\n").unwrap();
    assert_eq!(load_from(&path), Settings::default(), "no known keys -> all default");
    // Non-UTF8 -> read error -> defaults, no panic.
    std::fs::write(&path, [0x00u8, 0xFF, 0xFE, b'x']).unwrap();
    assert_eq!(load_from(&path), Settings::default());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_preserves_an_existing_unknown_key() {
    let path = tmp("merge.ini");
    std::fs::write(&path, "SoundBufSize=57\r\nVolume=50\r\n").unwrap();
    save_to(&path, &Settings::default()); // default volume 1.0 -> "100"
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("SoundBufSize=57"), "unknown key preserved");
    assert!(text.contains("Volume=100"), "mapped key updated in place");
    let _ = std::fs::remove_file(&path);
}
