//! `App` discrete-action dispatch: [`App::run_action`] (shared by the keyboard
//! map and the debugger menu items so a hotkey and its menu entry never
//! diverge), the [`MenuOutcome`] applier, and the screenshot / memory-dump
//! exporters.

use std::fs;

use slopgb_core::{SCREEN_H, SCREEN_W};
use winit::event_loop::ActiveEventLoop;

use crate::input::Action;
use crate::windows::debugger::MenuOutcome;
use crate::windows::mainwin::InfoBox;
use crate::{App, dbg, screenshot, ui, windows};

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
                self.session.reset();
                self.resync_pacing();
            }
            Action::Quit => event_loop.exit(),
            Action::ToggleTool(kind) => {
                self.tools.toggle(event_loop, kind);
                // A freshly-opened debugger must reflect the current disasm
                // settings (syntax/hex/clocks) and loaded symbols, not just the
                // `DisasmFmt::default` / empty-table defaults.
                self.push_disasm_fmt();
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
                self.refresh_after_step();
            }
            Action::DbgStepOver if self.dbg.is_broken() => {
                self.dbg.step_over(&mut self.session.gb);
                self.refresh_after_step();
            }
            Action::DbgStepOut if self.dbg.is_broken() => {
                self.dbg.step_out(&mut self.session.gb);
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
                self.refresh_after_step();
            }
            Action::DbgJumpToCursor => {
                let addr = self.debug_cursor_or_pc();
                self.dbg
                    .apply(&mut self.session.gb, dbg::DebugAction::SetPc(addr));
                self.update_title();
                self.refresh_after_step();
            }
            Action::DbgGoto => self.tools.open_debugger_goto(),
            Action::DbgGoToPc => self.tools.debugger_goto_pc(),
            Action::DbgMemScroll(rows) => self.tools.scroll_debugger_memory(rows),
            Action::DbgMemPage(dir) => self.tools.page_debugger_memory(dir),
            // bp/wp manager (RM15): build a list popup from the App-owned sets
            // (each row clears its entry) and hand it to the debugger window.
            Action::DbgManageBreakpoints => {
                let addrs = self.dbg.breakpoints().pc_list();
                let menu = windows::debugger::address_list_menu(
                    &addrs,
                    false,
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
                    true,
                    &self.symbols,
                    MANAGER_ORIGIN,
                );
                self.tools.set_debugger_menu(menu);
            }
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
            Action::SaveScreenshot => self.save_screenshot(),
            Action::DbgSaveMemDump => self.save_memory_dump(),
            // bgb's "Options..." (F11): open the tabbed control panel, seeded
            // from the live settings. Routed/applied by `on_game_click` +
            // `handle_key` like the other modals.
            Action::MainOptions => {
                self.options = Some(windows::options::OptionsState::new(self.settings.clone()));
                self.request_game_redraw();
            }
            Action::MainCheats => {
                self.info_box = Some(InfoBox::new("Cheats", vec!["(no cheats loaded)".into()]));
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

    /// Write the current frame to `slopgb-<unix-millis>.bmp` in the working
    /// directory (bgb's "Save screenshot"); log the path or any I/O error.
    fn save_screenshot(&self) {
        let bmp = screenshot::to_bmp(self.session.gb.frame(), SCREEN_W, SCREEN_H);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = format!("slopgb-{stamp}.bmp");
        match fs::write(&path, &bmp) {
            Ok(()) => eprintln!("saved screenshot to {path}"),
            Err(e) => eprintln!("error: could not save screenshot: {e}"),
        }
    }

    /// The debugger's selected cursor address, or PC if no line is selected —
    /// what a keyboard breakpoint / run-to-cursor acts on.
    fn debug_cursor_or_pc(&self) -> u16 {
        self.tools
            .debugger_cursor()
            .unwrap_or_else(|| self.session.gb.cpu_regs().pc)
    }
}
