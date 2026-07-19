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

    /// Drain the controller into the joypad: poll `gilrs` for button/stick edges
    /// (Options → Joypad → game controller) and feed each through the same
    /// deferred sub-frame path as the keyboard. A no-op with no controller.
    pub(crate) fn poll_gamepad(&mut self) {
        // A rebind wizard is open: swallow controller presses to assign buttons
        // instead of driving the player.
        if self.gamepad_wizard.is_some() {
            if let Some(gp) = self.gamepad.next_pressed() {
                if let Some(w) = self.gamepad_wizard.as_mut() {
                    w.bind(gp);
                }
                self.commit_gamepad_wizard_if_done();
            }
            return;
        }
        let ops = self.gamepad.poll(&self.gamepad_bindings);
        // "Game controller works only if app has focus": drop the edges when
        // unfocused (still drained above, so nothing backs up). Any button held
        // across focus loss is released by `release_all_input`.
        if self.settings.gamepad_needs_focus && !self.window_focused {
            return;
        }
        for (button, pressed) in ops {
            self.set_gamepad_button(button, pressed);
        }
    }

    /// Commit a finished controller-config wizard: adopt its bindings, persist
    /// them, and drop the wizard.
    pub(crate) fn commit_gamepad_wizard_if_done(&mut self) {
        if let Some(binds) = self.gamepad_wizard.as_ref().and_then(|w| w.finished()) {
            let cfg = binds.to_config();
            self.gamepad_bindings = binds;
            self.gamepad_wizard = None;
            self.apply_gamepad_map(cfg);
        }
    }

    /// Commit a new controller map (from the wizard or "clear"): store it, mirror
    /// it into an open Options dialog's working + baseline scratch so a later
    /// Apply/Cancel can't revert this already-committed change, and persist.
    pub(crate) fn apply_gamepad_map(&mut self, cfg: String) {
        self.settings.gamepad_map = cfg;
        if let Some(o) = &mut self.options {
            o.working.gamepad_map = self.settings.gamepad_map.clone();
            o.baseline.gamepad_map = self.settings.gamepad_map.clone();
        }
        crate::settings_file::save(&self.settings, &self.recent);
    }

    /// Apply a controller button edge to the joypad, with the same SOCD filter as
    /// the keyboard (Joypad → "allow pressing L+R or U+D") over a controller-only
    /// held-set — a stick can't report opposing directions, but a face button
    /// mapped to a direction could.
    fn set_gamepad_button(&mut self, button: Button, pressed: bool) {
        let idx = crate::gamepad::gb_index(button);
        self.gamepad_held[idx] = pressed;
        if pressed {
            if let Some(opp) = keymap::socd_suppress(button, self.settings.allow_opposing) {
                self.queue_input(opp, false);
            }
            self.queue_input(button, true);
        } else {
            self.queue_input(button, false);
            if let Some(opp) = keymap::socd_suppress(button, self.settings.allow_opposing) {
                if self.gamepad_held[crate::gamepad::gb_index(opp)] {
                    self.queue_input(opp, true);
                }
            }
        }
    }

    /// Rapid-fire (Joypad "Rapid speed"): while `[`/`]` is held, toggle A/B every
    /// `rapid_speed` frames; release cleanly when the key is let go. Queued into
    /// the same deferred-input path as a real press. Called once per emulated
    /// frame batch, before `apply_pending_input`.
    pub(crate) fn apply_autofire(&mut self) {
        self.rapid_counter = self.rapid_counter.wrapping_add(1);
        let period = self.settings.rapid_speed.max(1);
        let on = (self.rapid_counter / period) % 2 == 0;
        if self.rapid_a {
            if on != self.rapid_a_on {
                self.queue_input(Button::A, on);
                self.rapid_a_on = on;
            }
        } else if self.rapid_a_on {
            self.queue_input(Button::A, false);
            self.rapid_a_on = false;
        }
        if self.rapid_b {
            if on != self.rapid_b_on {
                self.queue_input(Button::B, on);
                self.rapid_b_on = on;
            }
        } else if self.rapid_b_on {
            self.queue_input(Button::B, false);
            self.rapid_b_on = false;
        }
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
        self.gamepad_held = [false; 8];
        // No release events will arrive for the keys held at focus loss, so forget
        // them — else a later fresh press would look like a still-held repeat and
        // be dropped by the key-repeat guard.
        self.held_keys.clear();
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
