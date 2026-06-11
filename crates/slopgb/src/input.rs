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
}
