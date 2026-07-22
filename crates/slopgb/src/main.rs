//! slopgb desktop frontend: CLI parsing, winit event loop, emulation pacing,
//! and battery-RAM persistence. Video lives in [`video`], audio in [`audio`],
//! the keymap in [`input`].
//!
//! `App` is split across cohesive `impl` blocks: the discrete-action dispatch
//! in [`app_run`], the game-window menu handling in [`app_menu`], the emulation
//! pacing loop in [`app_pacing`], the keyboard dispatch in [`app_keys`], the
//! game-window presentation in [`app_draw`], and the startup resource
//! resolution in [`app_boot`]. One loaded ROM (the machine + save
//! persistence) is [`session::Session`]; CLI parsing is [`cli`]; the audio pipe
//! / watchdog / pacing decision are [`pacing`].
//!
//! Pacing: with audio on, emulation is driven by the audio clock — we emulate
//! exactly enough frames to keep ~50 ms queued for the cpal callback. Muted
//! (or if the device fails to open), a wall-clock loop paces frames at the
//! hardware rate, 4194304 / 70224 ≈ 59.7275 Hz.

mod app_boot;
mod app_draw;
mod app_handler;
mod app_input;
mod app_keys;
mod app_menu;
mod app_pacing;
mod app_path;
mod app_run;
mod audio;
mod avi;
mod cdl;
mod cheat;
mod cheat_ui;
mod cli;
mod clipboard;
mod dbg;
mod file_picker;
mod gamepad;
mod input;
mod keymap;
mod link;
mod mcp;
mod menupopup;
mod msu1;
mod net_worker;
mod pacing;
mod postfx;
mod rtc_export;
mod screenshot;
mod session;
mod settings_file;
mod symbols;
mod toolwin;
mod ui;
mod video;
mod wav;
mod windows;

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slopgb_core::{Button, CLOCK_HZ, CYCLES_PER_FRAME, SCREEN_PIXELS};
use slopgb_plugin_host::PluginHost;
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, ModifiersState};
use winit::window::Window;

use app_boot::{load_msu1, load_plugins, resolve_boot_rom, resolve_sf2, resolve_sgb_bios};
use app_draw::blank_frame;
pub(crate) use app_keys::dialog_key_from;
use audio::AudioOutput;
use cli::{Options, ParseOutcome, USAGE};
use input::ButtonTracker;
use menupopup::MenuPopup;
use pacing::{AudioPipe, StallWatchdog};
use session::Session;
use ui::canvas::Rect;
use ui::dialog::InputDialog;
use video::Video;
use windows::mainwin::{InfoBox, WindowSizeChoice};

/// Wall-clock duration of one emulated frame: 70224 T-cycles at 4194304 Hz
/// (~59.7275 Hz).
const FRAME_DURATION: Duration =
    Duration::from_nanos(CYCLES_PER_FRAME as u64 * 1_000_000_000 / CLOCK_HZ as u64);

/// How often the recovery save state is rewritten while a ROM runs (Misc →
/// "Recovery save state").
const RECOVERY_INTERVAL: Duration = Duration::from_secs(10);

fn main() {
    let opts = match Options::parse(env::args().skip(1)) {
        Ok(ParseOutcome::Run(opts)) => opts,
        Ok(ParseOutcome::Help) => {
            print!("{USAGE}");
            return;
        }
        Err(e) => {
            eprintln!("error: {e}\n");
            eprint!("{USAGE}");
            process::exit(2);
        }
    };
    // No ROM on the command line → boot to a blank LCD (bgb behaviour); a ROM
    // loads later via drag-drop / the Load ROM... menu. With a ROM, a load error
    // still aborts (the user named a file that can't be read).
    // Optional boot ROM (--boot / SLOPGB_BOOT): executed from power-on by every
    // ROM load. Read once here; a bad path is logged and treated as no boot ROM.
    let boot_rom = resolve_boot_rom(&opts);
    // Optional SGB BIOS (--sgb-bios / SLOPGB_SGB_BIOS): feeds the SGB audio path
    // on every ROM (re)load; border/palette are not extracted (HLE).
    let sgb_bios = resolve_sgb_bios(&opts);
    // Optional SF2 soundfont (--sf2 / SLOPGB_SF2): overrides the SGB N-SPC
    // sample bank on every ROM (re)load, independent of the engine choice.
    let sf2 = resolve_sf2(&opts);
    // Effective emulated-system choice for this load: an explicit CLI `--model`
    // wins, else the persisted Options choice (so a saved SGB / "prefer SGB" /
    // border selection is honored at startup, not just after opening Options).
    let loaded = settings_file::load();
    let model_choice = opts.model.map_or(loaded.settings.model, |m| {
        windows::options::ModelChoice::from_option(Some(m))
    });
    // Effective power-on RAM init: CLI `--ram-init` wins, else bgb's persisted
    // `UninitedWRAM` toggle.
    let ram_init = cli::effective_ram_init(opts.ram_init, loaded.settings.uninited_wram);
    let (mut session, rom_loaded) = match &opts.rom {
        Some(rom) => match Session::load(
            rom,
            model_choice,
            &session::BootSpec::cli(boot_rom.as_deref()),
            ram_init,
        ) {
            Ok(s) => (s, true),
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        },
        None => (
            Session::blank(model_choice.resolve(&[0u8; 0x8000]).0),
            false,
        ),
    };
    session.set_sgb_bios(sgb_bios.clone());
    session.set_sf2(sf2.clone());
    // The plugins dir (and the SGB coprocessor it auto-loads) is applied in
    // `App::new`, from the CLI/env/persisted dir it reconciles into `settings`.
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot create event loop: {e}");
            process::exit(1);
        }
    };
    let mut app = App::new(opts, session, rom_loaded, boot_rom, sgb_bios, sf2);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("error: event loop failed: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Application

/// What an accepted [`App::path_dialog`] entry does — the path modal is shared
/// by Load ROM (MN4) and on-disk save states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathPurpose {
    /// Load a ROM from the typed path (the existing drop path).
    LoadRom,
    /// Write the serialized machine to the typed path.
    SaveState,
    /// Restore the machine from the typed path (atomic; a bad file is logged).
    LoadState,
    /// Dial a serial-link peer at the typed `host:port` (bare host → port 8765).
    LinkConnect,
    /// Start the MCP server on the typed port (blank/invalid → the default port).
    McpStart,
    /// Set a bootrom path in the open Options dialog's working scratch
    /// (Options → System → DMG/GBC/SGB bootrom `...`).
    Bootrom(windows::options::BootromSlot),
    /// Set the plugins directory in the open Options dialog's working scratch
    /// (Options → Plugins → `...`). Applied (rescanned) on OK/Apply.
    PluginsDir,
    /// Load a `.sym` symbol file from the typed path (debugger labels/go-to).
    SymbolFile,
    /// Save the CDL flags to the typed path (RLE-compressed).
    CdlSave,
    /// Load CDL flags from the typed path.
    CdlLoad,
    /// Import settings from a bgb-format ini at the typed path.
    SettingsImportBgb,
    /// Export settings to a bgb-format ini at the typed path.
    SettingsExportBgb,
    /// Load cheats from a cheat file at the typed path.
    CheatLoad,
    /// Save cheats to a cheat file at the typed path.
    CheatSave,
}

struct App {
    opts: Options,
    /// Boot ROM bytes (from `--boot`/`SLOPGB_BOOT`), executed from power-on on
    /// every ROM load. `None` = the direct post-boot install (default).
    boot_rom: Option<Vec<u8>>,
    /// Optional SGB BIOS bytes (from `--sgb-bios`/`SLOPGB_SGB_BIOS`), re-applied
    /// to the fresh machine on every ROM (re)load. `None` = no SGB BIOS.
    sgb_bios: Option<Vec<u8>>,
    /// Optional SF2 soundfont path (from `--sf2`/`SLOPGB_SF2`), re-applied to the
    /// fresh machine on every ROM (re)load. `None` = the ROM's own N-SPC samples.
    sf2: Option<PathBuf>,
    session: Session,
    /// Whether a real ROM is loaded. `false` at a no-ROM (bgb-style) startup:
    /// the blank machine is frozen at power-on (emulation gated off) and the LCD
    /// shows [`Self::blank_frame`] until a ROM is loaded (drag-drop / Load ROM).
    rom_loaded: bool,
    /// A solid LCD-off frame (the palette's lightest shade) shown while no ROM is
    /// loaded — bgb's pale-green blank screen. Rebuilt when the palette changes.
    blank_frame: Box<[u32; SCREEN_PIXELS]>,
    /// Scratch copy of the presented frame, reused only when a VRAM OAM hover
    /// asks for a sprite outline drawn into the (immutable) core frame.
    overlay_frame: Vec<u32>,
    /// The last SNES-side frame a full-takeover SGB coprocessor rendered
    /// (256x224, converted to 0xRRGGBB). `Some` switches presentation to the
    /// SNES picture; cleared on ROM load. `None` everywhere the coprocessor
    /// (or its PPU plugin) is absent — the golden presentation paths.
    snes_frame: Option<Vec<u32>>,
    /// Scratch for the presentation filters (`postfx`): the core frame is copied
    /// here and filtered in place before the blit, so the core buffer is never
    /// touched. Empty on the all-off path (the borrow is presented directly).
    postfx_buf: Vec<u32>,
    /// The previously presented (pre-filter) frame, used by "frame blend".
    prev_frame: Vec<u32>,
    /// Scratch for the "doubler" scale2x output (2× the source), presented in
    /// place of the base frame when the doubler is on.
    scale_buf: Vec<u32>,
    /// Misc → "Recovery save state": the `<rom>.recovery` path for the loaded
    /// ROM (None with no ROM). Written periodically and deleted on a clean quit,
    /// so its presence at load time means the last session crashed.
    recovery_path: Option<std::path::PathBuf>,
    /// Wall-clock deadline for the next recovery-state write.
    recovery_next: Instant,
    window: Option<Rc<Window>>,
    video: Option<Video>,
    audio: Option<AudioPipe>,
    /// The Sound-tab prefs the open audio stream was built with, so Apply only
    /// rebuilds it (a brief glitch) when a device/rate/latency/8-bit/quality
    /// setting actually changed.
    audio_prefs_applied: audio::AudioPrefs,
    audio_hq_applied: bool,
    /// Joypad → "Video": the active AVI recorder while recording, else `None`.
    /// Started/finalised by `sync_video_recording`; fed one LCD frame per
    /// rendered batch in `about_to_wait`.
    video_rec: Option<avi::AviWriter>,
    /// Runtime audio mute (bgb's "Enable sound" toggle). Initialised from the
    /// `--mute` flag; gates audio pacing so the pipe drains to silence without
    /// tearing down the cpal stream. See [`pacing::audio_pacing`].
    muted: bool,
    paused: bool,
    turbo: bool,
    /// Backspace held with rewind enabled: step backward through the save-state
    /// ring instead of advancing (see `about_to_wait`).
    rewinding: bool,
    /// Rapid-fire held state (`[`/`]`) + the last auto-fired level per button,
    /// and the frame counter driving the "Rapid speed" toggle cadence.
    rapid_a: bool,
    rapid_b: bool,
    rapid_a_on: bool,
    rapid_b_on: bool,
    rapid_counter: u32,
    /// Game controller input (Options → Joypad): the `gilrs` handle, the
    /// controller→Game-Boy button map, and the controller-only held-set for the
    /// SOCD filter.
    gamepad: gamepad::Gamepads,
    gamepad_bindings: gamepad::GamepadBindings,
    gamepad_held: [bool; 8],
    /// The open "configure game controller" wizard, if any (floats over the LCD
    /// like the keyboard wizard; captures controller presses to rebind).
    gamepad_wizard: Option<gamepad::GamepadConfigWizard>,
    /// Per-key hold state, so two keys mapped to one button release cleanly.
    buttons: ButtonTracker,
    /// Rebindable keyboard → Game Boy button map (Joypad "configure keyboard").
    bindings: keymap::KeyBindings,
    /// Joypad ops `(button, pressed)` deferred from the winit event to the next
    /// emulated frame, applied at [`Self::input_offset`] so the joypad interrupt
    /// fires at a realistic, varied LCD line (input entropy — see
    /// [`input::apply_input`]). Empty between presses.
    input_ops: Vec<(Button, bool)>,
    /// Sub-frame T-cycle offset at which to apply [`Self::input_ops`], captured
    /// from the wall-clock phase of the keypress (so consecutive presses land on
    /// different lines, as on hardware).
    input_offset: u32,
    /// Monotonic reference for the keypress wall-clock phase ([`Self::input_offset`]).
    epoch: Instant,
    /// Detects a cpal stream that stopped draining (see [`StallWatchdog`]).
    watchdog: StallWatchdog,
    /// Deadline of the next frame for wall-clock pacing.
    next_frame: Instant,
    /// Scratch for draining (and discarding) APU output while muted.
    discard_buf: Vec<(f32, f32)>,
    fps_frames: u32,
    fps_since: Instant,
    fps: f64,
    /// GB-CPU-usage meter (Debug → "GB CPU usage meter"): the non-halted duty %,
    /// recomputed each FPS window from the `cycles`/`halt_cycles` deltas below.
    cpu_usage: f64,
    /// Machine `cycles` / `halt_cycles` at the last FPS-window sample.
    cpu_cycles_prev: u64,
    cpu_halt_prev: u64,
    /// Open bgb-style debug tool windows (F2/F3/F4). The game window is handled
    /// directly; these are routed by [`toolwin::ToolWindows::owns`].
    tools: toolwin::ToolWindows,
    /// Debugger execution control (break / step / breakpoints).
    dbg: dbg::Debugger,
    /// Current keyboard modifiers, for the focus-dependent key map (Ctrl+G).
    modifiers: ModifiersState,
    /// Physically-held keys, for a platform-independent key-repeat guard (winit's
    /// `KeyEvent::repeat` flag is unreliable on some Wayland compositors, so a
    /// held step key would otherwise step repeatedly). See [`input::accept_key`].
    held_keys: HashSet<KeyCode>,
    /// The loaded `.sym` symbol table (source of truth), shared into the debugger
    /// view and used for go-to-by-name and the breakpoint-manager labels.
    symbols: Rc<symbols::SymbolTable>,
    /// A pending request (from Options) to open/close the standalone memory
    /// window, reconciled in `about_to_wait` where the event loop is available.
    pending_mem_window: Option<bool>,
    /// The open game-window right-click menu (bgb's `rc-main.png`), if any — its
    /// **own borderless window** (so it can extend past the game window's edge
    /// instead of being clipped), holding the main menu + open submenu.
    menu_popup: Option<MenuPopup>,
    /// An open info box (Other → Cart info / System info / About), drawn centred
    /// over the LCD; any click or Escape closes it.
    info_box: Option<InfoBox>,
    /// GameShark/Game-Genie cheat list (bgb's Cheat dialog). Enabled RAM pokes
    /// re-applied each frame by the run loop.
    cheats: cheat::CheatList,
    /// The open Cheat dialog (main menu "Cheat.../F10"), drawn over the LCD.
    cheat_dialog: Option<cheat_ui::CheatDialog>,
    /// The open path-entry modal, drawn centred over the LCD; accept routes by
    /// [`Self::path_purpose`] (Load ROM / Save state / Load state), Escape closes.
    path_dialog: Option<InputDialog>,
    /// What the open [`Self::path_dialog`] does on accept.
    path_purpose: PathPurpose,
    /// The in-app file browser ([`file_picker::FilePicker`]) — the picker for
    /// every FILE purpose (Load ROM / save+load state / symbols / bootrom /
    /// CDL / cheats). The non-file purposes (`PathEntry::Modal` — link `host:port`
    /// / MCP port) use [`Self::path_dialog`] instead.
    file_picker: Option<file_picker::FilePicker>,
    /// Time + position of the last left-press on [`Self::file_picker`]'s
    /// list, for synthesizing a double-click (winit delivers no such event;
    /// mirrors `toolwin::ToolView::note_click`).
    picker_last_click: Option<(Instant, i32, i32)>,
    /// Recently loaded ROM paths (MN4), most-recent first, deduped, capped — the
    /// Recent ROMs submenu. In-memory only (on-disk persistence deferred).
    recent: Vec<PathBuf>,
    /// The current window size, for the "Window size" submenu check-mark and the
    /// stretched-fullscreen blit. Init from `--scale`.
    window_size: WindowSizeChoice,
    /// Last cursor position over the game window (physical px), so a right-click
    /// can open the menu where the pointer is.
    game_cursor: (i32, i32),
    /// The currently-applied Options settings (bgb's Options control panel) —
    /// the source of truth read by pacing/audio/title/debugger render.
    settings: windows::options::Settings,
    /// The open Options dialog (bgb "Options..."/F11), drawn centred over the
    /// LCD; modal like the info box. `None` when closed.
    options: Option<windows::options::OptionsState>,
    /// The open key-rebind wizard (Options → Joypad → "configure keyboard"),
    /// floating above everything; captures all game-window keys while open.
    key_wizard: Option<keymap::KeyConfigWizard>,
    /// Whether the current pause was auto-induced by focus loss (Options → Misc
    /// → "Pause if losing focus"), so refocus auto-resumes — but a *manual* pause
    /// is never clobbered on refocus.
    paused_by_focus: bool,
    /// Whether the game window currently has OS focus — gates controller input
    /// when "Game controller works only if app has focus" is on (the gamepad,
    /// unlike the keyboard, delivers events regardless of focus).
    window_focused: bool,
    /// Last windowed integer scale chosen (CLI or Window-size menu), restored
    /// when leaving fullscreen-stretched so the menu-picked size isn't lost.
    last_scale: u32,
    /// Serial Link-cable transport (bgb's Link submenu). Inert until Listen /
    /// Connect; pumped once per emulated frame to swap bytes with the peer.
    link: link::Link,
    /// Opt-in MCP debug server (`--mcp-port` / `SLOPGB_MCP_PORT`). Inert unless
    /// started; pumped each wake to serve an agent's tool calls against the live
    /// machine.
    mcp: mcp::Mcp,
    /// Opt-in wasm plugins (`--plugins` / `SLOPGB_PLUGINS_DIR`). Empty unless a
    /// directory was given; pumped once per rendered frame with a read-only view.
    plugins: PluginHost,
    /// Opt-in MSU-1 streaming-audio pack (`--msu1` / `SLOPGB_MSU1`). `None` unless
    /// a pack loaded; when present, its resampled PCM is mixed into the audio each
    /// frame and its registers ($A000-$A007) are polled from the running game.
    /// With no pack the core + audio path are byte-identical (golden-safe).
    msu1: Option<msu1::Msu1>,
    /// Custom themes loaded from the settings file's `[theme.NAME]` sections
    /// (the theming API's registry) — what `settings.theme`'s `Custom(name)`
    /// variant resolves against. Loaded once at startup; like every other
    /// persisted setting, a config edit needs a restart to take effect.
    custom_themes: ui::CustomThemes,
}

impl App {
    fn new(
        opts: Options,
        session: Session,
        rom_loaded: bool,
        boot_rom: Option<Vec<u8>>,
        sgb_bios: Option<Vec<u8>>,
        sf2: Option<PathBuf>,
    ) -> Self {
        let muted = opts.mute;
        let scale = opts.scale;
        let window_size = WindowSizeChoice::Scale(scale);
        // Seed Options' model from the persistent `--model` preference (the value
        // reused for every ROM load), NOT the resolved session model — so it
        // can't desync when a later ROM auto-detects to a different system, and
        // Apply with the default (Auto) never force-switches the running game.
        // Persisted settings (bgb.ini) seed everything. Precedence for the model:
        // an explicit CLI `--model` wins the session, else the persisted choice.
        let loaded = settings_file::load();
        let recent = loaded.recent;
        let custom_themes = settings_file::load_custom_themes();
        let mut settings = windows::options::Settings {
            model: match opts.model {
                Some(m) => windows::options::ModelChoice::from_option(Some(m)),
                None => loaded.settings.model,
            },
            ..loaded.settings
        };
        let blank_frame = blank_frame(settings.dmg_palette[0]);
        // Load the plugins, then reconcile the persisted config with the live
        // host: remember the resolved directory, apply the remembered-disabled
        // set (so an off plugin stays skipped), and mirror the live list (name +
        // caps + enabled) back into `settings.plugins.entries` for the UI.
        let mut plugins = load_plugins(&opts, &settings);
        if let Some(dir) = plugins.dir() {
            settings.plugins.dir = dir.display().to_string();
        }
        for name in settings.plugins.disabled_names() {
            plugins.set_enabled(&name, false);
        }
        settings.plugins.entries = plugins
            .infos()
            .into_iter()
            .map(windows::options::PluginEntry::from)
            .collect();
        let mcp = mcp::Mcp::with_tool_plugins(mcp::plugin_host::ToolPlugins::from_options(&opts));
        // Opt-in MSU-1 pack (--msu1 / SLOPGB_MSU1); None keeps the golden path.
        let msu1 = load_msu1(&opts);
        // Build the controller map before `settings` is moved into the struct.
        let gamepad_bindings = gamepad::GamepadBindings::from_config(&settings.gamepad_map);
        let bindings = keymap::KeyBindings::from_config(&settings.key_map);
        let mut app = Self {
            opts,
            boot_rom,
            sgb_bios,
            sf2,
            session,
            rom_loaded,
            blank_frame,
            overlay_frame: Vec::new(),
            snes_frame: None,
            postfx_buf: Vec::new(),
            prev_frame: Vec::new(),
            scale_buf: Vec::new(),
            recovery_path: None,
            recovery_next: Instant::now(),
            settings,
            options: None,
            key_wizard: None,
            paused_by_focus: false,
            window_focused: true,
            last_scale: scale,
            window: None,
            video: None,
            audio: None,
            audio_prefs_applied: audio::AudioPrefs::default(),
            audio_hq_applied: true,
            video_rec: None,
            muted,
            paused: false,
            turbo: false,
            rewinding: false,
            rapid_a: false,
            rapid_b: false,
            rapid_a_on: false,
            rapid_b_on: false,
            rapid_counter: 0,
            gamepad: gamepad::Gamepads::new(),
            gamepad_bindings,
            gamepad_held: [false; 8],
            gamepad_wizard: None,
            buttons: ButtonTracker::default(),
            bindings,
            input_ops: Vec::new(),
            input_offset: 0,
            epoch: Instant::now(),
            watchdog: StallWatchdog::new(),
            next_frame: Instant::now(),
            discard_buf: Vec::new(),
            fps_frames: 0,
            fps_since: Instant::now(),
            fps: 0.0,
            cpu_usage: 0.0,
            cpu_cycles_prev: 0,
            cpu_halt_prev: 0,
            tools: toolwin::ToolWindows::new(),
            dbg: dbg::Debugger::default(),
            modifiers: ModifiersState::empty(),
            held_keys: HashSet::new(),
            symbols: Rc::new(symbols::SymbolTable::default()),
            pending_mem_window: None,
            info_box: None,
            cheats: cheat::CheatList::default(),
            cheat_dialog: None,
            path_dialog: None,
            path_purpose: PathPurpose::LoadRom,
            file_picker: None,
            picker_last_click: None,
            link: link::Link::new(),
            mcp,
            plugins,
            msu1,
            recent,
            menu_popup: None,
            window_size,
            game_cursor: (0, 0),
            custom_themes,
        };
        // Push the default DMG palette (bgb's pale green) onto the freshly-built
        // machine so loaded DMG games look like bgb out of the box, not the core's
        // grayscale power-on default.
        app.apply_palette();
        // Arm the default exception-break mask (bgb's "break on invalid opcode").
        app.apply_exceptions();
        // The SGB coprocessor is a plugin: point the session at the resolved
        // plugins dir (CLI `--plugins` / env / persisted — the single source
        // `load_plugins` already reconciled into `settings.plugins.dir`) so it
        // auto-loads `spc700.wasm` + `w65c816.wasm` on an SGB machine at startup.
        app.session.set_plugins_dir(
            (!app.settings.plugins.dir.is_empty())
                .then(|| PathBuf::from(&app.settings.plugins.dir)),
        );
        app
    }

    fn update_title(&self) {
        if let Some(window) = &self.window {
            let state = if self.dbg.is_broken() {
                " (debugging)".to_owned()
            } else if self.paused {
                " — paused".to_owned()
            } else {
                // FPS and the GB-CPU-usage meter both append when enabled.
                let mut s = String::new();
                if self.settings.show_framerate {
                    s.push_str(&format!(" — {:.1} fps", self.fps));
                }
                if self.settings.cpu_usage_meter {
                    s.push_str(&format!(" — {:.0}% cpu", self.cpu_usage));
                }
                s
            };
            let mut title = window_title(self.rom_loaded, &self.session.title, &state);
            // The serial-link status (bgb shows it in the title bar) is appended
            // after window_title so it shows even at the no-ROM startup screen,
            // whose title is otherwise a bare "slopgb".
            if let Some(link) = self.link.status_label() {
                title.push_str(&format!(" — {link}"));
            }
            if let Some(mcp) = self.mcp.status_label() {
                title.push_str(&format!(" — {mcp}"));
            }
            window.set_title(&title);
        }
    }

    /// Apply an accepted Cheat Add/Edit entry to the cheat list.
    fn apply_cheat_edit(&mut self, e: &cheat_ui::CheatEdit) {
        match e.editing {
            Some(i) => self.cheats.edit(i, &e.comment, &e.code),
            None => {
                self.cheats.add(&e.comment, &e.code);
            }
        }
    }

    /// Keep the Cheat dialog selection in range after a delete.
    fn clamp_cheat_sel(&mut self) {
        let n = self.cheats.len();
        if let Some(d) = &mut self.cheat_dialog {
            d.sel = d.sel.min(n.saturating_sub(1));
        }
    }

    /// Restart wall-clock pacing from now (after pause, turbo, load, reset),
    /// and give the audio stall watchdog a fresh grace period.
    fn resync_pacing(&mut self) {
        self.next_frame = Instant::now();
        self.watchdog.reset();
    }

    /// After a single/over step, repaint the game window (the LCD may have
    /// advanced) and every open tool window so they track the new PC.
    fn refresh_after_step(&mut self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.tools.request_redraw_all();
    }

    /// Repaint the game window (the menu overlay changed, but emulation didn't).
    fn request_game_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// The game window's content rect in physical pixels — the area the overlay
    /// modal renders into, so a click hit-tests against the same bounds (MN4).
    fn window_area(&self) -> Rect {
        self.window.as_ref().map_or(Rect::new(0, 0, 0, 0), |w| {
            let s = w.inner_size();
            Rect::new(0, 0, s.width as i32, s.height as i32)
        })
    }

    /// Basenames of the recent ROMs for the Recent ROMs submenu (MN4).
    fn recent_names(&self) -> Vec<String> {
        self.recent
            .iter()
            .map(|p| {
                p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            })
            .collect()
    }

    /// Open the cpal output stream if it isn't already open. Called at startup
    /// (when not launched `--mute`) and when "Enable sound" is toggled on after a
    /// muted start, so the menu toggle always restores audio. A device that won't
    /// open just leaves `audio` `None` — the timer paces, silently.
    /// The current Sound-tab device preferences (used to open the stream + to
    /// detect when a re-open is needed on Apply).
    fn audio_prefs(&self) -> audio::AudioPrefs {
        audio::AudioPrefs {
            device: self.settings.audio_device.clone(),
            sample_rate: self.settings.audio_sample_rate,
            latency_frames: audio_latency_frames(self.settings.audio_latency),
            eight_bit: self.settings.audio_8bit,
        }
    }

    /// Open the audio stream if it isn't already. Returns the device error on
    /// failure so a user-initiated open (Enable sound / a Sound-tab device change)
    /// can surface it in a modal; the passive startup open just logs it, to avoid
    /// nagging a deliberately audio-less (headless / VM) run every launch.
    fn try_open_audio(&mut self) -> Result<(), String> {
        if self.audio.is_some() {
            return Ok(());
        }
        let prefs = self.audio_prefs();
        self.audio_prefs_applied = prefs.clone();
        self.audio_hq_applied = self.settings.audio_hq;
        match AudioOutput::with_prefs(&prefs) {
            Ok(out) => {
                let mut pipe = AudioPipe::new_with_quality(out, self.settings.audio_hq);
                pipe.set_volume(self.settings.volume, self.settings.mono);
                self.audio = Some(pipe);
                Ok(())
            }
            Err(e) => {
                eprintln!("slopgb: audio disabled: {e}");
                Err(e)
            }
        }
    }

    /// Re-open the audio stream with the current Sound-tab preferences (device /
    /// samplerate / latency / 8-bit / quality). No-op when audio isn't running
    /// (e.g. `--mute`), so it never forces the stream open behind the user.
    pub(crate) fn reopen_audio(&mut self) {
        if self.audio.is_none() {
            return;
        }
        self.audio = None;
        // A device/samplerate change the user just applied: surface a failure
        // (else the stream silently drops with no clue why sound stopped).
        if let Err(e) = self.try_open_audio() {
            self.show_error("Audio device failed", e);
        }
        self.resync_pacing();
    }

    /// The boot ROM spec for a ROM load: the Options bootrom paths (when enabled)
    /// take precedence over the `--boot`/`SLOPGB_BOOT` fallback.
    fn boot_spec(&self) -> session::BootSpec<'_> {
        session::BootSpec {
            enabled: self.settings.bootroms_enabled,
            dmg: &self.settings.bootrom_dmg,
            gbc: &self.settings.bootrom_gbc,
            sgb: &self.settings.bootrom_sgb,
            fallback: self.boot_rom.as_deref(),
        }
    }

    fn load_dropped(&mut self, path: &Path) {
        // Persist the outgoing game *before* the new session reads its .sav:
        // if the dropped file is the currently loaded ROM, loading first
        // would resurrect a stale save and later overwrite the fresh one.
        self.session.flush_save();
        let ram_init = cli::effective_ram_init(self.opts.ram_init, self.settings.uninited_wram);
        match Session::load(path, self.settings.model, &self.boot_spec(), ram_init) {
            Ok(mut new) => {
                new.set_sgb_bios(self.sgb_bios.clone());
                new.set_sf2(self.sf2.clone());
                // Carry the live plugins dir (seeded from --plugins at startup,
                // possibly re-pointed via the UI) so the SGB coprocessor plugin
                // re-injects into the fresh machine.
                new.set_plugins_dir(
                    (!self.settings.plugins.dir.is_empty())
                        .then(|| PathBuf::from(&self.settings.plugins.dir)),
                );
                new.set_rtc_vba_export(self.settings.rtc_vba_sav);
                new.set_rtc_bgb_legacy(self.settings.rtc_bgb_legacy);
                self.session = new;
                // A rejected/unreadable `.sav` is data the next save overwrites:
                // surface it in a modal (it also went to the console at load).
                if let Some(w) = self.session.load_warning.take() {
                    self.show_error("Save file ignored", w);
                }
                // A loaded ROM starts emulation: leave the no-ROM blank state and
                // (re)apply the DMG palette to the fresh machine (GameBoy::new
                // resets it to the core grayscale default).
                self.rom_loaded = true;
                self.snes_frame = None;
                self.apply_palette();
                // The fresh machine starts with no exception mask; re-arm it.
                self.apply_exceptions();
                self.paused = false;
                self.push_recent(path);
                self.arm_recovery(path);
                // Auto-load a sidecar `.sym` (foo.gb -> foo.sym) if present, so
                // symbols reach the disassembler and memory viewer without a
                // manual load. Absent sidecar = silent no-op.
                if let Some(sym) = crate::app_path::sym_sidecar(path) {
                    self.load_symbols(&sym);
                }
                self.resync_pacing();
                self.update_title();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Err(e) => {
                eprintln!("slopgb: load ignored: {e}");
                // Misc → "Show errors on ROM load": surface the failure in a
                // modal info box (bgb behaviour); otherwise it stays console-only.
                self.info_box =
                    rom_load_error_box(self.settings.show_errors_on_rom_load, &e.to_string());
            }
        }
    }
}

/// The info box shown when a ROM fails to load, or `None` when the "Show errors
/// on ROM load" option is off. A free function so the gate is unit-testable
/// without a live event loop.
fn rom_load_error_box(show: bool, msg: &str) -> Option<InfoBox> {
    show.then(|| InfoBox::new("ROM load failed", vec![msg.to_string()]))
}

/// Whether emulation should idle (emulate zero frames) this wake: when paused,
/// when the debugger has broken, or — the no-ROM startup case — when no ROM is
/// loaded (the blank machine is frozen at power-on like bgb). A free function so
/// the gate is unit-testable without a live event loop.
fn should_idle(paused: bool, broken: bool, rom_loaded: bool) -> bool {
    paused || broken || !rom_loaded
}

/// The window title: with a ROM, `"<stem> — slopgb<state>"`; with none, a bare
/// `"slopgb"` (no game name / no leading separator), matching bgb's no-ROM
/// window. A free function so the formatting is unit-testable.
fn window_title(rom_loaded: bool, title: &str, state: &str) -> String {
    if rom_loaded {
        format!("{title} — slopgb{state}")
    } else {
        "slopgb".to_owned()
    }
}

/// Whether `about_to_wait` should busy-poll instead of parking until the next
/// frame: always while turbo runs flat-out, and when "reduce CPU usage" is off
/// (spin for lowest input latency). A free function so the choice is testable.
fn should_poll(turbo: bool, reduce_cpu: bool) -> bool {
    turbo || !reduce_cpu
}

/// Map the Sound-tab latency slider fraction (0..=1) to a device buffer size in
/// frames: ~128 (low latency) to ~4096 (high). A free function so the mapping is
/// unit-testable.
fn audio_latency_frames(frac: f32) -> u32 {
    (128.0 + frac.clamp(0.0, 1.0) * (4096.0 - 128.0)) as u32
}

/// GB CPU duty percent over a sample window: the share of `delta_cycles` the CPU
/// was NOT halted (`delta_cycles - delta_halt`). 0 when no cycles elapsed (paused
/// / no ROM). A free function so it is unit-testable without a live machine.
fn cpu_usage_pct(delta_cycles: u64, delta_halt: u64) -> f64 {
    if delta_cycles == 0 {
        return 0.0;
    }
    let active = delta_cycles.saturating_sub(delta_halt);
    100.0 * active as f64 / delta_cycles as f64
}

/// Insert `path` at the front of the recent-ROMs list (MN4): de-duplicated,
/// most-recent first, capped at 10. A free function so the list logic is
/// unit-testable without a live `App`.
fn push_recent_into(recent: &mut Vec<PathBuf>, path: &Path) {
    let p = path.to_path_buf();
    recent.retain(|e| e != &p);
    recent.insert(0, p);
    recent.truncate(10);
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
