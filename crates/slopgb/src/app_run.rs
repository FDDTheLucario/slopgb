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
            Action::ToggleTool(kind) => self.tools.toggle(event_loop, kind),
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
            // bp/wp manager (RM15): build a list popup from the App-owned sets
            // (each row clears its entry) and hand it to the debugger window.
            Action::DbgManageBreakpoints => {
                let addrs = self.dbg.breakpoints().pc_list();
                let menu = windows::debugger::address_list_menu(&addrs, false, MANAGER_ORIGIN);
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
                let menu = windows::debugger::address_list_menu(&addrs, true, MANAGER_ORIGIN);
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
            // bgb's "Options..." / "Cheat..." — partial stubs (the real config /
            // cheat subsystems aren't built): a read-only info box of the live
            // settings, and an empty cheat list (MN7).
            Action::MainOptions => {
                self.info_box = Some(InfoBox::new(
                    "Options",
                    vec![
                        format!("sound:  {}", if self.muted { "off" } else { "on" }),
                        format!("scale:  {}", crate::app_menu::scale_label(self.window_size)),
                        format!("model:  {:?}", self.session.model),
                    ],
                ));
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
