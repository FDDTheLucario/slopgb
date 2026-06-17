//! `App` joypad-input handling: keyboard → Game Boy button application with the
//! SOCD filter (Joypad → "allow pressing L+R or U+D"), plus the **sub-frame
//! input timing** that gives the joypad interrupt a realistic, varied LCD line.
//!
//! Live key events arrive between emulated frames, so applying a press directly
//! would fire the joypad interrupt at the same `LY` every time (the `tellinglys`
//! input-entropy ROM's "Incorrect behavior"). Instead a press is queued with the
//! keypress's wall-clock sub-frame phase and applied at that T-cycle offset into
//! the next frame ([`crate::input::apply_input`]), so consecutive presses land
//! on different lines — "Pass! Joypad interrupt timing is realistic".

use slopgb_core::{Button, CYCLES_PER_FRAME};
use winit::keyboard::KeyCode;

use crate::{App, FRAME_DURATION, input, keymap};

impl App {
    /// Apply a key press/release to the joypad, through the SOCD filter and the
    /// deferred-timing queue. (Reverse-mapped to its `Button` by `handle_key`.)
    pub(crate) fn set_button(&mut self, code: KeyCode, button: Button, pressed: bool) {
        if pressed {
            self.buttons.press(code, button);
            // SOCD filter (off by default = bgb): a new direction suppresses its
            // opposite so the joypad never reports both — last input wins.
            if let Some(opp) = keymap::socd_suppress(button, self.settings.allow_opposing) {
                self.queue_input(opp, false);
            }
            self.queue_input(button, true);
        } else if self.buttons.release(code, button) {
            self.queue_input(button, false);
            // Resurrection (last-input priority): if the opposite direction is
            // still physically held, re-press it — releasing the newer key
            // returns control to the older one that was suppressed.
            if let Some(opp) = keymap::socd_suppress(button, self.settings.allow_opposing) {
                if self.buttons.is_held(opp) {
                    self.queue_input(opp, true);
                }
            }
        }
    }

    /// Queue a joypad op for the next emulated frame (applied by
    /// [`Self::apply_pending_input`]). On the first op of a batch, capture the
    /// keypress's wall-clock phase within the frame period as a T-cycle offset,
    /// so consecutive presses fire the joypad interrupt on different LCD lines —
    /// the input entropy real hardware has (`input::apply_input`).
    fn queue_input(&mut self, button: Button, pressed: bool) {
        if self.input_ops.is_empty() {
            let frame_ns = FRAME_DURATION.as_nanos();
            let phase = self.epoch.elapsed().as_nanos() % frame_ns;
            self.input_offset = (phase * u128::from(CYCLES_PER_FRAME) / frame_ns) as u32;
        }
        self.input_ops.push((button, pressed));
    }

    /// Apply any deferred joypad ops at their captured sub-frame offset, just
    /// before the frame pacers run. A no-op when nothing is queued.
    pub(crate) fn apply_pending_input(&mut self) {
        input::apply_input(&mut self.session.gb, &mut self.input_ops, self.input_offset);
    }

    /// While frozen (paused / no ROM / debugger-broken) no frame runs, so a
    /// queued op would otherwise apply with a stale offset on resume. Drop the
    /// *presses* (a press on a frozen machine shouldn't register), but still
    /// apply the *releases* directly — else a button physically released while
    /// paused would stay stuck held when emulation resumes.
    pub(crate) fn flush_idle_input(&mut self) {
        for (button, pressed) in self.input_ops.drain(..) {
            if !pressed {
                self.session.gb.release(button);
            }
        }
    }

    /// Focus lost or window occluded: no release events will arrive for keys
    /// held right now, so release every Game Boy button and drop turbo before
    /// they stick.
    pub(crate) fn release_all_input(&mut self) {
        self.buttons.clear();
        self.input_ops.clear(); // drop any deferred press; we're releasing all
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
}
