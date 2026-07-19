//! `App` game-window keyboard dispatch: modal capture (wizards / dialogs /
//! Options), the rebindable joypad map, the focus-dependent hotkey actions, and
//! the key-rebind wizard plumbing. Split out of `main.rs` to keep it under the
//! size cap.

use winit::event::KeyEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::input::{Action, Focus};
use crate::ui::dialog::DialogKey;
use crate::{App, cheat_ui, file_picker, input, keymap, ui};

impl App {
    pub(crate) fn handle_key(
        &mut self,
        event_loop: &ActiveEventLoop,
        key: &KeyEvent,
        focus: Focus,
    ) {
        // In the debugger, memory-nav keys (arrows / PageUp-Down) auto-repeat so a
        // held arrow scrolls the memory pane continuously; every other key — and
        // the same arrows in the game window, where they are the D-pad — is
        // de-repeated (see the guards below).
        let nav = focus == Focus::Debugger
            && matches!(
                key.physical_key,
                PhysicalKey::Code(
                    KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::PageUp | KeyCode::PageDown
                )
            );
        if key.repeat && !nav {
            return;
        }
        // Platform-independent key-repeat guard: some Wayland compositors don't
        // set winit's `repeat` flag, so a held step key (F7/F3/F8) would step
        // repeatedly. Drop a press for an already-held key; always honor releases.
        if let PhysicalKey::Code(code) = key.physical_key {
            if !nav && !input::accept_key(&mut self.held_keys, code, key.state.is_pressed()) {
                return;
            }
        }
        // The key-rebind wizard (Joypad → "configure keyboard") is the topmost
        // game-window modal: every key is captured. Escape cancels the whole
        // wizard (edits discarded); any other key binds the current button and
        // advances — finishing commits the new bindings.
        if focus == Focus::Game && key.state.is_pressed() && self.key_wizard.is_some() {
            if let PhysicalKey::Code(code) = key.physical_key {
                if code == KeyCode::Escape {
                    self.key_wizard = None;
                } else if let Some(w) = self.key_wizard.as_mut() {
                    w.bind_key(code);
                    self.commit_wizard_if_done();
                }
            }
            self.request_game_redraw();
            return;
        }
        // The controller-rebind wizard captures game-window keys too: Escape
        // cancels it; other keys are swallowed (the binding target is the
        // controller, not the keyboard) so they don't move the game mid-config.
        if focus == Focus::Game && key.state.is_pressed() && self.gamepad_wizard.is_some() {
            if let PhysicalKey::Code(KeyCode::Escape) = key.physical_key {
                self.gamepad_wizard = None;
            }
            self.request_game_redraw();
            return;
        }
        // A path modal captures every key while open (so typing a path can't
        // fire a hotkey); Enter accepts, Esc cancels. Checked before Options
        // because it can float over the dialog (the bootrom `...` browse).
        if focus == Focus::Game && key.state.is_pressed() && self.path_dialog.is_some() {
            if let Some(dk) = dialog_key_from(key) {
                if let Some(result) = self.path_dialog.as_mut().map(|d| d.on_key(dk)) {
                    self.resolve_path_dialog(result);
                }
            }
            return;
        }
        // The in-app file browser captures keys with the same rule as the path
        // modal above, translated through `file_picker::winit_key_to_picker`
        // instead of `dialog_key_from`.
        if focus == Focus::Game && key.state.is_pressed() && self.file_picker.is_some() {
            if let PhysicalKey::Code(code) = key.physical_key {
                if let Some(pk) =
                    file_picker::winit_key_to_picker(code, key.text.as_deref(), self.modifiers)
                {
                    let outcome = self.file_picker.as_mut().map(|fp| fp.feed_key(pk));
                    self.resolve_file_picker(outcome);
                }
            }
            return;
        }
        // The Cheat dialog captures keys while open. An open Add/Edit entry takes
        // every key (typing a code can't fire a hotkey); otherwise arrows move the
        // selection, Space toggles enable, Delete removes, Escape closes.
        if focus == Focus::Game && key.state.is_pressed() && self.cheat_dialog.is_some() {
            if self
                .cheat_dialog
                .as_ref()
                .is_some_and(cheat_ui::CheatDialog::editor_open)
            {
                if let PhysicalKey::Code(code) = key.physical_key {
                    match code {
                        KeyCode::Tab => {
                            if let Some(d) = &mut self.cheat_dialog {
                                d.switch_field();
                            }
                        }
                        KeyCode::Enter | KeyCode::NumpadEnter => {
                            let edit = self
                                .cheat_dialog
                                .as_mut()
                                .and_then(cheat_ui::CheatDialog::accept);
                            if let Some(e) = edit {
                                self.apply_cheat_edit(&e);
                            }
                        }
                        KeyCode::Escape => {
                            if let Some(d) = &mut self.cheat_dialog {
                                d.cancel_editor();
                            }
                        }
                        KeyCode::Backspace => {
                            if let Some(d) = &mut self.cheat_dialog {
                                d.backspace();
                            }
                        }
                        _ => {
                            if let Some(ch) = key.text.as_ref().and_then(|t| t.chars().next()) {
                                if !ch.is_control() {
                                    if let Some(d) = &mut self.cheat_dialog {
                                        d.type_char(ch);
                                    }
                                }
                            }
                        }
                    }
                }
            } else if let PhysicalKey::Code(code) = key.physical_key {
                let sel = self.cheat_dialog.as_ref().map_or(0, |d| d.sel);
                match code {
                    KeyCode::Escape => self.cheat_dialog = None,
                    KeyCode::ArrowUp => {
                        if let Some(d) = &mut self.cheat_dialog {
                            d.sel = d.sel.saturating_sub(1);
                        }
                    }
                    KeyCode::ArrowDown => {
                        let n = self.cheats.len();
                        if let Some(d) = &mut self.cheat_dialog {
                            d.sel = (d.sel + 1).min(n.saturating_sub(1));
                        }
                    }
                    KeyCode::Space => {
                        self.cheats.toggle(sel);
                    }
                    KeyCode::Delete => {
                        self.cheats.remove(sel);
                        self.clamp_cheat_sel();
                    }
                    _ => {}
                }
            }
            self.request_game_redraw();
            return;
        }
        // Options control panel is modal: while it's open every key is swallowed
        // (so a hotkey can't fire underneath it); Escape cancels (reverts edits)
        // and closes, matching a Windows dialog's Esc.
        if focus == Focus::Game && key.state.is_pressed() && self.options.is_some() {
            if let PhysicalKey::Code(KeyCode::Escape) = key.physical_key {
                // Esc = Cancel: just drop the dialog without applying — the live
                // state already equals the baseline (only OK/Apply push live), so
                // discarding the unapplied `working` edits is the whole revert.
                self.options = None;
                self.request_game_redraw();
            }
            return;
        }
        // With a game-window overlay open, Escape closes it (rather than quitting
        // the emulator) and is swallowed so it can't also fire a hotkey. The info
        // box peels first; the right-click popup (its own window) also closes on
        // its own Escape, but close it here too in case the game window kept focus.
        let overlay_open = self.info_box.is_some() || self.menu_popup.is_some();
        if focus == Focus::Game && key.state.is_pressed() && overlay_open {
            if let PhysicalKey::Code(KeyCode::Escape) = key.physical_key {
                if self.info_box.take().is_none() {
                    self.menu_popup = None;
                }
                self.request_game_redraw();
                return;
            }
        }
        // Modal capture: while the debugger's modal prompt (Go to… / edit
        // register) is open, every key goes to it (so typing an address can't
        // trigger a debugger hotkey). An `edit register` accept yields a
        // register write, applied through the same path a menu/click uses.
        if focus == Focus::Debugger && key.state.is_pressed() && self.tools.debugger_modal_active()
        {
            if let Some(dk) = dialog_key_from(key) {
                if let Some(outcome) = self.tools.feed_debugger_dialog(dk) {
                    self.apply_menu_outcome(outcome, event_loop);
                }
            }
            return;
        }
        let PhysicalKey::Code(code) = key.physical_key else {
            return;
        };
        let pressed = key.state.is_pressed();
        // Game Boy buttons resolve through the rebindable map first, before the
        // focus-specific actions — but only in the game window. A tool window
        // (e.g. the debugger) must not drive the joypad, so its arrow keys can
        // scroll the memory pane instead of moving the D-pad.
        if focus == Focus::Game {
            if let Some(b) = self.bindings.button_for(code) {
                self.set_button(code, b, pressed);
                return;
            }
        }
        // bgb shows the debugger on Esc — it never quits the emulator. Handled
        // here (not in the pure `input::map`) because honouring the Options
        // "pressing Esc shows debugger" toggle needs the runtime setting. Toggles
        // from any focus (game/viewer opens, debugger closes); the modal guards
        // above already consumed Esc where a dialog was open. BUG-1.
        if code == KeyCode::Escape {
            if pressed && self.settings.esc_shows_debugger {
                self.run_action(Action::ToggleTool(ui::ToolWindow::Debugger), event_loop);
            }
            return;
        }
        let Some(action) = input::map(code, self.modifiers, focus) else {
            return;
        };
        match action {
            Action::Turbo => {
                self.turbo = pressed;
                if !pressed {
                    self.resync_pacing();
                }
            }
            // Rewind while held (System → "Rewind enabled"); resume forward play
            // on release. A no-op if rewind is off / the ring is empty.
            Action::Rewind => {
                self.rewinding = pressed;
                if !pressed {
                    self.resync_pacing();
                }
            }
            // Rapid-fire A / B while held (Joypad "Rapid speed" cadence).
            Action::RapidA => self.rapid_a = pressed,
            Action::RapidB => self.rapid_b = pressed,
            // Every other action fires on press only; the debugger menu items
            // reuse this same dispatch via `run_action`, so a hotkey and its
            // menu entry can never diverge.
            _ if pressed => self.run_action(action, event_loop),
            _ => {}
        }
    }

    /// Open the Joypad "configure keyboard" wizard seeded from the live map.
    pub(crate) fn open_key_wizard(&mut self) {
        self.key_wizard = Some(keymap::KeyConfigWizard::open(self.bindings));
    }

    /// If the wizard has run through all eight buttons, commit its working map
    /// to the live `bindings` and close it. Any buttons held under the old map
    /// are released so a remap can't leave a key stuck down.
    pub(crate) fn commit_wizard_if_done(&mut self) {
        if let Some(bindings) = self.key_wizard.as_ref().and_then(|w| w.finished()) {
            self.bindings = bindings;
            self.release_all_input();
            self.key_wizard = None;
        }
    }
}

/// Translate a winit key event into an abstract [`DialogKey`] for the modal
/// prompt: the named editing keys (backspace / enter / escape), else the typed
/// character.
pub(crate) fn dialog_key_from(key: &KeyEvent) -> Option<DialogKey> {
    if let PhysicalKey::Code(code) = key.physical_key {
        match code {
            KeyCode::Backspace => return Some(DialogKey::Backspace),
            KeyCode::Enter | KeyCode::NumpadEnter => return Some(DialogKey::Enter),
            KeyCode::Escape => return Some(DialogKey::Escape),
            _ => {}
        }
    }
    let ch = key.text.as_ref()?.chars().next()?;
    (!ch.is_control()).then_some(DialogKey::Char(ch))
}
