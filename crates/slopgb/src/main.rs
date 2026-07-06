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

mod app_handler;
mod app_input;
mod app_menu;
mod app_pacing;
mod app_path;
mod app_run;
mod audio;
mod cdl;
mod cheat;
mod cheat_ui;
mod cli;
mod clipboard;
mod dbg;
mod fallback_picker;
mod filepicker;
mod input;
mod keymap;
mod link;
mod menupopup;
mod pacing;
mod screenshot;
mod session;
mod settings_file;
mod symbols;
mod toolwin;
mod ui;
mod video;
mod windows;

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slopgb_core::{Button, CLOCK_HZ, CYCLES_PER_FRAME, SCREEN_PIXELS};
use winit::event::KeyEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::Window;

use audio::AudioOutput;
use cli::{Options, ParseOutcome, USAGE};
use input::{Action, ButtonTracker, Focus};
use menupopup::MenuPopup;
use pacing::{AudioPipe, StallWatchdog};
use session::Session;
use ui::canvas::Rect;
use ui::dialog::{self, DialogKey, InputDialog};
use video::Video;
use windows::mainwin::{InfoBox, WindowSizeChoice};

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
    // Optional boot ROM (--boot / SLOPGB_BOOT): executed from power-on by every
    // ROM load. Read once here; a bad path is logged and treated as no boot ROM.
    let boot_rom = resolve_boot_rom(&opts);
    let (session, rom_loaded) = match &opts.rom {
        Some(rom) => match Session::load(
            rom,
            opts.model,
            &session::BootSpec::cli(boot_rom.as_deref()),
        ) {
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
    let mut app = App::new(opts, session, rom_loaded, boot_rom);
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
    /// Set a bootrom path in the open Options dialog's working scratch
    /// (Options → System → DMG/GBC/SGB bootrom `...`).
    Bootrom(windows::options::BootromSlot),
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
}

/// Resolve the boot ROM bytes from `--boot` or the `SLOPGB_BOOT` env var,
/// reading the file. A read error is logged and treated as no boot ROM
/// (non-fatal) — the machine then boots post-boot as usual.
fn resolve_boot_rom(opts: &Options) -> Option<Vec<u8>> {
    let path = opts
        .boot
        .clone()
        .or_else(|| env::var_os("SLOPGB_BOOT").map(PathBuf::from))?;
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            eprintln!("slopgb: cannot read boot ROM '{}': {e}", path.display());
            None
        }
    }
}

struct App {
    opts: Options,
    /// Boot ROM bytes (from `--boot`/`SLOPGB_BOOT`), executed from power-on on
    /// every ROM load. `None` = the direct post-boot install (default).
    boot_rom: Option<Vec<u8>>,
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
    /// The in-app fallback file browser ([`fallback_picker::FallbackPicker`]),
    /// opened instead of [`Self::path_dialog`] for FILE purposes when no
    /// native picker tool is installed (`PickKind::None` — the link
    /// host:port prompt — always keeps the typed modal).
    fallback_picker: Option<fallback_picker::FallbackPicker>,
    /// Time + position of the last left-press on [`Self::fallback_picker`]'s
    /// list, for synthesizing a double-click (winit delivers no such event;
    /// mirrors `toolwin::ToolView::note_click`).
    fallback_last_click: Option<(Instant, i32, i32)>,
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
    /// Serial Link-cable transport (bgb's Link submenu). Inert until Listen /
    /// Connect; pumped once per emulated frame to swap bytes with the peer.
    link: link::Link,
}

impl App {
    fn new(opts: Options, session: Session, rom_loaded: bool, boot_rom: Option<Vec<u8>>) -> Self {
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
        let settings = windows::options::Settings {
            model: match opts.model {
                Some(m) => windows::options::ModelChoice::from_option(Some(m)),
                None => loaded.settings.model,
            },
            ..loaded.settings
        };
        let blank_frame = blank_frame(settings.dmg_palette[0]);
        let mut app = Self {
            opts,
            boot_rom,
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
            input_ops: Vec::new(),
            input_offset: 0,
            epoch: Instant::now(),
            watchdog: StallWatchdog::new(),
            next_frame: Instant::now(),
            discard_buf: Vec::new(),
            fps_frames: 0,
            fps_since: Instant::now(),
            fps: 0.0,
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
            fallback_picker: None,
            fallback_last_click: None,
            link: link::Link::new(),
            recent,
            menu_popup: None,
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
            let mut title = window_title(self.rom_loaded, &self.session.title, &state);
            // The serial-link status (bgb shows it in the title bar) is appended
            // after window_title so it shows even at the no-ROM startup screen,
            // whose title is otherwise a bare "slopgb".
            if let Some(link) = self.link.status_label() {
                title.push_str(&format!(" — {link}"));
            }
            window.set_title(&title);
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
        // The right-click menu is its own window now (see `menupopup`), so it is
        // not part of the game-window overlay. The remaining overlays (info box /
        // Options / path modal / key wizard) stay centred/modal here. (Captures
        // locals, not `self`, so the disjoint field borrows stay clean.)
        let info = self.info_box.as_ref();
        let cheat = self.cheat_dialog.as_ref();
        let cheat_list = &self.cheats;
        let path_dlg = self.path_dialog.as_ref();
        // `&mut` (not `&ref`, unlike the other overlays): the picker's `view()`
        // is a live widget call, not a plain read — see `fallback_picker.rs`.
        // Still a disjoint field borrow from `video`/`options`/etc above, and
        // `video.draw`'s overlay is `FnOnce`, so moving this `Option<&mut _>`
        // into the closure (called exactly once) borrow-checks cleanly.
        let fallback = self.fallback_picker.as_mut();
        let options = self.options.as_ref();
        let wizard = self.key_wizard.as_ref();
        let theme = ui::Theme::BGB;
        let stretch = self.window_size == WindowSizeChoice::FullscreenStretched;
        if let Err(e) = video.draw(window, frame, stretch, |canvas| {
            // The info box / Load-ROM modal draw on top of everything (modal).
            if let Some(i) = info {
                windows::mainwin::render_info(canvas, i, &theme);
            }
            // The Cheat dialog draws as a modal over the LCD.
            if let Some(cd) = cheat {
                cheat_ui::render(canvas, cd, cheat_list, &theme);
            }
            // The Options control panel draws on top of the menus/info box.
            if let Some(o) = options {
                windows::options::render(canvas, o, &theme);
            }
            // A path modal draws above Options too — it can float over the dialog
            // (the bootrom `...` browse) as well as stand alone.
            if let Some(d) = path_dlg {
                let area = canvas.bounds();
                dialog::render(canvas, area, d, &theme);
            }
            // The in-app fallback file browser is the same kind of standalone
            // overlay as the path modal (never open at the same time as it).
            if let Some(fp) = fallback {
                let area = canvas.bounds();
                fp.render(canvas, area.w, area.h, &theme);
            }
            // The key-rebind wizard floats above even the Options dialog.
            if let Some(w) = wizard {
                w.render(canvas, &theme);
            }
        }) {
            eprintln!("slopgb: failed to present frame: {e}");
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

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key: &KeyEvent, focus: Focus) {
        // In the debugger, memory-nav keys (arrows / PageUp-Down) auto-repeat so a
        // held arrow scrolls the memory pane continuously; every other key — and
        // the same arrows in the game window, where they are the D-pad — is
        // de-repeated (see the guards below).
        let nav = focus == Focus::Debugger
            && matches!(
                key.physical_key,
                PhysicalKey::Code(
                    KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::PageUp | KeyCode::PageDown
                )
            );
        if key.repeat && !nav {
            return;
        }
        // Platform-independent key-repeat guard: some Wayland compositors don't
        // set winit's `repeat` flag, so a held step key (F7/F3/F8) would step
        // repeatedly. Drop a press for an already-held key; always honor releases.
        if let PhysicalKey::Code(code) = key.physical_key {
            if !nav && !input::accept_key(&mut self.held_keys, code, key.state.is_pressed()) {
                return;
            }
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
        // A path modal captures every key while open (so typing a path can't
        // fire a hotkey); Enter accepts, Esc cancels. Checked before Options
        // because it can float over the dialog (the bootrom `...` browse).
        if focus == Focus::Game && key.state.is_pressed() && self.path_dialog.is_some() {
            if let Some(dk) = dialog_key_from(key) {
                if let Some(result) = self.path_dialog.as_mut().map(|d| d.on_key(dk)) {
                    self.resolve_path_dialog(result);
                }
            }
            return;
        }
        // The in-app fallback file browser (no native picker tool installed):
        // same capture rule as the path modal above, translated through
        // `fallback_picker::winit_key_to_picker` instead of `dialog_key_from`.
        if focus == Focus::Game && key.state.is_pressed() && self.fallback_picker.is_some() {
            if let PhysicalKey::Code(code) = key.physical_key {
                if let Some(pk) =
                    fallback_picker::winit_key_to_picker(code, key.text.as_deref(), self.modifiers)
                {
                    let outcome = self.fallback_picker.as_mut().map(|fp| fp.feed_key(pk));
                    self.resolve_fallback_picker(outcome);
                }
            }
            return;
        }
        // The Cheat dialog captures keys while open. An open Add/Edit entry takes
        // every key (typing a code can't fire a hotkey); otherwise arrows move the
        // selection, Space toggles enable, Delete removes, Escape closes.
        if focus == Focus::Game && key.state.is_pressed() && self.cheat_dialog.is_some() {
            if self.cheat_dialog.as_ref().is_some_and(cheat_ui::CheatDialog::input_open) {
                if let Some(dk) = dialog_key_from(key) {
                    let edit = self.cheat_dialog.as_mut().and_then(|d| d.input_key(dk));
                    if let Some(e) = edit {
                        self.apply_cheat_edit(&e);
                    }
                }
            } else if let PhysicalKey::Code(code) = key.physical_key {
                let sel = self.cheat_dialog.as_ref().map_or(0, |d| d.sel);
                match code {
                    KeyCode::Escape => self.cheat_dialog = None,
                    KeyCode::ArrowUp => {
                        if let Some(d) = &mut self.cheat_dialog {
                            d.sel = d.sel.saturating_sub(1);
                        }
                    }
                    KeyCode::ArrowDown => {
                        let n = self.cheats.len();
                        if let Some(d) = &mut self.cheat_dialog {
                            d.sel = (d.sel + 1).min(n.saturating_sub(1));
                        }
                    }
                    KeyCode::Space => {
                        self.cheats.toggle(sel);
                    }
                    KeyCode::Delete => {
                        self.cheats.remove(sel);
                        self.clamp_cheat_sel();
                    }
                    _ => {}
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
        // With a game-window overlay open, Escape closes it (rather than quitting
        // the emulator) and is swallowed so it can't also fire a hotkey. The info
        // box peels first; the right-click popup (its own window) also closes on
        // its own Escape, but close it here too in case the game window kept focus.
        let overlay_open = self.info_box.is_some() || self.menu_popup.is_some();
        if focus == Focus::Game && key.state.is_pressed() && overlay_open {
            if let PhysicalKey::Code(KeyCode::Escape) = key.physical_key {
                if self.info_box.take().is_none() {
                    self.menu_popup = None;
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
        // Game Boy buttons resolve through the rebindable map first, before the
        // focus-specific actions — but only in the game window. A tool window
        // (e.g. the debugger) must not drive the joypad, so its arrow keys can
        // scroll the memory pane instead of moving the D-pad.
        if focus == Focus::Game {
            if let Some(b) = self.bindings.button_for(code) {
                self.set_button(code, b, pressed);
                return;
            }
        }
        // bgb shows the debugger on Esc — it never quits the emulator. Handled
        // here (not in the pure `input::map`) because honouring the Options
        // "pressing Esc shows debugger" toggle needs the runtime setting. Toggles
        // from any focus (game/viewer opens, debugger closes); the modal guards
        // above already consumed Esc where a dialog was open. BUG-1.
        if code == KeyCode::Escape {
            if pressed && self.settings.esc_shows_debugger {
                self.run_action(Action::ToggleTool(ui::ToolWindow::Debugger), event_loop);
            }
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
        match Session::load(path, self.opts.model, &self.boot_spec()) {
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
pub(crate) fn dialog_key_from(key: &KeyEvent) -> Option<DialogKey> {
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

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
