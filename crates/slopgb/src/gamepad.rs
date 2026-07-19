//! Game-controller input (Options → Joypad → "configure game controller").
//! `gilrs` reads the platform gamepad(s) and maps arbitrary controllers to
//! standardized buttons via its bundled SDL_GameControllerDB, so A/B/Start/
//! Select land correctly across pads. We translate those to the 8 Game Boy
//! buttons through a rebindable [`GamepadBindings`] and feed the same deferred
//! sub-frame input path the keyboard uses (`App::set_gamepad_button`).
//!
//! The analog left stick also drives the D-pad (with a deadzone), independent
//! of the button bindings — matching how most emulators treat a stick.

use gilrs::{Axis, Button as GpButton, EventType, Gilrs};
use slopgb_core::Button;

/// The 8 Game Boy buttons in a fixed index order (shared with the keyboard
/// bindings' order), so a `[T; 8]` can be indexed by button.
const GB_ORDER: [Button; 8] = [
    Button::Right,
    Button::Left,
    Button::Up,
    Button::Down,
    Button::A,
    Button::B,
    Button::Select,
    Button::Start,
];

pub(crate) fn gb_index(b: Button) -> usize {
    match b {
        Button::Right => 0,
        Button::Left => 1,
        Button::Up => 2,
        Button::Down => 3,
        Button::A => 4,
        Button::B => 5,
        Button::Select => 6,
        Button::Start => 7,
    }
}

/// Which controller button drives each Game Boy button. `None` = unbound.
/// The default is the standard face/D-pad layout (South=A, East=B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GamepadBindings {
    /// Indexed by [`gb_index`].
    map: [Option<GpButton>; 8],
}

impl Default for GamepadBindings {
    fn default() -> Self {
        let mut map = [None; 8];
        map[gb_index(Button::Right)] = Some(GpButton::DPadRight);
        map[gb_index(Button::Left)] = Some(GpButton::DPadLeft);
        map[gb_index(Button::Up)] = Some(GpButton::DPadUp);
        map[gb_index(Button::Down)] = Some(GpButton::DPadDown);
        map[gb_index(Button::A)] = Some(GpButton::South);
        map[gb_index(Button::B)] = Some(GpButton::East);
        map[gb_index(Button::Select)] = Some(GpButton::Select);
        map[gb_index(Button::Start)] = Some(GpButton::Start);
        Self { map }
    }
}

impl GamepadBindings {
    /// The Game Boy button a controller button is bound to, if any.
    #[must_use]
    pub(crate) fn gb_for(&self, gp: GpButton) -> Option<Button> {
        self.map
            .iter()
            .zip(GB_ORDER)
            .find_map(|(m, gb)| (*m == Some(gp)).then_some(gb))
    }

    /// The controller button bound to `gb`, if any (for the config wizard's UI).
    #[must_use]
    pub(crate) fn gp_for(&self, gb: Button) -> Option<GpButton> {
        self.map[gb_index(gb)]
    }

    /// Bind `gb` to controller button `gp`, removing any prior use of `gp` so a
    /// controller button never drives two Game Boy buttons at once.
    pub(crate) fn bind(&mut self, gb: Button, gp: GpButton) {
        for slot in &mut self.map {
            if *slot == Some(gp) {
                *slot = None;
            }
        }
        self.map[gb_index(gb)] = Some(gp);
    }

    /// Unbind a single Game Boy button (the wizard's "Skip/clear").
    pub(crate) fn unbind(&mut self, gb: Button) {
        self.map[gb_index(gb)] = None;
    }

    /// Clear all bindings (Options → "clear game controller").
    pub(crate) fn clear(&mut self) {
        self.map = [None; 8];
    }

    /// Persist as 8 comma-separated controller-button names (`-` = unbound), in
    /// [`GB_ORDER`]. Round-trips through [`Self::from_config`].
    #[must_use]
    pub(crate) fn to_config(&self) -> String {
        self.map
            .iter()
            .map(|m| m.map_or("-", gp_name))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse [`Self::to_config`]; unknown/absent names default to the standard
    /// binding for that slot, so a truncated or hand-edited config can't wedge.
    #[must_use]
    pub(crate) fn from_config(s: &str) -> Self {
        let def = Self::default();
        let mut out = def.clone();
        for (i, tok) in s.split(',').enumerate().take(8) {
            out.map[i] = if tok == "-" {
                None
            } else {
                gp_from_name(tok).or(def.map[i])
            };
        }
        out
    }
}

/// The default controller map as a config string (seeds `Settings::default`).
#[must_use]
pub(crate) fn default_map_config() -> String {
    GamepadBindings::default().to_config()
}

/// Stable name for a controller button (persistence + the wizard's "currently
/// mapped to" line). Uncommon buttons fall back to `"?"`.
#[must_use]
pub(crate) fn gp_name(b: GpButton) -> &'static str {
    match b {
        GpButton::South => "South",
        GpButton::East => "East",
        GpButton::North => "North",
        GpButton::West => "West",
        GpButton::C => "C",
        GpButton::Z => "Z",
        GpButton::LeftTrigger => "L1",
        GpButton::LeftTrigger2 => "L2",
        GpButton::RightTrigger => "R1",
        GpButton::RightTrigger2 => "R2",
        GpButton::Select => "Select",
        GpButton::Start => "Start",
        GpButton::Mode => "Mode",
        GpButton::LeftThumb => "L3",
        GpButton::RightThumb => "R3",
        GpButton::DPadUp => "DPadUp",
        GpButton::DPadDown => "DPadDown",
        GpButton::DPadLeft => "DPadLeft",
        GpButton::DPadRight => "DPadRight",
        _ => "?",
    }
}

/// Inverse of [`gp_name`]; `None` for an unrecognized name.
#[must_use]
pub(crate) fn gp_from_name(s: &str) -> Option<GpButton> {
    Some(match s {
        "South" => GpButton::South,
        "East" => GpButton::East,
        "North" => GpButton::North,
        "West" => GpButton::West,
        "C" => GpButton::C,
        "Z" => GpButton::Z,
        "L1" => GpButton::LeftTrigger,
        "L2" => GpButton::LeftTrigger2,
        "R1" => GpButton::RightTrigger,
        "R2" => GpButton::RightTrigger2,
        "Select" => GpButton::Select,
        "Start" => GpButton::Start,
        "Mode" => GpButton::Mode,
        "L3" => GpButton::LeftThumb,
        "R3" => GpButton::RightThumb,
        "DPadUp" => GpButton::DPadUp,
        "DPadDown" => GpButton::DPadDown,
        "DPadLeft" => GpButton::DPadLeft,
        "DPadRight" => GpButton::DPadRight,
        _ => return None,
    })
}

/// The prompt name for a Game Boy button (matches the keyboard wizard: directions
/// + select/start lowercase, A/B uppercase).
#[must_use]
fn gb_prompt(b: Button) -> &'static str {
    match b {
        Button::Right => "right",
        Button::Left => "left",
        Button::Up => "up",
        Button::Down => "down",
        Button::A => "A",
        Button::B => "B",
        Button::Select => "select",
        Button::Start => "start",
    }
}

/// The controller-config wizard (Options → Joypad → "configure game controller"):
/// steps through the 8 Game Boy buttons, binding each to the next controller
/// button pressed. A faithful clone of the keyboard [`crate::keymap::
/// KeyConfigWizard`] flow, sharing its modal render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GamepadConfigWizard {
    step: usize,
    working: GamepadBindings,
}

impl GamepadConfigWizard {
    #[must_use]
    pub(crate) fn open(current: GamepadBindings) -> Self {
        Self {
            step: 0,
            working: current,
        }
    }

    #[must_use]
    pub(crate) fn current_button(&self) -> Option<Button> {
        GB_ORDER.get(self.step).copied()
    }

    /// Bind the current button to controller button `gp` and advance.
    pub(crate) fn bind(&mut self, gp: GpButton) {
        if let Some(b) = self.current_button() {
            self.working.bind(b, gp);
            self.step += 1;
        }
    }

    /// Keep the current binding and advance (Skip/keep).
    pub(crate) fn skip_keep(&mut self) {
        if self.step < GB_ORDER.len() {
            self.step += 1;
        }
    }

    /// Unbind the current button and advance (Skip/clear).
    pub(crate) fn skip_clear(&mut self) {
        if let Some(b) = self.current_button() {
            self.working.unbind(b);
            self.step += 1;
        }
    }

    /// The finished bindings once every step is past; `None` while configuring.
    #[must_use]
    pub(crate) fn finished(&self) -> Option<GamepadBindings> {
        (self.step >= GB_ORDER.len()).then(|| self.working.clone())
    }

    /// Draw the modal (reuses the keyboard wizard's shared render).
    pub(crate) fn render(&self, c: &mut crate::ui::canvas::Canvas, theme: &crate::ui::Theme) {
        let prompt = self.current_button().map_or("", gb_prompt);
        let mapped = self
            .current_button()
            .and_then(|b| self.working.gp_for(b))
            .map_or("(none)", gp_name);
        crate::keymap::render_rebind_wizard(c, theme, prompt, mapped, self.current_button());
    }
}

/// Which Game Boy directions a stick axis value drives past the deadzone:
/// `(positive, negative)`. X → `(Right, Left)`, Y → `(Up, Down)` (gilrs Y is
/// positive-up). Pure so the deadzone edges are unit-tested without hardware.
#[must_use]
pub(crate) fn axis_dirs(val: f32, deadzone: f32) -> (bool, bool) {
    (val > deadzone, val < -deadzone)
}

/// Live gamepad state: the `gilrs` context (absent if it failed to initialise,
/// e.g. headless) and the left-stick's current per-direction held state, so
/// stick motion emits press/release *edges* like the digital buttons.
pub(crate) struct Gamepads {
    gilrs: Option<Gilrs>,
    /// Held state of the 4 stick-driven directions, indexed by [`gb_index`]
    /// (only the direction slots 0..4 are used).
    stick_held: [bool; 4],
    deadzone: f32,
}

impl Gamepads {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            // A missing gamepad subsystem (no udev, headless) must not be fatal:
            // the emulator just runs without controller input.
            gilrs: Gilrs::new().ok(),
            stick_held: [false; 4],
            deadzone: 0.5,
        }
    }

    /// Drain pending controller events into Game Boy button edges
    /// `(button, pressed)`, resolving digital buttons through `bindings` and the
    /// left stick through [`axis_dirs`]. Returns an empty vec with no gamepad
    /// subsystem or no pending events.
    pub(crate) fn poll(&mut self, bindings: &GamepadBindings) -> Vec<(Button, bool)> {
        // Drain gilrs first (releasing its `&mut self.gilrs` borrow) so the stick
        // handling below can borrow the rest of `self`.
        let mut events = Vec::new();
        if let Some(gilrs) = self.gilrs.as_mut() {
            while let Some(ev) = gilrs.next_event() {
                events.push(ev.event);
            }
        }
        let mut ops = Vec::new();
        for event in events {
            match event {
                EventType::ButtonPressed(b, _) => {
                    if let Some(gb) = bindings.gb_for(b) {
                        ops.push((gb, true));
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if let Some(gb) = bindings.gb_for(b) {
                        ops.push((gb, false));
                    }
                }
                EventType::AxisChanged(Axis::LeftStickX, val, _) => {
                    self.stick_edge(&mut ops, val, Button::Right, Button::Left);
                }
                EventType::AxisChanged(Axis::LeftStickY, val, _) => {
                    self.stick_edge(&mut ops, val, Button::Up, Button::Down);
                }
                _ => {}
            }
        }
        ops
    }

    /// Drain events and return the first controller button pressed this poll, for
    /// the config wizard. Remaining events are discarded — the wizard binds one
    /// press then advances. Resets the stick held-state so a stick nudged during
    /// configuration doesn't linger as a phantom hold once the wizard closes.
    pub(crate) fn next_pressed(&mut self) -> Option<GpButton> {
        self.stick_held = [false; 4];
        let mut hit = None;
        if let Some(gilrs) = self.gilrs.as_mut() {
            while let Some(ev) = gilrs.next_event() {
                if let EventType::ButtonPressed(b, _) = ev.event {
                    hit.get_or_insert(b);
                }
            }
        }
        hit
    }

    /// Compare a stick axis to its stored held state, pushing an edge only when
    /// a direction crosses the deadzone (so a held stick doesn't spam presses).
    fn stick_edge(&mut self, ops: &mut Vec<(Button, bool)>, val: f32, pos: Button, neg: Button) {
        let (want_pos, want_neg) = axis_dirs(val, self.deadzone);
        for (button, want) in [(pos, want_pos), (neg, want_neg)] {
            let slot = gb_index(button);
            if self.stick_held[slot] != want {
                self.stick_held[slot] = want;
                ops.push((button, want));
            }
        }
    }
}

#[cfg(test)]
#[path = "gamepad_tests.rs"]
mod tests;
