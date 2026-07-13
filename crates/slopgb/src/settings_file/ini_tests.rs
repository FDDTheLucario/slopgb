use super::*;

// A CRLF bgb-flavored sample: bools, a repeated list key, an empty value, a
// value containing spaces, and a leading-'=' oddity.
const SAMPLE: &str = "Version=1060400\r\nSoundMono=0\r\nLastRomDir=\r\nColorScheme=a.b\r\nColorScheme=c.d\r\nJoyRapidRate=2 2\r\nColor0=CCFCE8\r\n";

#[test]
fn round_trips_byte_identical() {
    assert_eq!(
        Ini::parse(SAMPLE).serialize(),
        SAMPLE,
        "unmodified CRLF file is byte-identical"
    );
    // LF-only + no trailing newline also round-trips.
    let lf = "A=1\nB=2";
    assert_eq!(Ini::parse(lf).serialize(), lf);
}

#[test]
fn set_edits_in_place_and_appends() {
    let mut ini = Ini::parse(SAMPLE);
    ini.set("SoundMono", "1"); // existing -> edited in place
    ini.set("NewKey", "x"); // absent -> appended
    let out = ini.serialize();
    assert!(out.contains("SoundMono=1"));
    assert!(!out.contains("SoundMono=0"));
    // Appended as a new last line (with the file's trailing EOL preserved).
    assert!(out.ends_with("\r\nNewKey=x\r\n"));
    // Untouched lines survive verbatim.
    assert!(out.contains("JoyRapidRate=2 2"));
    assert!(out.contains("LastRomDir=\r\n"));
}

#[test]
fn get_reads_the_first_value() {
    let ini = Ini::parse(SAMPLE);
    assert_eq!(ini.get("Version"), Some("1060400"));
    assert_eq!(ini.get("LastRomDir"), Some(""));
    assert_eq!(ini.get("Missing"), None);
    assert_eq!(
        ini.get("ColorScheme"),
        Some("a.b"),
        "first of a repeated key"
    );
}

#[test]
fn bool_codec() {
    assert!(parse_bool("1"));
    assert!(!parse_bool("0"));
    assert!(!parse_bool(""));
    assert_eq!(fmt_bool(true), "1");
    assert_eq!(fmt_bool(false), "0");
}

#[test]
fn color_hex_bgr_swap_matches_the_real_palette() {
    // bgb Color0=CCFCE8 (BGR) is the E8FCCC pale-green our palette uses.
    assert_eq!(parse_color_hex("CCFCE8"), Some(0x00E8_FCCC));
    assert_eq!(
        fmt_color_hex(0x00E8_FCCC),
        "CCFCE8",
        "re-encode is symmetric"
    );
    assert_eq!(parse_color_hex("zzz"), None, "garbage -> None");
}

#[test]
fn real_bgb_ini_round_trips_byte_identical() {
    // The committed real bgb 1.6.4 fixture must survive parse->serialize verbatim
    // (the preserve-unknown-keys invariant over all ~250 keys).
    let real = include_str!("../../../../docs/bgb-reference/bgb.ini");
    assert_eq!(Ini::parse(real).serialize(), real);
}
