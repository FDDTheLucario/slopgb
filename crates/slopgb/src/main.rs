//! slopgb desktop frontend: CLI parsing, winit event loop, emulation pacing,
//! and battery-RAM persistence. Video lives in [`video`], audio in [`audio`],
//! the keymap in [`input`].
//!
//! `App` is split across cohesive `impl` blocks: the discrete-action dispatch
//! in [`app_run`], the game-window menu handling in [`app_menu`], and the
//! emulation pacing loop in [`app_pacing`]. One loaded ROM (the machine + save
//! persistence) is [`session::Session`]; CLI parsing is [`cli`]; the audio pipe
//! / watchdog / pacing decision are [`pacing`].
//!
//! Pacing: with audio on, emulation is driven by the audio clock — we emulate
//! exactly enough frames to keep ~50 ms queued for the cpal callback. Muted
//! (or if the device fails to open), a wall-clock loop paces frames at the
//! hardware rate, 4194304 / 70224 ≈ 59.7275 Hz.

mod app_menu;
mod app_pacing;
mod app_run;
mod audio;
mod cli;
mod clipboard;
mod dbg;
mod input;
mod keymap;
mod pacing;
mod screenshot;
mod session;
mod toolwin;
mod ui;
mod video;
mod windows;

use std::env;
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slopgb_core::{Button, CLOCK_HZ, CYCLES_PER_FRAME, SCREEN_H, SCREEN_PIXELS, SCREEN_W};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use audio::AudioOutput;
use cli::{Options, ParseOutcome, USAGE};
use input::{Action, ButtonTracker, Focus};
use pacing::{AudioPipe, StallWatchdog, audio_pacing};
use session::Session;
use ui::canvas::Rect;
use ui::dialog::{self, DialogKey, DialogResult, InputDialog};
use video::Video;
use windows::mainwin::{InfoBox, MainMenu, SubMenu, WindowSizeChoice};

/// Wall-clock duration of one emulated frame: 70224 T-cycles at 4194304 Hz
/// (~59.7275 Hz).
const FRAME_DURATION: Duration =
    Duration::from_nanos(CYCLES_PER_FRAME as u64 * 1_000_000_000 / CLOCK_HZ as u64);

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
    let (session, rom_loaded) = match &opts.rom {
        Some(rom) => match Session::load(rom, opts.model) {
            Ok(s) => (s, true),
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        },
        None => (
            Session::blank(opts.model.unwrap_or(slopgb_core::Model::Dmg)),
            false,
        ),
    };
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot create event loop: {e}");
            process::exit(1);
        }
    };
    let mut app = App::new(opts, session, rom_loaded);
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
}

struct App {
    opts: Options,
    session: Session,
    /// Whether a real ROM is loaded. `false` at a no-ROM (bgb-style) startup:
    /// the blank machine is frozen at power-on (emulation gated off) and the LCD
    /// shows [`Self::blank_frame`] until a ROM is loaded (drag-drop / Load ROM).
    rom_loaded: bool,
    /// A solid LCD-off frame (the palette's lightest shade) shown while no ROM is
    /// loaded — bgb's pale-green blank screen. Rebuilt when the palette changes.
    blank_frame: Box<[u32; SCREEN_PIXELS]>,
    window: Option<Rc<Window>>,
    video: Option<Video>,
    audio: Option<AudioPipe>,
    /// Runtime audio mute (bgb's "Enable sound" toggle). Initialised from the
    /// `--mute` flag; gates audio pacing so the pipe drains to silence without
    /// tearing down the cpal stream. See [`pacing::audio_pacing`].
    muted: bool,
    paused: bool,
    turbo: bool,
    /// Per-key hold state, so two keys mapped to one button release cleanly.
    buttons: ButtonTracker,
    /// Rebindable keyboard → Game Boy button map (Joypad "configure keyboard").
    bindings: keymap::KeyBindings,
    /// Detects a cpal stream that stopped draining (see [`StallWatchdog`]).
    watchdog: StallWatchdog,
    /// Deadline of the next frame for wall-clock pacing.
    next_frame: Instant,
    /// Scratch for draining (and discarding) APU output while muted.
    discard_buf: Vec<(f32, f32)>,
    fps_frames: u32,
    fps_since: Instant,
    fps: f64,
    /// Open bgb-style debug tool windows (F2/F3/F4). The game window is handled
    /// directly; these are routed by [`toolwin::ToolWindows::owns`].
    tools: toolwin::ToolWindows,
    /// Debugger execution control (break / step / breakpoints).
    dbg: dbg::Debugger,
    /// Current keyboard modifiers, for the focus-dependent key map (Ctrl+G).
    modifiers: ModifiersState,
    /// The open game-window right-click menu (bgb's `rc-main.png`), if any —
    /// drawn as an overlay over the LCD and routed by the game-window mouse.
    main_menu: Option<MainMenu>,
    /// The open child submenu (Window size / Sound channel / Other), drawn to the
    /// right of its parent row over the main menu.
    main_submenu: Option<SubMenu>,
    /// An open info box (Other → Cart info / System info / About), drawn centred
    /// over the LCD; any click or Escape closes it.
    info_box: Option<InfoBox>,
    /// The open path-entry modal, drawn centred over the LCD; accept routes by
    /// [`Self::path_purpose`] (Load ROM / Save state / Load state), Escape closes.
    path_dialog: Option<InputDialog>,
    /// What the open [`Self::path_dialog`] does on accept.
    path_purpose: PathPurpose,
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
    /// Last windowed integer scale chosen (CLI or Window-size menu), restored
    /// when leaving fullscreen-stretched so the menu-picked size isn't lost.
    last_scale: u32,
}

impl App {
    fn new(opts: Options, session: Session, rom_loaded: bool) -> Self {
        let muted = opts.mute;
        let scale = opts.scale;
        let window_size = WindowSizeChoice::Scale(scale);
        // Seed Options' model from the persistent `--model` preference (the value
        // reused for every ROM load), NOT the resolved session model — so it
        // can't desync when a later ROM auto-detects to a different system, and
        // Apply with the default (Auto) never force-switches the running game.
        let settings = windows::options::Settings {
            model: windows::options::ModelChoice::from_option(opts.model),
            ..windows::options::Settings::default()
        };
        let blank_frame = blank_frame(settings.dmg_palette[0]);
        let mut app = Self {
            opts,
            session,
            rom_loaded,
            blank_frame,
            settings,
            options: None,
            key_wizard: None,
            paused_by_focus: false,
            last_scale: scale,
            window: None,
            video: None,
            audio: None,
            muted,
            paused: false,
            turbo: false,
            buttons: ButtonTracker::default(),
            bindings: keymap::KeyBindings::default(),
            watchdog: StallWatchdog::new(),
            next_frame: Instant::now(),
            discard_buf: Vec::new(),
            fps_frames: 0,
            fps_since: Instant::now(),
            fps: 0.0,
            tools: toolwin::ToolWindows::new(),
            dbg: dbg::Debugger::default(),
            modifiers: ModifiersState::empty(),
            info_box: None,
            path_dialog: None,
            path_purpose: PathPurpose::LoadRom,
            recent: Vec::new(),
            main_menu: None,
            main_submenu: None,
            window_size,
            game_cursor: (0, 0),
        };
        // Push the default DMG palette (bgb's pale green) onto the freshly-built
        // machine so loaded DMG games look like bgb out of the box, not the core's
        // grayscale power-on default.
        app.apply_palette();
        // Arm the default exception-break mask (bgb's "break on invalid opcode").
        app.apply_exceptions();
        app
    }

    /// Push the current DMG palette to the live machine and rebuild the no-ROM
    /// blank frame from its lightest shade. Called after every machine (re)build
    /// (startup, ROM load) since `GameBoy::new` resets the palette to the core
    /// grayscale default; Options OK/Apply applies the palette through its own
    /// path (`apply_settings`).
    fn apply_palette(&mut self) {
        self.session.gb.set_dmg_palette(self.settings.dmg_palette);
        self.blank_frame = blank_frame(self.settings.dmg_palette[0]);
    }

    fn update_title(&self) {
        if let Some(window) = &self.window {
            let state = if self.dbg.is_broken() {
                " (debugging)".to_owned()
            } else if self.paused {
                " — paused".to_owned()
            } else if self.settings.show_framerate {
                format!(" — {:.1} fps", self.fps)
            } else {
                String::new()
            };
            window.set_title(&window_title(self.rom_loaded, &self.session.title, &state));
        }
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(video) = self.video.as_mut() else {
            return;
        };
        // With no ROM loaded the LCD shows a solid lightest-shade blank (bgb's
        // pale-green off screen); the machine is frozen so its own front buffer
        // never paints.
        let frame: &[u32; SCREEN_PIXELS] = if self.rom_loaded {
            self.session.gb.frame()
        } else {
            &self.blank_frame
        };
        // Overlay the game-window right-click menu + open submenu, if any
        // (captures locals, not `self`, so the disjoint field borrows stay clean).
        let menu = self.main_menu.as_ref();
        let sub = self.main_submenu.as_ref();
        let info = self.info_box.as_ref();
        let path_dlg = self.path_dialog.as_ref();
        let options = self.options.as_ref();
        let wizard = self.key_wizard.as_ref();
        let theme = ui::Theme::BGB;
        let stretch = self.window_size == WindowSizeChoice::FullscreenStretched;
        if let Err(e) = video.draw(window, frame, stretch, |canvas| {
            if let Some(m) = menu {
                windows::mainwin::render(canvas, m, &theme);
            }
            if let Some(s) = sub {
                windows::mainwin::render_sub(canvas, s, &theme);
            }
            // The info box / Load-ROM modal draw on top of everything (modal).
            if let Some(i) = info {
                windows::mainwin::render_info(canvas, i, &theme);
            }
            if let Some(d) = path_dlg {
                let area = canvas.bounds();
                dialog::render(canvas, area, d, &theme);
            }
            // The Options control panel draws on top of everything (modal).
            if let Some(o) = options {
                windows::options::render(canvas, o, &theme);
            }
            // The key-rebind wizard floats above even the Options dialog.
            if let Some(w) = wizard {
                w.render(canvas, &theme);
            }
        }) {
            eprintln!("slopgb: failed to present frame: {e}");
        }
    }

    /// Restart wall-clock pacing from now (after pause, turbo, load, reset),
    /// and give the audio stall watchdog a fresh grace period.
    fn resync_pacing(&mut self) {
        self.next_frame = Instant::now();
        self.watchdog.reset();
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key: &KeyEvent, focus: Focus) {
        if key.repeat {
            return;
        }
        // The key-rebind wizard (Joypad → "configure keyboard") is the topmost
        // game-window modal: every key is captured. Escape cancels the whole
        // wizard (edits discarded); any other key binds the current button and
        // advances — finishing commits the new bindings.
        if focus == Focus::Game && key.state.is_pressed() && self.key_wizard.is_some() {
            if let PhysicalKey::Code(code) = key.physical_key {
                if code == KeyCode::Escape {
                    self.key_wizard = None;
                } else if let Some(w) = self.key_wizard.as_mut() {
                    w.bind_key(code);
                    self.commit_wizard_if_done();
                }
            }
            self.request_game_redraw();
            return;
        }
        // Options control panel is modal: while it's open every key is swallowed
        // (so a hotkey can't fire underneath it); Escape cancels (reverts edits)
        // and closes, matching a Windows dialog's Esc.
        if focus == Focus::Game && key.state.is_pressed() && self.options.is_some() {
            if let PhysicalKey::Code(KeyCode::Escape) = key.physical_key {
                // Esc = Cancel: just drop the dialog without applying — the live
                // state already equals the baseline (only OK/Apply push live), so
                // discarding the unapplied `working` edits is the whole revert.
                self.options = None;
                self.request_game_redraw();
            }
            return;
        }
        // Game-window "Load ROM" modal capture (MN4): every key goes to it while
        // open (so typing a path can't fire a hotkey); Enter loads, Esc cancels.
        if focus == Focus::Game && key.state.is_pressed() && self.path_dialog.is_some() {
            if let Some(dk) = dialog_key_from(key) {
                if let Some(result) = self.path_dialog.as_mut().map(|d| d.on_key(dk)) {
                    self.resolve_path_dialog(result);
                }
            }
            return;
        }
        // With a game-window overlay open, Escape closes it (rather than quitting
        // the emulator) and is swallowed so it can't also fire a hotkey. The info
        // box peels first, then an open submenu, then the main menu.
        let overlay_open =
            self.info_box.is_some() || self.main_menu.is_some() || self.main_submenu.is_some();
        if focus == Focus::Game && key.state.is_pressed() && overlay_open {
            if let PhysicalKey::Code(KeyCode::Escape) = key.physical_key {
                if self.info_box.take().is_none() && self.main_submenu.take().is_none() {
                    self.main_menu = None;
                }
                self.request_game_redraw();
                return;
            }
        }
        // Modal capture: while the debugger's modal prompt (Go to… / edit
        // register) is open, every key goes to it (so typing an address can't
        // trigger a debugger hotkey). An `edit register` accept yields a
        // register write, applied through the same path a menu/click uses.
        if focus == Focus::Debugger && key.state.is_pressed() && self.tools.debugger_modal_active()
        {
            if let Some(dk) = dialog_key_from(key) {
                if let Some(outcome) = self.tools.feed_debugger_dialog(dk) {
                    self.apply_menu_outcome(outcome, event_loop);
                }
            }
            return;
        }
        let PhysicalKey::Code(code) = key.physical_key else {
            return;
        };
        let pressed = key.state.is_pressed();
        // Game Boy buttons resolve through the rebindable map first (any focus,
        // matching bgb's joypad bindings), before the focus-specific actions.
        if let Some(b) = self.bindings.button_for(code) {
            self.set_button(code, b, pressed);
            return;
        }
        let Some(action) = input::map(code, self.modifiers, focus) else {
            return;
        };
        match action {
            Action::Turbo => {
                self.turbo = pressed;
                if !pressed {
                    self.resync_pacing();
                }
            }
            // Every other action fires on press only; the debugger menu items
            // reuse this same dispatch via `run_action`, so a hotkey and its
            // menu entry can never diverge.
            _ if pressed => self.run_action(action, event_loop),
            _ => {}
        }
    }

    /// Open the Joypad "configure keyboard" wizard seeded from the live map.
    pub(crate) fn open_key_wizard(&mut self) {
        self.key_wizard = Some(keymap::KeyConfigWizard::open(self.bindings));
    }

    /// If the wizard has run through all eight buttons, commit its working map
    /// to the live `bindings` and close it. Any buttons held under the old map
    /// are released so a remap can't leave a key stuck down.
    pub(crate) fn commit_wizard_if_done(&mut self) {
        if let Some(bindings) = self.key_wizard.as_ref().and_then(|w| w.finished()) {
            self.bindings = bindings;
            self.release_all_input();
            self.key_wizard = None;
        }
    }

    /// Press or release a Game Boy `button` resolved from key `code`, tracking
    /// the held key so two keys mapped to one button release cleanly.
    fn set_button(&mut self, code: KeyCode, button: Button, pressed: bool) {
        if pressed {
            self.buttons.press(code, button);
            // SOCD filter (Joypad → "allow pressing L+R or U+D" off, the bgb
            // default): a new direction suppresses its opposite so the joypad
            // never reports both — last input wins.
            if let Some(opp) = keymap::socd_suppress(button, self.settings.allow_opposing) {
                self.session.gb.release(opp);
            }
            self.session.gb.press(button);
        } else if self.buttons.release(code, button) {
            self.session.gb.release(button);
            // Resurrection (last-input priority): if the opposite direction is
            // still physically held, re-press it — releasing the newer key
            // returns control to the older one that was suppressed.
            if let Some(opp) = keymap::socd_suppress(button, self.settings.allow_opposing) {
                if self.buttons.is_held(opp) {
                    self.session.gb.press(opp);
                }
            }
        }
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

    /// Open the shared path-entry modal for `purpose` over the LCD.
    pub(crate) fn open_path_prompt(&mut self, title: &str, purpose: PathPurpose) {
        self.path_purpose = purpose;
        self.path_dialog = Some(crate::ui::dialog::InputDialog::new(title, false));
        self.request_game_redraw();
    }

    /// Apply a path-modal result: accept routes by [`Self::path_purpose`] (a
    /// blank entry just closes), cancel closes; continue keeps editing.
    fn resolve_path_dialog(&mut self, result: DialogResult) {
        match result {
            DialogResult::Accept(path) => {
                let purpose = self.path_purpose;
                self.path_dialog = None;
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    self.run_path_action(purpose, Path::new(trimmed));
                }
            }
            DialogResult::Cancel => self.path_dialog = None,
            DialogResult::Continue => {}
        }
        self.request_game_redraw();
    }

    /// Carry out an accepted path entry per its purpose.
    fn run_path_action(&mut self, purpose: PathPurpose, path: &Path) {
        match purpose {
            PathPurpose::LoadRom => self.load_dropped(path),
            PathPurpose::SaveState => match self.session.save_state_to(path) {
                Ok(()) => println!("slopgb: saved state to {}", path.display()),
                Err(e) => eprintln!("slopgb: save state failed: {e}"),
            },
            PathPurpose::LoadState => match self.session.load_state_from(path) {
                Ok(()) => {
                    println!("slopgb: loaded state from {}", path.display());
                    self.resync_pacing();
                    self.request_game_redraw();
                }
                Err(e) => eprintln!("slopgb: load state failed: {e}"),
            },
        }
    }

    /// Record a successfully loaded ROM in the recent list (MN4). Skipped when
    /// Options → Misc → "freeze recent ROMs menu" is set (bgb pins the list).
    fn push_recent(&mut self, path: &Path) {
        if self.settings.freeze_recent {
            return;
        }
        push_recent_into(&mut self.recent, path);
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
    fn try_open_audio(&mut self) {
        if self.audio.is_some() {
            return;
        }
        match AudioOutput::new() {
            Ok(out) => {
                let mut pipe = AudioPipe::new(out);
                pipe.set_volume(self.settings.volume, self.settings.mono);
                self.audio = Some(pipe);
            }
            Err(e) => eprintln!("slopgb: audio disabled: {e}"),
        }
    }

    /// Focus lost or window occluded: no release events will arrive for keys
    /// held right now, so release every Game Boy button and drop turbo before
    /// they stick.
    fn release_all_input(&mut self) {
        self.buttons.clear();
        for b in [
            Button::A,
            Button::B,
            Button::Select,
            Button::Start,
            Button::Up,
            Button::Down,
            Button::Left,
            Button::Right,
        ] {
            self.session.gb.release(b);
        }
        if self.turbo {
            self.turbo = false;
            self.resync_pacing();
        }
    }

    fn load_dropped(&mut self, path: &Path) {
        // Persist the outgoing game *before* the new session reads its .sav:
        // if the dropped file is the currently loaded ROM, loading first
        // would resurrect a stale save and later overwrite the fresh one.
        self.session.flush_save();
        match Session::load(path, self.opts.model) {
            Ok(new) => {
                self.session = new;
                // A loaded ROM starts emulation: leave the no-ROM blank state and
                // (re)apply the DMG palette to the fresh machine (GameBoy::new
                // resets it to the core grayscale default).
                self.rom_loaded = true;
                self.apply_palette();
                // The fresh machine starts with no exception mask; re-arm it.
                self.apply_exceptions();
                self.paused = false;
                self.push_recent(path);
                self.resync_pacing();
                self.update_title();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Err(e) => eprintln!("slopgb: load ignored: {e}"),
        }
    }
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

/// A solid LCD frame filled with `color` (the palette's lightest shade) — the
/// no-ROM blank screen. A free function so the fill is unit-testable.
fn blank_frame(color: u32) -> Box<[u32; SCREEN_PIXELS]> {
    Box::new([color; SCREEN_PIXELS])
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

/// Translate a winit key event into an abstract [`DialogKey`] for the modal
/// prompt: the named editing keys (backspace / enter / escape), else the typed
/// character.
fn dialog_key_from(key: &KeyEvent) -> Option<DialogKey> {
    if let PhysicalKey::Code(code) = key.physical_key {
        match code {
            KeyCode::Backspace => return Some(DialogKey::Backspace),
            KeyCode::Enter | KeyCode::NumpadEnter => return Some(DialogKey::Enter),
            KeyCode::Escape => return Some(DialogKey::Escape),
            _ => {}
        }
    }
    let ch = key.text.as_ref()?.chars().next()?;
    (!ch.is_control()).then_some(DialogKey::Char(ch))
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let scale = self.opts.scale;
        let attrs = Window::default_attributes()
            .with_title(window_title(self.rom_loaded, &self.session.title, ""))
            .with_inner_size(LogicalSize::new(
                f64::from(SCREEN_W as u32 * scale),
                f64::from(SCREEN_H as u32 * scale),
            ))
            .with_min_inner_size(LogicalSize::new(SCREEN_W as f64, SCREEN_H as f64));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("error: cannot create window: {e}");
                event_loop.exit();
                return;
            }
        };
        match Video::new(window.clone()) {
            Ok(v) => self.video = Some(v),
            Err(e) => {
                eprintln!("error: cannot create render surface: {e}");
                event_loop.exit();
                return;
            }
        }
        if !self.opts.mute {
            self.try_open_audio();
        }
        self.window = Some(window);
        // Optionally open debug tool windows at startup (comma-separated
        // `debugger,vram,iomap` in `SLOPGB_OPEN_TOOLS`) — handy for screenshot
        // verification and for users who always want them up.
        if let Ok(list) = env::var("SLOPGB_OPEN_TOOLS") {
            for kind in list.split(',').filter_map(|s| match s.trim() {
                "debugger" => Some(ui::ToolWindow::Debugger),
                "vram" => Some(ui::ToolWindow::Vram),
                "iomap" => Some(ui::ToolWindow::IoMap),
                _ => None,
            }) {
                self.tools.toggle(event_loop, kind);
            }
        }
        self.resync_pacing();
        self.update_title();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Track keyboard modifiers for the focus-dependent key map (Ctrl+G),
        // regardless of which window has focus.
        if let WindowEvent::ModifiersChanged(m) = &event {
            self.modifiers = m.state();
            return;
        }
        // A debug tool window owns its events; the game window path below is
        // untouched. Its close button closes just that window (not the app).
        if self.tools.owns(window_id) {
            match event {
                WindowEvent::CloseRequested => {
                    self.tools.close(window_id);
                }
                WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {
                    self.tools
                        .redraw(window_id, &self.session.gb, self.dbg.breakpoints());
                }
                // Mouse drives the tool windows' tabs/checkboxes/details and the
                // debugger's context menu (left selects/acts, right opens it).
                WindowEvent::CursorMoved { position, .. } => {
                    self.tools
                        .on_cursor_moved(window_id, position.x, position.y);
                }
                WindowEvent::CursorLeft { .. } => self.tools.on_cursor_left(window_id),
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button,
                    ..
                } if matches!(button, MouseButton::Left | MouseButton::Right) => {
                    let outcome = if button == MouseButton::Left {
                        self.tools.on_mouse_left(window_id, &self.session.gb)
                    } else {
                        self.tools.on_mouse_right(window_id, &self.session.gb)
                    };
                    if let Some(outcome) = outcome {
                        self.apply_menu_outcome(outcome, event_loop);
                    }
                }
                // Hotkeys route by focused window kind: the debugger window gets
                // bgb's debugger keys, the viewers keep the game keymap.
                WindowEvent::KeyboardInput { event, .. } => {
                    let focus = if self.tools.kind_of(window_id) == Some(ui::ToolWindow::Debugger) {
                        Focus::Debugger
                    } else {
                        Focus::Game
                    };
                    self.handle_key(event_loop, &event, focus);
                }
                _ => {}
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => self.load_dropped(&path),
            // Focus loss and occlusion both mean held keys won't get release
            // events, so drop all input before any button can stick. With
            // Options → Misc → "Pause if losing focus" set, also pause.
            WindowEvent::Focused(false) | WindowEvent::Occluded(true) => {
                self.release_all_input();
                if self.settings.pause_on_focus_loss && !self.paused {
                    self.paused = true;
                    self.paused_by_focus = true;
                    self.session.flush_save();
                    self.update_title();
                }
            }
            // Refocus auto-resumes, but only a pause we induced — a manual pause
            // (P) stays put (bgb's "Pause if losing focus" resume behaviour).
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                if self.paused_by_focus && self.paused {
                    self.paused = false;
                    self.resync_pacing();
                    self.update_title();
                }
                self.paused_by_focus = false;
            }
            // Track the pointer (for opening the menu where it sits) and, with a
            // menu open, highlight the hovered row of the frontmost popup.
            WindowEvent::CursorMoved { position, .. } => {
                self.game_cursor = (position.x as i32, position.y as i32);
                let (px, py) = self.game_cursor;
                let changed = if let Some(s) = &mut self.main_submenu {
                    s.hover_at(px, py)
                } else if let Some(m) = &mut self.main_menu {
                    m.hover_at(px, py)
                } else {
                    false
                };
                if changed {
                    self.request_game_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } if matches!(button, MouseButton::Left | MouseButton::Right) => {
                self.on_game_click(button, event_loop);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_key(event_loop, &event, Focus::Game)
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            return; // not resumed yet
        }
        // A debugger break freezes emulation exactly like pause: the LCD holds
        // its last frame and zero frames are emulated until F9/step. With no ROM
        // loaded the blank machine is likewise frozen (bgb's no-ROM screen).
        if should_idle(self.paused, self.dbg.is_broken(), self.rom_loaded) {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let (frames, hit_bp) = if self.turbo {
            self.run_turbo()
        } else if audio_pacing(self.audio.is_some(), self.muted) {
            self.run_audio_paced()
        } else {
            self.run_timer_paced()
        };
        // A free-running breakpoint hit freezes the debugger; the top guard then
        // idles to `Wait` on the next wake (bgb's halt-at-breakpoint).
        if hit_bp {
            self.dbg.set_broken(true);
            self.update_title();
        }
        // A dead stream would otherwise pin `frames` at 0 forever.
        self.check_audio_health(frames);
        if frames > 0 {
            self.session.autosave();
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            // Keep the open debug windows tracking live machine state.
            self.tools.request_redraw_all();
        }
        self.update_fps(frames);
        let flow = if self.turbo {
            ControlFlow::Poll
        } else if audio_pacing(self.audio.is_some(), self.muted) {
            // Wake well before ~50 ms of queued audio can drain.
            ControlFlow::WaitUntil(Instant::now() + FRAME_DURATION / 4)
        } else {
            ControlFlow::WaitUntil(self.next_frame)
        };
        event_loop.set_control_flow(flow);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.session.flush_save();
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
