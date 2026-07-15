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
    /// Graphics → "frame blend" combo (cycles off ↔ on).
    FrameBlend,
    /// GB Colors → "DMG on GBC LCD colors" checkbox.
    DmgGbcLcd,
    /// GB Colors → contrast wheel slider (maps the click fraction to 0.0..=1.0).
    Contrast,
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
    /// System → "uninitialized RAM at power-on" (bgb's `UninitedWRAM`): power on
    /// with seeded-random garbage RAM. Takes effect on the next reset/load.
    UninitedWram,
    /// System → "automatic reset on system change".
    AutoResetOnSystemChange,
    Model(ModelChoice),
    Volume,
    /// Sound → the SGB audio-backend dropdown (cycles Built-in ↔ coprocessor).
    AudioBackend,
    /// Fast-forward speed slider (maps the click fraction to 1..=`FF_SPEED_MAX`).
    FfSpeed,
    /// Framerate-limit slider (maps the click fraction to the `FRAMERATE_STEPS`).
    FramerateLimit,
    SchemeCycle,
    /// Joypad → "configure keyboard": opens the key-rebind wizard (does not
    /// mutate [`Settings`]; routes a [`super::OptionsOutcome`] out instead).
    ConfigureKeyboard,
    /// Joypad → "allow pressing L+R or U+D" (the SOCD filter toggle).
    AllowOpposing,
    /// Exceptions → "break on ld b,b (40h)".
    BreakLdBB,
    /// Exceptions → "break on invalid opcode".
    BreakInvalidOp,
    /// Exceptions → "break on ram echo (E000-FDFF) access".
    BreakEchoRam,
    /// Exceptions → "break on disabling LCD outside vblank".
    BreakLcdOffVblank,
    /// System → "bootroms enabled" checkbox.
    BootromsEnabled,
    /// System → a `...` bootrom-path browse button (routes a
    /// [`super::OptionsOutcome::PickBootrom`] out, like ConfigureKeyboard).
    PickBootrom(super::BootromSlot),
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
                c.outline_rect(r, super::fg(enabled, theme));
                let tx = x + (*w - measure(label)) / 2;
                draw_text(c, tx, y + 2, label, super::fg(enabled, theme));
            }
            Kind::GroupBox { label, w, h } => {
                // Caption sits just inside the top-left of the frame (drawing it
                // straddling the border would clip against the content area).
                c.outline_rect(Rect::new(x, y, *w, *h), theme.border);
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
        Field::PickBootrom(slot) => return Some(super::OptionsOutcome::PickBootrom(slot)),
        _ => {}
    }
    apply(field, s, &ct, px);
    None
}

fn apply(field: Field, s: &mut Settings, ct: &Ctrl, px: i32) {
    match field {
        Field::Stretch => s.stretch = !s.stretch,
        Field::FrameBlend => s.frame_blend = !s.frame_blend,
        Field::DmgGbcLcd => s.dmg_gbc_lcd = !s.dmg_gbc_lcd,
        Field::Contrast => s.contrast = slider_frac(ct.rect, px),
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
        Field::UninitedWram => s.uninited_wram = !s.uninited_wram,
        Field::AutoResetOnSystemChange => {
            s.auto_reset_on_system_change = !s.auto_reset_on_system_change;
        }
        Field::Model(m) => s.model = m,
        Field::Volume => s.volume = slider_frac(ct.rect, px),
        Field::AudioBackend => s.audio_backend = s.audio_backend.next(),
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
        Field::BreakLdBB => s.break_ld_b_b = !s.break_ld_b_b,
        Field::BreakInvalidOp => s.break_invalid_op = !s.break_invalid_op,
        Field::BreakEchoRam => s.break_echo_ram = !s.break_echo_ram,
        Field::BreakLcdOffVblank => s.break_lcd_off_vblank = !s.break_lcd_off_vblank,
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
        Field::ConfigureKeyboard | Field::PickBootrom(_) => {}
    }
}

/// Reset only the active tab's live fields to their defaults.
pub(crate) fn reset_defaults(tab: OptionsTab, s: &mut Settings) {
    let d = Settings::default();
    match tab {
        OptionsTab::Graphics => {
            s.stretch = d.stretch;
            s.frame_blend = d.frame_blend;
            s.sgb_border_screenshot = d.sgb_border_screenshot;
        }
        OptionsTab::System => {
            s.model = d.model;
            s.uninited_wram = d.uninited_wram;
            s.auto_reset_on_system_change = d.auto_reset_on_system_change;
        }
        OptionsTab::Debug => {
            // The live Debug fields; lowercase_disasm is inert (fixed).
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
        // The four wired break conditions; the rest of the tab is inert.
        OptionsTab::Exceptions => {
            s.break_ld_b_b = d.break_ld_b_b;
            s.break_invalid_op = d.break_invalid_op;
            s.break_echo_ram = d.break_echo_ram;
            s.break_lcd_off_vblank = d.break_lcd_off_vblank;
        }
        OptionsTab::Sound => {
            s.volume = d.volume;
            s.mono = d.mono;
            s.audio_backend = d.audio_backend;
        }
        OptionsTab::GbColors => {
            s.select_scheme(d.scheme);
            s.dmg_gbc_lcd = d.dmg_gbc_lcd;
            s.contrast = d.contrast;
        }
        // configure-keyboard is not a Settings field; the SOCD toggle + the
        // screenshot format are the live Joypad fields that reset.
        OptionsTab::Joypad => {
            s.allow_opposing = d.allow_opposing;
            s.screenshot_format = d.screenshot_format;
            s.screenshot_copies = d.screenshot_copies;
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
