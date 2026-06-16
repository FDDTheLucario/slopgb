use super::*;
use slopgb_core::Model;

/// A blank no-ROM App, as `main` builds it when launched without a ROM.
fn blank_app() -> App {
    let opts = Options {
        rom: None,
        model: None,
        scale: 3,
        mute: true,
    };
    App::new(opts, Session::blank(Model::Dmg), false)
}

#[test]
fn no_rom_idles_emulation_like_pause() {
    // The blank machine never advances: with no ROM loaded, about_to_wait must
    // emulate zero frames regardless of pause/break (bgb shows the off LCD and
    // doesn't run the CPU). Running + a ROM is the only case that emulates.
    assert!(should_idle(false, false, false), "no ROM idles");
    assert!(should_idle(true, false, false));
    assert!(should_idle(false, true, false));
    assert!(should_idle(true, false, true), "paused idles");
    assert!(should_idle(false, true, true), "broken idles");
    assert!(
        !should_idle(false, false, true),
        "running with a ROM emulates"
    );
}

#[test]
fn no_rom_title_is_bare_slopgb() {
    // bgb with no ROM titles the window "bgb"; slopgb titles it "slopgb" (no
    // game name, no leading separator). With a ROM the game stem leads.
    assert_eq!(window_title(false, "anything", " — paused"), "slopgb");
    assert_eq!(window_title(true, "pokemon", ""), "pokemon — slopgb");
    assert_eq!(
        window_title(true, "tetris", " (debugging)"),
        "tetris — slopgb (debugging)"
    );
}

#[test]
fn blank_frame_is_solid_lightest_shade() {
    // The no-ROM screen is a solid fill of the palette's lightest shade (bgb's
    // pale-green LCD-off colour by default), built from dmg_palette[0].
    let f = blank_frame(0x00E8_FCCC);
    assert_eq!(f.len(), SCREEN_PIXELS);
    assert!(f.iter().all(|&p| p == 0x00E8_FCCC));
}

#[test]
fn blank_app_starts_not_loaded_and_loading_flips_the_flag() {
    let mut app = blank_app();
    assert!(!app.rom_loaded, "no ROM at startup");
    // The blank screen is bgb green (the default palette's lightest shade).
    assert_eq!(app.blank_frame[0], app.settings.dmg_palette[0]);

    // Loading a real ROM (the drag-drop / Load ROM / Recent path) starts it.
    let dir = std::env::temp_dir().join(format!("slopgb-noload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rom_path = dir.join("blank.gb");
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x00; // ROM ONLY
    std::fs::write(&rom_path, &rom).unwrap();
    app.load_dropped(&rom_path);
    assert!(app.rom_loaded, "a loaded ROM starts emulation");

    // A bad path is ignored and must not silently "start" a non-existent game.
    let mut app2 = blank_app();
    app2.load_dropped(Path::new("/no/such/rom.gb"));
    assert!(
        !app2.rom_loaded,
        "a failed load leaves the blank state intact"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

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
