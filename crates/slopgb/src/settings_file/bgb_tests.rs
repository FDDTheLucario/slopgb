use super::*;
use crate::ui::ThemeChoice;
use crate::windows::options::ModelChoice;

const REAL: &str = include_str!("../../../../docs/bgb-reference/bgb.ini");

#[test]
fn settings_round_trip_through_bgb_ini() {
    // Non-default across every value type; dmg_palette unchanged (so scheme
    // stays matched) — everything (incl. model) survives to_ini -> from_ini.
    let s = Settings {
        model: ModelChoice::Cgb,
        stretch: true,
        mono: true,
        volume: 0.5,
        lowercase_hex: true,
        show_clocks: false,
        rgbds_disasm: false,
        tile_hex_8bit: true,
        memory_window: true,
        break_ld_b_b: true,
        break_echo_ram: true,
        break_oam_dma_bad: true,
        ff_speed: 5,
        framerate_limit: 30,
        bootrom_dmg: "dmg.bin".to_string(),
        esc_shows_debugger: false,
        allow_opposing: true,
        uninited_wram: true, // bgb's UninitedWRAM key
        frame_blend: true,
        dmg_gbc_lcd: true,
        contrast: 0.75,
        sgb_border_screenshot: true,
        screenshot_format: crate::windows::options::ScreenshotFormat::Png,
        screenshot_copies: true,
        show_errors_on_rom_load: false,
        load_rom_dialog_on_startup: true,
        reduce_cpu: false,
        auto_reset_on_system_change: false,
        registers_editable: false,
        start_in_debugger: true,
        mem_live_update: false,
        cpu_usage_meter: true,
        ..Settings::default()
    };
    let mut ini = Ini::parse("");
    to_ini(&s, &mut ini);
    let back = from_ini(&ini);
    assert_eq!(back, s, "mapped + slopgb-extra fields round-trip");
    assert!(ini.serialize().contains("UninitedWRAM=1"), "bgb key name");
}

#[test]
fn theme_choice_round_trips_through_bgb_ini_as_a_slopgb_extra() {
    // bgb has no theming concept, so this rides the `Slopgb*` extra-key
    // convention (like `SlopgbStretch` etc.) rather than a real bgb key.
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
        let mut ini = Ini::parse("");
        to_ini(&s, &mut ini);
        let back = from_ini(&ini);
        assert_eq!(back.theme, choice, "{choice:?} round-trips");
    }
}

#[test]
fn plugin_config_round_trips_and_preserves_unknown_keys() {
    use crate::windows::options::{PluginConfig, PluginEntry};
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
    // Round-trip over the REAL bgb.ini so the byte-for-byte preserve invariant is
    // exercised alongside the new Slopgb* plugin keys.
    let mut ini = Ini::parse(REAL);
    to_ini(&s, &mut ini);
    let out = ini.serialize();
    assert!(out.contains("SlopgbPluginsDir=/opt/plugins"));
    assert!(out.contains("SlopgbPluginsAllowMutation=1"));
    assert!(out.contains("SlopgbPluginsDisabled=b"));
    // An unrelated unmodelled bgb key still survives verbatim.
    assert!(out.contains("SoundBufSize=57"));

    let back = from_ini(&ini);
    assert_eq!(back.plugins.dir, "/opt/plugins");
    assert!(back.plugins.allow_mutation);
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
fn save_preserves_unknown_bgb_keys_and_writes_slopgb_extras() {
    let mut ini = Ini::parse(REAL);
    let s = Settings {
        tile_hex_8bit: true,
        ..Settings::default()
    };
    to_ini(&s, &mut ini);
    let out = ini.serialize();
    // Keys we don't model survive untouched (the preserve invariant).
    assert!(out.contains("SoundBufSize=57"));
    assert!(out.contains("CamExposure=800"));
    assert!(out.contains("Joypad0=272526285341100DFFFF6B73716A6D1BFFFFFF09FF"));
    // Our extra is written (bgb ignores unknown keys).
    assert!(out.contains("SlopgbTileHex8bit=1"));
    // A mapped key reflects our value (default rgbds=true replaces bgb's no$gmb).
    assert!(out.contains("DisasmSyntax=rgbds"));
    assert!(!out.contains("DisasmSyntax=no$gmb"));
}

#[test]
fn from_real_ini_reads_known_values() {
    let s = from_ini(&Ini::parse(REAL));
    assert!((s.volume - 0.9).abs() < 1e-6, "Volume=90 -> 0.9");
    assert!(!s.rgbds_disasm, "DisasmSyntax=no$gmb -> not rgbds");
    assert!(!s.lowercase_hex, "DebugHexLower=0");
    assert!(s.show_clocks, "DebugCountedClocks=1");
    assert!(s.break_invalid_op, "InvalidOpBreak=1");
    assert!(!s.mono, "SoundMono=0");
    assert!(s.esc_shows_debugger, "DebugEsc=1");
    assert_eq!(
        s.dmg_palette[0], 0x00E8_FCCC,
        "Color0=CCFCE8 (BGR) -> E8FCCC"
    );
    assert_eq!(
        s.model,
        ModelChoice::Auto,
        "SystemMode=3 -> automatic prefer GBC"
    );
}

#[test]
fn model_maps_to_bgb_system_mode() {
    let sysmode = |m| {
        let mut f = Ini::parse("");
        to_ini(
            &Settings {
                model: m,
                ..Settings::default()
            },
            &mut f,
        );
        f.get("SystemMode").unwrap().to_string()
    };
    assert_eq!(sysmode(ModelChoice::Dmg), "0", "Gameboy");
    assert_eq!(sysmode(ModelChoice::Cgb), "1", "Gameboy Color");
    assert_eq!(sysmode(ModelChoice::Sgb), "2", "Super Gameboy");
    assert_eq!(sysmode(ModelChoice::Auto), "3", "automatic prefer GBC");
    assert_eq!(sysmode(ModelChoice::AutoSgb), "4", "automatic prefer SGB");
    assert_eq!(sysmode(ModelChoice::Sgb2), "5", "SGB + GBC");
    assert_eq!(
        sysmode(ModelChoice::CgbBorder),
        "6",
        "GBC + initial SGB border"
    );
    assert_eq!(sysmode(ModelChoice::AutoNoSgb), "7", "Gameboy or GBC");
    // Read-back is 1:1 for 0..7; an unknown value falls back to Auto.
    for (v, want) in [
        ("0", ModelChoice::Dmg),
        ("1", ModelChoice::Cgb),
        ("2", ModelChoice::Sgb),
        ("3", ModelChoice::Auto),
        ("4", ModelChoice::AutoSgb),
        ("5", ModelChoice::Sgb2),
        ("6", ModelChoice::CgbBorder),
        ("7", ModelChoice::AutoNoSgb),
        ("9", ModelChoice::Auto),
    ] {
        assert_eq!(
            from_ini(&Ini::parse(&format!("SystemMode={v}\r\n"))).model,
            want
        );
    }
}
