//! slopgb desktop frontend: CLI parsing, winit event loop, emulation pacing,
//! and battery-RAM persistence. Video lives in [`video`], audio in [`audio`],
//! the keymap in [`input`].
//!
//! Pacing: with audio on, emulation is driven by the audio clock — we emulate
//! exactly enough frames to keep ~50 ms queued for the cpal callback. Muted
//! (or if the device fails to open), a wall-clock loop paces frames at the
//! hardware rate, 4194304 / 70224 ≈ 59.7275 Hz.

mod audio;
mod dbg;
mod input;
mod toolwin;
mod ui;
mod video;
mod windows;

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;
use std::time::{Duration, Instant};

// The APU's default output rate, exported by the core so the resampler ratio
// can't silently drift from it. The frontend resamples it to the device rate;
// once `GameBoy` exposes `set_sample_rate`, set it to the device rate and the
// resampler becomes a pass-through.
use slopgb_core::DEFAULT_SAMPLE_RATE as CORE_SAMPLE_RATE;
use slopgb_core::{Button, CLOCK_HZ, CYCLES_PER_FRAME, GameBoy, Model, SCREEN_H, SCREEN_W};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use audio::{AudioOutput, Resampler};
use input::{Action, ButtonTracker, Focus};
use ui::dialog::DialogKey;
use video::Video;

const USAGE: &str = "\
slopgb — Game Boy / Game Boy Color emulator

USAGE:
    slopgb <rom.gb|.gbc> [OPTIONS]

OPTIONS:
    --model <MODEL>   Hardware model: dmg, dmg0, mgb, sgb, sgb2, cgb, agb
                      (default: auto-detect from the ROM header)
    --scale <N>       Initial window scale factor, 1-16 (default: 3)
    --mute            Disable audio output
    -h, --help        Print this help

KEYS:
    Z = A        X = B        Enter = Start    RShift/Backspace = Select
    Arrow keys = D-pad        Tab (hold) = turbo
    P = pause    R = reset    Esc = quit       F9 = break/resume
    F2 = debugger    F3 = VRAM viewer    F4 = I/O map  (bgb-style debug windows)

When the debugger window is focused its keys follow bgb: F2 toggle breakpoint,
F3 step over, F7 trace (step), F4 run to cursor, Ctrl+G go to, F5/F10 open the
VRAM viewer / I/O map. Right-click a debugger pane for its context menu.

A ROM file dropped onto the window is loaded in place of the current one.
Set SLOPGB_OPEN_TOOLS=debugger,vram,iomap to open debug windows at startup.
";

/// Wall-clock duration of one emulated frame: 70224 T-cycles at 4194304 Hz
/// (~59.7275 Hz).
const FRAME_DURATION: Duration =
    Duration::from_nanos(CYCLES_PER_FRAME as u64 * 1_000_000_000 / CLOCK_HZ as u64);

/// Audio-driven pacing keeps about this much queued for the device.
const AUDIO_TARGET_MS: u64 = 50;

/// Audio-paced emulation falls back to wall-clock pacing if the device queue
/// stops draining for this long (the cpal stream stalled or died without
/// reporting an error).
const AUDIO_STALL_TIMEOUT: Duration = Duration::from_secs(1);

/// Autosave battery RAM every 5 seconds of emulated time.
const AUTOSAVE_CYCLES: u64 = 5 * CLOCK_HZ as u64;

/// Upper bound on frames emulated per event-loop wake (non-turbo), so a host
/// that can't keep up stays responsive instead of spiraling.
const MAX_FRAMES_PER_WAKE: u32 = 8;

/// Wall-clock emulation budget per wake while turbo is held.
const TURBO_BUDGET: Duration = Duration::from_millis(10);

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
// CLI

struct Options {
    rom: PathBuf,
    model: Option<Model>,
    scale: u32,
    mute: bool,
}

/// What a successful argument parse asks the program to do. Printing the
/// help text (and exiting) is the caller's job, keeping `parse` pure and
/// testable.
enum ParseOutcome {
    Run(Options),
    Help,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<ParseOutcome, String> {
        let mut rom = None;
        let mut model = None;
        let mut scale = 3u32;
        let mut mute = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(ParseOutcome::Help),
                "--mute" => mute = true,
                "--model" => {
                    let v = args.next().ok_or("--model requires a value")?;
                    model = Some(parse_model(&v)?);
                }
                "--scale" => {
                    let v = args.next().ok_or("--scale requires a value")?;
                    scale = v
                        .parse::<u32>()
                        .ok()
                        .filter(|n| (1..=16).contains(n))
                        .ok_or_else(|| format!("invalid --scale '{v}' (expected 1-16)"))?;
                }
                s if s.starts_with('-') => return Err(format!("unknown option '{s}'")),
                _ => {
                    if rom.is_some() {
                        return Err(format!("unexpected extra argument '{arg}'"));
                    }
                    rom = Some(PathBuf::from(arg));
                }
            }
        }
        let rom = rom.ok_or("missing ROM path")?;
        Ok(ParseOutcome::Run(Self {
            rom,
            model,
            scale,
            mute,
        }))
    }
}

fn parse_model(s: &str) -> Result<Model, String> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "dmg" => Model::Dmg,
        "dmg0" => Model::Dmg0,
        "mgb" => Model::Mgb,
        "sgb" => Model::Sgb,
        "sgb2" => Model::Sgb2,
        "cgb" => Model::Cgb,
        "agb" => Model::Agb,
        _ => {
            return Err(format!(
                "unknown model '{s}' (expected dmg, dmg0, mgb, sgb, sgb2, cgb or agb)"
            ));
        }
    })
}

// ---------------------------------------------------------------------------
// Emulation session (one loaded ROM)

struct Session {
    gb: GameBoy,
    /// Original ROM image, kept for reset.
    rom_bytes: Vec<u8>,
    model: Model,
    /// ROM file stem, for the window title.
    title: String,
    sav_path: PathBuf,
    /// Last battery-RAM image written to disk (dirty check).
    last_saved: Option<Vec<u8>>,
    /// Emulated-cycle deadline for the next autosave.
    next_autosave: u64,
}

impl Session {
    /// Load a ROM, pick its model (CLI override beats header auto-detect),
    /// and restore `<rom>.sav` if present.
    fn load(path: &Path, model_override: Option<Model>) -> Result<Self, String> {
        let rom_bytes =
            fs::read(path).map_err(|e| format!("cannot read ROM '{}': {e}", path.display()))?;
        let model = model_override.unwrap_or_else(|| GameBoy::auto_model(&rom_bytes));
        let mut gb = GameBoy::new(model, rom_bytes.clone())
            .map_err(|e| format!("cannot load ROM '{}': {e}", path.display()))?;
        let sav_path = path.with_extension("sav");
        let mut last_saved = None;
        match fs::read(&sav_path) {
            Ok(data) => {
                if gb.load_save_data(&data) {
                    last_saved = Some(data);
                } else {
                    eprintln!(
                        "slopgb: ignoring save file '{}' (wrong size or no battery)",
                        sav_path.display()
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!(
                "slopgb: cannot read save file '{}': {e}",
                sav_path.display()
            ),
        }
        let title = path
            .file_stem()
            .map_or_else(|| "rom".to_owned(), |s| s.to_string_lossy().into_owned());
        Ok(Self {
            gb,
            rom_bytes,
            model,
            title,
            sav_path,
            last_saved,
            next_autosave: AUTOSAVE_CYCLES,
        })
    }

    /// Power-cycle: fresh machine, save RAM reloaded from disk.
    fn reset(&mut self) {
        self.flush_save();
        match GameBoy::new(self.model, self.rom_bytes.clone()) {
            Ok(mut gb) => {
                if let Ok(data) = fs::read(&self.sav_path) {
                    let _ = gb.load_save_data(&data); // rejection already warned at load
                }
                self.gb = gb;
                self.next_autosave = AUTOSAVE_CYCLES;
            }
            // Can't happen (the same image loaded before), but never panic.
            Err(e) => eprintln!("slopgb: reset failed: {e}"),
        }
    }

    /// Write battery RAM to `<rom>.sav` if it changed since the last write.
    fn flush_save(&mut self) {
        let Some(data) = self.gb.save_data() else {
            return; // cartridge has no battery RAM
        };
        if self.last_saved.as_deref() == Some(data.as_slice()) {
            return;
        }
        match write_atomic(&self.sav_path, &data) {
            Ok(()) => self.last_saved = Some(data),
            Err(e) => eprintln!(
                "slopgb: cannot write save file '{}': {e}",
                self.sav_path.display()
            ),
        }
    }

    /// Flush battery RAM at most once per [`AUTOSAVE_CYCLES`] of emulated time.
    fn autosave(&mut self) {
        if self.gb.cycles() >= self.next_autosave {
            self.next_autosave = self.gb.cycles().saturating_add(AUTOSAVE_CYCLES);
            self.flush_save();
        }
    }
}

/// Write `data` to `path` via a temp file, fsync and rename, so a crash —
/// of the process or the whole machine — mid-write never truncates an
/// existing save: the data blocks are durable before the rename can commit.
fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("sav.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    // Best effort: also persist the rename itself (the directory entry), so
    // power loss right after the rename can't roll back to the old contents.
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Audio pipeline (emulation side)

struct AudioPipe {
    out: AudioOutput,
    resampler: Resampler,
    /// Queue fill target in device-rate frames (~[`AUDIO_TARGET_MS`]).
    target_fill: usize,
    /// Scratch: samples drained from the core (core rate).
    drain_buf: Vec<(f32, f32)>,
    /// Scratch: resampled samples (device rate).
    device_buf: Vec<(f32, f32)>,
}

impl AudioPipe {
    fn new(out: AudioOutput) -> Self {
        let rate = out.sample_rate();
        Self {
            resampler: Resampler::new(CORE_SAMPLE_RATE, rate),
            target_fill: usize::try_from(u64::from(rate) * AUDIO_TARGET_MS / 1000)
                .unwrap_or(usize::MAX),
            out,
            drain_buf: Vec::new(),
            device_buf: Vec::new(),
        }
    }

    /// Move all pending core samples to the device queue (resampling on the
    /// way). Excess beyond the queue capacity is dropped by `push`.
    fn pump(&mut self, gb: &mut GameBoy) {
        self.drain_buf.clear();
        gb.drain_audio(&mut self.drain_buf);
        self.device_buf.clear();
        self.resampler.run(&self.drain_buf, &mut self.device_buf);
        self.out.push(&self.device_buf);
    }

    fn needs_more(&self) -> bool {
        self.out.queued() < self.target_fill
    }
}

/// Watchdog for a dead audio stream. Audio-paced emulation only makes
/// progress when the device drains the queue, so "zero frames emulated and
/// the queue level never dropping" sustained for [`AUDIO_STALL_TIMEOUT`]
/// means the stream is stalled even if cpal never reported an error — and
/// without intervention the emulator would silently freeze.
struct StallWatchdog {
    /// Queue level at the last observation.
    last_queued: usize,
    /// Last time the queue drained or emulation produced frames.
    progress_at: Instant,
}

impl StallWatchdog {
    fn new() -> Self {
        Self {
            last_queued: usize::MAX,
            progress_at: Instant::now(),
        }
    }

    /// Restart the grace period (after pause, resume, audio re-open).
    fn reset(&mut self) {
        self.last_queued = usize::MAX;
        self.progress_at = Instant::now();
    }

    /// Record one wake's outcome; true if the stream looks stalled.
    fn is_stalled(&mut self, frames_emulated: u32, queued: usize, now: Instant) -> bool {
        if frames_emulated > 0 || queued < self.last_queued {
            self.last_queued = queued;
            self.progress_at = now;
            return false;
        }
        self.last_queued = queued;
        now.duration_since(self.progress_at) > AUDIO_STALL_TIMEOUT
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
    /// directly; these are routed by [`ToolWindows::owns`].
    tools: toolwin::ToolWindows,
    /// Debugger execution control (break / step / breakpoints).
    dbg: dbg::Debugger,
    /// Current keyboard modifiers, for the focus-dependent key map (Ctrl+G).
    modifiers: ModifiersState,
}

impl App {
    fn new(opts: Options, session: Session) -> Self {
        Self {
            opts,
            session,
            window: None,
            video: None,
            audio: None,
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
        if let (Some(window), Some(video)) = (&self.window, &mut self.video) {
            if let Err(e) = video.draw(window, self.session.gb.frame()) {
                eprintln!("slopgb: failed to present frame: {e}");
            }
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
        // Modal capture: while the debugger's Go-to prompt is open, every key
        // goes to it (so typing an address can't trigger a debugger hotkey).
        if focus == Focus::Debugger && key.state.is_pressed() && self.tools.debugger_modal_active()
        {
            if let Some(dk) = dialog_key_from(key) {
                self.tools.feed_debugger_dialog(dk);
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
            Action::Pause if pressed => {
                self.paused = !self.paused;
                if self.paused {
                    self.session.flush_save();
                } else {
                    self.resync_pacing();
                }
                self.update_title();
            }
            Action::Reset if pressed => {
                self.session.reset();
                self.resync_pacing();
            }
            Action::Quit if pressed => event_loop.exit(),
            Action::ToggleTool(kind) if pressed => self.tools.toggle(event_loop, kind),
            // F9 enters a break only with the debugger window up (so the key is
            // inert during normal play), but always *resumes* one — otherwise
            // closing the window while broken would strand the frozen machine.
            Action::DbgBreak
                if pressed
                    && (self.dbg.is_broken() || self.tools.is_open(ui::ToolWindow::Debugger)) =>
            {
                self.dbg.toggle_break();
                if !self.dbg.is_broken() {
                    self.resync_pacing();
                }
                self.update_title();
                self.tools.request_redraw_all();
            }
            Action::DbgStep if pressed && self.dbg.is_broken() => {
                self.dbg.step(&mut self.session.gb);
                self.refresh_after_step();
            }
            Action::DbgStepOver if pressed && self.dbg.is_broken() => {
                self.dbg.step_over(&mut self.session.gb);
                self.refresh_after_step();
            }
            // Debugger F2 / F4 act on the cursor (or PC when nothing is selected).
            Action::DbgToggleBreakpoint if pressed => {
                let addr = self.debug_cursor_or_pc();
                self.dbg.apply(
                    &mut self.session.gb,
                    dbg::DebugAction::ToggleBreakpoint(addr),
                );
                self.refresh_after_step();
            }
            Action::DbgRunToCursor if pressed => {
                let addr = self.debug_cursor_or_pc();
                self.dbg
                    .apply(&mut self.session.gb, dbg::DebugAction::RunToCursor(addr));
                self.update_title();
                self.refresh_after_step();
            }
            Action::DbgGoto if pressed => {
                self.tools.open_debugger_goto();
            }
            _ => {}
        }
    }

    /// The debugger's selected cursor address, or PC if no line is selected —
    /// what a keyboard breakpoint / run-to-cursor acts on.
    fn debug_cursor_or_pc(&self) -> u16 {
        self.tools
            .debugger_cursor()
            .unwrap_or_else(|| self.session.gb.cpu_regs().pc)
    }

    /// After a single/over step, repaint the game window (the LCD may have
    /// advanced) and every open tool window so they track the new PC.
    fn refresh_after_step(&mut self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.tools.request_redraw_all();
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

    /// Drain APU output nobody will play, so the core's sample buffer can't
    /// grow without bound while muted.
    fn discard_audio(&mut self) {
        self.discard_buf.clear();
        self.session.gb.drain_audio(&mut self.discard_buf);
    }

    /// Whether the debugger is "armed": its window is open and at least one
    /// breakpoint is set, so the free-run loop watches for a halt.
    fn dbg_armed(&self) -> bool {
        self.tools.is_open(ui::ToolWindow::Debugger) && !self.dbg.breakpoints().is_empty()
    }

    /// The breakpoint PC list to watch this wake, or `None` when not armed (the
    /// pacers then run plain frames). Computed once before the pacing loop so it
    /// doesn't re-borrow `self` while the audio pipe is held.
    fn run_breakpoints(&self) -> Option<Vec<u16>> {
        self.dbg_armed().then(|| self.dbg.breakpoints().pc_list())
    }

    /// Emulate enough frames to keep the audio queue at its fill target. Returns
    /// the frame count and whether a breakpoint halted emulation.
    fn run_audio_paced(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let mut frames = 0;
        let mut hit = false;
        {
            let Some(pipe) = &mut self.audio else {
                return (0, false);
            };
            while frames < MAX_FRAMES_PER_WAKE && pipe.needs_more() && !hit {
                hit = run_one_frame(&mut self.session.gb, &bps);
                pipe.pump(&mut self.session.gb);
                frames += 1;
            }
        }
        (frames, hit)
    }

    /// Emulate frames owed according to the wall clock at ~59.7275 Hz.
    fn run_timer_paced(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let now = Instant::now();
        // If we fell far behind (stall, drag, debugger), resync instead of
        // fast-forwarding through the backlog.
        if now.duration_since(self.next_frame) > 8 * FRAME_DURATION {
            self.next_frame = now;
        }
        let mut frames = 0;
        let mut hit = false;
        while frames < MAX_FRAMES_PER_WAKE && self.next_frame <= now && !hit {
            hit = run_one_frame(&mut self.session.gb, &bps);
            self.discard_audio();
            self.next_frame += FRAME_DURATION;
            frames += 1;
        }
        (frames, hit)
    }

    /// Turbo: emulate as much as fits in a small wall-clock budget.
    fn run_turbo(&mut self) -> (u32, bool) {
        let bps = self.run_breakpoints();
        let start = Instant::now();
        let mut frames = 0;
        let mut hit = false;
        while start.elapsed() < TURBO_BUDGET && !hit {
            hit = run_one_frame(&mut self.session.gb, &bps);
            match &mut self.audio {
                // The queue keeps ~250 ms and drops the rest.
                Some(pipe) => pipe.pump(&mut self.session.gb),
                None => self.discard_audio(),
            }
            frames += 1;
        }
        self.resync_pacing();
        (frames, hit)
    }

    /// Detect a dead or stalled cpal stream and fall back to wall-clock
    /// pacing, so audio-paced emulation can't freeze forever waiting on a
    /// queue nobody drains.
    fn check_audio_health(&mut self, frames: u32) {
        let Some(pipe) = &self.audio else { return };
        let failed = pipe.out.failed();
        if failed
            || self
                .watchdog
                .is_stalled(frames, pipe.out.queued(), Instant::now())
        {
            eprintln!(
                "slopgb: audio stream {}; falling back to timer pacing",
                if failed { "failed" } else { "stalled" }
            );
            self.audio = None;
            self.resync_pacing();
        }
    }

    fn update_fps(&mut self, frames: u32) {
        self.fps_frames += frames;
        let elapsed = self.fps_since.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.fps = f64::from(self.fps_frames) / elapsed.as_secs_f64();
            self.fps_frames = 0;
            self.fps_since = Instant::now();
            self.update_title();
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

/// Run one frame, halting early at a breakpoint when armed. A free function (not
/// a method) so the pacers can call it while the audio pipe holds `&mut
/// self.audio` — borrowing only the machine, not all of `self`. Returns whether
/// a breakpoint stopped the frame.
fn run_one_frame(gb: &mut GameBoy, breakpoints: &Option<Vec<u16>>) -> bool {
    match breakpoints {
        Some(list) => gb.run_frame_until_breakpoint(list).is_some(),
        None => {
            gb.run_frame();
            false
        }
    }
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
        if !self.opts.mute && self.audio.is_none() {
            match AudioOutput::new() {
                Ok(out) => self.audio = Some(AudioPipe::new(out)),
                Err(e) => eprintln!("slopgb: audio disabled: {e}"),
            }
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
                    let action = if button == MouseButton::Left {
                        self.tools.on_mouse_left(window_id, &self.session.gb)
                    } else {
                        self.tools.on_mouse_right(window_id, &self.session.gb)
                    };
                    if let Some(a) = action {
                        self.dbg.apply(&mut self.session.gb, a);
                        self.update_title();
                        self.refresh_after_step();
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
        } else if self.audio.is_some() {
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
        } else if self.audio.is_some() {
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
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ParseOutcome, String> {
        Options::parse(args.iter().map(ToString::to_string))
    }

    /// Parse args expected to yield a run (not help).
    fn parse_run(args: &[&str]) -> Result<Options, String> {
        match parse(args)? {
            ParseOutcome::Run(opts) => Ok(opts),
            ParseOutcome::Help => panic!("unexpected help outcome for {args:?}"),
        }
    }

    #[test]
    fn parse_rom_only_defaults() {
        let opts = parse_run(&["game.gb"]).unwrap();
        assert_eq!(opts.rom, PathBuf::from("game.gb"));
        assert_eq!(opts.model, None);
        assert_eq!(opts.scale, 3);
        assert!(!opts.mute);
    }

    #[test]
    fn parse_all_options() {
        let opts = parse_run(&["--model", "cgb", "--scale", "5", "--mute", "x.gbc"]).unwrap();
        assert_eq!(opts.rom, PathBuf::from("x.gbc"));
        assert_eq!(opts.model, Some(Model::Cgb));
        assert_eq!(opts.scale, 5);
        assert!(opts.mute);
    }

    #[test]
    fn parse_help_returns_outcome_instead_of_exiting() {
        assert!(matches!(parse(&["-h"]), Ok(ParseOutcome::Help)));
        assert!(matches!(parse(&["--help"]), Ok(ParseOutcome::Help)));
        // Help wins even when mixed with other (even bogus) arguments.
        assert!(matches!(parse(&["x.gb", "--help"]), Ok(ParseOutcome::Help)));
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse(&[]).is_err()); // missing ROM
        assert!(parse(&["--model", "snes", "x.gb"]).is_err());
        assert!(parse(&["--scale", "0", "x.gb"]).is_err());
        assert!(parse(&["--scale", "huge", "x.gb"]).is_err());
        assert!(parse(&["--frobnicate", "x.gb"]).is_err());
        assert!(parse(&["a.gb", "b.gb"]).is_err());
        assert!(parse(&["--model"]).is_err()); // value missing
    }

    #[test]
    fn parse_model_accepts_every_variant() {
        for (s, m) in [
            ("dmg", Model::Dmg),
            ("dmg0", Model::Dmg0),
            ("mgb", Model::Mgb),
            ("sgb", Model::Sgb),
            ("sgb2", Model::Sgb2),
            ("cgb", Model::Cgb),
            ("agb", Model::Agb),
        ] {
            assert_eq!(parse_model(s).unwrap(), m);
        }
    }

    #[test]
    fn frame_duration_matches_hardware_rate() {
        // 70224 / 4194304 s = 16.742706... ms
        assert_eq!(FRAME_DURATION.as_nanos(), 16_742_706);
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        // Per-process directory so concurrent test runs can't race on it.
        let dir = std::env::temp_dir().join(format!("slopgb-test-sav-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("game.sav");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(!path.with_extension("sav.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchdog_trips_after_sustained_stall() {
        let mut w = StallWatchdog::new();
        let t0 = Instant::now();
        // First observation records the baseline.
        assert!(!w.is_stalled(0, 100, t0));
        // Queue stuck at the same level, no frames: not stalled until the
        // timeout has fully elapsed.
        assert!(!w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT / 2));
        assert!(!w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT));
        assert!(w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT * 2));
    }

    #[test]
    fn watchdog_treats_drain_or_frames_as_progress() {
        let long = AUDIO_STALL_TIMEOUT * 2;
        // Queue level dropping counts as progress.
        let mut w = StallWatchdog::new();
        let t0 = Instant::now();
        assert!(!w.is_stalled(0, 100, t0));
        assert!(!w.is_stalled(0, 99, t0 + long));
        assert!(!w.is_stalled(0, 99, t0 + long + AUDIO_STALL_TIMEOUT / 2));
        // Emulated frames count as progress even if the queue grew.
        let mut w = StallWatchdog::new();
        assert!(!w.is_stalled(0, 100, t0));
        assert!(!w.is_stalled(3, 200, t0 + long));
        assert!(!w.is_stalled(0, 200, t0 + long + AUDIO_STALL_TIMEOUT / 2));
    }

    #[test]
    fn watchdog_reset_restarts_grace_period() {
        let mut w = StallWatchdog::new();
        let t0 = Instant::now();
        assert!(!w.is_stalled(0, 100, t0));
        w.reset();
        // Stale `progress_at` must not trip right after a reset (unpause).
        assert!(!w.is_stalled(0, 100, t0 + AUDIO_STALL_TIMEOUT * 2));
    }
}
