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
mod dbg;
mod input;
mod pacing;
mod screenshot;
mod session;
mod toolwin;
mod ui;
mod video;
mod windows;

use std::env;
use std::path::Path;
use std::process;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slopgb_core::{Button, CLOCK_HZ, CYCLES_PER_FRAME, SCREEN_H, SCREEN_W};
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
use ui::dialog::DialogKey;
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
    let session = match Session::load(&opts.rom, opts.model) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot create event loop: {e}");
            process::exit(1);
        }
    };
    let mut app = App::new(opts, session);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("error: event loop failed: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Application

struct App {
    opts: Options,
    session: Session,
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
    /// The current window size, for the "Window size" submenu check-mark and the
    /// stretched-fullscreen blit. Init from `--scale`.
    window_size: WindowSizeChoice,
    /// Last cursor position over the game window (physical px), so a right-click
    /// can open the menu where the pointer is.
    game_cursor: (i32, i32),
}

impl App {
    fn new(opts: Options, session: Session) -> Self {
        let muted = opts.mute;
        let window_size = WindowSizeChoice::Scale(opts.scale);
        Self {
            opts,
            session,
            window: None,
            video: None,
            audio: None,
            muted,
            paused: false,
            turbo: false,
            buttons: ButtonTracker::default(),
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
            main_menu: None,
            main_submenu: None,
            window_size,
            game_cursor: (0, 0),
        }
    }

    fn update_title(&self) {
        if let Some(window) = &self.window {
            let state = if self.dbg.is_broken() {
                " (debugging)".to_owned()
            } else if self.paused {
                " — paused".to_owned()
            } else {
                format!(" — {:.1} fps", self.fps)
            };
            window.set_title(&format!("{} — slopgb{state}", self.session.title));
        }
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(video) = self.video.as_mut() else {
            return;
        };
        let frame = self.session.gb.frame();
        // Overlay the game-window right-click menu + open submenu, if any
        // (captures locals, not `self`, so the disjoint field borrows stay clean).
        let menu = self.main_menu.as_ref();
        let sub = self.main_submenu.as_ref();
        let info = self.info_box.as_ref();
        let theme = ui::Theme::BGB;
        let stretch = self.window_size == WindowSizeChoice::FullscreenStretched;
        if let Err(e) = video.draw(window, frame, stretch, |canvas| {
            if let Some(m) = menu {
                windows::mainwin::render(canvas, m, &theme);
            }
            if let Some(s) = sub {
                windows::mainwin::render_sub(canvas, s, &theme);
            }
            // The info box draws on top of everything (it's modal).
            if let Some(i) = info {
                windows::mainwin::render_info(canvas, i, &theme);
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
        let Some(action) = input::map(code, self.modifiers, focus) else {
            return;
        };
        let pressed = key.state.is_pressed();
        match action {
            Action::Button(b) => {
                if pressed {
                    self.buttons.press(code, b);
                    self.session.gb.press(b);
                } else if self.buttons.release(code, b) {
                    self.session.gb.release(b);
                }
            }
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

    /// Open the cpal output stream if it isn't already open. Called at startup
    /// (when not launched `--mute`) and when "Enable sound" is toggled on after a
    /// muted start, so the menu toggle always restores audio. A device that won't
    /// open just leaves `audio` `None` — the timer paces, silently.
    fn try_open_audio(&mut self) {
        if self.audio.is_some() {
            return;
        }
        match AudioOutput::new() {
            Ok(out) => self.audio = Some(AudioPipe::new(out)),
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
                self.paused = false;
                self.resync_pacing();
                self.update_title();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Err(e) => eprintln!("slopgb: drop ignored: {e}"),
        }
    }
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
            .with_title(format!("{} — slopgb", self.session.title))
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
            // events, so drop all input before any button can stick.
            WindowEvent::Focused(false) | WindowEvent::Occluded(true) => self.release_all_input(),
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
        // its last frame and zero frames are emulated until F9/step.
        if self.paused || self.dbg.is_broken() {
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
