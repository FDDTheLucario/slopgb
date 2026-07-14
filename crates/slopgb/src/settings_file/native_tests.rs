use super::*;

#[test]
fn writes_version_and_sections() {
    let mut d = Doc::default();
    to_doc(&Settings::default(), &[], &mut d);
    let out = d.serialize();
    assert!(
        out.starts_with("version = 1\n"),
        "version at the top: {out:?}"
    );
    assert!(out.contains("[system]\nmodel = auto"));
    assert!(out.contains("[graphics]"));
    assert!(out.contains("palette = 0xE8FCCC, 0xACD490, 0x548C70, 0x142C38"));
    assert!(out.contains("[sound]\nvolume = 1"));
}

#[test]
fn settings_and_recent_round_trip() {
    let s = Settings {
        model: ModelChoice::Cgb,
        mono: true,
        volume: 0.4,
        tile_hex_8bit: true,
        ff_speed: 6,
        break_ld_b_b: true,
        stretch: true,
        frame_blend: true,
        dmg_gbc_lcd: true,
        contrast: 0.75,
        sgb_border_screenshot: true,
        screenshot_format: crate::windows::options::ScreenshotFormat::Png,
        show_errors_on_rom_load: false,
        load_rom_dialog_on_startup: true,
        bootrom_dmg: "x.bin".to_string(),
        ..Settings::default()
    };
    let recent = vec!["/roms/a.gb".to_string(), "/roms/b.gbc".to_string()];
    let mut d = Doc::default();
    to_doc(&s, &recent, &mut d);
    let (back, back_recent) = from_doc(&Doc::parse(&d.serialize()));
    assert_eq!(back, s, "all fields round-trip");
    assert_eq!(back_recent, recent, "recent round-trips");
}

#[test]
fn plugin_config_round_trips_dir_allow_mutation_and_disabled() {
    use crate::windows::options::PluginEntry;
    let s = Settings {
        plugins: PluginConfig {
            dir: "/opt/plugins".into(),
            allow_mutation: true,
            entries: vec![
                PluginEntry {
                    name: "a".into(),
                    capabilities: "introspection".into(),
                    enabled: true,
                },
                PluginEntry {
                    name: "b".into(),
                    capabilities: "introspection".into(),
                    enabled: false,
                },
            ],
        },
        ..Settings::default()
    };
    let mut d = Doc::default();
    to_doc(&s, &[], &mut d);
    let text = d.serialize();
    assert!(text.contains("[plugins]"), "{text}");
    assert!(text.contains("dir = /opt/plugins"));
    assert!(text.contains("allow_mutation = true"));
    assert!(
        text.contains("disabled = b"),
        "only the off plugin persists"
    );

    let (back, _) = from_doc(&Doc::parse(&text));
    assert_eq!(back.plugins.dir, "/opt/plugins");
    assert!(back.plugins.allow_mutation);
    // Only the disabled plugin survives (an enabled one defaults on), rebuilt as
    // a placeholder — its capability label is unknown until the host is synced.
    assert_eq!(
        back.plugins.entries,
        vec![PluginEntry {
            name: "b".into(),
            capabilities: String::new(),
            enabled: false,
        }]
    );
}

#[test]
fn unknown_keys_and_sections_survive_a_save() {
    let src = "version = 1\n[system]\nmodel = dmg\nfuture_key = 42\n\n[weird]\nx = y\n";
    let mut d = Doc::parse(src);
    to_doc(&Settings::default(), &[], &mut d);
    let out = d.serialize();
    assert!(
        out.contains("future_key = 42"),
        "unknown key in a known section preserved"
    );
    assert!(
        out.contains("[weird]") && out.contains("x = y"),
        "unknown section preserved"
    );
    assert!(
        out.contains("model = auto"),
        "known key overwritten to the new value"
    );
}

#[test]
fn missing_keys_default_and_bad_palette_falls_back() {
    // A near-empty doc: everything defaults.
    let (s, recent) = from_doc(&Doc::parse("version = 1\n[system]\nmodel = cgb\n"));
    assert_eq!(s.model, ModelChoice::Cgb);
    assert_eq!(
        s.volume,
        Settings::default().volume,
        "absent volume -> default"
    );
    assert!(recent.is_empty());
    // A 3-entry palette is malformed -> default palette kept.
    let (s2, _) = from_doc(&Doc::parse(
        "[graphics]\npalette = 0x111111, 0x222222, 0x333333\n",
    ));
    assert_eq!(s2.dmg_palette, Settings::default().dmg_palette);
}

#[test]
fn comments_and_blank_lines_survive() {
    let src = "# my config\nversion = 1\n\n[sound]\nvolume = 0.5\n";
    let d = Doc::parse(src);
    let out = d.serialize();
    assert!(out.contains("# my config"), "comment preserved");
    assert_eq!(d.get("sound", "volume"), Some("0.5"));
}

// --- Task 6: ThemeChoice persistence -----------------------------------------

#[test]
fn theme_choice_round_trips_every_variant() {
    for choice in [
        ThemeChoice::Light,
        ThemeChoice::Dark,
        ThemeChoice::Classic,
        ThemeChoice::Custom("solarized".to_string()),
    ] {
        let s = Settings {
            theme: choice.clone(),
            ..Settings::default()
        };
        let mut d = Doc::default();
        to_doc(&s, &[], &mut d);
        let (back, _) = from_doc(&Doc::parse(&d.serialize()));
        assert_eq!(back.theme, choice, "{choice:?} round-trips");
    }
}

#[test]
fn audio_backend_round_trips_and_unknown_falls_back() {
    for choice in [AudioBackend::Builtin, AudioBackend::SgbCoprocessor] {
        let s = Settings {
            audio_backend: choice,
            ..Settings::default()
        };
        let mut d = Doc::default();
        to_doc(&s, &[], &mut d);
        let (back, _) = from_doc(&Doc::parse(&d.serialize()));
        assert_eq!(back.audio_backend, choice, "{choice:?} round-trips");
    }
    // Unknown / missing value falls back to the default (Built-in), non-fatally.
    let (s, _) = from_doc(&Doc::parse("[sound]\naudio_backend = bogus\n"));
    assert_eq!(s.audio_backend, AudioBackend::Builtin);
    let (s2, _) = from_doc(&Doc::parse("version = 1\n"));
    assert_eq!(s2.audio_backend, AudioBackend::Builtin);
}

#[test]
fn unknown_theme_value_falls_back_to_default_non_fatally() {
    let (s, _) = from_doc(&Doc::parse("[ui]\ntheme = bogus\n"));
    assert_eq!(s.theme, ThemeChoice::default());
    // A missing key defaults the same way.
    let (s2, _) = from_doc(&Doc::parse("version = 1\n"));
    assert_eq!(s2.theme, ThemeChoice::default());
}

// --- Task 9: custom theme registry loader ------------------------------------

#[test]
fn custom_themes_loads_every_theme_section() {
    let d = Doc::parse(
        "[theme.solarized]\nbg = 0x002B36\ntext = 0x93A1A1\n\n\
         [theme.hotdog]\nbg = 0xFF0000\n",
    );
    let themes = custom_themes(&d);
    assert_eq!(themes.get("solarized").unwrap().bg, 0x0000_2B36);
    assert_eq!(themes.get("solarized").unwrap().text, 0x0093_A1A1);
    assert_eq!(themes.get("hotdog").unwrap().bg, 0x00FF_0000);
    assert!(themes.get("nonexistent").is_none());
}

#[test]
fn custom_themes_skips_a_malformed_section_without_panicking() {
    // An unknown role in one section must not stop the rest from loading.
    let d = Doc::parse("[theme.bad]\nnot_a_role = purple\n\n[theme.good]\nbg = 0x111111\n");
    let themes = custom_themes(&d);
    assert!(themes.get("bad").is_none(), "malformed section skipped");
    assert_eq!(themes.get("good").unwrap().bg, 0x0011_1111);
}
