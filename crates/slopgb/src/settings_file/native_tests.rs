use super::*;

#[test]
fn writes_version_and_sections() {
    let mut d = Doc::default();
    to_doc(&Settings::default(), &[], &mut d);
    let out = d.serialize();
    assert!(out.starts_with("version = 1\n"), "version at the top: {out:?}");
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
fn unknown_keys_and_sections_survive_a_save() {
    let src = "version = 1\n[system]\nmodel = dmg\nfuture_key = 42\n\n[weird]\nx = y\n";
    let mut d = Doc::parse(src);
    to_doc(&Settings::default(), &[], &mut d);
    let out = d.serialize();
    assert!(out.contains("future_key = 42"), "unknown key in a known section preserved");
    assert!(out.contains("[weird]") && out.contains("x = y"), "unknown section preserved");
    assert!(out.contains("model = auto"), "known key overwritten to the new value");
}

#[test]
fn missing_keys_default_and_bad_palette_falls_back() {
    // A near-empty doc: everything defaults.
    let (s, recent) = from_doc(&Doc::parse("version = 1\n[system]\nmodel = cgb\n"));
    assert_eq!(s.model, ModelChoice::Cgb);
    assert_eq!(s.volume, Settings::default().volume, "absent volume -> default");
    assert!(recent.is_empty());
    // A 3-entry palette is malformed -> default palette kept.
    let (s2, _) = from_doc(&Doc::parse("[graphics]\npalette = 0x111111, 0x222222, 0x333333\n"));
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
