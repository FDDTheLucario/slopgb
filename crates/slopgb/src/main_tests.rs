use super::*;
use slopgb_core::Model;

/// A blank no-ROM App, as `main` builds it when launched without a ROM.
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
fn recovery_save_state_restores_a_crashed_session_and_clears_on_clean_quit() {
    let dir = std::env::temp_dir().join(format!("slopgb-recov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rom_path = dir.join("recov.gb");
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x00; // ROM ONLY
    std::fs::write(&rom_path, &rom).unwrap();

    // Load, stamp a WRAM marker, and force a recovery write (bypass the throttle).
    let mut app = blank_app();
    app.load_dropped(&rom_path);
    app.session.gb.debug_write(0xC000, 0xAB);
    app.recovery_next = std::time::Instant::now();
    app.write_recovery_state();
    assert!(
        app.recovery_path.as_ref().unwrap().exists(),
        "recovery file written"
    );

    // A crash = no clean quit → the file survives, so the next load restores it.
    let mut crashed = blank_app();
    crashed.load_dropped(&rom_path);
    assert_eq!(
        crashed.session.gb.debug_read(0xC000),
        0xAB,
        "restored the crashed machine's WRAM"
    );

    // A clean quit deletes the recovery, so the following load starts fresh.
    crashed.clear_recovery_state();
    let mut fresh = blank_app();
    fresh.load_dropped(&rom_path);
    assert_eq!(
        fresh.session.gb.debug_read(0xC000),
        0x00,
        "fresh machine after a clean quit"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_latency_frames_maps_the_slider_monotonically() {
    assert_eq!(audio_latency_frames(0.0), 128, "low end");
    assert_eq!(audio_latency_frames(1.0), 4096, "high end");
    assert!(audio_latency_frames(0.5) > 128 && audio_latency_frames(0.5) < 4096);
    // Out-of-range fractions clamp, never panic.
    assert_eq!(audio_latency_frames(-1.0), 128);
    assert_eq!(audio_latency_frames(2.0), 4096);
}

#[test]
fn should_poll_spins_for_turbo_or_reduce_cpu_off() {
    assert!(
        !should_poll(false, true),
        "normal run parks (reduce-cpu on)"
    );
    assert!(should_poll(true, true), "turbo always polls");
    assert!(should_poll(false, false), "reduce-cpu off spins");
    assert!(should_poll(true, false));
}

#[test]
fn cpu_usage_pct_is_the_non_halted_share() {
    assert_eq!(cpu_usage_pct(0, 0), 0.0, "no elapsed cycles → 0");
    assert_eq!(cpu_usage_pct(1000, 0), 100.0, "never halted → 100%");
    assert_eq!(cpu_usage_pct(1000, 1000), 0.0, "fully halted → 0%");
    assert_eq!(cpu_usage_pct(1000, 250), 75.0, "quarter halted → 75%");
    // halt can't exceed total, but a bad delta must not underflow/panic.
    assert_eq!(cpu_usage_pct(100, 200), 0.0, "saturates, no panic");
}

#[test]
fn rom_load_error_box_respects_the_show_errors_option() {
    // Off → no box (silent, console-only). On → a box carrying the message.
    assert_eq!(rom_load_error_box(false, "bad rom"), None);
    let b = rom_load_error_box(true, "bad rom").expect("box shown when enabled");
    assert_eq!(b.title, "ROM load failed");
    assert_eq!(b.lines, vec!["bad rom".to_string()]);
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
fn parse_host_port_handles_link_connect_forms() {
    let dp = crate::link::DEFAULT_PORT;
    assert_eq!(
        crate::link::parse_host_port("127.0.0.1:8765"),
        ("127.0.0.1".into(), 8765)
    );
    assert_eq!(
        crate::link::parse_host_port("localhost"),
        ("localhost".into(), dp)
    );
    // Bracketed IPv6 literal: brackets stripped, port honored — and the bare
    // bracketed form (no port) resolves to the inner address at the default.
    assert_eq!(
        crate::link::parse_host_port("[::1]:9000"),
        ("::1".into(), 9000)
    );
    assert_eq!(crate::link::parse_host_port("[::1]"), ("::1".into(), dp));
    assert_eq!(
        crate::link::parse_host_port("[fe80::1]"),
        ("fe80::1".into(), dp)
    );
    // Unparseable / overflowing port → default; never panics.
    assert_eq!(
        crate::link::parse_host_port("host:notaport"),
        ("host".into(), dp)
    );
    assert_eq!(crate::link::parse_host_port("h:99999"), ("h".into(), dp));
    assert_eq!(crate::link::parse_host_port(""), (String::new(), dp));
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
fn default_button_keys_resolve_through_app_bindings_and_drive_the_core() {
    // JP2: the App owns the rebindable map; every default button key reverse-
    // resolves to its button and `set_button` drives the real machine without
    // panicking (press then release, tracked per key).
    let mut app = blank_app();
    for b in keymap::WIZARD_ORDER {
        let code = app.bindings.key_for(b).expect("default binding");
        assert_eq!(app.bindings.button_for(code), Some(b), "round-trips");
        app.set_button(code, b, true);
        app.apply_pending_input();
        app.set_button(code, b, false);
        app.apply_pending_input();
    }
}

#[test]
fn key_wizard_commits_new_bindings_when_it_finishes() {
    // JP6: open the wizard, bind each of the 8 buttons to a fresh key (mirroring
    // handle_key's capture), and confirm the live map is rebound + the wizard
    // closes.
    let mut app = blank_app();
    app.open_key_wizard();
    assert!(app.key_wizard.is_some());
    let codes = [
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyK,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyL,
    ];
    for &code in &codes {
        app.key_wizard.as_mut().unwrap().bind_key(code);
        app.commit_wizard_if_done();
    }
    assert!(app.key_wizard.is_none(), "wizard closed after 8 binds");
    for (b, &code) in keymap::WIZARD_ORDER.iter().zip(&codes) {
        assert_eq!(app.bindings.button_for(code), Some(*b));
    }
}

#[test]
fn socd_filter_suppresses_then_resurrects_the_opposite_direction() {
    // JP7 + review fix: with the filter ON (default), the joypad never reports
    // both L+R; releasing the newer direction returns to the still-held older
    // one (last-input priority).
    let mut app = blank_app();
    assert!(!app.settings.allow_opposing, "filter on by default");
    let left = app.bindings.key_for(Button::Left).unwrap();
    let right = app.bindings.key_for(Button::Right).unwrap();

    // Input is deferred to a sub-frame offset, so flush it after each press.
    app.set_button(left, Button::Left, true);
    app.apply_pending_input();
    assert!(app.session.gb.debug_button(Button::Left));
    // Press Right while Left is held → Left suppressed, only Right reported.
    app.set_button(right, Button::Right, true);
    app.apply_pending_input();
    assert!(app.session.gb.debug_button(Button::Right));
    assert!(!app.session.gb.debug_button(Button::Left), "L+R never both");
    // Release Right → Left resurrects (its key is still held).
    app.set_button(right, Button::Right, false);
    app.apply_pending_input();
    assert!(
        app.session.gb.debug_button(Button::Left),
        "older resurrects"
    );
    assert!(!app.session.gb.debug_button(Button::Right));
}

#[test]
fn allow_opposing_lets_both_directions_register() {
    let mut app = blank_app();
    app.settings.allow_opposing = true;
    let up = app.bindings.key_for(Button::Up).unwrap();
    let down = app.bindings.key_for(Button::Down).unwrap();
    app.set_button(up, Button::Up, true);
    app.apply_pending_input();
    app.set_button(down, Button::Down, true);
    app.apply_pending_input();
    assert!(app.session.gb.debug_button(Button::Up));
    assert!(
        app.session.gb.debug_button(Button::Down),
        "filter off = both"
    );
}

#[test]
fn idle_input_drops_presses_but_honors_releases() {
    // While frozen, a press shouldn't register, but a release must still apply
    // so a button released while paused doesn't stick held on resume.
    let mut app = blank_app();
    let a = app.bindings.key_for(Button::A).unwrap();
    app.set_button(a, Button::A, true);
    app.apply_pending_input();
    assert!(app.session.gb.debug_button(Button::A), "held while running");
    // Release queued while idle is honored (no stuck button).
    app.set_button(a, Button::A, false);
    app.flush_idle_input();
    assert!(
        !app.session.gb.debug_button(Button::A),
        "release honored while idle"
    );
    // A press queued while idle is dropped (frozen machine).
    app.set_button(a, Button::A, true);
    app.flush_idle_input();
    assert!(
        !app.session.gb.debug_button(Button::A),
        "press dropped while idle"
    );
}

#[test]
fn key_wizard_cancel_leaves_bindings_unchanged() {
    let mut app = blank_app();
    let before = app.bindings;
    app.open_key_wizard();
    app.key_wizard.as_mut().unwrap().bind_key(KeyCode::KeyM); // right := M (uncommitted)
    // Cancel = drop the wizard without running to completion.
    app.key_wizard = None;
    assert_eq!(app.bindings, before, "an aborted wizard discards its edits");
}

#[test]
fn apply_exceptions_pushes_the_mask_to_the_machine() {
    use slopgb_core::{EXC_ECHO_RAM, EXC_INVALID_OPCODE};
    // App::new already armed the default mask (bgb's "break on invalid opcode").
    let mut app = blank_app();
    assert_eq!(
        app.session.gb.exceptions(),
        EXC_INVALID_OPCODE,
        "default invalid-opcode break armed at startup"
    );
    // Changing settings + applying pushes the new mask to the live machine.
    app.settings.break_invalid_op = false;
    app.settings.break_echo_ram = true;
    app.apply_exceptions();
    assert_eq!(app.session.gb.exceptions(), EXC_ECHO_RAM);
    // Disarming everything is golden-safe (mask 0).
    app.settings.break_echo_ram = false;
    app.apply_exceptions();
    assert_eq!(app.session.gb.exceptions(), 0);
}

#[test]
fn debugger_copy_text_matches_memory_and_disasm() {
    // RM10: "Copy data" yields the 16 hex bytes at the address (matching
    // debug_read); "Copy code" yields disasm lines, the first tagged with the
    // address.
    let app = blank_app();
    let addr = 0x0100u16;
    let data = app.tools.debugger_copy_text(&app.session.gb, addr, false);
    let expect: String = (0..16u16)
        .map(|i| format!("{:02X}", app.session.gb.debug_read(addr.wrapping_add(i))))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(data, expect);
    assert_eq!(data.split(' ').count(), 16, "16 bytes");

    let code = app.tools.debugger_copy_text(&app.session.gb, addr, true);
    let first = code.lines().next().expect("at least one disasm line");
    assert!(
        first.contains("0100"),
        "first disasm line tags the addr: {first}"
    );
}

#[test]
fn clipboard_candidates_are_dep_free_tools() {
    // RM10 is implemented without a clipboard *crate* — only std shell-outs.
    let c = crate::clipboard::clipboard_candidates();
    assert_eq!(c.len(), 3);
    assert!(c.iter().all(|(prog, _)| !prog.is_empty()));
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
