use super::*;
use crate::cli::Options;
use crate::session::Session;
use slopgb_core::Model;

/// A blank no-ROM `App`, mirroring `main_tests::blank_app` (private to that
/// module, so this file needs its own copy). Headless: `window` is `None`, so
/// `apply_window_size` reconciles the stretch/window-size state but performs no
/// actual resize. The blank ROM-only cart has no battery RAM, so `set_model`
/// never flushes a save file — nothing here touches disk.
fn blank_app() -> App {
    let opts = Options {
        rom: None,
        model: None,
        scale: 3,
        mute: true,
        boot: None,
        sgb_bios: None,
        mcp_port: None,
        ram_init: None,
    };
    App::new(opts, Session::blank(Model::Dmg), false, None, None)
}

/// The stretch <-> window-size reconciliation must hold both directions, and
/// exercising it must write nothing: `apply_settings_no_persist` is the
/// disk-free half of `apply_settings` (the `settings_file::save` side effect is
/// structurally excluded), so this whole test touches no file at all.
#[test]
fn stretch_and_window_size_reconcile_both_ways() {
    let mut app = blank_app();
    // Pin a known baseline independent of the developer's persisted config
    // (blank_app reads the real slopgb.conf; only these three fields matter and
    // App::new leaves window_size at the CLI scale regardless of saved stretch).
    app.settings.stretch = false;
    app.window_size = WindowSizeChoice::Scale(3);
    app.last_scale = 3;

    // Direction 1a: ticking "stretch" in Options reconciles the window size up
    // to fullscreen-stretched on Apply.
    app.settings.stretch = true;
    app.apply_settings_no_persist();
    assert_eq!(app.window_size, WindowSizeChoice::FullscreenStretched);
    assert!(app.settings.stretch);

    // Direction 1b: unticking it drops back to the *remembered* windowed scale
    // (last_scale), not the launch scale.
    app.settings.stretch = false;
    app.apply_settings_no_persist();
    assert_eq!(app.window_size, WindowSizeChoice::Scale(3));
    assert!(!app.settings.stretch);

    // Direction 2 -- the "previously fought itself" case: choosing
    // fullscreen-stretched via the Window-size menu writes `stretch` back, and a
    // later plain Apply (stretch unchanged) must NOT silently revert it to
    // windowed. This is the invariant the `apply_window_size` write-back guards.
    app.apply_window_size(WindowSizeChoice::FullscreenStretched);
    assert_eq!(app.window_size, WindowSizeChoice::FullscreenStretched);
    assert!(app.settings.stretch, "menu choice writes stretch back");
    app.apply_settings_no_persist();
    assert_eq!(
        app.window_size,
        WindowSizeChoice::FullscreenStretched,
        "a plain Apply must not revert a deliberate fullscreen-stretched choice"
    );
    assert!(app.settings.stretch);
}
