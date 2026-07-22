use super::tabs::{Field, Kind, ThemeRadio, controls};
use super::*;

const T: Theme = Theme::BGB;
const BOUNDS: Rect = Rect::new(0, 0, 500, 420);

fn dialog() -> Rect {
    OptionsState::dialog_rect(BOUNDS)
}

/// Find the live control driving `field` on `tab`, return its hit-rect.
fn field_rect(tab: OptionsTab, s: &Settings, field: Field) -> Rect {
    let content = OptionsState::content_rect(dialog());
    controls(tab, s, content)
        .into_iter()
        .find(|c| c.field == Some(field))
        .unwrap_or_else(|| panic!("no live control for {field:?} on {tab:?}"))
        .rect
}

/// Click the centre of the control driving `field`.
fn click_field(st: &mut OptionsState, field: Field) {
    let r = field_rect(st.active, &st.working, field);
    st.on_click(r.x + r.w / 2, r.y + r.h / 2, BOUNDS);
}

// --- Task 9: System-tab bootrom controls ------------------------------------

#[test]
fn system_tab_has_bootrom_controls() {
    // The System tab carries the three labeled bootrom path fields' "..." browse
    // buttons (live PickBootrom) and the "bootroms enabled" checkbox, matching
    // options-system.png. A path box shows the configured path.
    let s = Settings {
        bootrom_dmg: "boot.bin".into(),
        ..Settings::default()
    };
    let content = OptionsState::content_rect(dialog());
    let ctrls = controls(OptionsTab::System, &s, content);
    for slot in [BootromSlot::Dmg, BootromSlot::Gbc, BootromSlot::Sgb] {
        assert!(
            ctrls
                .iter()
                .any(|c| c.field == Some(Field::PickBootrom(slot))),
            "missing {slot:?} browse button"
        );
    }
    assert!(
        ctrls
            .iter()
            .any(|c| c.field == Some(Field::BootromsEnabled)),
        "missing bootroms-enabled checkbox"
    );
    // The DMG path box renders the configured path.
    assert!(
        ctrls.iter().any(|c| matches!(&c.kind,
            super::tabs::Kind::Dropdown { value, .. } if value == "boot.bin")),
        "DMG path box shows the path"
    );
}

// --- Plugins tab ------------------------------------------------------------

fn settings_with_plugins() -> Settings {
    Settings {
        plugins: PluginConfig {
            dir: "/opt/plugins".into(),
            allow_mutation: false,
            entries: vec![
                PluginEntry {
                    name: "framecounter".into(),
                    capabilities: "introspection".into(),
                    enabled: true,
                },
                PluginEntry {
                    name: "tracer".into(),
                    capabilities: "introspection".into(),
                    enabled: false,
                },
            ],
        },
        ..Settings::default()
    }
}

#[test]
fn plugins_tab_lists_entries_dir_and_toggle() {
    let s = settings_with_plugins();
    let content = OptionsState::content_rect(dialog());
    let ctrls = controls(OptionsTab::Plugins, &s, content);

    // The allow-mutation toggle is present + live.
    assert!(
        ctrls
            .iter()
            .any(|c| c.field == Some(Field::PluginAllowMutation)),
        "missing allow-mutation toggle"
    );
    // One enable checkbox per plugin, each mapped to the right index.
    for i in 0..s.plugins.entries.len() {
        let ctrl = ctrls
            .iter()
            .find(|c| c.field == Some(Field::PluginEnable(i)))
            .unwrap_or_else(|| panic!("missing enable checkbox for plugin {i}"));
        let checked = matches!(ctrl.kind, Kind::Check { checked, .. } if checked);
        assert_eq!(checked, s.plugins.entries[i].enabled, "checkbox {i} state");
    }
    // The plugins dir is shown somewhere (an inert label).
    assert!(
        ctrls.iter().any(|c| matches!(&c.kind,
            Kind::Label { text } if text.contains("/opt/plugins"))),
        "missing plugins-dir display"
    );
    // A "..." browse button changes the plugins dir.
    assert!(
        ctrls.iter().any(|c| c.field == Some(Field::PickPluginsDir)),
        "missing plugins-dir browse button"
    );
}

#[test]
fn plugins_dir_browse_routes_the_outcome_without_mutating() {
    let mut st = OptionsState::new(settings_with_plugins());
    st.active = OptionsTab::Plugins;
    let before = st.working.clone();
    let r = field_rect(OptionsTab::Plugins, &st.working, Field::PickPluginsDir);
    let out = st.on_click(r.x + r.w / 2, r.y + r.h / 2, BOUNDS);
    assert_eq!(out, Some(OptionsOutcome::PickPluginsDir));
    assert_eq!(st.working, before, "browse doesn't mutate settings");
}

#[test]
fn plugins_tab_enable_checkbox_toggles_entry() {
    let mut st = OptionsState::new(settings_with_plugins());
    st.active = OptionsTab::Plugins;
    assert!(st.working.plugins.entries[0].enabled);
    click_field(&mut st, Field::PluginEnable(0));
    assert!(
        !st.working.plugins.entries[0].enabled,
        "click must toggle it"
    );
}

#[test]
fn bootrom_checkbox_toggles_and_browse_routes_outcome() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::System;
    // The "bootroms enabled" checkbox flips the working setting.
    assert!(!st.working.bootroms_enabled);
    click_field(&mut st, Field::BootromsEnabled);
    assert!(st.working.bootroms_enabled, "checkbox toggled the flag");
    // A "..." browse button routes the PickBootrom outcome without mutating.
    let before = st.working.clone();
    let r = field_rect(
        OptionsTab::System,
        &st.working,
        Field::PickBootrom(BootromSlot::Gbc),
    );
    let out = st.on_click(r.x + r.w / 2, r.y + r.h / 2, BOUNDS);
    assert_eq!(out, Some(OptionsOutcome::PickBootrom(BootromSlot::Gbc)));
    assert_eq!(st.working, before, "browse doesn't mutate settings");
}

#[test]
fn sound_volume_slider_full_range() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    let r = field_rect(OptionsTab::Sound, &st.working, Field::Volume);
    st.on_click(r.right() - 1, r.y + r.h / 2, BOUNDS);
    assert!(
        st.working.volume > 0.9,
        "right edge = loud, got {}",
        st.working.volume
    );
}

#[test]
fn sound_tab_device_rate_latency_and_format_controls_are_live() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    // Samplerate radio: 48000 selects that rate.
    click_field(&mut st, Field::SampleRate(48000));
    assert_eq!(st.working.audio_sample_rate, 48000);
    // 8-bit + HQ checkboxes.
    assert!(!st.working.audio_8bit);
    click_field(&mut st, Field::EightBit);
    assert!(st.working.audio_8bit);
    assert!(st.working.audio_hq, "HQ on by default");
    click_field(&mut st, Field::AudioHq);
    assert!(!st.working.audio_hq);
    // Latency slider tracks the click x.
    let r = field_rect(OptionsTab::Sound, &st.working, Field::Latency);
    st.on_click(r.right() - 1, r.y + r.h / 2, BOUNDS);
    assert!(st.working.audio_latency > 0.9, "latency slider right edge");
    // Soundcard dropdown routes the cycle outcome without mutating settings here.
    let before = st.working.clone();
    let r = field_rect(OptionsTab::Sound, &st.working, Field::SoundCard);
    let out = st.on_click(r.x + r.w / 2, r.y + r.h / 2, BOUNDS);
    assert_eq!(out, Some(OptionsOutcome::CycleSoundcard));
    assert_eq!(st.working, before, "cycle is applied by the App, not here");
}

// --- JP5: configure-keyboard routes an outcome -------------------------------

#[test]
fn configure_keyboard_button_routes_outcome_without_applying_or_closing() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    let before = st.working.clone();
    let r = field_rect(OptionsTab::Joypad, &st.working, Field::ConfigureKeyboard);
    let out = st.on_click(r.x + r.w / 2, r.y + r.h / 2, BOUNDS);
    assert_eq!(out, Some(OptionsOutcome::ConfigureKeyboard));
    // It opens the wizard, not a settings change, and never closes the dialog.
    assert_eq!(st.working, before, "no settings mutated");
    assert!(!OptionsOutcome::ConfigureKeyboard.applies());
    assert!(!OptionsOutcome::ConfigureKeyboard.closes());
}

// --- Tasks 6-13: per-tab live control hit-tests ------------------------------

#[test]
fn system_tab_model_radios() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::System;
    click_field(&mut st, Field::Model(ModelChoice::Cgb));
    assert_eq!(st.working.model, ModelChoice::Cgb);
    click_field(&mut st, Field::Model(ModelChoice::Dmg));
    assert_eq!(st.working.model, ModelChoice::Dmg);
}

#[test]
fn debug_tab_display_flags_toggle() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Debug;
    // "lowercase disassembler" on by default → click flips it (mnemonic case).
    assert!(st.working.lowercase_disasm);
    click_field(&mut st, Field::LowercaseDisasm);
    assert!(!st.working.lowercase_disasm);
    click_field(&mut st, Field::LowercaseHex);
    assert!(st.working.lowercase_hex);
    click_field(&mut st, Field::ShowClocks);
    assert!(!st.working.show_clocks);
    // "pressing Esc shows debugger" is live (BUG-1): on by default, click flips it.
    assert!(
        st.working.esc_shows_debugger,
        "esc-shows-debugger on by default"
    );
    click_field(&mut st, Field::EscShowsDebugger);
    assert!(!st.working.esc_shows_debugger);
    // RGBDS syntax + memory-window are the slopgb-departure toggles.
    assert!(st.working.rgbds_disasm, "rgbds on by default");
    click_field(&mut st, Field::RgbdsDisasm);
    assert!(!st.working.rgbds_disasm);
    // "8-bit tile hex" is off by default; the click turns it on.
    assert!(!st.working.tile_hex_8bit, "8-bit tile hex off by default");
    click_field(&mut st, Field::TileHex8bit);
    assert!(st.working.tile_hex_8bit);
    // "Registers can be edited" on by default (bgb) → click flips off.
    assert!(st.working.registers_editable);
    click_field(&mut st, Field::RegistersEditable);
    assert!(!st.working.registers_editable);
    // "Start in debugger" off by default → click turns it on.
    assert!(!st.working.start_in_debugger);
    click_field(&mut st, Field::StartInDebugger);
    assert!(st.working.start_in_debugger);
    // "Live update memory viewer" on by default → click turns it off.
    assert!(st.working.mem_live_update);
    click_field(&mut st, Field::MemLiveUpdate);
    assert!(!st.working.mem_live_update);
    // "GB CPU usage meter" off by default → click turns it on.
    assert!(!st.working.cpu_usage_meter);
    click_field(&mut st, Field::CpuUsageMeter);
    assert!(st.working.cpu_usage_meter);
}

#[test]
fn debug_tab_defaults_restores_esc_shows_debugger() {
    // Turning the toggle off then pressing Defaults on the Debug tab restores it
    // to the bgb-faithful default (true). BUG-1.
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Debug;
    st.working.esc_shows_debugger = false;
    st.press(OptionsButton::Defaults);
    assert!(
        st.working.esc_shows_debugger,
        "Defaults restores esc-shows-debugger"
    );
}

#[test]
fn debug_tab_syntax_is_a_single_live_dropdown() {
    // Exactly one live control drives the disasm syntax — the dropdown (no
    // duplicate "RGBDS syntax" checkbox), and it shows the live value.
    let s = Settings::default();
    let content = OptionsState::content_rect(dialog());
    let ctrls = controls(OptionsTab::Debug, &s, content);
    let syntax: Vec<_> = ctrls
        .iter()
        .filter(|c| c.field == Some(Field::RgbdsDisasm))
        .collect();
    assert_eq!(syntax.len(), 1, "one syntax control, not two");
    match &syntax[0].kind {
        Kind::Dropdown { value, .. } => assert_eq!(value, "rgbds", "default rgbds shown"),
        other => panic!("syntax control should be a dropdown, got {other:?}"),
    }
}

#[test]
fn debug_tab_pure_bgb_toggles_every_departure() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Debug;
    // Defaults are slopgb-flavored (rgbds on); enabling pure-bgb flips them off.
    st.working.memory_window = true;
    st.working.tile_hex_8bit = true;
    click_field(&mut st, Field::PureBgb);
    assert!(!st.working.rgbds_disasm, "pure bgb -> bgb disasm syntax");
    assert!(
        !st.working.memory_window,
        "pure bgb -> integrated memory pane"
    );
    assert!(
        !st.working.tile_hex_8bit,
        "pure bgb -> full tile hex ($17F)"
    );
    // Toggling it again restores the slopgb defaults.
    click_field(&mut st, Field::PureBgb);
    assert!(st.working.rgbds_disasm, "back to slopgb defaults");
}

#[test]
fn sound_tab_volume_and_mono() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    // click near the left of the volume slider → low volume
    let r = field_rect(OptionsTab::Sound, &st.working, Field::Volume);
    st.on_click(r.x + 2, r.y + r.h / 2, BOUNDS);
    assert!(
        st.working.volume < 0.1,
        "left click = quiet, got {}",
        st.working.volume
    );
    click_field(&mut st, Field::Mono);
    assert!(st.working.mono);
}

#[test]
fn graphics_tab_stretch_toggle() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Graphics;
    click_field(&mut st, Field::Stretch);
    assert!(st.working.stretch);
}

#[test]
fn graphics_disable_sgb_colors_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Graphics;
    assert!(!st.working.disable_sgb_colors);
    click_field(&mut st, Field::DisableSgbColors);
    assert!(st.working.disable_sgb_colors);
}

#[test]
fn graphics_frame_blend_dropdown_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Graphics;
    assert!(!st.working.frame_blend);
    click_field(&mut st, Field::FrameBlend);
    assert!(st.working.frame_blend, "frame blend on after click");
    // "doubler" dropdown cycles off ↔ scale2x.
    assert!(!st.working.doubler);
    click_field(&mut st, Field::Doubler);
    assert!(st.working.doubler);
}

#[test]
fn graphics_sgb_border_screenshot_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Graphics;
    assert!(!st.working.sgb_border_screenshot);
    click_field(&mut st, Field::SgbBorderScreenshot);
    assert!(st.working.sgb_border_screenshot);
}

#[test]
fn joypad_screenshot_format_dropdown_cycles() {
    use crate::windows::options::ScreenshotFormat;
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    assert_eq!(st.working.screenshot_format, ScreenshotFormat::Bmp);
    click_field(&mut st, Field::ScreenshotFormat);
    assert_eq!(st.working.screenshot_format, ScreenshotFormat::Png);
    click_field(&mut st, Field::ScreenshotFormat);
    assert_eq!(st.working.screenshot_format, ScreenshotFormat::Bmp);
}

#[test]
fn joypad_screenshot_button_mode_dropdown_cycles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    assert!(!st.working.screenshot_copies, "saves-to-file by default");
    click_field(&mut st, Field::ScreenshotButtonMode);
    assert!(
        st.working.screenshot_copies,
        "cycles to copies-to-clipboard"
    );
    click_field(&mut st, Field::ScreenshotButtonMode);
    assert!(!st.working.screenshot_copies);
}

#[test]
fn gbcolors_dmg_gbc_lcd_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::GbColors;
    assert!(!st.working.dmg_gbc_lcd);
    click_field(&mut st, Field::DmgGbcLcd);
    assert!(st.working.dmg_gbc_lcd);
}

#[test]
fn gbcolors_contrast_slider_tracks_click_position() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::GbColors;
    let r = field_rect(OptionsTab::GbColors, &st.working, Field::Contrast);
    // Click near the right end → a high contrast fraction (default is 0.5).
    st.on_click(r.x + r.w - 2, r.y + r.h / 2, BOUNDS);
    assert!(
        st.working.contrast > 0.9,
        "contrast slider tracks click x: {}",
        st.working.contrast
    );
}

#[test]
fn exceptions_tab_break_conditions_are_live() {
    // These break conditions are wired to the core exception-break mask; the
    // rest of the tab stays faithfully inert (no clean detector / no backend).
    let s = Settings::default();
    let content = OptionsState::content_rect(dialog());
    let live: Vec<_> = controls(OptionsTab::Exceptions, &s, content)
        .into_iter()
        .filter_map(|c| c.field)
        .collect();
    for f in [
        Field::BreakLdBB,
        Field::BreakInvalidOp,
        Field::BreakEchoRam,
        Field::BreakLcdOffVblank,
        Field::BreakOamDmaBad,
        Field::BreakIncDecFexx,
        Field::BreakSgbTransfer,
    ] {
        assert!(live.contains(&f), "{f:?} is live");
    }
    assert_eq!(
        live.len(),
        7,
        "all seven break conditions are live (greyed accuracy locks aside)"
    );
}

#[test]
fn exceptions_tab_toggles_flip_settings() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Exceptions;
    // invalid-opcode starts checked (bgb default) — a click clears it.
    assert!(st.working.break_invalid_op);
    click_field(&mut st, Field::BreakInvalidOp);
    assert!(!st.working.break_invalid_op);
    // the other three start off — a click arms them.
    click_field(&mut st, Field::BreakLdBB);
    click_field(&mut st, Field::BreakEchoRam);
    click_field(&mut st, Field::BreakLcdOffVblank);
    assert!(st.working.break_ld_b_b);
    assert!(st.working.break_echo_ram);
    assert!(st.working.break_lcd_off_vblank);
}

#[test]
fn gbcolors_scheme_cycles_palette() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::GbColors;
    assert_eq!(st.working.scheme, 0);
    click_field(&mut st, Field::SchemeCycle);
    assert_eq!(st.working.scheme, 1);
    assert_eq!(st.working.dmg_palette, SCHEMES[1].colors);
    // Cycle the rest of the way round — the wrap must return to scheme 0.
    for _ in 1..SCHEMES.len() {
        click_field(&mut st, Field::SchemeCycle);
    }
    assert_eq!(st.working.scheme, 0, "scheme wraps back to 0");
    assert_eq!(st.working.dmg_palette, SCHEMES[0].colors);
}

#[test]
fn theme_tab_radios_select_theme() {
    // The Theme tab exposes the three built-in colour themes as live radios;
    // clicking one sets `working.theme` (persisted via the usual OK/Apply flow).
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Theme;
    // The Dark radio is a live control on the tab.
    let content = OptionsState::content_rect(dialog());
    assert!(
        controls(OptionsTab::Theme, &st.working, content)
            .iter()
            .any(|c| c.field == Some(Field::Theme(ThemeRadio::Dark))),
        "Theme tab has a live Dark radio"
    );
    // Default is Light; clicking Dark → Classic → Light round-trips the choice.
    assert_eq!(st.working.theme, ThemeChoice::Light);
    click_field(&mut st, Field::Theme(ThemeRadio::Dark));
    assert_eq!(st.working.theme, ThemeChoice::Dark);
    click_field(&mut st, Field::Theme(ThemeRadio::Classic));
    assert_eq!(st.working.theme, ThemeChoice::Classic);
    click_field(&mut st, Field::Theme(ThemeRadio::Light));
    assert_eq!(st.working.theme, ThemeChoice::Light);
}

#[test]
fn misc_tab_live_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Misc;
    click_field(&mut st, Field::ShowFramerate);
    assert!(st.working.show_framerate);
    click_field(&mut st, Field::FreezeRecent);
    assert!(st.working.freeze_recent);
    click_field(&mut st, Field::PauseOnFocusLoss);
    assert!(st.working.pause_on_focus_loss);
    // "Show errors on ROM load" defaults on (bgb-faithful) → click turns it off.
    assert!(st.working.show_errors_on_rom_load);
    click_field(&mut st, Field::ShowErrorsOnRomLoad);
    assert!(!st.working.show_errors_on_rom_load);
    assert!(!st.working.load_rom_dialog_on_startup);
    click_field(&mut st, Field::LoadRomDialogOnStartup);
    assert!(st.working.load_rom_dialog_on_startup);
    // "reduce CPU usage" on by default → click turns it off.
    assert!(st.working.reduce_cpu);
    click_field(&mut st, Field::ReduceCpu);
    assert!(!st.working.reduce_cpu);
    // "Recovery save state" on by default → click turns it off.
    assert!(st.working.recovery_save_state);
    click_field(&mut st, Field::RecoverySaveState);
    assert!(!st.working.recovery_save_state);
}

#[test]
fn misc_tab_pacing_sliders() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Misc;
    // Right edge of the fast-forward slider → the max speed.
    let r = field_rect(OptionsTab::Misc, &st.working, Field::FfSpeed);
    st.on_click(r.right() - 1, r.y + r.h / 2, BOUNDS);
    assert_eq!(st.working.ff_speed, 20, "ff slider right edge = max");
    // Left edge → minimum (never 0).
    let r = field_rect(OptionsTab::Misc, &st.working, Field::FfSpeed);
    st.on_click(r.x, r.y + r.h / 2, BOUNDS);
    assert_eq!(st.working.ff_speed, 1, "ff slider left edge = 1");
    // Framerate slider right edge → the top discrete step.
    let r = field_rect(OptionsTab::Misc, &st.working, Field::FramerateLimit);
    st.on_click(r.right() - 1, r.y + r.h / 2, BOUNDS);
    assert_eq!(
        st.working.framerate_limit, 300,
        "framerate slider right edge"
    );
    // Left edge → 0 (real speed).
    let r = field_rect(OptionsTab::Misc, &st.working, Field::FramerateLimit);
    st.on_click(r.x, r.y + r.h / 2, BOUNDS);
    assert_eq!(
        st.working.framerate_limit, 0,
        "framerate slider left edge = real speed"
    );
}

#[test]
fn joypad_tab_live_controls_are_the_functional_ones() {
    // "configure keyboard" is the only live control on the Joypad tab at this
    // point (JP7 adds "allow pressing L+R or U+D"); the rest are faithful but
    // inert (no gamepad/recording/joystick backend).
    let s = Settings::default();
    let content = OptionsState::content_rect(dialog());
    let live: Vec<_> = controls(OptionsTab::Joypad, &s, content)
        .into_iter()
        .filter_map(|c| c.field)
        .collect();
    assert!(
        live.contains(&Field::ConfigureKeyboard),
        "configure keyboard is live"
    );
    assert!(
        live.contains(&Field::AllowOpposing),
        "allow L+R / U+D is live"
    );
    // The game-controller controls are live now (gilrs backend).
    for f in [
        Field::ConfigureGamepad,
        Field::ClearGamepad,
        Field::GamepadNeedsFocus,
    ] {
        assert!(live.contains(&f), "{f:?} is live");
    }
}

#[test]
fn joypad_gamepad_focus_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    assert!(st.working.gamepad_needs_focus, "bgb default: on");
    click_field(&mut st, Field::GamepadNeedsFocus);
    assert!(!st.working.gamepad_needs_focus);
}

#[test]
fn joypad_rapid_speed_dropdown_cycles_1_to_4() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    assert_eq!(st.working.rapid_speed, 2, "default 2");
    click_field(&mut st, Field::RapidSpeed); // 2 -> 3
    click_field(&mut st, Field::RapidSpeed); // 3 -> 4
    assert_eq!(st.working.rapid_speed, 4);
    click_field(&mut st, Field::RapidSpeed); // 4 wraps -> 1
    assert_eq!(st.working.rapid_speed, 1);
}

#[test]
fn joypad_audio_record_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    assert!(!st.working.record_audio);
    click_field(&mut st, Field::RecordAudio);
    assert!(st.working.record_audio);
}

#[test]
fn joypad_video_record_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    assert!(!st.working.record_video);
    click_field(&mut st, Field::RecordVideo);
    assert!(st.working.record_video);
}

#[test]
fn gb_colors_rgb_editor_controls_are_live() {
    let s = Settings::default();
    let content = OptionsState::content_rect(dialog());
    let ctrls = controls(OptionsTab::GbColors, &s, content);
    for f in [
        Field::PaletteR,
        Field::PaletteG,
        Field::PaletteB,
        Field::Palette031,
        Field::PaletteSelectShade,
    ] {
        assert!(
            ctrls.iter().any(|c| c.field == Some(f)),
            "GB Colors control {f:?} must be live"
        );
    }
}

#[test]
fn joypad_audio_channels_record_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    assert!(!st.working.record_audio_channels);
    click_field(&mut st, Field::RecordAudioChannels);
    assert!(st.working.record_audio_channels);
}

#[test]
fn system_rtc_vba_sav_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::System;
    assert!(!st.working.rtc_vba_sav);
    click_field(&mut st, Field::RtcVbaSav);
    assert!(st.working.rtc_vba_sav);
}

#[test]
fn system_rtc_bgb_legacy_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::System;
    assert!(!st.working.rtc_bgb_legacy);
    click_field(&mut st, Field::RtcBgbLegacy);
    assert!(st.working.rtc_bgb_legacy);
}

#[test]
fn joypad_tab_transcribes_the_capture_controls() {
    // JP8: the inert chrome matching options-joypad.png is present (dropdowns,
    // the Mappable-button-records groupbox + its checks, the joystick-ID field).
    use super::tabs::Kind;
    let s = Settings::default();
    let content = OptionsState::content_rect(dialog());
    let ctrls = controls(OptionsTab::Joypad, &s, content);
    // "Rapid speed" is now a live dropdown cycling the auto-fire period.
    assert!(
        ctrls.iter().any(
            |c| matches!(&c.kind, Kind::Dropdown { value, .. } if value == "2 2")
                && c.field == Some(Field::RapidSpeed)
        ),
        "Rapid speed dropdown is live"
    );
    // "Screenshot button" is now a live dropdown (saves ↔ copies).
    assert!(
        ctrls.iter().any(
            |c| matches!(&c.kind, Kind::Dropdown { value, .. } if value == "saves")
                && c.field == Some(Field::ScreenshotButtonMode)
        ),
        "Screenshot-button mode dropdown is live"
    );
    // "Screenshots" is now a live dropdown driving the image format.
    assert!(
        ctrls.iter().any(
            |c| matches!(&c.kind, Kind::Dropdown { value, .. } if value == "bmp")
                && c.field == Some(Field::ScreenshotFormat)
        ),
        "Screenshots format dropdown is live"
    );
    assert!(
        ctrls
            .iter()
            .any(|c| matches!(&c.kind, Kind::GroupBox { label, .. }
            if *label == "Mappable button records")),
        "Mappable button records groupbox"
    );
    // The whole Mappable-button-records row is live: Audio→WAV, Video→AVI,
    // Audio channels→4 per-channel WAVs.
    assert!(
        ctrls
            .iter()
            .any(|c| matches!(&c.kind, Kind::Check { label: "Audio", .. })
                && c.field == Some(Field::RecordAudio)),
        "Audio record checkbox is live"
    );
    assert!(
        ctrls
            .iter()
            .any(|c| matches!(&c.kind, Kind::Check { label: "Video", .. })
                && c.field == Some(Field::RecordVideo)),
        "Video record checkbox is live"
    );
    assert!(
        ctrls.iter().any(|c| matches!(
            &c.kind,
            Kind::Check {
                label: "Audio channels",
                ..
            }
        ) && c.field == Some(Field::RecordAudioChannels)),
        "Audio channels record checkbox is live"
    );
    assert!(
        ctrls
            .iter()
            .any(|c| matches!(&c.kind, Kind::Button { label, .. } if *label == "0")),
        "joystick-ID field"
    );
}

#[test]
fn allow_opposing_defaults_off_toggles_and_resets() {
    // bgb default: "allow pressing L+R or U+D" unchecked (filter on).
    assert!(!Settings::default().allow_opposing);
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Joypad;
    click_field(&mut st, Field::AllowOpposing);
    assert!(
        st.working.allow_opposing,
        "click toggles the SOCD filter off"
    );
    // Defaults restores it (the only live Joypad Settings field).
    st.press(OptionsButton::Defaults);
    assert!(!st.working.allow_opposing, "Defaults restores filter on");
}

#[test]
fn uninited_ram_checkbox_toggles_the_setting() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::System;
    assert!(!st.working.uninited_wram, "default off");
    let content = OptionsState::content_rect(dialog());
    let cb = controls(OptionsTab::System, &st.working, content)
        .into_iter()
        .find(|c| matches!(&c.kind, super::tabs::Kind::Check { label, .. } if label.contains("uninitialized RAM")))
        .expect("uninitialized RAM checkbox present on the System tab");
    st.on_click(cb.rect.x + 2, cb.rect.y + 2, BOUNDS);
    assert!(st.working.uninited_wram, "clicking it turns it on");
}

#[test]
fn auto_reset_on_system_change_checkbox_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::System;
    assert!(st.working.auto_reset_on_system_change, "on by default");
    click_field(&mut st, Field::AutoResetOnSystemChange);
    assert!(!st.working.auto_reset_on_system_change);
    // "Rewind enabled" off by default → click turns it on.
    assert!(!st.working.rewind_enabled);
    click_field(&mut st, Field::RewindEnabled);
    assert!(st.working.rewind_enabled);
}

#[test]
fn super_gameboy_radio_selects_sgb() {
    // The Super Gameboy System radio is live (slopgb has a full SGB): clicking
    // it selects SGB.
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::System;
    let content = OptionsState::content_rect(dialog());
    let sgb = controls(OptionsTab::System, &st.working, content)
        .into_iter()
        .find(|c| matches!(&c.kind, super::tabs::Kind::Radio { label, .. } if *label == "Super Gameboy"))
        .unwrap();
    st.on_click(sgb.rect.x + 2, sgb.rect.y + 2, BOUNDS);
    assert_eq!(
        st.working.model,
        ModelChoice::Sgb,
        "Super Gameboy radio selects SGB"
    );
}

// Test category modules (split for the 1000-line cap); each is a `mod`
// via `use super::*`, reaching the shared fixtures above + the crate items.
#[path = "options_tests/chrome.rs"]
mod chrome;
#[path = "options_tests/settings.rs"]
mod settings;
