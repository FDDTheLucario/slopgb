//! Keyboard mapping: physical key codes to Game Boy buttons and frontend
//! actions, **focus-dependent** like bgb (see [`map`] / [`Focus`]).
//!
//! Global (any focus): Z=A, X=B, Enter=Start, Right Shift / Backspace=Select,
//! arrows=D-pad, Tab (held)=turbo, P=pause, R=reset, Esc=quit, F9=break toggle.
//! Game-window F-keys: F2/F3/F4 open the debugger / VRAM / I-O-map windows.
//! Debugger-window F-keys: F2 toggle breakpoint, F3 step over, F4 run to cursor,
//! F7 trace (step), F8 step out, Ctrl+G go to, F5/F10 open VRAM/iomap.
//!
//! F1 (DMG palette toggle) is intentionally unmapped: `Ppu::set_dmg_palette`
//! is not reachable through the public `GameBoy` API. See the frontend report
//! for the requested core addition.

use slopgb_core::Button;
use winit::keyboard::{KeyCode, ModifiersState};

use crate::ui::ToolWindow;

/// Which window currently has focus — the key map is focus-dependent, exactly
/// like bgb (the debugger's F-keys differ from the game window's). Resolved in
/// `main` from the window the key event arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The game LCD window (and the VRAM / I/O-map viewers).
    Game,
    /// The debugger window.
    Debugger,
}

/// What a mapped key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Press/release a Game Boy button.
    Button(Button),
    /// Uncapped emulation speed while held.
    Turbo,
    /// Toggle pause (on press).
    Pause,
    /// Power-cycle the machine, reloading save RAM (on press).
    Reset,
    /// Quit the emulator (on press).
    Quit,
    /// Open/close a bgb-style debug tool window (on press).
    ToggleTool(ToolWindow),
    /// Toggle the debugger break (freeze emulation) — F9, focus-independent.
    DbgBreak,
    /// Single-step one instruction (bgb "Trace") — debugger F7, on press.
    DbgStep,
    /// Step over a call/rst — debugger F3, on press.
    DbgStepOver,
    /// Toggle a breakpoint at the cursor — debugger F2, on press.
    DbgToggleBreakpoint,
    /// Run to the cursor — debugger F4, on press.
    DbgRunToCursor,
    /// Jump PC to the cursor without running — debugger F6, on press.
    DbgJumpToCursor,
    /// Open the breakpoint manager (list) — debugger Ctrl+H, on press.
    DbgManageBreakpoints,
    /// Open the watchpoint manager (list) — debugger Ctrl+J, on press.
    DbgManageWatchpoints,
    /// Re-center the disasm on PC (unpin) — debugger Ctrl+A, on press.
    DbgGoToPc,
    /// Step out of the current subroutine — debugger F8, on press.
    DbgStepOut,
    /// Open the Go-to address prompt — debugger Ctrl+G, on press.
    DbgGoto,
    /// Toggle audio output (bgb's main-menu "Enable sound"). Menu-only — no key
    /// binds it, so [`map`] never returns it; it rides the shared `run_action`.
    ToggleSound,
    /// Save the current frame to a BMP (bgb's main-menu "Save screenshot").
    /// Menu-only — no key binds it.
    SaveScreenshot,
    /// Dump the whole 64 KiB address space to a file (debugger File →
    /// "save memory_dump..."). Menu-only.
    DbgSaveMemDump,
    /// Show the Options info box (main menu "Options...", F11). Menu-only stub.
    MainOptions,
    /// Show the Cheats info box (main menu "Cheat...", F10). Menu-only stub.
    MainCheats,
    /// Execution profiler → "logging mode": tally each executed PC (MB5).
    /// Menu-only (Execution-profiler dropdown).
    ProfilerLogging,
    /// Execution profiler → "break mode": logging + halt on each address's first
    /// execution. Menu-only.
    ProfilerBreak,
    /// Execution profiler → "stop": disable profiling. Menu-only.
    ProfilerStop,
    /// Execution profiler → "clear buffer": zero the tally. Menu-only.
    ProfilerClear,
}

/// Tracks which physical keys currently hold each button, so two keys mapped
/// to the same button (Right Shift and Backspace both press Select) don't
/// release it while the other key is still physically held.
#[derive(Default)]
pub struct ButtonTracker {
    /// Currently held mapped keys. At most one entry per `KeyCode`; tiny, so
    /// a Vec beats a set.
    held: Vec<(KeyCode, Button)>,
}

impl ButtonTracker {
    /// Record a key press for `button`.
    pub fn press(&mut self, code: KeyCode, button: Button) {
        if !self.held.iter().any(|&(c, _)| c == code) {
            self.held.push((code, button));
        }
    }

    /// Record a key release for `button`. Returns true if the button should
    /// actually be released (no other held key still maps to it).
    pub fn release(&mut self, code: KeyCode, button: Button) -> bool {
        self.held.retain(|&(c, _)| c != code);
        !self.held.iter().any(|&(_, b)| b == button)
    }

    /// Forget all held keys (e.g. on focus loss, when release events for
    /// currently held keys will never arrive).
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

/// Map a physical key to its action under the current window `focus`, if any.
///
/// Game-button + global keys (turbo/pause/reset/quit, and **F9** break which
/// stays focus-independent so a frozen machine is always resumable) bind in any
/// focus. The F-keys then split by focus, matching bgb: in the **game** window
/// F2/F3/F4 open the debugger/VRAM/iomap windows; in the **debugger** window
/// they become bgb's debugger keys (F2 toggle breakpoint, F3 step over, F4 run
/// to cursor, F6 jump to cursor, F7 trace, F8 step out, Ctrl+G go to, F5/F10
/// open VRAM/iomap). Keys for not-yet-built features (F12 load ROM) stay unmapped.
#[must_use]
pub fn map(code: KeyCode, mods: ModifiersState, focus: Focus) -> Option<Action> {
    // Global (any focus): buttons + emulator controls + the break toggle.
    let global = match code {
        KeyCode::KeyZ => Some(Action::Button(Button::A)),
        KeyCode::KeyX => Some(Action::Button(Button::B)),
        KeyCode::Enter => Some(Action::Button(Button::Start)),
        KeyCode::ShiftRight | KeyCode::Backspace => Some(Action::Button(Button::Select)),
        KeyCode::ArrowUp => Some(Action::Button(Button::Up)),
        KeyCode::ArrowDown => Some(Action::Button(Button::Down)),
        KeyCode::ArrowLeft => Some(Action::Button(Button::Left)),
        KeyCode::ArrowRight => Some(Action::Button(Button::Right)),
        KeyCode::Tab => Some(Action::Turbo),
        KeyCode::KeyP => Some(Action::Pause),
        KeyCode::KeyR => Some(Action::Reset),
        KeyCode::Escape => Some(Action::Quit),
        KeyCode::F9 => Some(Action::DbgBreak),
        _ => None,
    };
    if global.is_some() {
        return global;
    }
    match focus {
        Focus::Debugger => match code {
            KeyCode::F2 => Some(Action::DbgToggleBreakpoint),
            KeyCode::F3 => Some(Action::DbgStepOver),
            KeyCode::F4 => Some(Action::DbgRunToCursor),
            KeyCode::F6 => Some(Action::DbgJumpToCursor),
            KeyCode::F7 => Some(Action::DbgStep),
            KeyCode::F8 => Some(Action::DbgStepOut),
            KeyCode::F5 => Some(Action::ToggleTool(ToolWindow::Vram)),
            KeyCode::F10 => Some(Action::ToggleTool(ToolWindow::IoMap)),
            KeyCode::KeyG if mods.control_key() => Some(Action::DbgGoto),
            KeyCode::KeyH if mods.control_key() => Some(Action::DbgManageBreakpoints),
            KeyCode::KeyJ if mods.control_key() => Some(Action::DbgManageWatchpoints),
            KeyCode::KeyA if mods.control_key() => Some(Action::DbgGoToPc),
            _ => None,
        },
        Focus::Game => match code {
            KeyCode::F2 => Some(Action::ToggleTool(ToolWindow::Debugger)),
            KeyCode::F3 => Some(Action::ToggleTool(ToolWindow::Vram)),
            KeyCode::F4 => Some(Action::ToggleTool(ToolWindow::IoMap)),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: ModifiersState = ModifiersState::empty();
    const CTRL: ModifiersState = ModifiersState::CONTROL;

    /// Map under game focus, no modifiers (the common case).
    fn g(code: KeyCode) -> Option<Action> {
        map(code, NONE, Focus::Game)
    }
    /// Map under debugger focus, no modifiers.
    fn d(code: KeyCode) -> Option<Action> {
        map(code, NONE, Focus::Debugger)
    }

    #[test]
    fn dpad_maps_to_matching_buttons() {
        // Buttons are global — same under either focus.
        for f in [Focus::Game, Focus::Debugger] {
            assert_eq!(
                map(KeyCode::ArrowUp, NONE, f),
                Some(Action::Button(Button::Up))
            );
            assert_eq!(
                map(KeyCode::ArrowRight, NONE, f),
                Some(Action::Button(Button::Right))
            );
        }
    }

    #[test]
    fn unmapped_keys_do_nothing() {
        assert_eq!(g(KeyCode::KeyQ), None);
        assert_eq!(g(KeyCode::F1), None); // palette toggle: needs core API
        // F6 in the *game* focus is still unmapped (debugger-only key).
        assert_eq!(g(KeyCode::F6), None);
        // Not-yet-built debugger keys stay unmapped (no dead actions).
        assert_eq!(d(KeyCode::F12), None); // load ROM (MB2/MN4)
    }

    #[test]
    fn game_focus_function_keys_open_the_tool_windows() {
        assert_eq!(
            g(KeyCode::F2),
            Some(Action::ToggleTool(ToolWindow::Debugger))
        );
        assert_eq!(g(KeyCode::F3), Some(Action::ToggleTool(ToolWindow::Vram)));
        assert_eq!(g(KeyCode::F4), Some(Action::ToggleTool(ToolWindow::IoMap)));
    }

    #[test]
    fn debugger_focus_function_keys_are_bgb_debugger_keys() {
        assert_eq!(d(KeyCode::F2), Some(Action::DbgToggleBreakpoint));
        assert_eq!(d(KeyCode::F3), Some(Action::DbgStepOver));
        assert_eq!(d(KeyCode::F4), Some(Action::DbgRunToCursor));
        assert_eq!(d(KeyCode::F6), Some(Action::DbgJumpToCursor));
        assert_eq!(d(KeyCode::F7), Some(Action::DbgStep));
        assert_eq!(d(KeyCode::F8), Some(Action::DbgStepOut));
        // F8 step out is debugger-only — the game window keeps it unmapped.
        assert_eq!(g(KeyCode::F8), None);
        assert_eq!(d(KeyCode::F5), Some(Action::ToggleTool(ToolWindow::Vram)));
        assert_eq!(d(KeyCode::F10), Some(Action::ToggleTool(ToolWindow::IoMap)));
        // Ctrl+G is Go to; plain G does nothing.
        assert_eq!(
            map(KeyCode::KeyG, CTRL, Focus::Debugger),
            Some(Action::DbgGoto)
        );
        assert_eq!(d(KeyCode::KeyG), None);
        // Ctrl+H / Ctrl+J open the breakpoint / watchpoint managers (RM15).
        assert_eq!(
            map(KeyCode::KeyH, CTRL, Focus::Debugger),
            Some(Action::DbgManageBreakpoints)
        );
        assert_eq!(
            map(KeyCode::KeyJ, CTRL, Focus::Debugger),
            Some(Action::DbgManageWatchpoints)
        );
        // They are debugger-only + need Ctrl.
        assert_eq!(map(KeyCode::KeyH, CTRL, Focus::Game), None);
        assert_eq!(d(KeyCode::KeyH), None);
        // Ctrl+A re-centers the disasm on PC (Search → go to PC).
        assert_eq!(
            map(KeyCode::KeyA, CTRL, Focus::Debugger),
            Some(Action::DbgGoToPc)
        );
        assert_eq!(map(KeyCode::KeyA, CTRL, Focus::Game), None);
        assert_eq!(d(KeyCode::KeyA), None, "needs Ctrl");
    }

    #[test]
    fn f9_break_is_focus_independent() {
        // Resume-safety: F9 toggles break from any focus.
        assert_eq!(g(KeyCode::F9), Some(Action::DbgBreak));
        assert_eq!(d(KeyCode::F9), Some(Action::DbgBreak));
    }

    #[test]
    fn tracker_keeps_select_held_while_either_key_is_down() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::ShiftRight, Button::Select);
        t.press(KeyCode::Backspace, Button::Select);
        // Backspace still holds Select after Right Shift is released.
        assert!(!t.release(KeyCode::ShiftRight, Button::Select));
        assert!(t.release(KeyCode::Backspace, Button::Select));
    }

    #[test]
    fn tracker_releases_independent_buttons_independently() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::KeyZ, Button::A);
        t.press(KeyCode::KeyX, Button::B);
        assert!(t.release(KeyCode::KeyZ, Button::A));
        assert!(t.release(KeyCode::KeyX, Button::B));
    }

    #[test]
    fn tracker_release_without_press_still_releases() {
        // A release whose press was never seen (key held before focus gain)
        // must not leave the button stuck.
        let mut t = ButtonTracker::default();
        assert!(t.release(KeyCode::KeyZ, Button::A));
    }

    #[test]
    fn tracker_ignores_duplicate_presses() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::ShiftRight, Button::Select);
        t.press(KeyCode::ShiftRight, Button::Select);
        assert!(t.release(KeyCode::ShiftRight, Button::Select));
    }

    #[test]
    fn tracker_clear_forgets_held_keys() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::ShiftRight, Button::Select);
        t.clear();
        assert!(t.release(KeyCode::Backspace, Button::Select));
    }
}
