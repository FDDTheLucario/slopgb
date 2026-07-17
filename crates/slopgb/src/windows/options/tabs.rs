//! Per-tab control descriptors for the Options dialog. Each tab builds a flat
//! list of placed [`Ctrl`]s from the current [`Settings`]; the same list drives
//! both rendering and click hit-testing, so the two can't diverge. Live controls
//! carry a [`Field`]; inert ones (`field: None`) render faithfully (greyed only
//! where bgb itself greys them) but do nothing on click.
//!
//! Layout coordinates are content-area-relative absolutes computed from the
//! `content` rect, so a click maps directly. Faithful to the bgb 1.6.4 captures
//! in `docs/bgb-reference/options/`.

use super::{ModelChoice, SCHEMES, Settings, check, dropdown, radio, slider, slider_frac};
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height, measure};
use crate::ui::{Theme, ThemeChoice};
use crate::windows::options::OptionsTab;

mod builders;
use builders::*;

/// The three built-in themes the Theme tab's radios select (the `Custom` theme
/// is config-only, so it is not a radio choice here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemeRadio {
    Light,
    Dark,
    Classic,
}

/// A live setting a control drives. Inert controls have `field: None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Field {
    Stretch,
    /// Debug → "lowercase disassembler" (mnemonic/register case).
    LowercaseDisasm,
    /// Graphics → "disable SGB colors" checkbox.
    DisableSgbColors,
    /// Graphics → "frame blend" combo (cycles off ↔ on).
    FrameBlend,
    /// Graphics → "doubler" combo (cycles off ↔ scale2x).
    Doubler,
    /// GB Colors → "DMG on GBC LCD colors" checkbox.
    DmgGbcLcd,
    /// GB Colors → contrast wheel slider (maps the click fraction to 0.0..=1.0).
    Contrast,
    /// GB Colors → "0-31 numbers": toggle the RGB sliders between 8-bit (0-255)
    /// and native 5-bit (0-31) display.
    Palette031,
    /// GB Colors → "select" dropdown: cycle which shade (0..3) the RGB sliders
    /// edit.
    PaletteSelectShade,
    /// GB Colors → the R / G / B sliders editing the selected shade (channel
    /// 0 / 1 / 2); maps the click fraction to the channel value.
    PaletteR,
    PaletteG,
    PaletteB,
    /// Graphics → "SGB border in screenshot" checkbox.
    SgbBorderScreenshot,
    /// Joypad → "Screenshots" format dropdown (cycles BMP ↔ PNG).
    ScreenshotFormat,
    /// Joypad → "Screenshot button" dropdown (cycles saves ↔ copies).
    ScreenshotButtonMode,
    Mono,
    LowercaseHex,
    ShowClocks,
    RgbdsDisasm,
    TileHex8bit,
    MemoryWindow,
    /// Debug → "pressing Esc shows debugger" (Esc opens the debugger, never
    /// quits). See BUG-1.
    EscShowsDebugger,
    /// Debug → "Registers can be edited".
    RegistersEditable,
    /// Debug → "Start in debugger".
    StartInDebugger,
    /// Debug → "Live update memory viewer".
    MemLiveUpdate,
    /// Debug → "GB CPU usage meter".
    CpuUsageMeter,
    /// Toggle "pure bgb mode": flip every slopgb-departure setting to its
    /// bgb-faithful value (and back to the slopgb defaults).
    PureBgb,
    ShowFramerate,
    FreezeRecent,
    PauseOnFocusLoss,
    /// Misc → "Show errors on ROM load".
    ShowErrorsOnRomLoad,
    /// Misc → "Load ROM dialog on startup".
    LoadRomDialogOnStartup,
    /// Misc → "reduce CPU usage".
    ReduceCpu,
    /// Misc → "Recovery save state".
    RecoverySaveState,
    /// System → "uninitialized RAM at power-on" (bgb's `UninitedWRAM`): power on
    /// with seeded-random garbage RAM. Takes effect on the next reset/load.
    UninitedWram,
    /// System → "automatic reset on system change".
    AutoResetOnSystemChange,
    /// System → "Rewind enabled".
    RewindEnabled,
    Model(ModelChoice),
    Volume,
    /// Sound → "soundcard" dropdown: cycle the output device (routes a
    /// [`super::OptionsOutcome::CycleSoundcard`] out — needs the live device list).
    SoundCard,
    /// Sound → a samplerate radio: sets the requested rate (0 = Auto).
    SampleRate(u32),
    /// Sound → the latency slider (maps the click fraction to 0.0..=1.0).
    Latency,
    /// Sound → "8 bits output".
    EightBit,
    /// Sound → "High quality sound rendering".
    AudioHq,
    /// Fast-forward speed slider (maps the click fraction to 1..=`FF_SPEED_MAX`).
    FfSpeed,
    /// Framerate-limit slider (maps the click fraction to the `FRAMERATE_STEPS`).
    FramerateLimit,
    SchemeCycle,
    /// Joypad → "configure keyboard": opens the key-rebind wizard (does not
    /// mutate [`Settings`]; routes a [`super::OptionsOutcome`] out instead).
    ConfigureKeyboard,
    /// Joypad → "configure game controller": opens the controller-rebind wizard.
    ConfigureGamepad,
    /// Joypad → "clear game controller": unbinds every controller button.
    ClearGamepad,
    /// Joypad → "Game controller works only if app has focus" (gates controller
    /// input on window focus).
    GamepadNeedsFocus,
    /// Joypad → "allow pressing L+R or U+D" (the SOCD filter toggle).
    AllowOpposing,
    /// Joypad → "Rapid speed" combo (cycles the auto-fire period 1→4).
    RapidSpeed,
    /// Joypad → "Audio" (Mappable button records): record audio to a WAV.
    RecordAudio,
    /// Joypad → "Video" (Mappable button records): record the LCD to an AVI.
    RecordVideo,
    /// Joypad → "Audio channels": record the 4 sound channels to separate WAVs.
    RecordAudioChannels,
    /// System → "Save RTC in SAV file (VBA compatible)".
    RtcVbaSav,
    /// System → "Save BGB legacy RTC files" (a `<rom>.rtc` sidecar).
    RtcBgbLegacy,
    /// Exceptions → "break on ld b,b (40h)".
    BreakLdBB,
    /// Exceptions → "break on invalid opcode".
    BreakInvalidOp,
    /// Exceptions → "break on ram echo (E000-FDFF) access".
    BreakEchoRam,
    /// Exceptions → "break on disabling LCD outside vblank".
    BreakLcdOffVblank,
    /// Exceptions → "break on OAM DMA bad accesses".
    BreakOamDmaBad,
    /// Exceptions → "break on 16 bits inc/dec FE00-FEFF".
    BreakIncDecFexx,
    /// Exceptions → "break on SGB transfer start".
    BreakSgbTransfer,
    /// System → "bootroms enabled" checkbox.
    BootromsEnabled,
    /// System → a `...` bootrom-path browse button (routes a
    /// [`super::OptionsOutcome::PickBootrom`] out, like ConfigureKeyboard).
    PickBootrom(super::BootromSlot),
    /// Plugins → the `...` button: routes [`super::OptionsOutcome::PickPluginsDir`]
    /// out to open the path modal.
    PickPluginsDir,
    /// Theme → the active UI colour theme (one of the three built-in themes).
    Theme(ThemeRadio),
    /// Plugins → "allow plugin mutation" toggle.
    PluginAllowMutation,
    /// Plugins → the enable checkbox for the discovered plugin at this index
    /// into [`super::Settings::plugins`]'s entries.
    PluginEnable(usize),
}

/// Fast-forward speed slider range (1..=`FF_SPEED_MAX`).
const FF_SPEED_MAX: u32 = 20;
/// Discrete framerate-limit choices the Misc slider snaps to (0 = real speed).
const FRAMERATE_STEPS: [u32; 6] = [0, 30, 60, 120, 240, 300];

/// How a control draws + hit-tests.
#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Check { checked: bool, label: &'static str },
    Radio { selected: bool, label: &'static str },
    Dropdown { value: String, w: i32 },
    Slider { frac: f32, w: i32 },
    Button { label: &'static str, w: i32 },
    GroupBox { label: &'static str, w: i32, h: i32 },
    Label { text: String },
    Swatch { color: u32 },
}

/// A placed control: where it is, how it looks/acts, whether it is live, and
/// whether bgb itself greys it (visual only).
#[derive(Clone, Debug)]
pub(crate) struct Ctrl {
    pub rect: Rect,
    pub kind: Kind,
    pub field: Option<Field>,
    pub greyed: bool,
}

impl Ctrl {
    fn live(rect: Rect, kind: Kind, field: Field) -> Self {
        Self {
            rect,
            kind,
            field: Some(field),
            greyed: false,
        }
    }
    /// Inert but drawn black (bgb shows it normal, just unwired here).
    fn inert(rect: Rect, kind: Kind) -> Self {
        Self {
            rect,
            kind,
            field: None,
            greyed: false,
        }
    }
    /// Inert and greyed (bgb itself greys it out).
    fn grey(rect: Rect, kind: Kind) -> Self {
        Self {
            rect,
            kind,
            field: None,
            greyed: true,
        }
    }
}

/// Whether every slopgb-departure setting is at its bgb-faithful value (i.e.
/// "pure bgb mode": bgb disasm syntax, the integrated memory pane, full tile hex).
fn pure_bgb(s: &Settings) -> bool {
    !s.rgbds_disasm && !s.memory_window && !s.tile_hex_8bit
}

/// Build the active tab's control list.
pub(crate) fn controls(tab: OptionsTab, s: &Settings, content: Rect) -> Vec<Ctrl> {
    match tab {
        OptionsTab::Graphics => graphics(s, content),
        OptionsTab::System => system(s, content),
        OptionsTab::Debug => debug(s, content),
        OptionsTab::Exceptions => exceptions(s, content),
        OptionsTab::Sound => sound(s, content),
        OptionsTab::GbColors => gb_colors(s, content),
        OptionsTab::Joypad => joypad(s, content),
        OptionsTab::Misc => misc(s, content),
        OptionsTab::Theme => theme_tab(s, content),
        OptionsTab::Plugins => plugins(s, content),
    }
}

/// Render the active tab.
pub(crate) fn render(tab: OptionsTab, s: &Settings, c: &mut Canvas, content: Rect, theme: &Theme) {
    for ct in controls(tab, s, content) {
        let enabled = !ct.greyed;
        let (x, y) = (ct.rect.x, ct.rect.y);
        match &ct.kind {
            Kind::Check { checked, label } => {
                check(c, x, y, *checked, label, enabled, theme);
            }
            Kind::Radio { selected, label } => {
                radio(c, x, y, *selected, label, enabled, theme);
            }
            Kind::Dropdown { value, w } => {
                dropdown(c, x, y, *w, value, enabled, theme);
            }
            Kind::Slider { frac, w } => {
                slider(c, x, y, *w, *frac, enabled, theme);
            }
            Kind::Button { label, w } => {
                let r = Rect::new(x, y, *w, line_height() + 4);
                theme.frame(c, r, super::fg(enabled, theme));
                let tx = x + (*w - measure(label)) / 2;
                draw_text(c, tx, y + 2, label, super::fg(enabled, theme));
            }
            Kind::GroupBox { label, w, h } => {
                // Caption sits just inside the top-left of the frame (drawing it
                // straddling the border would clip against the content area).
                theme.frame(c, Rect::new(x, y, *w, *h), theme.border);
                draw_text(c, x + 4, y + 1, label, theme.text);
            }
            Kind::Label { text } => {
                draw_text(c, x, y, text, super::fg(enabled, theme));
            }
            Kind::Swatch { color } => {
                c.fill_rect(ct.rect, *color);
                c.outline_rect(ct.rect, theme.text);
            }
        }
    }
}

/// Apply a content click: the first live control containing the point fires.
/// Most controls mutate `s` and return `None`; a few (e.g. "configure
/// keyboard") route a [`super::OptionsOutcome`] back to the caller instead.
pub(crate) fn on_content_click(
    tab: OptionsTab,
    s: &mut Settings,
    px: i32,
    py: i32,
    content: Rect,
) -> Option<super::OptionsOutcome> {
    // Build against an immutable snapshot so the borrow ends before we mutate.
    let hit = controls(tab, s, content)
        .into_iter()
        .find(|ct| ct.field.is_some() && ct.rect.contains(px, py));
    let ct = hit?;
    let field = ct.field?;
    // Fields that open a sub-modal over the dialog route an outcome out instead
    // of mutating `Settings` here.
    match field {
        Field::ConfigureKeyboard => return Some(super::OptionsOutcome::ConfigureKeyboard),
        Field::ConfigureGamepad => return Some(super::OptionsOutcome::ConfigureGamepad),
        Field::ClearGamepad => return Some(super::OptionsOutcome::ClearGamepad),
        Field::PickBootrom(slot) => return Some(super::OptionsOutcome::PickBootrom(slot)),
        Field::PickPluginsDir => return Some(super::OptionsOutcome::PickPluginsDir),
        Field::SoundCard => return Some(super::OptionsOutcome::CycleSoundcard),
        _ => {}
    }
    apply(field, s, &ct, px);
    None
}

fn apply(field: Field, s: &mut Settings, ct: &Ctrl, px: i32) {
    match field {
        Field::Stretch => s.stretch = !s.stretch,
        Field::DisableSgbColors => s.disable_sgb_colors = !s.disable_sgb_colors,
        Field::LowercaseDisasm => s.lowercase_disasm = !s.lowercase_disasm,
        Field::FrameBlend => s.frame_blend = !s.frame_blend,
        Field::Doubler => s.doubler = !s.doubler,
        Field::DmgGbcLcd => s.dmg_gbc_lcd = !s.dmg_gbc_lcd,
        Field::Contrast => s.contrast = slider_frac(ct.rect, px),
        Field::Palette031 => s.palette_0_31 = !s.palette_0_31,
        Field::PaletteSelectShade => s.palette_edit_shade = (s.palette_edit_shade + 1) % 4,
        Field::PaletteR => s.set_palette_channel(0, slider_frac(ct.rect, px)),
        Field::PaletteG => s.set_palette_channel(1, slider_frac(ct.rect, px)),
        Field::PaletteB => s.set_palette_channel(2, slider_frac(ct.rect, px)),
        Field::SgbBorderScreenshot => s.sgb_border_screenshot = !s.sgb_border_screenshot,
        Field::ScreenshotFormat => s.screenshot_format = s.screenshot_format.next(),
        Field::ScreenshotButtonMode => s.screenshot_copies = !s.screenshot_copies,
        Field::Mono => s.mono = !s.mono,
        Field::LowercaseHex => s.lowercase_hex = !s.lowercase_hex,
        Field::ShowClocks => s.show_clocks = !s.show_clocks,
        Field::RgbdsDisasm => s.rgbds_disasm = !s.rgbds_disasm,
        Field::TileHex8bit => s.tile_hex_8bit = !s.tile_hex_8bit,
        Field::MemoryWindow => s.memory_window = !s.memory_window,
        Field::EscShowsDebugger => s.esc_shows_debugger = !s.esc_shows_debugger,
        Field::RegistersEditable => s.registers_editable = !s.registers_editable,
        Field::StartInDebugger => s.start_in_debugger = !s.start_in_debugger,
        Field::MemLiveUpdate => s.mem_live_update = !s.mem_live_update,
        Field::CpuUsageMeter => s.cpu_usage_meter = !s.cpu_usage_meter,
        Field::PureBgb => {
            if pure_bgb(s) {
                // Already bgb-faithful → restore the slopgb defaults.
                s.rgbds_disasm = true;
            } else {
                // Flip every slopgb-departure setting to its bgb value.
                s.rgbds_disasm = false;
                s.memory_window = false;
                s.tile_hex_8bit = false;
            }
        }
        Field::ShowFramerate => s.show_framerate = !s.show_framerate,
        Field::FreezeRecent => s.freeze_recent = !s.freeze_recent,
        Field::PauseOnFocusLoss => s.pause_on_focus_loss = !s.pause_on_focus_loss,
        Field::ShowErrorsOnRomLoad => s.show_errors_on_rom_load = !s.show_errors_on_rom_load,
        Field::LoadRomDialogOnStartup => {
            s.load_rom_dialog_on_startup = !s.load_rom_dialog_on_startup;
        }
        Field::ReduceCpu => s.reduce_cpu = !s.reduce_cpu,
        Field::RecoverySaveState => s.recovery_save_state = !s.recovery_save_state,
        Field::UninitedWram => s.uninited_wram = !s.uninited_wram,
        Field::AutoResetOnSystemChange => {
            s.auto_reset_on_system_change = !s.auto_reset_on_system_change;
        }
        Field::RewindEnabled => s.rewind_enabled = !s.rewind_enabled,
        Field::Model(m) => s.model = m,
        Field::Volume => s.volume = slider_frac(ct.rect, px),
        Field::SampleRate(hz) => s.audio_sample_rate = hz,
        Field::Latency => s.audio_latency = slider_frac(ct.rect, px),
        Field::EightBit => s.audio_8bit = !s.audio_8bit,
        Field::AudioHq => s.audio_hq = !s.audio_hq,
        // 1..=FF_SPEED_MAX mapped from the click fraction along the track.
        Field::FfSpeed => {
            let f = slider_frac(ct.rect, px);
            s.ff_speed = 1 + (f * (FF_SPEED_MAX - 1) as f32).round() as u32;
        }
        // Snap to the nearest discrete framerate step.
        Field::FramerateLimit => {
            let f = slider_frac(ct.rect, px);
            let idx = (f * (FRAMERATE_STEPS.len() - 1) as f32).round() as usize;
            s.framerate_limit = FRAMERATE_STEPS[idx.min(FRAMERATE_STEPS.len() - 1)];
        }
        Field::SchemeCycle => s.select_scheme((s.scheme + 1) % SCHEMES.len()),
        Field::AllowOpposing => s.allow_opposing = !s.allow_opposing,
        Field::GamepadNeedsFocus => s.gamepad_needs_focus = !s.gamepad_needs_focus,
        Field::RapidSpeed => {
            s.rapid_speed = if s.rapid_speed >= 4 {
                1
            } else {
                s.rapid_speed + 1
            }
        }
        Field::RecordAudio => s.record_audio = !s.record_audio,
        Field::RecordVideo => s.record_video = !s.record_video,
        Field::RecordAudioChannels => s.record_audio_channels = !s.record_audio_channels,
        Field::RtcVbaSav => s.rtc_vba_sav = !s.rtc_vba_sav,
        Field::RtcBgbLegacy => s.rtc_bgb_legacy = !s.rtc_bgb_legacy,
        Field::BreakLdBB => s.break_ld_b_b = !s.break_ld_b_b,
        Field::BreakInvalidOp => s.break_invalid_op = !s.break_invalid_op,
        Field::BreakEchoRam => s.break_echo_ram = !s.break_echo_ram,
        Field::BreakLcdOffVblank => s.break_lcd_off_vblank = !s.break_lcd_off_vblank,
        Field::BreakOamDmaBad => s.break_oam_dma_bad = !s.break_oam_dma_bad,
        Field::BreakIncDecFexx => s.break_incdec_fexx = !s.break_incdec_fexx,
        Field::BreakSgbTransfer => s.break_sgb_transfer = !s.break_sgb_transfer,
        Field::BootromsEnabled => s.bootroms_enabled = !s.bootroms_enabled,
        Field::Theme(t) => {
            s.theme = match t {
                ThemeRadio::Light => ThemeChoice::Light,
                ThemeRadio::Dark => ThemeChoice::Dark,
                ThemeRadio::Classic => ThemeChoice::Classic,
            };
        }
        Field::PluginAllowMutation => s.plugins.allow_mutation = !s.plugins.allow_mutation,
        Field::PluginEnable(i) => {
            if let Some(e) = s.plugins.entries.get_mut(i) {
                e.enabled = !e.enabled;
            }
        }
        // Routed out by `on_content_click` before reaching here.
        Field::ConfigureKeyboard
        | Field::ConfigureGamepad
        | Field::ClearGamepad
        | Field::PickBootrom(_)
        | Field::PickPluginsDir
        | Field::SoundCard => {}
    }
}

/// Reset only the active tab's live fields to their defaults.
pub(crate) fn reset_defaults(tab: OptionsTab, s: &mut Settings) {
    let d = Settings::default();
    match tab {
        OptionsTab::Graphics => {
            s.stretch = d.stretch;
            s.frame_blend = d.frame_blend;
            s.doubler = d.doubler;
            s.sgb_border_screenshot = d.sgb_border_screenshot;
            s.disable_sgb_colors = d.disable_sgb_colors;
        }
        OptionsTab::System => {
            s.model = d.model;
            s.uninited_wram = d.uninited_wram;
            s.auto_reset_on_system_change = d.auto_reset_on_system_change;
            s.rewind_enabled = d.rewind_enabled;
            s.rtc_vba_sav = d.rtc_vba_sav;
            s.rtc_bgb_legacy = d.rtc_bgb_legacy;
        }
        OptionsTab::Debug => {
            s.lowercase_disasm = d.lowercase_disasm;
            s.lowercase_hex = d.lowercase_hex;
            s.show_clocks = d.show_clocks;
            s.rgbds_disasm = d.rgbds_disasm;
            s.tile_hex_8bit = d.tile_hex_8bit;
            s.memory_window = d.memory_window;
            s.esc_shows_debugger = d.esc_shows_debugger;
            s.registers_editable = d.registers_editable;
            s.start_in_debugger = d.start_in_debugger;
            s.mem_live_update = d.mem_live_update;
            s.cpu_usage_meter = d.cpu_usage_meter;
        }
        // The wired break conditions; the rest of the tab is inert.
        OptionsTab::Exceptions => {
            s.break_ld_b_b = d.break_ld_b_b;
            s.break_invalid_op = d.break_invalid_op;
            s.break_echo_ram = d.break_echo_ram;
            s.break_lcd_off_vblank = d.break_lcd_off_vblank;
            s.break_oam_dma_bad = d.break_oam_dma_bad;
            s.break_incdec_fexx = d.break_incdec_fexx;
            s.break_sgb_transfer = d.break_sgb_transfer;
        }
        OptionsTab::Sound => {
            s.volume = d.volume;
            s.mono = d.mono;
            s.audio_device = d.audio_device.clone();
            s.audio_sample_rate = d.audio_sample_rate;
            s.audio_latency = d.audio_latency;
            s.audio_8bit = d.audio_8bit;
            s.audio_hq = d.audio_hq;
        }
        OptionsTab::GbColors => {
            s.select_scheme(d.scheme);
            s.dmg_gbc_lcd = d.dmg_gbc_lcd;
            s.contrast = d.contrast;
            s.palette_edit_shade = d.palette_edit_shade;
            s.palette_0_31 = d.palette_0_31;
        }
        // configure-keyboard is not a Settings field; the SOCD toggle + the
        // screenshot format are the live Joypad fields that reset.
        OptionsTab::Joypad => {
            s.allow_opposing = d.allow_opposing;
            s.gamepad_needs_focus = d.gamepad_needs_focus;
            s.gamepad_map = d.gamepad_map.clone();
            s.screenshot_format = d.screenshot_format;
            s.screenshot_copies = d.screenshot_copies;
            s.rapid_speed = d.rapid_speed;
            s.record_audio = d.record_audio;
            s.record_video = d.record_video;
            s.record_audio_channels = d.record_audio_channels;
        }
        OptionsTab::Misc => {
            s.ff_speed = d.ff_speed;
            s.framerate_limit = d.framerate_limit;
            s.show_framerate = d.show_framerate;
            s.freeze_recent = d.freeze_recent;
            s.pause_on_focus_loss = d.pause_on_focus_loss;
            s.show_errors_on_rom_load = d.show_errors_on_rom_load;
            s.load_rom_dialog_on_startup = d.load_rom_dialog_on_startup;
            s.reduce_cpu = d.reduce_cpu;
            s.recovery_save_state = d.recovery_save_state;
        }
        OptionsTab::Theme => s.theme = d.theme,
        // Reset the allow-mutation toggle + re-enable every discovered plugin
        // (the dir + which plugins exist aren't "defaults" to reset).
        OptionsTab::Plugins => {
            s.plugins.allow_mutation = d.plugins.allow_mutation;
            for e in &mut s.plugins.entries {
                e.enabled = true;
            }
        }
    }
}
