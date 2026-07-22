//! Rebindable keyboard → Game Boy button map and the bgb-style "configure
//! keyboard" wizard.
//!
//! [`KeyBindings`] replaces the old hard-coded `Action::Button` arms in
//! [`crate::input::map`]: `App` owns a `KeyBindings` and resolves held buttons
//! through it, so the Options → Joypad "configure keyboard" wizard can rebind
//! any key at runtime. The wizard ([`KeyConfigWizard`]) is a faithful clone of
//! bgb's sequential dialog (captured in
//! `docs/bgb-reference/options/joypad-keyconfig.png`).

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height, measure};
use slopgb_core::Button;
use winit::keyboard::KeyCode;

/// The eight Game Boy buttons in the order bgb's keyboard-config wizard walks
/// them (see `joypad-keyconfig.png`): right, left, up, down, A, B, select,
/// start. Used both to index [`KeyBindings`] and to drive the wizard.
pub(crate) const WIZARD_ORDER: [Button; 8] = [
    Button::Right,
    Button::Left,
    Button::Up,
    Button::Down,
    Button::A,
    Button::B,
    Button::Select,
    Button::Start,
];

/// Stable slot index for a button (matches [`WIZARD_ORDER`]).
fn index(b: Button) -> usize {
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

/// A keyboard → button map: each of the eight buttons holds at most one key.
/// The default reproduces slopgb's historical hard-coded bindings (Z=A, X=B,
/// Enter=Start, Right-Shift=Select, arrows = D-pad) so existing muscle memory
/// is unchanged; the wizard can rebind any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBindings {
    keys: [Option<KeyCode>; 8],
}

impl Default for KeyBindings {
    fn default() -> Self {
        let mut b = Self { keys: [None; 8] };
        b.keys[index(Button::Right)] = Some(KeyCode::ArrowRight);
        b.keys[index(Button::Left)] = Some(KeyCode::ArrowLeft);
        b.keys[index(Button::Up)] = Some(KeyCode::ArrowUp);
        b.keys[index(Button::Down)] = Some(KeyCode::ArrowDown);
        b.keys[index(Button::A)] = Some(KeyCode::KeyZ);
        b.keys[index(Button::B)] = Some(KeyCode::KeyX);
        b.keys[index(Button::Select)] = Some(KeyCode::ShiftRight);
        b.keys[index(Button::Start)] = Some(KeyCode::Enter);
        b
    }
}

impl KeyBindings {
    /// The key currently bound to `button`, if any.
    #[must_use]
    pub fn key_for(&self, button: Button) -> Option<KeyCode> {
        self.keys[index(button)]
    }

    /// The button `code` is bound to, if any (reverse lookup). Bindings keep at
    /// most one button per key (see [`Self::set`]), so the first match is the
    /// only match.
    #[must_use]
    pub fn button_for(&self, code: KeyCode) -> Option<Button> {
        WIZARD_ORDER
            .into_iter()
            .find(|&b| self.keys[index(b)] == Some(code))
    }

    /// Bind `code` to `button`, first clearing `code` from any other button so
    /// the reverse lookup stays unambiguous (a key can drive only one button).
    pub fn set(&mut self, button: Button, code: KeyCode) {
        for slot in &mut self.keys {
            if *slot == Some(code) {
                *slot = None;
            }
        }
        self.keys[index(button)] = Some(code);
    }

    /// Unbind `button` (bgb's "Skip/clear").
    pub fn clear(&mut self, button: Button) {
        self.keys[index(button)] = None;
    }

    /// Serialize as eight comma-separated [`key_name`]s (`-` = unbound) in
    /// [`WIZARD_ORDER`]. Round-trips through [`Self::from_config`].
    #[must_use]
    pub(crate) fn to_config(self) -> String {
        self.keys
            .iter()
            .map(|k| k.map_or("-", key_name))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse [`Self::to_config`]; unknown/absent names default to the standard
    /// binding for that slot, so a truncated or hand-edited config can't wedge.
    #[must_use]
    pub(crate) fn from_config(s: &str) -> Self {
        let def = Self::default();
        let mut out = def;
        for (i, tok) in s.split(',').enumerate().take(8) {
            out.keys[i] = if tok == "-" {
                None
            } else {
                key_from_name(tok).or(def.keys[i])
            };
        }
        out
    }
}

/// The default keyboard map as a config string (seeds `Settings::default`).
#[must_use]
pub(crate) fn default_map_config() -> String {
    KeyBindings::default().to_config()
}

/// Reverse of [`key_name`], for loading a persisted map. Scans the bindable
/// keys rather than duplicating the name table, so the two can't drift.
///
/// ponytail: keys `key_name` labels `"?"` (F-keys, punctuation) don't survive a
/// save/load round-trip — they fall back to the slot default. Give them real
/// names in `key_name` if anyone binds them.
#[must_use]
fn key_from_name(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    const BINDABLE: [KeyCode; 51] = [
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Enter,
        Space,
        Tab,
        Backspace,
        Escape,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
    ];
    BINDABLE.into_iter().find(|&k| key_name(k) == name)
}

/// A short display label for a key, for the wizard's "currently mapped to:"
/// line. Covers the keys that can realistically be bound; anything else falls
/// back to `"?"` (it still binds — only the label is generic).
#[must_use]
pub fn key_name(code: KeyCode) -> &'static str {
    use KeyCode::*;
    match code {
        ArrowUp => "Up",
        ArrowDown => "Down",
        ArrowLeft => "Left",
        ArrowRight => "Right",
        Enter | NumpadEnter => "Enter",
        Space => "Space",
        Tab => "Tab",
        Backspace => "Backspace",
        Escape => "Esc",
        ShiftLeft => "LShift",
        ShiftRight => "RShift",
        ControlLeft => "LCtrl",
        ControlRight => "RCtrl",
        AltLeft => "LAlt",
        AltRight => "RAlt",
        KeyA => "A",
        KeyB => "B",
        KeyC => "C",
        KeyD => "D",
        KeyE => "E",
        KeyF => "F",
        KeyG => "G",
        KeyH => "H",
        KeyI => "I",
        KeyJ => "J",
        KeyK => "K",
        KeyL => "L",
        KeyM => "M",
        KeyN => "N",
        KeyO => "O",
        KeyP => "P",
        KeyQ => "Q",
        KeyR => "R",
        KeyS => "S",
        KeyT => "T",
        KeyU => "U",
        KeyV => "V",
        KeyW => "W",
        KeyX => "X",
        KeyY => "Y",
        KeyZ => "Z",
        Digit0 | Numpad0 => "0",
        Digit1 | Numpad1 => "1",
        Digit2 | Numpad2 => "2",
        Digit3 | Numpad3 => "3",
        Digit4 | Numpad4 => "4",
        Digit5 | Numpad5 => "5",
        Digit6 | Numpad6 => "6",
        Digit7 | Numpad7 => "7",
        Digit8 | Numpad8 => "8",
        Digit9 | Numpad9 => "9",
        _ => "?",
    }
}

/// bgb's three "configure keyboard" wizard buttons (see `joypad-keyconfig.png`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardButton {
    /// Abort the whole wizard, discarding edits (the App drops the wizard).
    Cancel,
    /// Unbind the current button and advance.
    SkipClear,
    /// Keep the current binding and advance.
    SkipKeep,
}

/// bgb's sequential "configure keyboard" dialog: it walks the eight buttons in
/// [`WIZARD_ORDER`], one per step. While a step is active, a keypress binds the
/// current button to that key and advances; Skip/keep advances keeping the
/// binding; Skip/clear unbinds and advances. The edits accumulate in a private
/// working copy so Cancel (the App dropping the wizard) is a clean no-op; the
/// finished bindings are committed only once every step is past
/// ([`Self::finished`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyConfigWizard {
    step: usize,
    working: KeyBindings,
}

impl KeyConfigWizard {
    /// Open the wizard seeded from the live `current` bindings.
    #[must_use]
    pub fn open(current: KeyBindings) -> Self {
        Self {
            step: 0,
            working: current,
        }
    }

    /// The button being configured at the current step (`None` once finished).
    #[must_use]
    pub fn current_button(&self) -> Option<Button> {
        WIZARD_ORDER.get(self.step).copied()
    }

    /// bgb's prompt name for the current button (matches the captured dialog:
    /// directions + select/start lowercase, A/B uppercase).
    #[must_use]
    pub fn prompt_name(&self) -> &'static str {
        match self.current_button() {
            Some(Button::Right) => "right",
            Some(Button::Left) => "left",
            Some(Button::Up) => "up",
            Some(Button::Down) => "down",
            Some(Button::A) => "A",
            Some(Button::B) => "B",
            Some(Button::Select) => "select",
            Some(Button::Start) => "start",
            None => "",
        }
    }

    /// The key currently mapped to the active button, for the dialog's
    /// "currently mapped to:" line.
    #[must_use]
    pub fn current_key(&self) -> Option<KeyCode> {
        self.current_button().and_then(|b| self.working.key_for(b))
    }

    /// Bind `code` to the current button and advance (a captured keypress).
    pub fn bind_key(&mut self, code: KeyCode) {
        if let Some(b) = self.current_button() {
            self.working.set(b, code);
            self.step += 1;
        }
    }

    /// Keep the current binding and advance (bgb's "Skip/keep").
    pub fn skip_keep(&mut self) {
        if self.step < WIZARD_ORDER.len() {
            self.step += 1;
        }
    }

    /// Unbind the current button and advance (bgb's "Skip/clear").
    pub fn skip_clear(&mut self) {
        if let Some(b) = self.current_button() {
            self.working.clear(b);
            self.step += 1;
        }
    }

    /// The finished bindings, once every step is past (the wizard ran to the
    /// end); `None` while still configuring. The App commits these to its live
    /// `bindings` and drops the wizard.
    #[must_use]
    pub fn finished(&self) -> Option<KeyBindings> {
        (self.step >= WIZARD_ORDER.len()).then_some(self.working)
    }

    /// Draw the wizard centred over the LCD, faithful to bgb's
    /// `joypad-keyconfig.png`: a GB illustration (current button red), the two
    /// text lines, and the Cancel / Skip/clear / Skip/keep buttons.
    pub fn render(&self, c: &mut Canvas, theme: &Theme) {
        let mapped = self.current_key().map_or("(none)", key_name);
        render_rebind_wizard(c, theme, self.prompt_name(), mapped, self.current_button());
    }

    /// Left-click hit-test over the wizard's three buttons.
    #[must_use]
    pub fn button_at(&self, bounds: Rect, px: i32, py: i32) -> Option<WizardButton> {
        wizard_button_at(bounds, px, py)
    }
}

/// Render the shared rebind-wizard modal (keyboard + game controller): the GB
/// illustration with `highlight`ed button, the "press … for X" prompt, the
/// "currently mapped to: …" line, and the Cancel / Skip-clear / Skip-keep row.
pub(crate) fn render_rebind_wizard(
    c: &mut Canvas,
    theme: &Theme,
    prompt_name: &str,
    mapped: &str,
    highlight: Option<Button>,
) {
    let bounds = c.bounds();
    let dlg = dialog_rect(bounds);
    c.fill_rect(dlg, theme.bg);
    c.outline_rect(dlg, theme.border);
    draw_wizard_gb(c, dlg, theme, highlight);

    let lh = line_height();
    let tx = dlg.x + 14;
    let ty = dlg.y + 84;
    draw_text(
        c,
        tx,
        ty,
        &format!("press and hold the button for {prompt_name}"),
        theme.text,
    );
    draw_text(
        c,
        tx,
        ty + lh + 4,
        &format!("currently mapped to: {mapped}"),
        theme.text,
    );
    for (kind, r) in button_rects(bounds) {
        c.outline_rect(r, theme.text);
        let label = kind.label();
        let lx = r.x + (r.w - measure(label)) / 2;
        draw_text(c, lx, r.y + (r.h - lh) / 2, label, theme.text);
    }
}

/// Left-click hit-test over the wizard's three bottom buttons (shared by both
/// rebind wizards; depends only on `bounds`).
#[must_use]
pub(crate) fn wizard_button_at(bounds: Rect, px: i32, py: i32) -> Option<WizardButton> {
    button_rects(bounds)
        .into_iter()
        .find(|(_, r)| r.contains(px, py))
        .map(|(b, _)| b)
}

/// Draw the Game Boy illustration (D-pad + A/B + select/start) with `highlight`
/// (the button being configured) drawn red.
fn draw_wizard_gb(c: &mut Canvas, dlg: Rect, theme: &Theme, highlight: Option<Button>) {
    {
        let ink = theme.text;
        let col = |b: Button| {
            if highlight == Some(b) {
                theme.breakpoint
            } else {
                ink
            }
        };
        let cell = 10;
        // D-pad cross, centred upper-left of the box.
        let cx = dlg.x + 52;
        let cy = dlg.y + 32;
        c.fill_rect(Rect::new(cx, cy - cell, cell, cell), col(Button::Up));
        c.fill_rect(Rect::new(cx, cy + cell, cell, cell), col(Button::Down));
        c.fill_rect(Rect::new(cx - cell, cy, cell, cell), col(Button::Left));
        c.fill_rect(Rect::new(cx + cell, cy, cell, cell), col(Button::Right));
        c.fill_rect(Rect::new(cx, cy, cell, cell), ink); // hub
        // A / B round buttons on the right (B lower-left, A upper-right).
        let bx = dlg.x + 150;
        let by = dlg.y + 20;
        c.fill_rect(Rect::new(bx, by + 8, cell, cell), col(Button::B));
        c.fill_rect(Rect::new(bx + 18, by, cell, cell), col(Button::A));
        // select / start pills below the centre.
        let sy = dlg.y + 56;
        c.fill_rect(Rect::new(dlg.x + 40, sy, 16, 5), col(Button::Select));
        c.fill_rect(Rect::new(dlg.x + 64, sy, 16, 5), col(Button::Start));
    }
}

impl WizardButton {
    fn label(self) -> &'static str {
        match self {
            WizardButton::Cancel => "Cancel",
            WizardButton::SkipClear => "Skip/clear",
            WizardButton::SkipKeep => "Skip/keep",
        }
    }
}

/// Width/height of the centred wizard box.
const DLG_W: i32 = 240;
const DLG_H: i32 = 176;

/// The wizard box centred within `bounds`.
fn dialog_rect(bounds: Rect) -> Rect {
    let x = bounds.x + (bounds.w - DLG_W) / 2;
    let y = bounds.y + (bounds.h - DLG_H) / 2;
    Rect::new(x, y, DLG_W, DLG_H)
}

/// The three button hit-rects along the bottom of the wizard box, evenly spaced.
fn button_rects(bounds: Rect) -> [(WizardButton, Rect); 3] {
    let dlg = dialog_rect(bounds);
    let (bw, bh) = (72, 18);
    let gap = (DLG_W - 3 * bw) / 4;
    let by = dlg.bottom() - bh - 8;
    let x0 = dlg.x + gap;
    [
        (WizardButton::Cancel, Rect::new(x0, by, bw, bh)),
        (
            WizardButton::SkipClear,
            Rect::new(x0 + bw + gap, by, bw, bh),
        ),
        (
            WizardButton::SkipKeep,
            Rect::new(x0 + 2 * (bw + gap), by, bw, bh),
        ),
    ]
}

/// The opposite cardinal direction (for the SOCD filter); `None` for the action
/// buttons. The two axes (L/R and U/D) are independent.
#[must_use]
pub fn opposite(b: Button) -> Option<Button> {
    Some(match b {
        Button::Left => Button::Right,
        Button::Right => Button::Left,
        Button::Up => Button::Down,
        Button::Down => Button::Up,
        _ => return None,
    })
}

/// When "allow pressing L+R or U+D" is off (bgb's default), pressing `button`
/// must suppress its opposite so the joypad never reports both directions at
/// once. Returns the button to release, or `None` when nothing is suppressed
/// (the filter is on, or `button` is not a direction).
#[must_use]
pub fn socd_suppress(button: Button, allow_opposing: bool) -> Option<Button> {
    if allow_opposing {
        None
    } else {
        opposite(button)
    }
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
