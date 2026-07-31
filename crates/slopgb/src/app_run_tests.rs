use super::*;
use crate::cli::Options;
use crate::session::Session;
use slopgb_core::Model;

/// A blank no-ROM `App`, mirroring `main_tests::blank_app` (private to that
/// module, so this file needs its own copy).
fn blank_app() -> App {
    let opts = Options {
        rom: None,
        model: None,
        scale: 3,
        mute: true,
        boot: None,
        sgb_bios: None,
        mcp_port: None,
        plugins_dir: None,
        ram_init: None,
        plugin_flags: Vec::new(),
    };
    App::new(
        opts,
        Session::blank(Model::Dmg),
        false,
        None,
        None,
        slopgb_plugin_host::PluginRegistry::new(),
    )
}

#[test]
fn theme_toggle_flips_light_dark_without_touching_layout() {
    let mut app = blank_app();
    // Pin a known baseline independent of the developer's persisted config.
    app.settings.theme = ui::ThemeChoice::Light;
    let window_size_before = app.window_size;
    let last_scale_before = app.last_scale;

    app.toggle_theme_no_persist();
    assert_eq!(
        app.settings.theme,
        ui::ThemeChoice::Dark,
        "T flips Light -> Dark"
    );
    app.toggle_theme_no_persist();
    assert_eq!(
        app.settings.theme,
        ui::ThemeChoice::Light,
        "T flips back to Light"
    );

    // A pure colour-choice flip must never touch window geometry.
    assert_eq!(app.window_size, window_size_before, "window size untouched");
    assert_eq!(app.last_scale, last_scale_before, "last_scale untouched");
}

#[test]
fn theme_toggle_from_classic_or_custom_lands_on_dark() {
    let mut app = blank_app();
    app.settings.theme = ui::ThemeChoice::Classic;
    app.toggle_theme_no_persist();
    assert_eq!(app.settings.theme, ui::ThemeChoice::Dark);

    app.settings.theme = ui::ThemeChoice::Custom("solarized".to_string());
    app.toggle_theme_no_persist();
    assert_eq!(app.settings.theme, ui::ThemeChoice::Dark);
}
