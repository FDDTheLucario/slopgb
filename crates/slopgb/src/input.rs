//! Keyboard mapping: physical key codes to Game Boy buttons and frontend
//! actions.
//!
//! Layout (fixed, no rebinding):
//! Z=A, X=B, Enter=Start, Right Shift / Backspace=Select, arrows=D-pad,
//! Tab (held)=turbo, P=pause, R=reset, Esc=quit.
//!
//! F1 (DMG palette toggle) is intentionally unmapped: `Ppu::set_dmg_palette`
//! is not reachable through the public `GameBoy` API. See the frontend report
//! for the requested core addition.

use slopgb_core::Button;
use winit::keyboard::KeyCode;

use crate::ui::ToolWindow;

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
    /// Toggle the debugger break (freeze emulation) — F9, on press.
    DbgBreak,
    /// Single-step one instruction while broken — F7, on press.
    DbgStep,
    /// Step over a call/rst while broken — F8, on press.
    DbgStepOver,
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

/// Map a physical key to its action, if it has one.
pub fn map(code: KeyCode) -> Option<Action> {
    Some(match code {
        KeyCode::KeyZ => Action::Button(Button::A),
        KeyCode::KeyX => Action::Button(Button::B),
        KeyCode::Enter => Action::Button(Button::Start),
        KeyCode::ShiftRight | KeyCode::Backspace => Action::Button(Button::Select),
        KeyCode::ArrowUp => Action::Button(Button::Up),
        KeyCode::ArrowDown => Action::Button(Button::Down),
        KeyCode::ArrowLeft => Action::Button(Button::Left),
        KeyCode::ArrowRight => Action::Button(Button::Right),
        KeyCode::Tab => Action::Turbo,
        KeyCode::KeyP => Action::Pause,
        KeyCode::KeyR => Action::Reset,
        KeyCode::Escape => Action::Quit,
        KeyCode::F2 => Action::ToggleTool(ToolWindow::Debugger),
        KeyCode::F3 => Action::ToggleTool(ToolWindow::Vram),
        KeyCode::F4 => Action::ToggleTool(ToolWindow::IoMap),
        KeyCode::F7 => Action::DbgStep,
        KeyCode::F8 => Action::DbgStepOver,
        KeyCode::F9 => Action::DbgBreak,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpad_maps_to_matching_buttons() {
        assert_eq!(map(KeyCode::ArrowUp), Some(Action::Button(Button::Up)));
        assert_eq!(map(KeyCode::ArrowDown), Some(Action::Button(Button::Down)));
        assert_eq!(map(KeyCode::ArrowLeft), Some(Action::Button(Button::Left)));
        assert_eq!(
            map(KeyCode::ArrowRight),
            Some(Action::Button(Button::Right))
        );
    }

    #[test]
    fn unmapped_keys_do_nothing() {
        assert_eq!(map(KeyCode::KeyQ), None);
        assert_eq!(map(KeyCode::F1), None); // palette toggle: needs core API
    }

    #[test]
    fn function_keys_toggle_the_tool_windows() {
        assert_eq!(
            map(KeyCode::F2),
            Some(Action::ToggleTool(ToolWindow::Debugger))
        );
        assert_eq!(map(KeyCode::F3), Some(Action::ToggleTool(ToolWindow::Vram)));
        assert_eq!(
            map(KeyCode::F4),
            Some(Action::ToggleTool(ToolWindow::IoMap))
        );
    }

    #[test]
    fn function_keys_drive_the_debugger() {
        assert_eq!(map(KeyCode::F7), Some(Action::DbgStep));
        assert_eq!(map(KeyCode::F8), Some(Action::DbgStepOver));
        assert_eq!(map(KeyCode::F9), Some(Action::DbgBreak));
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
