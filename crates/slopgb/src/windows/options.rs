//! The bgb "Options" control panel (functional clone of bgb 1.6.4's Options
//! property sheet — captures in `docs/bgb-reference/options/`). A modal overlay
//! over the LCD (like [`super::mainwin::InfoBox`]): a 9-tab dialog laid out in
//! bgb's two-row Windows tab control (row 1 Graphics/System/Debug/Exceptions,
//! row 2 Sound/GB Colors/Joypad/Misc plus a slopgb Theme tab), the active tab's
//! group sitting in the bottom row touching the content, with
//! OK/Cancel/Apply/Defaults buttons.
//!
//! Settings backed by a real slopgb capability are **live** (including SGB
//! borders, bootroms, and the emulated-system radios). The remaining bgb-only
//! controls (game-controller config, WAV/AVI recording, HQ sound, …) are drawn
//! faithfully but inert — in bgb's own colour, i.e. NOT greyed unless bgb itself
//! greys them, and a click on one is silently swallowed. Goal is functional 1:1,
//! not pixel or code parity, matching the bgb 1.6.4 captures in
//! `docs/bgb-reference/options/`.
//!
//! `main` owns the `Option<OptionsState>` and routes keys/clicks to it, then
//! applies an [`OptionsOutcome`] to `App`/`Session`/`GameBoy`. The per-tab
//! control descriptors live in [`tabs`].

use slopgb_core::{GameBoy, Model};

use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height, measure};
use crate::ui::{Theme, ThemeChoice};

pub mod tabs;

// --- Settings ---------------------------------------------------------------

/// Which emulated system the System tab selects — bgb's eight "Emulated system"
/// radios. Resolved against the loaded ROM header into a concrete [`Model`] plus
/// a border flag on the next (re)load; see [`Self::resolve`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelChoice {
    /// "automatic, prefer GBC" — `GameBoy::auto_model` (CGB if the header asks,
    /// else DMG; never SGB).
    Auto,
    /// "Gameboy" — force DMG.
    Dmg,
    /// "Gameboy Color" — force CGB.
    Cgb,
    /// "Super Gameboy" — force SGB (HLE border + palettes; optional `--sgb-bios`).
    Sgb,
    /// "SGB + GBC" — force SGB2.
    Sgb2,
    /// "automatic, prefer SGB" — SGB when the header unlocks it
    /// (`GameBoy::rom_supports_sgb`), else the `Auto` policy.
    AutoSgb,
    /// "GBC + initial SGB border" — run in CGB mode but show the built-in
    /// default SGB border around the screen (border-only, presentational).
    CgbBorder,
    /// "Gameboy or GBC" — same policy as [`Self::Auto`] (a distinct radio, so it
    /// round-trips + highlights on its own).
    AutoNoSgb,
}

impl ModelChoice {
    /// Resolve this choice against the ROM header into the concrete build spec
    /// applied on reload: the [`Model`] and whether to overlay the default SGB
    /// border on a non-SGB machine (`CgbBorder` only). `AutoSgb` needs the ROM
    /// to detect SGB support, which is why this can't be a plain `Option<Model>`.
    #[must_use]
    pub fn resolve(self, rom: &[u8]) -> (Model, bool) {
        match self {
            ModelChoice::Dmg => (Model::Dmg, false),
            ModelChoice::Cgb => (Model::Cgb, false),
            ModelChoice::Sgb => (Model::Sgb, false),
            ModelChoice::Sgb2 => (Model::Sgb2, false),
            ModelChoice::Auto | ModelChoice::AutoNoSgb => (GameBoy::auto_model(rom), false),
            ModelChoice::AutoSgb if GameBoy::rom_supports_sgb(rom) => (Model::Sgb, false),
            ModelChoice::AutoSgb => (GameBoy::auto_model(rom), false),
            ModelChoice::CgbBorder => (Model::Cgb, true),
        }
    }

    /// Recover the closest choice from a concrete live model (seeds from a CLI
    /// `--model`; the auto/border policies aren't recoverable from a plain
    /// `Model`, which is fine — they only come from the Options UI / settings).
    #[must_use]
    pub fn from_model(m: Model) -> Self {
        match m {
            Model::Sgb => ModelChoice::Sgb,
            Model::Sgb2 => ModelChoice::Sgb2,
            _ if m.is_cgb() => ModelChoice::Cgb,
            _ => ModelChoice::Dmg,
        }
    }

    /// Seed from the persistent model *preference* (`--model`, the value reused
    /// for every ROM load): `None` → `Auto` (bgb's default; auto-detects each
    /// ROM and so never desyncs across loads / never force-switches on Apply).
    #[must_use]
    pub fn from_option(m: Option<Model>) -> Self {
        m.map_or(ModelChoice::Auto, ModelChoice::from_model)
    }
}

/// Which SGB audio backend the Sound tab selects (a slopgb extra, no bgb
/// equivalent — the same seam the `--sgb-coprocessor` flag drives). `Builtin` is
/// the default HLE `SgbApu` (byte-identical golden path); `SgbCoprocessor` swaps
/// in the combined 65C816+SPC700+S-DSP chip. A no-op off `Model::Sgb`/`Sgb2`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioBackend {
    /// The built-in HLE SGB APU (default).
    #[default]
    Builtin,
    /// The combined SGB audio coprocessor (`--sgb-coprocessor`).
    SgbCoprocessor,
}

impl AudioBackend {
    /// Whether the coprocessor backend is selected — the bool `Session::
    /// set_sgb_coprocessor` takes.
    #[must_use]
    pub fn is_coprocessor(self) -> bool {
        matches!(self, AudioBackend::SgbCoprocessor)
    }

    /// The next backend in the cycle (the Sound-tab dropdown steps through both).
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            AudioBackend::Builtin => AudioBackend::SgbCoprocessor,
            AudioBackend::SgbCoprocessor => AudioBackend::Builtin,
        }
    }

    /// Display label for the dropdown.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            AudioBackend::Builtin => "Built-in",
            AudioBackend::SgbCoprocessor => "SGB coprocessor",
        }
    }

    /// Encode for persistence (native `[sound] audio_backend`, bgb-ini
    /// `SlopgbAudioBackend`).
    #[must_use]
    pub fn to_key(self) -> &'static str {
        match self {
            AudioBackend::Builtin => "builtin",
            AudioBackend::SgbCoprocessor => "sgb-coprocessor",
        }
    }

    /// Decode [`Self::to_key`]; anything unrecognized falls back to the default
    /// (`Builtin`), so a hand-edited config can't wedge startup.
    #[must_use]
    pub fn from_key(v: &str) -> Self {
        match v {
            "sgb-coprocessor" => AudioBackend::SgbCoprocessor,
            _ => AudioBackend::Builtin,
        }
    }
}

/// Screenshot image format (Joypad tab "Screenshots" dropdown). Both encoders
/// are std-only (`screenshot::to_bmp` / `mcp::png::encode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenshotFormat {
    /// Uncompressed 24-bit BMP (bgb's format, opens everywhere).
    #[default]
    Bmp,
    /// 8-bit RGB PNG.
    Png,
}

impl ScreenshotFormat {
    /// The next format in the cycle (the Joypad dropdown steps through both).
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            ScreenshotFormat::Bmp => ScreenshotFormat::Png,
            ScreenshotFormat::Png => ScreenshotFormat::Bmp,
        }
    }

    /// Display label + file extension (the dropdown shows this; the saver appends
    /// it to the filename).
    #[must_use]
    pub fn ext(self) -> &'static str {
        match self {
            ScreenshotFormat::Bmp => "bmp",
            ScreenshotFormat::Png => "png",
        }
    }

    /// Decode [`Self::ext`]; anything unrecognized falls back to the default BMP.
    #[must_use]
    pub fn from_key(v: &str) -> Self {
        match v {
            "png" => ScreenshotFormat::Png,
            _ => ScreenshotFormat::Bmp,
        }
    }
}

/// A named DMG palette preset (GB Colors tab "Scheme"). `colors[0]` is the
/// lightest shade (GB color 0), `colors[3]` the darkest — the order
/// `GameBoy::set_dmg_palette` expects.
#[derive(Clone, Copy, Debug)]
pub struct Scheme {
    pub name: &'static str,
    pub colors: [u32; 4],
}

/// Built-in DMG palette schemes. Index 0 ("BGB 0.3") is slopgb's default — bgb's
/// own default pale-green LCD palette, decoded straight from `bgb.ini`'s
/// `Color0..3` (stored BGR: `CCFCE8 90D4AC 708C54 382C14` → RGB below). So a
/// fresh slopgb (and its no-ROM blank screen) matches bgb out of the box; the
/// core power-on default stays grayscale, available here as a selectable scheme.
pub const SCHEMES: [Scheme; 4] = [
    Scheme {
        name: "BGB 0.3",
        colors: [0x00E8_FCCC, 0x00AC_D490, 0x0054_8C70, 0x0014_2C38],
    },
    Scheme {
        name: "Grayscale",
        colors: [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000],
    },
    Scheme {
        name: "DMG green",
        colors: [0x009B_BC0F, 0x008B_AC0F, 0x0030_6230, 0x000F_380F],
    },
    Scheme {
        name: "Pocket",
        colors: [0x00E3_E6C9, 0x00B5_B69E, 0x007B_7563, 0x002E_2D26],
    },
];

/// One discovered plugin as the Plugins tab renders it: its name, a human
/// capability label, and whether it is enabled. The name + capabilities are
/// runtime facts synced from the live [`crate::PluginHost`] when the dialog
/// opens; only the per-name enabled flag persists (see [`PluginConfig`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginEntry {
    pub name: String,
    pub capabilities: String,
    pub enabled: bool,
}

impl From<slopgb_plugin_host::PluginInfo> for PluginEntry {
    fn from(i: slopgb_plugin_host::PluginInfo) -> Self {
        Self {
            name: i.name,
            capabilities: i.capabilities,
            enabled: i.enabled,
        }
    }
}

/// The plugins feature's config (Options → Plugins tab). `dir` + `allow_mutation`
/// persist directly; `entries` is the live discovered list — only the *disabled*
/// names are persisted (a new plugin defaults to enabled), reconstructed as
/// placeholders on load and refilled from the host once it is scanned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginConfig {
    /// Directory scanned for `*.wasm` plugins (empty = none).
    pub dir: String,
    /// Whether mutation-capable plugins are permitted. Default off (golden-safe);
    /// only the read-only introspection tier is served today, so this is a
    /// persisted preference reserved for when the MUTATE tier lands.
    pub allow_mutation: bool,
    /// Discovered plugins with their enabled flag (synced from the live host).
    pub entries: Vec<PluginEntry>,
}

impl PluginConfig {
    /// The disabled plugin names, comma-joined — the persisted form of the
    /// enabled set (an empty list means every plugin is enabled).
    #[must_use]
    pub(crate) fn disabled_joined(&self) -> String {
        self.entries
            .iter()
            .filter(|e| !e.enabled)
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The disabled plugin names, owned — pushed to the host at startup so a
    /// remembered-off plugin stays skipped in `pump`.
    #[must_use]
    pub fn disabled_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| !e.enabled)
            .map(|e| e.name.clone())
            .collect()
    }

    /// Reconstruct from the persisted fields. `disabled` is a comma-list of
    /// plugin names to start disabled; each becomes a placeholder entry (its
    /// capability label is unknown until the live host is synced in).
    #[must_use]
    pub(crate) fn from_persisted(dir: String, allow_mutation: bool, disabled: &str) -> Self {
        let entries = disabled
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| PluginEntry {
                name: n.to_owned(),
                capabilities: String::new(),
                enabled: false,
            })
            .collect();
        Self {
            dir,
            allow_mutation,
            entries,
        }
    }
}

/// Every Options setting. Live fields drive a real slopgb capability; the dialog
/// also carries the (mostly inert) bgb controls implicitly via [`tabs`]. Cloned
/// into [`OptionsState`] as the working + baseline copies. (Not `Copy` — it
/// holds the bootrom path strings.)
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// System → Emulated system (applied on reload).
    pub model: ModelChoice,
    /// Graphics → stretch the LCD to fill (fullscreen stretched).
    pub stretch: bool,
    /// Graphics → "disable SGB colors": render an SGB game in the plain DMG
    /// palette instead of the SGB per-cell colors (a no-op off SGB).
    pub disable_sgb_colors: bool,
    /// Graphics → "frame blend": present each frame averaged with the previous
    /// one (a one-frame motion trail, softening flicker). Frontend-only.
    pub frame_blend: bool,
    /// Graphics → "doubler": scale2x the frame to 2× before the blit (an
    /// edge-preserving pixel doubler). Frontend-only.
    pub doubler: bool,
    /// GB Colors → "DMG on GBC LCD colors": tint the DMG output through the GBC
    /// LCD colour-correction curve (the washed-out panel look). Frontend-only.
    pub dmg_gbc_lcd: bool,
    /// GB Colors → contrast wheel, 0.0..=1.0 (0.5 = neutral / no change).
    pub contrast: f32,
    /// Graphics → "SGB border in screenshot": when an SGB border is loaded, the
    /// saved screenshot is the 256×224 composite instead of the bare 160×144 LCD.
    pub sgb_border_screenshot: bool,
    /// Joypad → "Screenshots" image format (BMP or PNG).
    pub screenshot_format: ScreenshotFormat,
    /// Joypad → "Screenshot button": `false` saves to a file (default), `true`
    /// copies the frame to the clipboard as a PNG image.
    pub screenshot_copies: bool,
    /// Sound → master volume, 0.0..=1.0.
    pub volume: f32,
    /// Sound → mono output (downmix L/R).
    pub mono: bool,
    /// Sound → SGB audio backend (Built-in HLE APU vs the combined coprocessor).
    /// Drives the same seam as `--sgb-coprocessor`; the CLI flag wins the launch.
    /// Default `Builtin` → byte-identical golden path. A no-op off SGB.
    pub audio_backend: AudioBackend,
    /// Sound → output device name (empty = the host default).
    pub audio_device: String,
    /// Sound → requested output sample rate (0 = the device default / "Auto").
    pub audio_sample_rate: u32,
    /// Sound → latency slider, 0.0..=1.0 (mapped to a device buffer size).
    pub audio_latency: f32,
    /// Sound → "8 bits output": prefer an 8-bit (`U8`) device format.
    pub audio_8bit: bool,
    /// Sound → "High quality sound rendering": use the higher-quality resampler.
    pub audio_hq: bool,
    /// Debug → lowercase disassembler mnemonics.
    pub lowercase_disasm: bool,
    /// Debug → lowercase hex digits in the disasm/memory panes.
    pub lowercase_hex: bool,
    /// Debug → show the counted-clocks column in the disasm pane.
    pub show_clocks: bool,
    /// Debug → disassemble in RGBDS syntax (vs bgb / no$gmb). Default on.
    pub rgbds_disasm: bool,
    /// Debug → show VRAM-viewer tile-number hex masked to the low byte
    /// (`383` → `$7F`), matching some GB tools, vs the full value (`$17F`).
    pub tile_hex_8bit: bool,
    /// Debug → pop the memory viewer out into its own window (a slopgb extra).
    pub memory_window: bool,
    /// Debug → "pressing Esc shows debugger": Esc opens the debugger (bgb's
    /// behaviour) instead of quitting. Default on. See BUG-1.
    pub esc_shows_debugger: bool,
    /// Debug → "Registers can be edited": allow the debugger's register-edit
    /// context menu. Default on (bgb ships it checked); off greys the item.
    pub registers_editable: bool,
    /// Debug → "Start in debugger": open the debugger window at launch.
    pub start_in_debugger: bool,
    /// Debug → "Live update memory viewer": auto-refresh the standalone memory
    /// window every frame. Default on; off means it only repaints on interaction
    /// (scroll / Go-to), matching bgb's non-continuous refresh.
    pub mem_live_update: bool,
    /// Debug → "GB CPU usage meter": show the emulated CPU's non-halted duty %
    /// (from `GameBoy::halt_cycles`) in the window title. Default off.
    pub cpu_usage_meter: bool,
    /// Misc → fast-forward speed multiplier (turbo), 1..=20.
    pub ff_speed: u32,
    /// Misc → framerate limit (0 = real speed / 60 fps).
    pub framerate_limit: u32,
    /// Misc → show the framerate in the title bar.
    pub show_framerate: bool,
    /// Misc → freeze the Recent ROMs menu (stop auto-updating it).
    pub freeze_recent: bool,
    /// Misc → pause emulation when the window loses focus.
    pub pause_on_focus_loss: bool,
    /// Misc → "Show errors on ROM load": pop an info box when a ROM fails to
    /// load (bgb ships this checked). When off, a failed load is silent.
    pub show_errors_on_rom_load: bool,
    /// Misc → "Load ROM dialog on startup": open the file picker at launch when
    /// no ROM was given on the command line.
    pub load_rom_dialog_on_startup: bool,
    /// Misc → "reduce CPU usage": between frames the event loop parks
    /// (`WaitUntil`) instead of busy-polling. Default on; off spins for lowest
    /// input latency at the cost of a pinned core.
    pub reduce_cpu: bool,
    /// Misc → "Recovery save state": periodically write `<rom>.recovery` and
    /// restore it on the next load of that ROM (crash recovery — deleted on a
    /// clean quit). Default on (bgb ships it checked).
    pub recovery_save_state: bool,
    /// GB Colors → selected scheme index into [`SCHEMES`].
    pub scheme: usize,
    /// GB Colors → the live DMG palette (lightest→darkest).
    pub dmg_palette: [u32; 4],
    /// GB Colors → which shade (0=lightest..3=darkest) the RGB sliders edit.
    pub palette_edit_shade: usize,
    /// GB Colors → "0-31 numbers": show/edit the RGB sliders in native 5-bit
    /// (0-31, bgb's `v8>>3` readout) instead of 8-bit (0-255). Captured from
    /// real bgb (`docs/bgb-reference/options/options-gbcolors-031.png`:
    /// 232/252/204 → 29/31/25).
    pub palette_0_31: bool,
    /// Joypad → "allow pressing L+R or U+D". `false` (bgb default) filters
    /// opposing directions so the joypad never reports both at once.
    pub allow_opposing: bool,
    /// Joypad → game controller: the controller→Game-Boy button map, persisted
    /// as `crate::gamepad::GamepadBindings::to_config` (8 comma-separated
    /// controller-button names in Right,Left,Up,Down,A,B,Select,Start order).
    pub gamepad_map: String,
    /// Joypad → "Game controller works only if app has focus". `true` (bgb
    /// default) gates controller input on window focus; `false` accepts it in
    /// the background (gilrs reads the device directly, unlike the keyboard).
    pub gamepad_needs_focus: bool,
    /// Joypad → "Rapid speed": the auto-fire toggle period in frames for the
    /// rapid-fire keys (`[` = rapid A, `]` = rapid B). 1..=4; bgb's "2 2".
    pub rapid_speed: u32,
    /// Joypad → "Audio" (Mappable button records): record the game audio to a
    /// WAV while set. Toggling it off on Apply finalises the file.
    pub record_audio: bool,
    /// Joypad → "Video" (Mappable button records): record the 160×144 LCD to an
    /// uncompressed AVI while set. Toggling it off on Apply finalises the file.
    pub record_video: bool,
    /// Joypad → "Audio channels" (Mappable button records): record the 4 GB
    /// sound channels to separate WAVs while set. Off on Apply finalises them.
    pub record_audio_channels: bool,
    /// System → "Save RTC in SAV file (VBA compatible)": write an MBC3 cart's
    /// RTC as VBA's `.sav` footer (portable to VBA/mGBA/SameBoy) instead of
    /// slopgb's own block. Off = slopgb's block (still self-round-tripping).
    pub rtc_vba_sav: bool,
    /// System → "Save BGB legacy RTC files": also write the RTC to a separate
    /// `<rom>.rtc` sidecar (the de-facto shared 48-byte footer) for old
    /// emulators that read a standalone RTC file. Write-only interop.
    pub rtc_bgb_legacy: bool,
    /// bgb's `UninitedWRAM` (ini-only, no dialog control in bgb 1.6.4): power on
    /// with uninitialised (seeded-random) RAM instead of the deterministic
    /// default. `false` (bgb default) = the stable 0xFF cart SRAM / zeroed
    /// work+video RAM. A CLI `--ram-init` overrides this per launch.
    pub uninited_wram: bool,
    /// Exceptions → "break on ld b,b (40h)".
    pub break_ld_b_b: bool,
    /// Exceptions → "break on invalid opcode" (bgb checks this by default).
    pub break_invalid_op: bool,
    /// Exceptions → "break on ram echo (E000-FDFF) access".
    pub break_echo_ram: bool,
    /// Exceptions → "break on disabling LCD outside vblank".
    pub break_lcd_off_vblank: bool,
    /// Exceptions → "break on OAM DMA bad accesses" (a CPU access outside HRAM
    /// while an OAM DMA transfers).
    pub break_oam_dma_bad: bool,
    /// Exceptions → "break on 16 bits inc/dec FE00-FEFF" (the OAM-corruption
    /// trigger: a 16-bit INC/DEC rr whose value is in that range).
    pub break_incdec_fexx: bool,
    /// Exceptions → "break on SGB transfer start" (a command packet's first P1
    /// reset pulse; a no-op off SGB models).
    pub break_sgb_transfer: bool,
    /// System → "automatic reset on system change": when on (default) picking a
    /// new Emulated-system radio rebuilds the machine immediately; when off the
    /// choice is deferred and applied on the next Reset.
    pub auto_reset_on_system_change: bool,
    /// System → "Rewind enabled": keep a ring of recent save states so Backspace
    /// rewinds emulation. Default off (it costs memory + a per-interval snapshot).
    pub rewind_enabled: bool,
    /// System → "bootroms enabled": execute the configured boot ROM on ROM load
    /// (bgb's checkbox). Off by default — slopgb then boots post-boot.
    pub bootroms_enabled: bool,
    /// System → DMG/MGB/SGB bootrom path (empty = none). Used when
    /// [`Self::bootroms_enabled`] and the loaded model is DMG-class.
    pub bootrom_dmg: String,
    /// System → GBC bootrom path (empty = none). Used for CGB/AGB models.
    pub bootrom_gbc: String,
    /// System → SGB bootrom path (empty = none). Faithful field; slopgb maps an
    /// SGB model to the DMG-class 256 B boot ROM, so this feeds the SGB models.
    pub bootrom_sgb: String,
    /// UI → active colour theme (Light/Dark/Classic/a named custom theme). No
    /// bgb equivalent — set via the Options Theme tab (the three built-ins), the
    /// config file, or the Light↔Dark hotkey (bare `T`,
    /// [`crate::input::Action::ToggleTheme`]). Default `Light` (a modern flat
    /// look); `Classic` reproduces bgb's original grey/white palette
    /// pixel-for-pixel. A named `Custom` theme stays config-only.
    pub theme: ThemeChoice,
    /// Plugins → the discovered-plugin list, plugins dir, and allow-mutation
    /// toggle (Options → Plugins tab). Default empty/off — the golden path is
    /// byte-identical with no plugins.
    pub plugins: PluginConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: ModelChoice::Auto,
            stretch: false,
            disable_sgb_colors: false,
            frame_blend: false,
            doubler: false,
            dmg_gbc_lcd: false,
            contrast: 0.5,
            sgb_border_screenshot: false,
            screenshot_format: ScreenshotFormat::Bmp,
            screenshot_copies: false,
            volume: 1.0,
            mono: false,
            audio_backend: AudioBackend::Builtin,
            audio_device: String::new(),
            audio_sample_rate: 0,
            audio_latency: 0.5,
            audio_8bit: false,
            audio_hq: true,
            lowercase_disasm: true,
            lowercase_hex: false,
            show_clocks: true,
            rgbds_disasm: true,
            tile_hex_8bit: false,
            memory_window: false,
            esc_shows_debugger: true,
            registers_editable: true,
            start_in_debugger: false,
            mem_live_update: true,
            cpu_usage_meter: false,
            ff_speed: 10,
            framerate_limit: 0,
            show_framerate: false,
            freeze_recent: false,
            pause_on_focus_loss: false,
            // bgb ships "Show errors on ROM load" checked.
            show_errors_on_rom_load: true,
            load_rom_dialog_on_startup: false,
            reduce_cpu: true,
            recovery_save_state: true,
            scheme: 0,
            dmg_palette: SCHEMES[0].colors,
            palette_edit_shade: 0,
            palette_0_31: false,
            allow_opposing: false,
            gamepad_map: crate::gamepad::default_map_config(),
            gamepad_needs_focus: true,
            rapid_speed: 2,
            record_audio: false,
            record_video: false,
            record_audio_channels: false,
            rtc_vba_sav: false,
            rtc_bgb_legacy: false,
            uninited_wram: false,
            break_ld_b_b: false,
            // bgb ships with "break on invalid opcode" checked.
            break_invalid_op: true,
            break_echo_ram: false,
            break_lcd_off_vblank: false,
            break_oam_dma_bad: false,
            break_incdec_fexx: false,
            break_sgb_transfer: false,
            auto_reset_on_system_change: true,
            rewind_enabled: false,
            bootroms_enabled: false,
            bootrom_dmg: String::new(),
            bootrom_gbc: String::new(),
            bootrom_sgb: String::new(),
            theme: ThemeChoice::Light,
            plugins: PluginConfig::default(),
        }
    }
}

impl Settings {
    /// Apply scheme `i`: select it and copy its palette into [`Self::dmg_palette`].
    pub fn select_scheme(&mut self, i: usize) {
        if let Some(s) = SCHEMES.get(i) {
            self.scheme = i;
            self.dmg_palette = s.colors;
        }
    }

    /// The displayed value of channel `ch` (0=R,1=G,2=B) of the selected shade:
    /// 8-bit (0-255) or, with "0-31 numbers" on, 5-bit (`v8>>3`, 0-31) — bgb's
    /// readout law (232→29, 252→31, 204→25, captured from real bgb).
    #[must_use]
    pub fn palette_channel_display(&self, ch: usize) -> u32 {
        let v8 = (self.dmg_palette[self.palette_edit_shade.min(3)] >> (16 - ch as u32 * 8)) & 0xFF;
        if self.palette_0_31 { v8 >> 3 } else { v8 }
    }

    /// The slider fraction (0..1) for channel `ch` of the selected shade.
    #[must_use]
    pub fn palette_channel_frac(&self, ch: usize) -> f32 {
        let v8 = (self.dmg_palette[self.palette_edit_shade.min(3)] >> (16 - ch as u32 * 8)) & 0xFF;
        v8 as f32 / 255.0
    }

    /// Set channel `ch` (0=R,1=G,2=B) of the selected shade from a slider
    /// fraction. With "0-31 numbers" on the value snaps to `v5<<3` (32 levels,
    /// the natural inverse of bgb's `v8>>3` readout); otherwise full 8-bit.
    pub fn set_palette_channel(&mut self, ch: usize, frac: f32) {
        let v8 = if self.palette_0_31 {
            ((frac * 31.0).round() as u32) << 3
        } else {
            (frac * 255.0).round() as u32
        }
        .min(255);
        let shift = 16 - ch as u32 * 8;
        let shade = self.palette_edit_shade.min(3);
        self.dmg_palette[shade] = (self.dmg_palette[shade] & !(0xFFu32 << shift)) | (v8 << shift);
    }

    /// The core exception-break mask (`EXC_*` bits) for the armed conditions —
    /// pushed to the machine by `App::apply_exceptions`.
    #[must_use]
    pub fn exception_mask(&self) -> u16 {
        use slopgb_core::{
            EXC_ECHO_RAM, EXC_INCDEC_FEXX, EXC_INVALID_OPCODE, EXC_LCD_OFF_VBLANK, EXC_LD_B_B,
            EXC_OAM_DMA_BAD, EXC_SGB_TRANSFER,
        };
        let mut m = 0;
        if self.break_ld_b_b {
            m |= EXC_LD_B_B;
        }
        if self.break_invalid_op {
            m |= EXC_INVALID_OPCODE;
        }
        if self.break_echo_ram {
            m |= EXC_ECHO_RAM;
        }
        if self.break_lcd_off_vblank {
            m |= EXC_LCD_OFF_VBLANK;
        }
        if self.break_oam_dma_bad {
            m |= EXC_OAM_DMA_BAD;
        }
        if self.break_incdec_fexx {
            m |= EXC_INCDEC_FEXX;
        }
        if self.break_sgb_transfer {
            m |= EXC_SGB_TRANSFER;
        }
        m
    }
}

// --- Tabs -------------------------------------------------------------------

/// The Options tabs: bgb's eight (row 1 then row 2) plus slopgb's Theme tab
/// (appended to row 2 — no bgb equivalent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsTab {
    Graphics,
    System,
    Debug,
    Exceptions,
    Sound,
    GbColors,
    Joypad,
    Misc,
    Theme,
    Plugins,
}

impl OptionsTab {
    /// Tabs in bgb's top group (row 1 when one of them is active).
    pub const GROUP_A: [OptionsTab; 4] = [
        OptionsTab::Graphics,
        OptionsTab::System,
        OptionsTab::Debug,
        OptionsTab::Exceptions,
    ];
    /// Tabs in bgb's bottom group (row 2 when one of them is active), with
    /// slopgb's Theme + Plugins tabs appended.
    pub const GROUP_B: [OptionsTab; 6] = [
        OptionsTab::Sound,
        OptionsTab::GbColors,
        OptionsTab::Joypad,
        OptionsTab::Misc,
        OptionsTab::Theme,
        OptionsTab::Plugins,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            OptionsTab::Graphics => "Graphics",
            OptionsTab::System => "System",
            OptionsTab::Debug => "Debug",
            OptionsTab::Exceptions => "Exceptions",
            OptionsTab::Sound => "Sound",
            OptionsTab::GbColors => "GB Colors",
            OptionsTab::Joypad => "Joypad",
            OptionsTab::Misc => "Misc",
            OptionsTab::Theme => "Theme",
            OptionsTab::Plugins => "Plugins",
        }
    }

    /// 0 for [`Self::GROUP_A`], 1 for [`Self::GROUP_B`].
    #[must_use]
    pub fn group(self) -> u8 {
        if OptionsTab::GROUP_A.contains(&self) {
            0
        } else {
            1
        }
    }
}

// --- Dialog geometry --------------------------------------------------------

/// bgb's Options dialog is 345×361. slopgb's fixed 7px font is wider than bgb's
/// proportional one, so the System tab's caption-sized box + the right-column
/// bootrom path fields need a little more width to fit the same layout; 420 keeps
/// every control visible at the default scale (centred, clipped if smaller).
pub const DIALOG_W: i32 = 420;
pub const DIALOG_H: i32 = 361;
/// Height of one tab row.
const TAB_ROW_H: i32 = 19;
/// Tab-label horizontal padding (matches the strip in [`OptionsState::tab_hitboxes`]).
const TAB_PAD: i32 = 6;
/// Button-row height + button size.
const BTN_H: i32 = 22;
const BTN_W: i32 = 70;

/// The four bottom buttons, left-to-right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsButton {
    Ok,
    Cancel,
    Apply,
    Defaults,
}

impl OptionsButton {
    pub const ALL: [OptionsButton; 4] = [
        OptionsButton::Ok,
        OptionsButton::Cancel,
        OptionsButton::Apply,
        OptionsButton::Defaults,
    ];
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            OptionsButton::Ok => "OK",
            OptionsButton::Cancel => "Cancel",
            OptionsButton::Apply => "Apply",
            OptionsButton::Defaults => "Defaults",
        }
    }
}

/// What `main` does after a button press. `*Apply` variants push `working` to the
/// live state; `Close`/`StayReset` do not (matching bgb: **Defaults only changes
/// the controls** — nothing goes live until OK/Apply; Cancel reverts and closes
/// without re-applying, since the live state already equals the baseline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsOutcome {
    /// OK: apply `working` to the live machine, then close.
    CloseApply,
    /// Cancel: `working` reverted to baseline; close without applying.
    Close,
    /// Apply: apply `working` to the live machine, stay open.
    StayApply,
    /// Defaults: `working` reset to the tab's defaults; stay open, do NOT apply.
    StayReset,
    /// Joypad → "configure keyboard": open the key-rebind wizard. Neither
    /// applies nor closes the dialog (the wizard floats above it).
    ConfigureKeyboard,
    /// Joypad → "configure game controller": open the controller-rebind wizard.
    /// Floats above the dialog like the keyboard wizard.
    ConfigureGamepad,
    /// Joypad → "clear game controller": unbind every controller button. Applied
    /// immediately (persisted), dialog stays open.
    ClearGamepad,
    /// System → a `...` bootrom-path button: open the shared path modal over the
    /// dialog to edit that slot's path. Neither applies nor closes.
    PickBootrom(BootromSlot),
    /// Plugins → the `...` button: open the path modal to edit the plugins
    /// directory. Neither applies nor closes (rescan happens on OK/Apply).
    PickPluginsDir,
    /// Sound → "soundcard": advance `working.audio_device` to the next enumerated
    /// output device (the live device list lives outside the dialog). Stays open.
    CycleSoundcard,
}

/// Which bootrom-path field (bgb's System tab: DMG / GBC / SGB bootrom). The
/// loaded model selects the slot (DMG/MGB → Dmg, CGB/AGB → Gbc, SGB/SGB2 → Sgb).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootromSlot {
    Dmg,
    Gbc,
    Sgb,
}

impl BootromSlot {
    /// This slot's path in `settings` (mutable, for the modal to write into).
    pub(crate) fn path_mut(self, s: &mut Settings) -> &mut String {
        match self {
            BootromSlot::Dmg => &mut s.bootrom_dmg,
            BootromSlot::Gbc => &mut s.bootrom_gbc,
            BootromSlot::Sgb => &mut s.bootrom_sgb,
        }
    }

    /// This slot's path in `settings` (read-only, for rendering).
    #[must_use]
    pub(crate) fn path(self, s: &Settings) -> &str {
        match self {
            BootromSlot::Dmg => &s.bootrom_dmg,
            BootromSlot::Gbc => &s.bootrom_gbc,
            BootromSlot::Sgb => &s.bootrom_sgb,
        }
    }
}

impl OptionsOutcome {
    /// Whether `main` should close the dialog after this outcome.
    #[must_use]
    pub fn closes(self) -> bool {
        matches!(self, OptionsOutcome::CloseApply | OptionsOutcome::Close)
    }
    /// Whether `main` should push `working` to the live machine.
    #[must_use]
    pub fn applies(self) -> bool {
        matches!(self, OptionsOutcome::CloseApply | OptionsOutcome::StayApply)
    }
}

// --- State ------------------------------------------------------------------

/// Live interactive state for the Options dialog. `working` holds the edits in
/// progress; `baseline` is the last-applied snapshot used to revert on Cancel.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionsState {
    pub active: OptionsTab,
    pub working: Settings,
    pub baseline: Settings,
}

impl OptionsState {
    /// Open the dialog seeded from the current live `settings`.
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        Self {
            active: OptionsTab::System,
            working: settings.clone(),
            baseline: settings,
        }
    }

    /// The centred dialog rect within `bounds`.
    #[must_use]
    pub fn dialog_rect(bounds: Rect) -> Rect {
        let x = bounds.x + (bounds.w - DIALOG_W) / 2;
        let y = bounds.y + (bounds.h - DIALOG_H) / 2;
        Rect::new(x, y, DIALOG_W, DIALOG_H)
    }

    /// The content area below the two tab rows and above the button row.
    #[must_use]
    pub fn content_rect(dialog: Rect) -> Rect {
        let top = dialog.y + 2 * TAB_ROW_H + 4;
        let bottom = dialog.bottom() - BTN_H - 8;
        Rect::new(dialog.x + 6, top, dialog.w - 12, bottom - top)
    }

    /// Each tab's hit-rect, with the active tab's group on the bottom row (bgb's
    /// multi-row tab control behaviour). Returns `(tab, rect)` in draw order
    /// (top row first, then bottom row).
    #[must_use]
    pub fn tab_hitboxes(&self, dialog: Rect) -> Vec<(OptionsTab, Rect)> {
        let active_group = self.active.group();
        // Slices: the two groups now differ in length (Theme extends GROUP_B), so
        // the swapped branches can't be equal-length arrays.
        let (top, bottom): (&[OptionsTab], &[OptionsTab]) = if active_group == 0 {
            (&OptionsTab::GROUP_B, &OptionsTab::GROUP_A)
        } else {
            (&OptionsTab::GROUP_A, &OptionsTab::GROUP_B)
        };
        let mut out = Vec::with_capacity(9);
        for (row, tabs) in [top, bottom].into_iter().enumerate() {
            let y = dialog.y + row as i32 * TAB_ROW_H;
            let mut cx = dialog.x + 4;
            for &t in tabs {
                let w = measure(t.label()) + TAB_PAD * 2;
                out.push((t, Rect::new(cx, y, w, TAB_ROW_H)));
                cx += w + 2;
            }
        }
        out
    }

    /// The four button hit-rects, in [`OptionsButton::ALL`] order.
    #[must_use]
    pub fn button_rects(dialog: Rect) -> Vec<(OptionsButton, Rect)> {
        let y = dialog.bottom() - BTN_H - 4;
        let gap = 8;
        // OK left-aligned; Cancel/Apply/Defaults follow; Defaults right-aligned.
        let mut out = Vec::with_capacity(4);
        let mut x = dialog.x + 8;
        for b in OptionsButton::ALL {
            out.push((b, Rect::new(x, y, BTN_W, BTN_H)));
            x += BTN_W + gap;
        }
        out
    }

    /// Route a left-click at `(px, py)` (window pixels). Tabs switch the active
    /// tab; buttons return their [`OptionsOutcome`]; content clicks mutate
    /// `working` (and a few — e.g. "configure keyboard" — return their own
    /// outcome). Returns `Some(outcome)` for a button press or such a control.
    pub fn on_click(&mut self, px: i32, py: i32, bounds: Rect) -> Option<OptionsOutcome> {
        let dialog = Self::dialog_rect(bounds);
        for (t, r) in self.tab_hitboxes(dialog) {
            if r.contains(px, py) {
                self.active = t;
                return None;
            }
        }
        for (b, r) in Self::button_rects(dialog) {
            if r.contains(px, py) {
                return Some(self.press(b));
            }
        }
        let content = Self::content_rect(dialog);
        tabs::on_content_click(self.active, &mut self.working, px, py, content)
    }

    /// Apply a button's semantics. OK applies + closes; Cancel reverts + closes;
    /// Apply commits the baseline + stays open; Defaults resets the active tab.
    pub fn press(&mut self, b: OptionsButton) -> OptionsOutcome {
        match b {
            OptionsButton::Ok => {
                self.baseline = self.working.clone();
                OptionsOutcome::CloseApply
            }
            OptionsButton::Cancel => {
                self.working = self.baseline.clone();
                OptionsOutcome::Close
            }
            OptionsButton::Apply => {
                self.baseline = self.working.clone();
                OptionsOutcome::StayApply
            }
            OptionsButton::Defaults => {
                // bgb's Defaults only resets the controls; nothing goes live until
                // the user presses OK/Apply, so this does NOT apply.
                tabs::reset_defaults(self.active, &mut self.working);
                OptionsOutcome::StayReset
            }
        }
    }
}

// --- Rendering --------------------------------------------------------------

/// Draw the whole Options dialog centred in `c`.
pub fn render(c: &mut Canvas, st: &OptionsState, theme: &Theme) {
    let dialog = OptionsState::dialog_rect(c.bounds());
    c.fill_rect(dialog, theme.bg);
    theme.frame(c, dialog, theme.border);
    // Tab strip (two rows; active tab outlined, others just labelled).
    for (t, r) in st.tab_hitboxes(dialog) {
        if t == st.active {
            c.fill_rect(r, theme.bg);
            theme.frame(c, r, theme.text);
        } else {
            theme.frame(c, r, theme.border);
        }
        draw_text(c, r.x + TAB_PAD, r.y + 3, t.label(), theme.text);
    }
    // Active tab content.
    let content = OptionsState::content_rect(dialog);
    let saved = c.push_clip(content);
    tabs::render(st.active, &st.working, c, content, theme);
    c.set_clip(saved);
    // Button row.
    for (b, r) in OptionsState::button_rects(dialog) {
        c.fill_rect(r, theme.button_face);
        theme.frame(c, r, theme.text);
        let tx = r.x + (r.w - measure(b.label())) / 2;
        draw_text(
            c,
            tx,
            r.y + (r.h - line_height()) / 2,
            b.label(),
            theme.text,
        );
    }
}

// --- Shared control-drawing helpers (enabled-aware; greyed when inert) ------

/// Foreground colour for a control: black when live, grey when inert (matches
/// bgb greying out unavailable options).
fn fg(enabled: bool, theme: &Theme) -> u32 {
    if enabled {
        theme.text
    } else {
        theme.disabled_text
    }
}

/// Draw a checkbox honouring `enabled` (greyed if inert). The clickable hit-rect
/// is owned by the [`tabs::Ctrl`] list, so this only draws.
pub(crate) fn check(
    c: &mut Canvas,
    x: i32,
    y: i32,
    checked: bool,
    label: &str,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let box_sz = line_height() - 4;
    c.outline_rect(Rect::new(x, y, box_sz, box_sz), color);
    if checked {
        c.fill_rect(Rect::new(x + 2, y + 2, box_sz - 4, box_sz - 4), color);
    }
    draw_text(c, x + box_sz + 3, y, label, color);
}

/// Draw a single radio option honouring `enabled`. Hit-rect owned by the caller.
pub(crate) fn radio(
    c: &mut Canvas,
    x: i32,
    y: i32,
    selected: bool,
    label: &str,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let dot = line_height() - 4;
    c.outline_rect(Rect::new(x, y, dot, dot), color);
    if selected {
        c.fill_rect(Rect::new(x + 2, y + 2, dot - 4, dot - 4), color);
    }
    draw_text(c, x + dot + 3, y, label, color);
}

/// Draw an inert "dropdown" (combo box): a bordered box showing `value` with a
/// `▼`-ish arrow. Used for both live (scheme) and inert combos.
pub(crate) fn dropdown(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    value: &str,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let h = line_height() + 2;
    let r = Rect::new(x, y, w, h);
    theme.frame(c, r, color);
    draw_text(c, x + 3, y + 1, value, color);
    // arrow box on the right
    c.vline(r.right() - h, y, h, color);
    draw_text(c, r.right() - h + 2, y + 1, "v", color);
}

/// Draw a horizontal slider with `frac` (0..1) thumb. The clickable track rect is
/// owned by the [`tabs::Ctrl`] list (and read back by [`slider_frac`]).
pub(crate) fn slider(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    frac: f32,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let mid = y + line_height() / 2;
    c.hline(x, mid, w, color);
    let f = frac.clamp(0.0, 1.0);
    let tx = x + (f * (w - 6) as f32) as i32;
    c.fill_rect(Rect::new(tx, y, 6, line_height()), color);
}

/// The fraction (0..1) a click at `px` maps to within a slider `track`.
#[must_use]
pub(crate) fn slider_frac(track: Rect, px: i32) -> f32 {
    if track.w <= 0 {
        return 0.0;
    }
    ((px - track.x) as f32 / track.w as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
