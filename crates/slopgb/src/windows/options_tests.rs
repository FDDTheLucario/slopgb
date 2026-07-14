use super::tabs::{Field, Kind, ThemeRadio, controls};
use super::*;
use crate::ui::canvas::Canvas;

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

// --- Task 1: Settings defaults ----------------------------------------------

#[test]
fn settings_default_matches_spec() {
    let d = Settings::default();
    assert_eq!(d.model, ModelChoice::Auto);
    assert_eq!(d.volume, 1.0);
    assert_eq!(d.ff_speed, 10);
    assert_eq!(d.framerate_limit, 0);
    assert!(!d.show_framerate);
    assert!(d.lowercase_disasm);
    assert!(!d.lowercase_hex);
    assert!(d.show_clocks);
    assert!(!d.freeze_recent);
    assert!(!d.pause_on_focus_loss);
    // bgb shows the debugger on Esc by default (BUG-1) — the slopgb-faithful default.
    assert!(d.esc_shows_debugger);
    assert_eq!(d.scheme, 0);
    assert_eq!(d.dmg_palette, SCHEMES[0].colors);
    // The default scheme is bgb's pale-green LCD ("BGB 0.3"), decoded from
    // bgb.ini Color0..3 (stored BGR) — so a fresh slopgb (and its no-ROM blank
    // screen) looks like bgb. The lightest shade is the captured #E8FCCC.
    assert_eq!(SCHEMES[0].name, "BGB 0.3");
    assert_eq!(d.dmg_palette[0], 0x00E8_FCCC);
    // Exceptions: bgb ships with "break on invalid opcode" checked, the rest off.
    assert!(d.break_invalid_op);
    assert!(!d.break_ld_b_b);
    assert!(!d.break_echo_ram);
    assert!(!d.break_lcd_off_vblank);
    // Boot ROMs: off + no paths by default (golden-safe — post-boot install).
    assert!(!d.bootroms_enabled);
    assert!(d.bootrom_dmg.is_empty() && d.bootrom_gbc.is_empty() && d.bootrom_sgb.is_empty());
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
fn exception_mask_maps_settings_to_core_bits() {
    use slopgb_core::{EXC_ECHO_RAM, EXC_INVALID_OPCODE, EXC_LCD_OFF_VBLANK, EXC_LD_B_B};
    // Default = invalid-opcode only.
    assert_eq!(Settings::default().exception_mask(), EXC_INVALID_OPCODE);
    // Nothing armed → 0 (golden-safe / inert).
    let none = Settings {
        break_invalid_op: false,
        ..Settings::default()
    };
    assert_eq!(none.exception_mask(), 0);
    // All four armed → all four bits.
    let all = Settings {
        break_ld_b_b: true,
        break_invalid_op: true,
        break_echo_ram: true,
        break_lcd_off_vblank: true,
        ..Settings::default()
    };
    assert_eq!(
        all.exception_mask(),
        EXC_LD_B_B | EXC_INVALID_OPCODE | EXC_ECHO_RAM | EXC_LCD_OFF_VBLANK
    );
}

// --- Task 2: tab labels + groups --------------------------------------------

#[test]
fn options_tab_labels_and_groups() {
    let all: Vec<OptionsTab> = OptionsTab::GROUP_A
        .iter()
        .chain(&OptionsTab::GROUP_B)
        .copied()
        .collect();
    assert_eq!(all.len(), 10);
    assert_eq!(
        all.iter().map(|t| t.label()).collect::<Vec<_>>(),
        [
            "Graphics",
            "System",
            "Debug",
            "Exceptions",
            "Sound",
            "GB Colors",
            "Joypad",
            "Misc",
            "Theme",
            "Plugins"
        ]
    );
    // The slopgb Theme tab lives in the bottom group.
    assert!(OptionsTab::GROUP_B.contains(&OptionsTab::Theme));
    for t in OptionsTab::GROUP_A {
        assert_eq!(t.group(), 0);
    }
    for t in OptionsTab::GROUP_B {
        assert_eq!(t.group(), 1);
    }
}

// --- Task 3: tab switching + two-row swap ------------------------------------

#[test]
fn tab_click_switches_active() {
    let mut st = OptionsState::new(Settings::default());
    let boxes = st.tab_hitboxes(dialog());
    let (tab, r) = boxes
        .iter()
        .find(|(t, _)| *t == OptionsTab::Sound)
        .cloned()
        .unwrap();
    assert_eq!(tab, OptionsTab::Sound);
    st.on_click(r.x + 2, r.y + 2, BOUNDS);
    assert_eq!(st.active, OptionsTab::Sound);
}

#[test]
fn active_group_sits_on_bottom_row() {
    // System (group A) active → group A is the bottom row (larger y).
    let st = OptionsState::new(Settings::default()); // active = System
    let boxes = st.tab_hitboxes(dialog());
    let row_y = |want: OptionsTab| boxes.iter().find(|(t, _)| *t == want).unwrap().1.y;
    assert!(
        row_y(OptionsTab::System) > row_y(OptionsTab::Sound),
        "active group A must be the bottom row"
    );

    // Switch to a group-B tab → group B drops to the bottom.
    let mut st2 = OptionsState::new(Settings::default());
    st2.active = OptionsTab::GbColors;
    let b2 = st2.tab_hitboxes(dialog());
    let y2 = |want: OptionsTab| b2.iter().find(|(t, _)| *t == want).unwrap().1.y;
    assert!(
        y2(OptionsTab::GbColors) > y2(OptionsTab::Graphics),
        "active group B must be the bottom row"
    );
}

// --- Task 4: chrome layout ---------------------------------------------------

#[test]
fn chrome_button_order() {
    let rects = OptionsState::button_rects(dialog());
    assert_eq!(
        rects.iter().map(|(b, _)| *b).collect::<Vec<_>>(),
        OptionsButton::ALL.to_vec()
    );
    // left-to-right
    for w in rects.windows(2) {
        assert!(w[0].1.x < w[1].1.x);
    }
}

#[test]
fn render_does_not_panic_and_draws() {
    let mut buf = vec![0u32; (BOUNDS.w * BOUNDS.h) as usize];
    let mut c = Canvas::new(&mut buf, BOUNDS.w as usize, BOUNDS.h as usize);
    let st = OptionsState::new(Settings::default());
    render(&mut c, &st, &T);
    let d = dialog();
    // The dialog bg (white) was written.
    let idx = ((d.y + 1) * BOUNDS.w + d.x + 1) as usize;
    assert_eq!(buf[idx], T.bg);
    // The button row drew ink (the OK button's border) somewhere on its row.
    let (_, ok) = OptionsState::button_rects(d)[0];
    let row_has_ink = (ok.x..ok.right()).any(|x| {
        let i = ((ok.y + ok.h / 2) * BOUNDS.w + x) as usize;
        buf[i] == T.text
    });
    assert!(row_has_ink, "button row should draw the OK button border");
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

// --- Task 5: scratch / button semantics --------------------------------------

#[test]
fn scratch_semantics_cancel_reverts() {
    let mut st = OptionsState::new(Settings::default());
    st.working.volume = 0.25;
    let out = st.press(OptionsButton::Cancel);
    assert_eq!(out, OptionsOutcome::Close);
    assert_eq!(st.working.volume, 1.0, "Cancel reverts to baseline");
}

#[test]
fn scratch_semantics_apply_commits_stays_open() {
    let mut st = OptionsState::new(Settings::default());
    st.working.volume = 0.25;
    let out = st.press(OptionsButton::Apply);
    assert_eq!(out, OptionsOutcome::StayApply);
    assert!(out.applies() && !out.closes(), "Apply applies + stays open");
    assert_eq!(st.baseline.volume, 0.25, "Apply commits baseline");
    // a subsequent Cancel keeps the committed value
    st.working.volume = 0.9;
    st.press(OptionsButton::Cancel);
    assert_eq!(st.working.volume, 0.25);
}

#[test]
fn scratch_semantics_ok_applies_and_closes() {
    let mut st = OptionsState::new(Settings::default());
    st.working.mono = true;
    let out = st.press(OptionsButton::Ok);
    assert_eq!(out, OptionsOutcome::CloseApply);
    assert!(out.applies() && out.closes(), "OK applies + closes");
    assert!(st.baseline.mono);
}

#[test]
fn defaults_button_stays_open_without_applying() {
    // bgb's Defaults only edits the controls — it does not push live until OK/Apply.
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    st.working.volume = 0.3;
    let out = st.press(OptionsButton::Defaults);
    assert_eq!(out, OptionsOutcome::StayReset);
    assert!(!out.applies(), "Defaults does not apply live");
    assert!(!out.closes(), "Defaults stays open");
    assert_eq!(st.working.volume, 1.0, "Defaults reset the working control");
}

#[test]
fn defaults_resets_only_active_tab() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    st.working.volume = 0.1;
    st.working.lowercase_hex = true; // a Debug-tab field, must survive
    st.press(OptionsButton::Defaults);
    assert_eq!(st.working.volume, 1.0, "Sound Defaults resets volume");
    assert!(
        st.working.lowercase_hex,
        "Debug field untouched by Sound Defaults"
    );
}

// --- Tasks 6-13: per-tab live control hit-tests ------------------------------

#[test]
fn model_choice_from_option_maps_preference() {
    use slopgb_core::Model;
    // The persistent --model preference seeds the dialog: None → Auto (bgb
    // default; never force-switches on Apply), explicit models → their radio.
    assert_eq!(ModelChoice::from_option(None), ModelChoice::Auto);
    assert_eq!(ModelChoice::from_option(Some(Model::Dmg)), ModelChoice::Dmg);
    assert_eq!(ModelChoice::from_option(Some(Model::Cgb)), ModelChoice::Cgb);
    assert_eq!(
        ModelChoice::from_option(Some(Model::Agb)),
        ModelChoice::Cgb,
        "AGB is CGB-family"
    );
}

#[test]
fn model_choice_resolve_maps_policies() {
    use slopgb_core::Model;
    // Forcing choices ignore the ROM header.
    let none = &[0u8; 0][..];
    assert_eq!(ModelChoice::Dmg.resolve(none), (Model::Dmg, false));
    assert_eq!(ModelChoice::Sgb.resolve(none), (Model::Sgb, false));
    assert_eq!(ModelChoice::Sgb2.resolve(none), (Model::Sgb2, false));
    // "GBC + initial SGB border" = a CGB machine plus the border-overlay flag.
    assert_eq!(ModelChoice::CgbBorder.resolve(none), (Model::Cgb, true));

    // A CGB-flagged, SGB-capable header.
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = 0xC0; // CGB only
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33; // SGB unlock (both bytes)
    // "prefer SGB" picks SGB when the header unlocks it...
    assert_eq!(ModelChoice::AutoSgb.resolve(&rom), (Model::Sgb, false));
    // ...while "prefer GBC" / "Gameboy or GBC" ignore SGB → CGB here.
    assert_eq!(ModelChoice::Auto.resolve(&rom), (Model::Cgb, false));
    assert_eq!(ModelChoice::AutoNoSgb.resolve(&rom), (Model::Cgb, false));
    // Without the SGB unlock bytes, prefer-SGB falls back to auto (DMG here).
    assert_eq!(
        ModelChoice::AutoSgb.resolve(&vec![0u8; 0x8000]),
        (Model::Dmg, false)
    );
}

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
fn graphics_frame_blend_dropdown_toggles() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Graphics;
    assert!(!st.working.frame_blend);
    click_field(&mut st, Field::FrameBlend);
    assert!(st.working.frame_blend, "frame blend on after click");
}

#[test]
fn screenshot_format_ext_next_and_key_roundtrip() {
    use crate::windows::options::ScreenshotFormat;
    assert_eq!(ScreenshotFormat::Bmp.ext(), "bmp");
    assert_eq!(ScreenshotFormat::Png.ext(), "png");
    assert_eq!(ScreenshotFormat::Bmp.next(), ScreenshotFormat::Png);
    assert_eq!(ScreenshotFormat::Png.next(), ScreenshotFormat::Bmp);
    assert_eq!(ScreenshotFormat::from_key("png"), ScreenshotFormat::Png);
    assert_eq!(ScreenshotFormat::from_key("garbage"), ScreenshotFormat::Bmp);
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
    // Four break conditions are wired to the core exception-break mask; the
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
    ] {
        assert!(live.contains(&f), "{f:?} is live");
    }
    assert_eq!(live.len(), 4, "exactly four live break conditions");
    // The inert rows (OAM DMA / 16-bit inc-dec / SGB) stay non-live.
    use super::tabs::Kind;
    let inert_present = |label: &str| {
        controls(OptionsTab::Exceptions, &s, content)
            .into_iter()
            .any(|c| {
                matches!(&c.kind, Kind::Check { label: l, .. } if *l == label) && c.field.is_none()
            })
    };
    assert!(inert_present("break on OAM DMA bad accesses"));
    assert!(inert_present("break on SGB transfer start"));
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
fn sound_tab_audio_backend_dropdown_cycles() {
    // The Sound tab exposes the SGB audio backend as a live dropdown; clicking it
    // cycles Built-in ↔ SGB coprocessor (the same seam --sgb-coprocessor drives).
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    let content = OptionsState::content_rect(dialog());
    assert!(
        controls(OptionsTab::Sound, &st.working, content)
            .iter()
            .any(|c| c.field == Some(Field::AudioBackend)),
        "Sound tab has a live audio-backend control"
    );
    // Default Built-in; clicking cycles to the coprocessor and back.
    assert_eq!(st.working.audio_backend, AudioBackend::Builtin);
    click_field(&mut st, Field::AudioBackend);
    assert_eq!(st.working.audio_backend, AudioBackend::SgbCoprocessor);
    click_field(&mut st, Field::AudioBackend);
    assert_eq!(st.working.audio_backend, AudioBackend::Builtin);
}

#[test]
fn defaults_resets_every_tab_only_its_own_fields() {
    // For each tab, mutate one of its live fields away from default, press
    // Defaults on that tab, and assert it reset (and an out-of-tab field is
    // untouched). Covers every reset_defaults branch, not just Sound.
    type Case = (OptionsTab, fn(&mut Settings), fn(&Settings) -> bool);
    let cases: &[Case] = &[
        (OptionsTab::Graphics, |s| s.stretch = true, |s| !s.stretch),
        (
            OptionsTab::System,
            |s| s.model = ModelChoice::Cgb,
            |s| s.model == ModelChoice::Auto,
        ),
        (
            OptionsTab::Debug,
            |s| s.lowercase_hex = true,
            |s| !s.lowercase_hex,
        ),
        (OptionsTab::Sound, |s| s.volume = 0.1, |s| s.volume == 1.0),
        (OptionsTab::GbColors, |s| s.scheme = 2, |s| s.scheme == 0),
        (OptionsTab::Misc, |s| s.ff_speed = 3, |s| s.ff_speed == 10),
        (
            OptionsTab::Theme,
            |s| s.theme = ThemeChoice::Dark,
            |s| s.theme == ThemeChoice::Light,
        ),
        (
            // invalid-opcode defaults checked, so flip it off to test the reset.
            OptionsTab::Exceptions,
            |s| s.break_invalid_op = false,
            |s| s.break_invalid_op,
        ),
    ];
    for (tab, mutate, is_default) in cases {
        let mut st = OptionsState::new(Settings::default());
        st.active = *tab;
        mutate(&mut st.working);
        st.working.mono = true; // an out-of-tab field (Sound) — survives unless tab==Sound
        st.press(OptionsButton::Defaults);
        assert!(
            is_default(&st.working),
            "{tab:?} Defaults did not reset its field"
        );
        if *tab != OptionsTab::Sound {
            assert!(
                st.working.mono,
                "{tab:?} Defaults clobbered an out-of-tab field"
            );
        }
    }
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
}

#[test]
fn joypad_tab_transcribes_the_capture_controls() {
    // JP8: the inert chrome matching options-joypad.png is present (dropdowns,
    // the Mappable-button-records groupbox + its checks, the joystick-ID field).
    use super::tabs::Kind;
    let s = Settings::default();
    let content = OptionsState::content_rect(dialog());
    let ctrls = controls(OptionsTab::Joypad, &s, content);
    let has_dropdown = |val: &str| {
        ctrls.iter().any(|c| {
            matches!(&c.kind, Kind::Dropdown { value, .. } if value == val) && c.field.is_none()
        })
    };
    assert!(has_dropdown("2 2"), "Rapid speed dropdown");
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
    for label in ["Audio", "Video", "Audio channels"] {
        assert!(
            ctrls.iter().any(
                |c| matches!(&c.kind, Kind::Check { label: l, .. } if *l == label)
                    && c.field.is_none()
            ),
            "inert check {label}"
        );
    }
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

// --- slider helper -----------------------------------------------------------

#[test]
fn slider_frac_maps_position() {
    let track = Rect::new(10, 0, 100, 10);
    assert_eq!(slider_frac(track, 10), 0.0);
    assert_eq!(slider_frac(track, 110), 1.0);
    assert!((slider_frac(track, 60) - 0.5).abs() < 0.01);
    assert_eq!(slider_frac(track, -5), 0.0, "clamped");
}
