use super::*;

/// Rec.709 relative luminance (0..1), no gamma correction — plenty precise
/// for a "clearly lighter/darker" contrast threshold check.
fn luminance(c: u32) -> f64 {
    let r = f64::from((c >> 16) & 0xFF);
    let g = f64::from((c >> 8) & 0xFF);
    let b = f64::from(c & 0xFF);
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
}

const CONTRAST_THRESHOLD: f64 = 0.3;

// --- Task 1: additive roles, BGB preserved ----------------------------------

#[test]
fn bgb_original_seven_are_unchanged_and_new_roles_are_present() {
    let t = Theme::BGB;
    // The original 7 (bgb.ini defaults, unchanged by adding new roles).
    assert_eq!(t.bg, 0x00FF_FFFF);
    assert_eq!(t.text, 0x0000_0000);
    assert_eq!(t.current, 0x0000_00FF);
    assert_eq!(t.breakpoint, 0x00FF_0000);
    assert_eq!(t.hilight, 0x0080_8080);
    assert_eq!(t.freeze, 0x00FF_FF00);
    assert_eq!(t.border, 0x0080_8080);
    // New roles are present and, for BGB, reproduce the value the old draw
    // code used at each call site (so pointing a call site at the new role
    // is pixel-identical to before).
    assert_eq!(t.panel, t.bg);
    assert_eq!(t.button_face, t.bg);
    assert_eq!(t.accent, t.text);
    assert_eq!(t.selection_bg, t.current);
    assert_eq!(t.selection_fg, t.bg);
    assert_eq!(t.disabled_text, t.hilight);
    assert_eq!(t.scrollbar, t.hilight);
}

// --- Task 2: CLASSIC ---------------------------------------------------------

#[test]
fn classic_equals_bgb() {
    assert_eq!(Theme::CLASSIC, Theme::BGB);
}

// --- Task 3: LIGHT -----------------------------------------------------------

#[test]
fn light_is_a_high_contrast_light_palette_distinct_from_classic() {
    let t = Theme::LIGHT;
    assert!(
        luminance(t.bg) > luminance(t.text) + CONTRAST_THRESHOLD,
        "bg must be clearly lighter than text"
    );
    assert_ne!(t, Theme::CLASSIC);
    assert_ne!(t, Theme::BGB);
}

// --- Task 4: DARK -------------------------------------------------------------

#[test]
fn dark_is_a_high_contrast_dark_palette_distinct_from_classic_and_light() {
    let t = Theme::DARK;
    assert!(
        luminance(t.text) > luminance(t.bg) + CONTRAST_THRESHOLD,
        "text must be clearly lighter than bg"
    );
    assert_ne!(t, Theme::CLASSIC);
    assert_ne!(t, Theme::LIGHT);
}

// --- Task 5: ThemeChoice + resolve -------------------------------------------

#[test]
fn default_theme_choice_resolves_to_light() {
    assert_eq!(ThemeChoice::default(), ThemeChoice::Light);
    let resolved = ThemeChoice::default().resolve(&CustomThemes::default());
    assert_eq!(resolved, Theme::LIGHT);
}

#[test]
fn classic_choice_resolves_byte_identical_to_bgb() {
    let resolved = ThemeChoice::Classic.resolve(&CustomThemes::default());
    assert_eq!(resolved, Theme::BGB);
}

// --- Task 8: theming API (from_pairs) ----------------------------------------

#[test]
fn from_pairs_overrides_named_roles_and_defaults_the_rest() {
    let t = Theme::from_pairs(&[("bg", "0x112233"), ("text", "0xAABBCC")]).unwrap();
    assert_eq!(t.bg, 0x0011_2233);
    assert_eq!(t.text, 0x00AA_BBCC);
    // Untouched roles fall back to the LIGHT base default.
    assert_eq!(t.current, Theme::LIGHT.current);
    assert_eq!(t.scrollbar, Theme::LIGHT.scrollbar);
}

#[test]
fn from_pairs_accepts_bare_hex_and_is_case_insensitive() {
    let t = Theme::from_pairs(&[("bg", "aabbcc")]).unwrap();
    assert_eq!(t.bg, 0x00AA_BBCC);
}

#[test]
fn from_pairs_rejects_unknown_role_without_panicking() {
    let err = Theme::from_pairs(&[("not_a_role", "0x000000")]).unwrap_err();
    assert_eq!(err, ThemeParseError::UnknownRole("not_a_role".to_string()));
}

#[test]
fn from_pairs_rejects_bad_hex_without_panicking() {
    let err = Theme::from_pairs(&[("bg", "purple")]).unwrap_err();
    assert_eq!(
        err,
        ThemeParseError::BadValue {
            role: "bg".to_string(),
            value: "purple".to_string(),
        }
    );
    // Display never panics either.
    assert!(err.to_string().contains("bg"));
}

// --- Task 9: custom theme registry -------------------------------------------

#[test]
fn theme_choice_custom_resolves_a_registered_theme() {
    let mut custom = CustomThemes::default();
    let solarized = Theme::from_pairs(&[("bg", "0x002B36"), ("text", "0x93A1A1")]).unwrap();
    custom.insert("solarized", solarized);
    let resolved = ThemeChoice::Custom("solarized".to_string()).resolve(&custom);
    assert_eq!(resolved, solarized);
}

#[test]
fn theme_choice_custom_falls_back_to_default_when_unregistered() {
    let custom = CustomThemes::default();
    let resolved = ThemeChoice::Custom("nonexistent".to_string()).resolve(&custom);
    assert_eq!(resolved, Theme::LIGHT, "unknown custom name falls back");
}

#[test]
fn theme_choice_key_round_trips_every_variant() {
    for choice in [
        ThemeChoice::Light,
        ThemeChoice::Dark,
        ThemeChoice::Classic,
        ThemeChoice::Custom("my-theme".to_string()),
    ] {
        assert_eq!(ThemeChoice::from_key(&choice.to_key()), choice);
    }
}

#[test]
fn empty_named_custom_encodes_as_the_default_key_not_a_lossy_custom() {
    // `Custom("")` isn't reachable through `from_key` (which maps a bare
    // `"custom:"` straight to `default()`), but a direct construction must
    // still encode/decode consistently rather than round-tripping to a
    // different `Custom("")` on the next load.
    let choice = ThemeChoice::Custom(String::new());
    assert_eq!(choice.to_key(), ThemeChoice::default().to_key());
    assert_eq!(
        ThemeChoice::from_key(&choice.to_key()),
        ThemeChoice::default()
    );
}

#[test]
fn theme_choice_from_key_falls_back_on_garbage() {
    assert_eq!(ThemeChoice::from_key(""), ThemeChoice::default());
    assert_eq!(ThemeChoice::from_key("bogus"), ThemeChoice::default());
    assert_eq!(ThemeChoice::from_key("custom:"), ThemeChoice::default());
}

// --- Task 10: LAYOUT-INVARIANCE guard ----------------------------------------

#[test]
fn theme_swap_only_recolors_a_whole_window_never_moves_it() {
    // A representative real window (the Options dialog) exercises tabs,
    // checkboxes, radios, a dropdown, a slider, and the OK/Cancel/Apply/
    // Defaults buttons — nearly every shared widget primitive — in one call.
    use crate::windows::options::{OptionsState, Settings};

    const W: usize = 480;
    const H: usize = 400;
    let state = OptionsState::new(Settings::default());

    let mut geoms = Vec::new();
    let mut pixels = Vec::new();
    for theme in [Theme::LIGHT, Theme::DARK, Theme::CLASSIC] {
        let mut buf = vec![0u32; W * H];
        {
            let mut c = crate::ui::canvas::Canvas::new_recording(&mut buf, W, H);
            crate::windows::options::render(&mut c, &state, &theme);
            geoms.push(c.drawn().to_vec());
        }
        pixels.push(buf);
    }
    assert_eq!(
        geoms[0], geoms[1],
        "LIGHT vs DARK must draw the identical set of rects"
    );
    assert_eq!(
        geoms[0], geoms[2],
        "LIGHT vs CLASSIC must draw the identical set of rects"
    );
    assert_ne!(pixels[0], pixels[1], "LIGHT vs DARK pixels must differ");
    assert_ne!(pixels[0], pixels[2], "LIGHT vs CLASSIC pixels must differ");
}

// --- Task 11: LIGHT is the default look --------------------------------------

#[test]
fn a_fresh_settings_choice_draws_light_not_classic() {
    // `Settings::default().theme` (task 11's "no-config app") must resolve to
    // LIGHT; CLASSIC remains selectable and reproduces the exact old pixels
    // (already proven by `classic_choice_resolves_byte_identical_to_bgb`).
    let theme = crate::windows::options::Settings::default()
        .theme
        .resolve(&CustomThemes::default());
    assert_eq!(theme, Theme::LIGHT);
}
