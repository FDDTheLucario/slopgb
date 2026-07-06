use super::*;

const REAL: &str = include_str!("../../../../docs/bgb-reference/bgb.ini");

#[test]
fn settings_round_trip_through_bgb_ini() {
    // Non-default across every value type; model stays Auto (not persisted) and
    // dmg_palette unchanged (so scheme stays matched) — everything else must
    // survive to_ini -> from_ini exactly.
    let s = Settings {
        mono: true,
        volume: 0.5,
        lowercase_hex: true,
        show_clocks: false,
        rgbds_disasm: false,
        tile_hex_8bit: true,
        memory_window: true,
        break_ld_b_b: true,
        break_echo_ram: true,
        ff_speed: 5,
        framerate_limit: 30,
        bootrom_dmg: "dmg.bin".to_string(),
        esc_shows_debugger: false,
        allow_opposing: true,
        ..Settings::default()
    };
    let mut ini = Ini::parse("");
    to_ini(&s, &mut ini);
    let back = from_ini(&ini);
    assert_eq!(back, s, "mapped + slopgb-extra fields round-trip");
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
    assert_eq!(s.dmg_palette[0], 0x00E8_FCCC, "Color0=CCFCE8 (BGR) -> E8FCCC");
}
