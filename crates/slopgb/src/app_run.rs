//! `App` discrete-action dispatch: [`App::run_action`] (shared by the keyboard
//! map and the debugger menu items so a hotkey and its menu entry never
//! diverge), the [`MenuOutcome`] applier, and the screenshot / memory-dump
//! exporters.

use std::fs;
use std::path::Path;

use slopgb_core::{SCREEN_H, SCREEN_W, SGB_BORDER_H, SGB_BORDER_W};
use winit::event_loop::ActiveEventLoop;

use crate::cheat_ui;
use crate::input::Action;
use crate::windows::debugger::MenuOutcome;
use crate::windows::options::ScreenshotFormat;
use crate::{App, PathPurpose, dbg, screenshot, ui, windows};

/// Top-left origin of the bp/wp manager list popup, below the debugger menu bar.
const MANAGER_ORIGIN: (i32, i32) = (40, 30);

impl App {
    /// Apply a debugger [`MenuOutcome`] from a menu item, pane click, or modal
    /// accept: an execution `Act` mutates the machine (then refreshes the
    /// views); a `Command` reuses the keyboard dispatch so a menu item and its
    /// hotkey never diverge.
    pub(crate) fn apply_menu_outcome(
        &mut self,
        outcome: MenuOutcome,
        event_loop: &ActiveEventLoop,
    ) {
        match outcome {
            MenuOutcome::Act(a) => {
                self.dbg.apply(&mut self.session.gb, a);
                self.update_title();
                self.refresh_after_step();
            }
            MenuOutcome::Command(act) => self.run_action(act, event_loop),
        }
    }

    /// Run a discrete, press-only frontend [`Action`] — shared by the keyboard
    /// map ([`Self::handle_key`]) and the debugger menu items
    /// ([`MenuOutcome::Command`]), so a hotkey and its menu entry stay in lock-
    /// step. `Button`/`Turbo` are held-state and stay in `handle_key`; they
    /// (and any guard-failed debugger action) no-op here.
    pub(crate) fn run_action(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::Pause => {
                self.paused = !self.paused;
                // A manual pause/unpause overrides the focus-loss auto-pause, so
                // refocus won't auto-resume a deliberately-paused emulator.
                self.paused_by_focus = false;
                if self.paused {
                    self.session.flush_save();
                } else {
                    self.resync_pacing();
                }
                self.update_title();
            }
            Action::Reset => {
                // Apply any deferred emulated-system choice (auto-reset off): a
                // changed model rebuilds via set_model, an unchanged one plain
                // power-cycles.
                if !self.session.set_model(self.settings.model) {
                    self.session.reset();
                }
                // The cached SNES frame belongs to the pre-reset machine; the
                // fresh coprocessor withholds frames until its display goes
                // live, so a stale (possibly blank) frame would stick.
                self.snes_frame = None;
                self.resync_pacing();
            }
            Action::Quit => {
                crate::settings_file::save(&self.settings, &self.recent);
                event_loop.exit();
            }
            Action::ToggleTool(kind) => {
                self.tools.toggle(event_loop, kind);
                // A freshly-opened debugger must reflect the current disasm
                // settings (syntax/hex/clocks) and loaded symbols, not just the
                // `DisasmFmt::default` / empty-table defaults.
                self.push_disasm_fmt();
                self.tools.set_tile_hex_8bit(self.settings.tile_hex_8bit);
                self.tools
                    .set_registers_editable(self.settings.registers_editable);
                self.tools.set_symbols(self.symbols.clone());
                // Keep the Options "memory viewer in own window" setting in sync
                // with reality, so a later Apply doesn't fight a menu toggle.
                if kind == crate::ui::ToolWindow::MemoryViewer {
                    self.settings.memory_window =
                        self.tools.is_open(crate::ui::ToolWindow::MemoryViewer);
                }
            }
            // F9 enters a break only with the debugger window up (so the key is
            // inert during normal play), but always *resumes* one — otherwise
            // closing the window while broken would strand the frozen machine.
            Action::DbgBreak
                if self.dbg.is_broken() || self.tools.is_open(ui::ToolWindow::Debugger) =>
            {
                self.dbg.toggle_break();
                if !self.dbg.is_broken() {
                    self.resync_pacing();
                }
                self.update_title();
                self.tools.request_redraw_all();
            }
            Action::DbgStep if self.dbg.is_broken() => {
                self.dbg.step(&mut self.session.gb);
                self.tools.center_debugger_on_pc(&self.session.gb);
                self.refresh_after_step();
            }
            Action::DbgStepOver if self.dbg.is_broken() => {
                self.dbg.step_over(&mut self.session.gb);
                self.tools.center_debugger_on_pc(&self.session.gb);
                self.refresh_after_step();
            }
            Action::DbgStepOut if self.dbg.is_broken() => {
                self.dbg.step_out(&mut self.session.gb);
                self.tools.center_debugger_on_pc(&self.session.gb);
                self.refresh_after_step();
            }
            // Debugger F2 / F4 act on the cursor (or PC when nothing is selected).
            Action::DbgToggleBreakpoint => {
                let addr = self.debug_cursor_or_pc();
                self.dbg.apply(
                    &mut self.session.gb,
                    dbg::DebugAction::ToggleBreakpoint(addr),
                );
                self.refresh_after_step();
            }
            Action::DbgRunToCursor => {
                let addr = self.debug_cursor_or_pc();
                self.dbg
                    .apply(&mut self.session.gb, dbg::DebugAction::RunToCursor(addr));
                self.update_title();
                self.tools.center_debugger_on_pc(&self.session.gb);
                self.refresh_after_step();
            }
            Action::DbgJumpToCursor => {
                let addr = self.debug_cursor_or_pc();
                self.dbg
                    .apply(&mut self.session.gb, dbg::DebugAction::SetPc(addr));
                self.update_title();
                self.tools.center_debugger_on_pc(&self.session.gb);
                self.refresh_after_step();
            }
            Action::DbgGoto => self.tools.open_debugger_goto(),
            Action::DbgGoToPc => self.tools.center_debugger_on_pc(&self.session.gb),
            Action::DbgMemScroll(rows) => self.tools.scroll_debugger_memory(rows),
            Action::DbgMemPage(dir) => self.tools.page_debugger_memory(dir),
            Action::DbgMemBankStep(delta) => {
                self.tools.step_debugger_bank(delta, &self.session.gb);
            }
            // bp/wp manager (RM15): build a list popup from the App-owned sets
            // (each row clears its entry) and hand it to the debugger window.
            Action::DbgManageBreakpoints => {
                let addrs = self.dbg.breakpoints().pc_list();
                let menu = windows::debugger::address_list_menu(
                    &addrs,
                    dbg::DebugAction::ClearBreakpoint,
                    &self.symbols,
                    MANAGER_ORIGIN,
                );
                self.tools.set_debugger_menu(menu);
            }
            Action::DbgManageWatchpoints => {
                let addrs: Vec<u16> = self
                    .dbg
                    .watchpoints()
                    .list()
                    .iter()
                    .map(|w| w.addr)
                    .collect();
                let menu = windows::debugger::address_list_menu(
                    &addrs,
                    dbg::DebugAction::ClearWatchpoint,
                    &self.symbols,
                    MANAGER_ORIGIN,
                );
                self.tools.set_debugger_menu(menu);
            }
            Action::DbgManageFreezes => {
                let addrs: Vec<u16> = self.dbg.freezes().list().iter().map(|&(a, _)| a).collect();
                let menu = windows::debugger::address_list_menu(
                    &addrs,
                    dbg::DebugAction::ClearFreeze,
                    &self.symbols,
                    MANAGER_ORIGIN,
                );
                self.tools.set_debugger_menu(menu);
            }
            // CDL (code/data logging): toggle logging, clear the buffer, or
            // save/load the flags (compressed) via the shared path modal.
            Action::DbgToggleCdl => {
                let on = self.session.gb.cdl_flags().is_none();
                self.session.gb.set_cdl(on);
            }
            Action::DbgClearCdl => self.session.gb.cdl_clear(),
            Action::DbgSaveCdl => self.open_path_prompt("Save CDL", PathPurpose::CdlSave),
            Action::DbgLoadCdl => self.open_path_prompt("Load CDL", PathPurpose::CdlLoad),
            // bgb's "Enable sound": flip the runtime mute. Unmuting lazily opens
            // the device (so a `--mute` start can still enable sound). Resync
            // pacing so the audio↔timer switch doesn't fast-forward a backlog.
            Action::ToggleSound => {
                self.muted = !self.muted;
                if !self.muted {
                    self.try_open_audio();
                }
                self.resync_pacing();
            }
            // UI → theme toggle (bare T, any focus): flips Light<->Dark and
            // persists immediately, so the choice survives a crash/kill (not
            // just a clean Quit). No on-screen widget — this hotkey is the
            // whole control surface.
            Action::ToggleTheme => {
                self.toggle_theme();
                self.tools.request_redraw_all();
                self.request_game_redraw();
            }
            Action::SaveScreenshot => self.save_screenshot(),
            Action::ExportSpc => self.export_spc(),
            Action::DbgSaveMemDump => self.save_memory_dump(),
            // bgb's "Options..." (F11): open the tabbed control panel, seeded
            // from the live settings. Routed/applied by `on_game_click` +
            // `handle_key` like the other modals.
            Action::MainOptions => {
                // Refresh the Plugins tab's list from the live host before the
                // working copy is snapshotted, so it shows the current plugins.
                self.sync_plugin_entries();
                self.options = Some(windows::options::OptionsState::new(self.settings.clone()));
                self.request_game_redraw();
            }
            Action::MainCheats => {
                self.cheat_dialog = Some(cheat_ui::CheatDialog::default());
                self.request_game_redraw();
            }
            // Execution profiler (MB5): the three radio modes + clear buffer.
            // "logging" and "break" both enable the tally; break also halts the
            // free run on each address's first execution.
            Action::ProfilerLogging => {
                self.session.gb.set_profiling(true);
                self.session.gb.set_profile_break(false);
                self.refresh_after_step();
            }
            Action::ProfilerBreak => {
                self.session.gb.set_profiling(true);
                self.session.gb.set_profile_break(true);
                self.refresh_after_step();
            }
            Action::ProfilerStop => {
                self.session.gb.set_profiling(false);
                self.refresh_after_step();
            }
            Action::ProfilerClear => {
                self.session.gb.clear_profile();
                self.refresh_after_step();
            }
            // Search menu (MB3). The scan/walk runs here, where the machine
            // memory (gb) and the App-owned breakpoints are both reachable.
            Action::DbgSearch => self.tools.open_debugger_search(),
            Action::DbgContinueSearch => self.tools.debugger_search(&self.session.gb),
            Action::DbgSetBookmark(slot) => {
                let addr = self.debug_cursor_or_pc();
                self.tools.set_debugger_bookmark(slot, addr);
            }
            Action::DbgGotoBookmark(slot) => self.tools.goto_debugger_bookmark(slot),
            // Next/previous bookmark walk over bookmarks ∪ breakpoints (bgb).
            Action::DbgNextBookmark | Action::DbgPrevBookmark => {
                let forward = action == Action::DbgNextBookmark;
                let mut marks = self.tools.debugger_bookmarks();
                marks.extend(self.dbg.breakpoints().pc_list());
                let from = self
                    .tools
                    .debugger_disasm_ref(self.session.gb.cpu_regs().pc);
                if let Some(addr) = windows::debugger::next_mark(&marks, from, forward) {
                    self.tools.debugger_goto_addr(addr);
                }
            }
            // Main menu (MN4): open the Load-ROM path-entry modal over the LCD.
            Action::MainLoadRom => {
                self.open_path_prompt("Load ROM (path)", crate::PathPurpose::LoadRom);
            }
            // File / State menu: on-disk save states via the shared path modal.
            Action::DbgSaveState => {
                self.open_path_prompt("Save state (path)", crate::PathPurpose::SaveState);
            }
            Action::DbgLoadState => {
                self.open_path_prompt("Load state (path)", crate::PathPurpose::LoadState);
            }
            Action::DbgLoadSymbols => {
                self.open_path_prompt("Load symbols (.sym path)", crate::PathPurpose::SymbolFile);
            }
            // Disasm/memory right-click Copy data/code (RM10): build the text and
            // push it to the system clipboard (dep-free shell-out; non-fatal).
            Action::DbgCopyData(addr) => self.copy_to_clipboard(addr, false),
            Action::DbgCopyCode(addr) => self.copy_to_clipboard(addr, true),
            // File menu (MB2): export the disassembly of the current region.
            Action::DbgSaveAsm => self.save_asm(),
            // Debug menu (RM14): evaluate an expression + the user-clock counter.
            Action::DbgEvaluate => self.tools.open_debugger_eval(),
            Action::DbgEvalRun => self.tools.debugger_eval(&self.session.gb),
            Action::DbgSetUserClocks => self.tools.reset_debugger_clocks(&self.session.gb),
            _ => {}
        }
    }

    /// Dump the whole 64 KiB address space (live IO-resolved via `debug_read`)
    /// to `slopgb-memdump-<unix-millis>.bin` (debugger File → "save
    /// memory_dump..."); log the path or any I/O error.
    fn save_memory_dump(&self) {
        let dump: Vec<u8> = (0..=0xFFFFu16)
            .map(|a| self.session.gb.debug_read(a))
            .collect();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = format!("slopgb-memdump-{stamp}.bin");
        match fs::write(&path, &dump) {
            Ok(()) => eprintln!("saved memory dump to {path}"),
            Err(e) => eprintln!("error: could not save memory dump: {e}"),
        }
    }

    /// Export the SGB audio chip's current state as `slopgb-<unix-millis>.spc`
    /// (main menu → "Export SPC"); best captured while a song is playing, so the
    /// SPC is self-sustaining. Logs the path, or a hint if there's no SPC to dump
    /// (not an SGB machine, or the coprocessor has no SPC700).
    fn export_spc(&self) {
        let Some(spc) = self.session.gb.export_spc() else {
            eprintln!("slopgb: no SPC to export (needs an SGB machine with audio)");
            return;
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = format!("slopgb-{stamp}.spc");
        match fs::write(&path, &spc) {
            Ok(()) => eprintln!("saved SPC ({} bytes) to {path}", spc.len()),
            Err(e) => eprintln!("error: could not save SPC: {e}"),
        }
    }

    /// Export the disassembly of the current region to `slopgb-asm-<unix-millis>
    /// .txt` (debugger File → "save asm..."); log the path or any I/O error.
    fn save_asm(&self) {
        let text = self.tools.debugger_disasm_dump(&self.session.gb);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = format!("slopgb-asm-{stamp}.txt");
        match fs::write(&path, text) {
            Ok(()) => eprintln!("saved asm to {path}"),
            Err(e) => eprintln!("error: could not save asm: {e}"),
        }
    }

    /// Copy the disassembly (`code`) or hex bytes (data) at `addr` to the system
    /// clipboard (RM10). Dep-free shell-out — a host with no clipboard tool just
    /// logs a hint (non-fatal).
    fn copy_to_clipboard(&self, addr: u16, code: bool) {
        let text = self.tools.debugger_copy_text(&self.session.gb, addr, code);
        if crate::clipboard::copy(&text) {
            println!("slopgb: copied {} chars to the clipboard", text.len());
        } else {
            eprintln!("slopgb: no clipboard tool found (install wl-copy/xclip/xsel)");
        }
    }

    /// Write the current frame to `slopgb-<unix-millis>.<ext>` in the working
    /// directory (bgb's "Save screenshot"); log the path or any I/O error. The
    /// image format (Joypad "Screenshots") and whether an SGB border is included
    /// (Graphics "SGB border in screenshot") follow the current settings.
    fn save_screenshot(&self) {
        // Include the 256×224 SGB composite only when the option is on and a
        // border is actually loaded; otherwise the bare 160×144 LCD.
        let border = self
            .settings
            .sgb_border_screenshot
            .then(|| self.session.gb.sgb_border())
            .flatten();
        let (frame, w, h): (&[u32], usize, usize) = match border {
            Some(b) => (&b[..], SGB_BORDER_W, SGB_BORDER_H),
            None => (&self.session.gb.frame()[..], SCREEN_W, SCREEN_H),
        };
        // Joypad "Screenshot button" → copies: put the frame on the clipboard as
        // a PNG (the universally-pasteable image format) instead of a file.
        if self.settings.screenshot_copies {
            let png = crate::mcp::png::encode(frame, w, h);
            if crate::clipboard::copy_image_png(&png) {
                eprintln!("copied screenshot to the clipboard");
            } else {
                eprintln!("slopgb: no image-clipboard tool found (install wl-copy/xclip)");
            }
            return;
        }
        let fmt = self.settings.screenshot_format;
        let data = match fmt {
            ScreenshotFormat::Bmp => screenshot::to_bmp(frame, w, h),
            ScreenshotFormat::Png => crate::mcp::png::encode(frame, w, h),
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = format!("slopgb-{stamp}.{}", fmt.ext());
        match fs::write(&path, &data) {
            Ok(()) => eprintln!("saved screenshot to {path}"),
            Err(e) => eprintln!("error: could not save screenshot: {e}"),
        }
    }

    /// Arm the recovery-save-state machinery for the ROM at `rom_path`: set its
    /// `<rom>.recovery` sidecar path and, if that file is already present (Misc →
    /// "Recovery save state" on), restore it over the freshly-loaded machine — a
    /// leftover file means the last session of this ROM crashed (a clean quit
    /// deletes it). Called from every load path (drag-drop + CLI startup).
    pub(crate) fn arm_recovery(&mut self, rom_path: &Path) {
        self.recovery_path = Some(rom_path.with_extension("recovery"));
        self.recovery_next = std::time::Instant::now() + crate::RECOVERY_INTERVAL;
        if !self.settings.recovery_save_state {
            return;
        }
        if let Some(rp) = self.recovery_path.clone() {
            if rp.exists() {
                match self.session.load_state_from(&rp) {
                    Ok(()) => {
                        eprintln!("slopgb: recovered unsaved progress from {}", rp.display());
                    }
                    Err(e) => eprintln!("slopgb: recovery state ignored: {e}"),
                }
            }
        }
    }

    /// Joypad → "Audio": start/stop the WAV audio recorder to match the setting.
    /// Recording needs a live audio pipe (a `--mute` run can't record). Toggling
    /// the setting off (or quitting mid-record) finalises the file.
    pub(crate) fn sync_audio_recording(&mut self) {
        let want = self.settings.record_audio;
        let Some(pipe) = self.audio.as_mut() else {
            return;
        };
        if want && !pipe.is_recording() {
            pipe.start_record();
        } else if !want && pipe.is_recording() {
            let frames = pipe.take_record(); // pipe borrow ends here
            self.save_wav_recording(&frames);
        }
    }

    /// Encode recorded audio frames to a timestamped WAV in the working dir.
    pub(crate) fn save_wav_recording(&self, frames: &[(f32, f32)]) {
        if frames.is_empty() {
            eprintln!("slopgb: audio recording was empty (no audio while recording)");
            return;
        }
        let wav = crate::wav::encode_wav(frames, crate::pacing::AudioPipe::record_rate());
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = format!("slopgb-{stamp}.wav");
        match fs::write(&path, &wav) {
            Ok(()) => eprintln!("saved audio recording to {path}"),
            Err(e) => eprintln!("error: could not save audio recording: {e}"),
        }
    }

    /// Joypad → "Audio channels": start/stop the per-channel recorder. Arms both
    /// the core APU tap and the pipe's capture; on stop, writes one WAV per GB
    /// sound channel. Needs a live audio pipe (a `--mute` run can't record).
    pub(crate) fn sync_channel_recording(&mut self) {
        let want = self.settings.record_audio_channels;
        let Some(pipe) = self.audio.as_mut() else {
            return;
        };
        if want && !pipe.is_recording_channels() {
            pipe.start_record_channels();
            self.session.gb.set_record_channels(true);
        } else if !want && pipe.is_recording_channels() {
            let chans = pipe.take_record_channels(); // pipe borrow ends here
            self.session.gb.set_record_channels(false);
            self.save_channel_recordings(&chans);
        }
    }

    /// Encode each channel that played to its own WAV (`slopgb-<stamp>-chN.wav`,
    /// N=1..4). A channel whose DAC never turned on is all-zero and is skipped.
    pub(crate) fn save_channel_recordings(&self, chans: &[Vec<f32>; 4]) {
        if chans.iter().all(Vec::is_empty) {
            eprintln!("slopgb: channel recording was empty (no audio while recording)");
            return;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let rate = crate::pacing::AudioPipe::record_rate();
        for (i, ch) in chans.iter().enumerate() {
            if ch.iter().all(|&s| s == 0.0) {
                continue; // this channel's DAC never turned on: nothing to save
            }
            let frames: Vec<(f32, f32)> = ch.iter().map(|&s| (s, s)).collect();
            let wav = crate::wav::encode_wav(&frames, rate);
            let path = format!("slopgb-{stamp}-ch{}.wav", i + 1);
            match fs::write(&path, &wav) {
                Ok(()) => eprintln!("saved channel {} recording to {path}", i + 1),
                Err(e) => eprintln!("error: could not save channel recording: {e}"),
            }
        }
    }

    /// Joypad → "Video": start/stop the AVI video recorder to match the setting.
    /// Toggling it off (or quitting mid-record) finalises the file.
    pub(crate) fn sync_video_recording(&mut self) {
        let want = self.settings.record_video;
        if want && self.video_rec.is_none() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let path = format!("slopgb-{stamp}.avi");
            let fps = slopgb_core::CLOCK_HZ as f64 / slopgb_core::CYCLES_PER_FRAME as f64;
            match crate::avi::AviWriter::create(
                Path::new(&path),
                SCREEN_W as u32,
                SCREEN_H as u32,
                fps,
            ) {
                Ok(w) => {
                    self.video_rec = Some(w);
                    eprintln!("recording video to {path}");
                }
                Err(e) => eprintln!("error: could not start video recording: {e}"),
            }
        } else if !want {
            self.finish_video_recording();
        }
    }

    /// Append the current 160×144 LCD to the AVI (one frame per rendered batch).
    /// No-op with no recorder or no ROM (a frozen machine has no fresh frame).
    pub(crate) fn write_video_frame(&mut self) {
        if !self.rom_loaded {
            return;
        }
        let Some(rec) = self.video_rec.as_mut() else {
            return;
        };
        if let Err(e) = rec.write_frame(self.session.gb.frame()) {
            eprintln!("slopgb: video frame write failed: {e}");
        }
    }

    /// Finalise the AVI (patch sizes + index) if one is recording.
    pub(crate) fn finish_video_recording(&mut self) {
        if let Some(mut rec) = self.video_rec.take() {
            match rec.finish() {
                Ok(()) => eprintln!("saved video recording"),
                Err(e) => eprintln!("error: could not finalise video recording: {e}"),
            }
        }
    }

    /// Misc → "Recovery save state": rewrite `<rom>.recovery` on the interval so
    /// a crash loses at most `RECOVERY_INTERVAL` of progress. No-op when the
    /// option is off, no ROM is loaded, or the interval hasn't elapsed.
    pub(crate) fn write_recovery_state(&mut self) {
        if !self.settings.recovery_save_state {
            return;
        }
        let now = std::time::Instant::now();
        if now < self.recovery_next {
            return;
        }
        if let Some(rp) = self.recovery_path.clone() {
            if let Err(e) = self.session.save_state_to(&rp) {
                eprintln!("slopgb: recovery save failed: {e}");
            }
        }
        self.recovery_next = now + crate::RECOVERY_INTERVAL;
    }

    /// Delete the recovery state on a clean quit, so it is only ever present —
    /// and therefore restored on the next load — after a crash.
    pub(crate) fn clear_recovery_state(&self) {
        if let Some(rp) = &self.recovery_path {
            let _ = std::fs::remove_file(rp);
        }
    }

    /// The debugger's selected cursor address, or PC if no line is selected —
    /// what a keyboard breakpoint / run-to-cursor acts on.
    fn debug_cursor_or_pc(&self) -> u16 {
        self.tools
            .debugger_cursor()
            .unwrap_or_else(|| self.session.gb.cpu_regs().pc)
    }

    /// Flip the active theme Light↔Dark and persist immediately (the theming
    /// feature's only control surface — no on-screen widget). Classic/Custom
    /// aren't part of the toggle cycle: they're config/CLI-only selections,
    /// so pressing the key from either lands on Dark (a defined, non-stuck
    /// outcome) rather than cycling through every possible choice.
    fn toggle_theme(&mut self) {
        self.toggle_theme_no_persist();
        crate::settings_file::save(&self.settings, &self.recent);
    }

    /// The state-mutating half of [`Self::toggle_theme`] with persistence
    /// removed, so a test can drive it without touching the real config file.
    fn toggle_theme_no_persist(&mut self) {
        self.settings.theme = if self.settings.theme == ui::ThemeChoice::Dark {
            ui::ThemeChoice::Light
        } else {
            ui::ThemeChoice::Dark
        };
    }
}

#[cfg(test)]
#[path = "app_run_tests.rs"]
mod tests;
