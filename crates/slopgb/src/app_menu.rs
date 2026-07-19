//! `App` game-window menu handling: the bgb right-click main menu, its
//! submenus (Window size / Sound channel / Other / State), the info boxes
//! (Cart info / System info / About), and the window-size resize. The menu
//! *widgets* live in [`crate::windows::mainwin`].

use winit::dpi::LogicalSize;
use winit::event::MouseButton;
use winit::event_loop::ActiveEventLoop;
use winit::window::Fullscreen;

use slopgb_core::{CLOCK_HZ, SCREEN_H, SCREEN_W};

use crate::cheat_ui::{self, CheatButton, CheatHit};
use crate::input::Action;
use crate::keymap::WizardButton;
use crate::menupopup::{MenuPopup, PopupOutcome};
use crate::ui::canvas::Rect;
use crate::windows::mainwin::{InfoBox, SubChoice, SubKind, SubMenu, WindowSizeChoice};
use crate::windows::options::OptionsOutcome;
use crate::{App, ui, windows};

impl App {
    /// A left/right press on the game window. Right-click (re)opens the bgb
    /// main-menu at the pointer (closing any submenu); left-click applies a
    /// submenu choice, runs a main-menu row's action, opens its submenu, or
    /// dismisses (a click off both popups closes them).
    pub(crate) fn on_game_click(&mut self, button: MouseButton, event_loop: &ActiveEventLoop) {
        let (px, py) = self.game_cursor;
        // The key-rebind wizard floats above everything (incl. Options): only a
        // left-click on one of its three buttons acts; anything else is swallowed
        // (a stray click can't leak to the dialog/menu beneath it).
        if self.key_wizard.is_some() {
            if button == MouseButton::Left {
                let area = self.window_area();
                let hit = self
                    .key_wizard
                    .as_ref()
                    .and_then(|w| w.button_at(area, px, py));
                match hit {
                    Some(WizardButton::Cancel) => self.key_wizard = None,
                    Some(WizardButton::SkipKeep) => {
                        if let Some(w) = self.key_wizard.as_mut() {
                            w.skip_keep();
                        }
                        self.commit_wizard_if_done();
                    }
                    Some(WizardButton::SkipClear) => {
                        if let Some(w) = self.key_wizard.as_mut() {
                            w.skip_clear();
                        }
                        self.commit_wizard_if_done();
                    }
                    None => {}
                }
            }
            self.request_game_redraw();
            return;
        }
        // The controller-rebind wizard shares the same modal slot + buttons.
        if self.gamepad_wizard.is_some() {
            if button == MouseButton::Left {
                let area = self.window_area();
                match crate::keymap::wizard_button_at(area, px, py) {
                    Some(WizardButton::Cancel) => self.gamepad_wizard = None,
                    Some(WizardButton::SkipKeep) => {
                        if let Some(w) = self.gamepad_wizard.as_mut() {
                            w.skip_keep();
                        }
                        self.commit_gamepad_wizard_if_done();
                    }
                    Some(WizardButton::SkipClear) => {
                        if let Some(w) = self.gamepad_wizard.as_mut() {
                            w.skip_clear();
                        }
                        self.commit_gamepad_wizard_if_done();
                    }
                    None => {}
                }
            }
            self.request_game_redraw();
            return;
        }
        // A path modal is topmost — it can float over the Options dialog (the
        // bootrom `...` browse) as well as stand alone (Load ROM / save state),
        // so it is checked before Options. Route the click to OK/Cancel.
        if self.path_dialog.is_some() {
            let area = self.window_area();
            if let Some(r) = self
                .path_dialog
                .as_ref()
                .map(|d| ui::dialog::click(d, area, px, py))
            {
                self.resolve_path_dialog(r);
            }
            return;
        }
        // The in-app file browser is topmost too (it opens in exactly
        // the situations the path modal would have): only the left button acts;
        // a second left-press close enough in time+space to the first is a
        // double-click (activate), matching `toolwin::ToolView::note_click`.
        if self.file_picker.is_some() {
            if button == MouseButton::Left {
                let now = std::time::Instant::now();
                let double = self.picker_last_click.is_some_and(|(t, lx, ly)| {
                    crate::toolwin::is_double_click(now.duration_since(t), px - lx, py - ly)
                });
                self.picker_last_click = if double { None } else { Some((now, px, py)) };
                let outcome = self
                    .file_picker
                    .as_mut()
                    .map(|fp| fp.on_click(px, py, double));
                self.resolve_file_picker(outcome);
            }
            return;
        }
        // The Cheat dialog (main menu "Cheat.../F10"): only the left button acts.
        // An open Add/Edit entry captures input (keyboard), so dialog clicks are
        // ignored then; otherwise a click selects a row or fires a button.
        if self.cheat_dialog.is_some() {
            if button == MouseButton::Left {
                let area = self.window_area();
                self.on_cheat_click(area, px, py);
            }
            self.request_game_redraw();
            return;
        }
        // The Options control panel is the next modal: only the left button acts
        // (tabs/controls/buttons); a right-click is swallowed so it can't be
        // misread as a left-click toggling a setting. OK/Cancel/Apply applies the
        // working settings (Cancel having reverted them first); Close drops it.
        if self.options.is_some() {
            if button != MouseButton::Left {
                return;
            }
            let area = self.window_area();
            let outcome = self.options.as_mut().and_then(|o| o.on_click(px, py, area));
            if let Some(out) = outcome {
                match out {
                    // Float the key-rebind wizard above the (still-open) dialog.
                    OptionsOutcome::ConfigureKeyboard => self.open_key_wizard(),
                    // Float the controller-rebind wizard above the dialog.
                    OptionsOutcome::ConfigureGamepad => {
                        self.gamepad_wizard = Some(crate::gamepad::GamepadConfigWizard::open(
                            self.gamepad_bindings.clone(),
                        ));
                    }
                    // Unbind every controller button, committing immediately.
                    OptionsOutcome::ClearGamepad => {
                        self.gamepad_bindings.clear();
                        let cfg = self.gamepad_bindings.to_config();
                        self.apply_gamepad_map(cfg);
                    }
                    // Open the path modal over the dialog to edit a bootrom path.
                    OptionsOutcome::PickBootrom(slot) => {
                        self.open_path_prompt("Bootrom path", crate::PathPurpose::Bootrom(slot))
                    }
                    // Open the path modal to edit the plugins directory.
                    OptionsOutcome::PickPluginsDir => {
                        self.open_path_prompt("Plugins dir", crate::PathPurpose::PluginsDir)
                    }
                    // Advance the working output device to the next enumerated one.
                    OptionsOutcome::CycleSoundcard => {
                        if let Some(o) = &mut self.options {
                            let devices = crate::audio::AudioOutput::device_names();
                            o.working.audio_device = crate::audio::cycle_output_device(
                                &o.working.audio_device,
                                &devices,
                            );
                        }
                    }
                    // OK/Apply push working live; Cancel/Defaults do not (Defaults
                    // only edits the controls, matching bgb — nothing goes live
                    // until OK/Apply).
                    _ => {
                        if out.applies() {
                            if let Some(o) = &self.options {
                                self.settings = o.working.clone();
                            }
                            self.apply_settings();
                        }
                        if out.closes() {
                            self.options = None;
                        }
                    }
                }
            }
            self.request_game_redraw();
            return;
        }
        // An open info box is modal: any click dismisses it and is swallowed.
        if self.info_box.take().is_some() {
            self.request_game_redraw();
            return;
        }
        // A left-click on the game window while the menu popup is open is a
        // click-away: dismiss it (the popup's own clicks come through its window).
        if self.menu_popup.is_some() && button == MouseButton::Left {
            self.menu_popup = None;
            return;
        }
        if button == MouseButton::Right {
            // (Re)open the right-click menu as its own borderless window, at the
            // pointer, so it can extend past the game window instead of clipping.
            if let Some(win) = self.window.clone() {
                let theme = self.settings.theme.resolve(&self.custom_themes);
                self.menu_popup =
                    MenuPopup::open(event_loop, &win, (px, py), !self.muted, self.paused, theme);
            }
        }
    }

    /// Route an event for the right-click menu's own borderless window
    /// ([`MenuPopup`]): render, hover, click (→ run an action / open a submenu),
    /// and dismiss on Escape / focus-loss / close.
    pub(crate) fn on_popup_event(
        &mut self,
        event: winit::event::WindowEvent,
        event_loop: &ActiveEventLoop,
    ) {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event {
            WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {
                if let Some(p) = &mut self.menu_popup {
                    p.redraw();
                }
            }
            // The WM closing the popup dismisses it outright.
            WindowEvent::CloseRequested => self.menu_popup = None,
            // Click-away (focus loss) dismisses — but only once the popup has
            // actually been focused (some WMs deliver a spurious on-map
            // `Focused(false)` that would otherwise close it instantly).
            WindowEvent::Focused(f) => {
                if self.menu_popup.as_mut().is_some_and(|p| p.note_focus(f)) {
                    self.menu_popup = None;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // A hover onto a submenu row auto-opens it (BUG-6), the same
                // OpenSub path a click takes.
                let outcome = self
                    .menu_popup
                    .as_mut()
                    .and_then(|p| p.on_cursor_moved(position.x, position.y));
                if let Some(PopupOutcome::OpenSub(kind, row)) = outcome {
                    let sub = self.open_submenu(kind, row);
                    if let Some(p) = &mut self.menu_popup {
                        p.set_submenu(sub);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.on_popup_click(event_loop),
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed()
                    && event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
            {
                self.menu_popup = None;
            }
            _ => {}
        }
    }

    /// Apply a left-click on the menu popup: run a leaf action, apply a submenu
    /// choice, open a submenu (grown into the same window), or dismiss.
    fn on_popup_click(&mut self, event_loop: &ActiveEventLoop) {
        let outcome = match &mut self.menu_popup {
            Some(p) => p.on_click(),
            None => return,
        };
        match outcome {
            PopupOutcome::Run(act) => {
                self.menu_popup = None;
                self.run_action(act, event_loop);
            }
            PopupOutcome::Sub(choice) => {
                self.menu_popup = None;
                self.apply_sub_choice(choice, event_loop);
            }
            PopupOutcome::OpenSub(kind, row) => {
                let sub = self.open_submenu(kind, row);
                if let Some(p) = &mut self.menu_popup {
                    p.set_submenu(sub);
                }
            }
            PopupOutcome::Close => self.menu_popup = None,
        }
    }

    /// Build the child submenu for `kind`, seeding its check-marks from the
    /// live state (current window size / per-channel mute), hung off `row`.
    /// Route a left-click inside the open Cheat dialog to a row selection or a
    /// button action. An open Add/Edit entry is keyboard-driven, so it's ignored.
    fn on_cheat_click(&mut self, area: Rect, px: i32, py: i32) {
        if self
            .cheat_dialog
            .as_ref()
            .is_some_and(cheat_ui::CheatDialog::editor_open)
        {
            return;
        }
        match cheat_ui::hit(area, &self.cheats, px, py) {
            Some(CheatHit::Row(i)) => {
                if let Some(d) = &mut self.cheat_dialog {
                    d.sel = i;
                }
            }
            Some(CheatHit::Button(b)) => self.cheat_button(b),
            None => {}
        }
    }

    /// Act on a Cheat-dialog button (bgb's Add/Edit/Delete/Enable/Disable/
    /// Enable all/Disable all/Poke; Close drops the dialog).
    fn cheat_button(&mut self, b: CheatButton) {
        let sel = self.cheat_dialog.as_ref().map_or(0, |d| d.sel);
        match b {
            CheatButton::Add => {
                if let Some(d) = &mut self.cheat_dialog {
                    d.open_add();
                }
            }
            CheatButton::Edit => {
                if let Some(c) = self.cheats.items().get(sel).cloned() {
                    if let Some(d) = &mut self.cheat_dialog {
                        d.open_edit(sel, &c.comment, &c.code);
                    }
                }
            }
            CheatButton::Delete => {
                self.cheats.remove(sel);
                self.clamp_cheat_sel();
            }
            CheatButton::Enable => self.cheats.set_enabled(sel, true),
            CheatButton::Disable => self.cheats.set_enabled(sel, false),
            CheatButton::EnableAll => self.cheats.enable_all(),
            CheatButton::DisableAll => self.cheats.disable_all(),
            CheatButton::Poke => {
                if let Some((a, v)) = self.cheats.poke_once(sel) {
                    self.session.gb.debug_write(a, v);
                }
            }
            CheatButton::Advanced => {
                if let Some(d) = &mut self.cheat_dialog {
                    d.advanced = !d.advanced;
                }
            }
            CheatButton::Load => {
                self.open_path_prompt("Load cheats (path)", crate::PathPurpose::CheatLoad);
            }
            CheatButton::Save => {
                self.open_path_prompt("Save cheats (path)", crate::PathPurpose::CheatSave);
            }
            CheatButton::Close => self.cheat_dialog = None,
        }
    }

    fn open_submenu(&self, kind: SubKind, row: Rect) -> SubMenu {
        match kind {
            SubKind::WindowSize => SubMenu::window_size(row, self.window_size),
            SubKind::SoundChannel => SubMenu::sound_channel(row, self.channel_mutes()),
            SubKind::Other => SubMenu::other(row),
            SubKind::State => SubMenu::state(row),
            SubKind::RecentRoms => SubMenu::recent_roms(row, &self.recent_names()),
            SubKind::Link => SubMenu::link(row, self.link.is_active(), self.link.is_listening()),
            SubKind::Mcp => SubMenu::mcp(row, self.mcp.is_active()),
            SubKind::Plugins => SubMenu::plugins(row, &self.plugin_rows()),
        }
    }

    /// The loaded plugins as `(name, enabled)` rows for the Plugins submenu (live
    /// status; the source of truth is the running [`PluginHost`]).
    fn plugin_rows(&self) -> Vec<(String, bool)> {
        self.plugins
            .infos()
            .into_iter()
            .map(|i| (i.name, i.enabled))
            .collect()
    }

    /// Rebuild the plugin host from `dir` (Options → Plugins → "..." changed the
    /// directory): load the new directory (empty = an empty host), then mirror
    /// the discovered set — including higher-tier subsystem plugins — into the
    /// Options tab. The plugins dir is the unified subsystem source, so it also
    /// feeds the SGB coprocessor seam: with `spc700.wasm` + `w65c816.wasm` present
    /// there, enabling the SGB-coprocessor backend loads them from this dir. A bad
    /// dir logs and leaves an empty host so a typo can't wedge the dialog.
    fn rebuild_plugins(&mut self, dir: &str) {
        self.plugins = if dir.is_empty() {
            slopgb_plugin_host::PluginHost::new()
        } else {
            slopgb_plugin_host::PluginHost::load_dir(std::path::Path::new(dir)).unwrap_or_else(
                |e| {
                    eprintln!("slopgb: cannot load plugins dir '{dir}': {e}");
                    slopgb_plugin_host::PluginHost::new()
                },
            )
        };
        // The SGB coprocessor is a plugin: point the session at the same dir so
        // spc700 + w65c816 auto-load (on SGB) from the plugins dir the UI just set.
        self.session
            .set_plugins_dir((!dir.is_empty()).then(|| std::path::PathBuf::from(dir)));
        self.sync_plugin_entries();
        for line in self.plugins.take_log() {
            eprintln!("{line}");
        }
    }

    /// Re-scan the plugins directory (Plugins submenu → "Reload plugins"), then
    /// refresh the Options-tab entry list from the live host and drain its log.
    fn reload_plugins(&mut self) {
        self.plugins.reload();
        self.sync_plugin_entries();
        for line in self.plugins.take_log() {
            eprintln!("{line}");
        }
    }

    /// Mirror the live host's plugin list (name + capability label + enabled)
    /// into `settings.plugins.entries`, so the Options → Plugins tab renders the
    /// current set. Called before opening Options and after a reload.
    pub(crate) fn sync_plugin_entries(&mut self) {
        self.settings.plugins.entries = self.plugins.infos().into_iter().map(Into::into).collect();
    }

    /// The live mute state of sound channels 1-4 (the APU is the single source
    /// of truth), for the "Sound channel" submenu check-marks.
    fn channel_mutes(&self) -> [bool; 4] {
        [1, 2, 3, 4].map(|ch| self.session.gb.channel_muted(ch))
    }

    /// Apply a submenu activation: dispatch on the variant to the window-size
    /// resize, a per-channel mute toggle, or an "Other" action (open the VRAM
    /// viewer / show an info box).
    fn apply_sub_choice(&mut self, choice: SubChoice, event_loop: &ActiveEventLoop) {
        match choice {
            SubChoice::WindowSize(c) => self.apply_window_size(c),
            SubChoice::SoundChannel(ch) => {
                let now = self.session.gb.channel_muted(ch);
                self.session.gb.set_channel_mute(ch, !now);
            }
            SubChoice::OpenVram => {
                self.run_action(Action::ToggleTool(ui::ToolWindow::Vram), event_loop);
            }
            SubChoice::CartInfo => self.info_box = Some(self.cart_info_box()),
            SubChoice::SystemInfo => self.info_box = Some(self.system_info_box()),
            SubChoice::About => self.info_box = Some(about_box()),
            // State → Quick Save / Quick Load (MN6): an in-memory whole-machine
            // snapshot. A load resyncs pacing + repaints (the LCD jumped).
            SubChoice::QuickSave => self.session.quick_save(),
            SubChoice::QuickLoad => {
                if self.session.quick_load() {
                    self.resync_pacing();
                    self.update_title();
                    self.request_game_redraw();
                }
            }
            // State → Load state... (on-disk): open the shared path modal.
            SubChoice::LoadState => {
                self.open_path_prompt("Load state (path)", crate::PathPurpose::LoadState);
            }
            // Import/Export settings in bgb's ini format (interop; the native
            // store stays the default). Route through the shared path modal.
            SubChoice::ImportBgb => {
                self.open_path_prompt(
                    "Import bgb.ini (path)",
                    crate::PathPurpose::SettingsImportBgb,
                );
            }
            SubChoice::ExportBgb => {
                self.open_path_prompt(
                    "Export bgb.ini (path)",
                    crate::PathPurpose::SettingsExportBgb,
                );
            }
            // Recent ROMs → reload that entry (MN4); clone the path out first so
            // the load can borrow `self` mutably.
            SubChoice::LoadRecent(i) => {
                if let Some(p) = self.recent.get(i).cloned() {
                    self.load_dropped(&p);
                }
            }
            // Link submenu: bind/dial/tear-down the serial link transport. Each
            // refreshes the title so its link-status suffix tracks immediately
            // (the connecting→linked transition is async, refreshed by the FPS
            // tick; these synchronous transitions are reflected at once).
            SubChoice::LinkListen => {
                match self.link.listen() {
                    Ok(()) => println!(
                        "slopgb: link listening on port {}",
                        self.link.port().unwrap_or(crate::link::DEFAULT_PORT)
                    ),
                    Err(e) => eprintln!("slopgb: link listen failed: {e}"),
                }
                self.update_title();
            }
            SubChoice::LinkConnect => {
                self.open_path_prompt("Connect to (host:port)", crate::PathPurpose::LinkConnect);
            }
            // Disconnect and Cancel listen both tear the socket down + detach
            // the core peer (bgb shows them as distinct rows; the effect is one).
            SubChoice::LinkDisconnect | SubChoice::LinkCancelListen => {
                self.link.disconnect(&mut self.session.gb);
                self.update_title();
            }
            // MCP submenu: start (via the shared port modal) / stop the server.
            SubChoice::McpStart => {
                self.open_path_prompt("MCP server port", crate::PathPurpose::McpStart);
            }
            SubChoice::McpStop => {
                self.mcp.stop();
                self.update_title();
            }
            // Plugins submenu → re-scan the plugins directory.
            SubChoice::ReloadPlugins => self.reload_plugins(),
        }
    }

    /// Cartridge-header facts for the Other → "Cart info" box, parsed from the
    /// loaded ROM image (the frontend already holds it for reset).
    fn cart_info_box(&self) -> InfoBox {
        InfoBox::new(
            "Cart info",
            crate::session::cart_info_lines(&self.session.rom_bytes),
        )
    }

    /// Emulated-model facts for the Other → "System info" box.
    fn system_info_box(&self) -> InfoBox {
        InfoBox::new(
            "System info",
            vec![
                format!("model: {:?}", self.session.model),
                format!("clock: {} Hz", CLOCK_HZ),
                format!("double speed: {}", self.session.gb.double_speed()),
                format!("config: {}", crate::settings_file::config_file_display()),
            ],
        )
    }

    /// Apply a "Window size" submenu choice: an integer scale resizes the window
    /// (and leaves fullscreen), a fullscreen mode goes borderless. `window_size`
    /// records the active choice for the submenu check-mark + the stretched blit.
    /// Push the current `self.settings` to the live machine/frontend after an
    /// Options OK/Apply: volume, DMG palette, emulated system, and stretch.
    /// Pacing (fast-forward / framerate) + show-framerate + the debugger display
    /// flags are read directly from `self.settings`, so they need no push here.
    pub(crate) fn apply_settings(&mut self) {
        self.apply_settings_no_persist();
        // Persist the committed settings + recent ROMs (bgb.ini), preserving
        // unknown bgb keys.
        crate::settings_file::save(&self.settings, &self.recent);
    }

    /// The state-mutating half of [`Self::apply_settings`] with the on-disk
    /// persistence removed: pushes the committed settings to the live machine /
    /// frontend and reconciles stretch <-> window size, but writes no file. The
    /// public entry point wraps this and then saves; tests drive the
    /// reconciliation through this seam without touching the real config.
    fn apply_settings_no_persist(&mut self) {
        let s = self.settings.clone();
        if let Some(pipe) = &mut self.audio {
            pipe.set_volume(s.volume, s.mono);
        }
        // Sound → device/samplerate/latency/8-bit/quality: rebuild the output
        // stream, but only when one of those actually changed (Apply otherwise
        // leaves the running stream untouched — no glitch).
        if self.audio_prefs() != self.audio_prefs_applied || s.audio_hq != self.audio_hq_applied {
            self.reopen_audio();
        }
        // Joypad → "Audio": start/stop the audio recorder to match the setting.
        self.sync_audio_recording();
        // Joypad → "Video": start/stop the AVI video recorder to match the setting.
        self.sync_video_recording();
        // Joypad → "Audio channels": start/stop the per-channel WAV recorder.
        self.sync_channel_recording();
        // System → "Save RTC in SAV file (VBA compatible)" + "Save BGB legacy RTC
        // files": choose the .sav RTC layout + the sidecar for the next write.
        self.session.set_rtc_vba_export(s.rtc_vba_sav);
        self.session.set_rtc_bgb_legacy(s.rtc_bgb_legacy);
        // Joypad → game controller: re-derive the live map (Defaults/import may
        // have changed it; the wizard/clear paths set both in lock-step already).
        self.gamepad_bindings = crate::gamepad::GamepadBindings::from_config(&s.gamepad_map);
        // Plugins → dir changed ("..."): rebuild the host from the new directory
        // (rescans) and refresh the tab's entry list. No-op if unchanged.
        let cur_dir = self
            .plugins
            .dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if s.plugins.dir != cur_dir {
            self.rebuild_plugins(&s.plugins.dir);
        }
        // Power-on RAM init (bgb's UninitedWRAM): store it for the next reset/
        // reload — power-on state, so it doesn't scramble the running machine.
        self.session.set_ram_init(crate::cli::effective_ram_init(
            self.opts.ram_init,
            s.uninited_wram,
        ));
        // Switch the emulated system FIRST: `set_model` rebuilds the machine from
        // the ROM, which resets the PPU palette to the power-on default — so the
        // DMG palette must be (re)applied to the (possibly fresh) machine after.
        // With "automatic reset on system change" off, the choice is deferred and
        // applied by the next Reset (see `Action::Reset`).
        if s.auto_reset_on_system_change && self.session.set_model(s.model) {
            self.resync_pacing();
            self.request_game_redraw();
        }
        // Push the palette + rebuild the no-ROM blank frame, so changing the GB
        // Colors scheme recolours even the blank screen (with no ROM loaded the
        // LCD shows `blank_frame`, not the machine's front buffer).
        self.apply_palette();
        // Stretch maps onto the Window-size fullscreen-stretched mode.
        if s.stretch && self.window_size != WindowSizeChoice::FullscreenStretched {
            self.apply_window_size(WindowSizeChoice::FullscreenStretched);
        } else if !s.stretch && self.window_size == WindowSizeChoice::FullscreenStretched {
            self.apply_window_size(WindowSizeChoice::Scale(self.last_scale));
        }
        // Debug-tab disasm display flags → the debugger view.
        self.push_disasm_fmt();
        // Debug-tab "Registers can be edited" → the debugger's edit menu.
        self.tools
            .set_registers_editable(self.settings.registers_editable);
        // Debug-tab "8-bit tile hex" → the VRAM viewer.
        self.tools.set_tile_hex_8bit(self.settings.tile_hex_8bit);
        // Defer opening/closing the standalone memory window to `about_to_wait`,
        // where the event loop is available (only on a real change, so it doesn't
        // fight a manual close).
        if self.tools.is_open(crate::ui::ToolWindow::MemoryViewer) != s.memory_window {
            self.pending_mem_window = Some(s.memory_window);
        }
        // Exceptions-tab break conditions → the core exception-break mask.
        self.apply_exceptions();
        // Plugins-tab enable checkboxes → the live host: a disabled plugin's
        // on_frame stops next pump. A no-op with no plugins (golden path stays
        // byte-identical).
        let states: Vec<(String, bool)> = self
            .settings
            .plugins
            .entries
            .iter()
            .map(|e| (e.name.clone(), e.enabled))
            .collect();
        for (name, enabled) in states {
            self.plugins.set_enabled(&name, enabled);
        }
        self.update_title();
    }

    /// Push the Debug-tab disasm display options (syntax / hex case / clocks) to
    /// any open debugger window. Shared by `apply_settings` and the tool-window
    /// toggle so a newly-opened debugger matches the current settings.
    pub(crate) fn push_disasm_fmt(&mut self) {
        self.tools.set_disasm_fmt(windows::debugger::DisasmFmt {
            lowercase_hex: self.settings.lowercase_hex,
            show_clocks: self.settings.show_clocks,
            rgbds: self.settings.rgbds_disasm,
            lowercase_disasm: self.settings.lowercase_disasm,
        });
    }

    /// Push the Options → Exceptions break mask to the live machine. Called
    /// after every machine (re)build (startup, ROM load — `GameBoy::new` clears
    /// the mask) and on Options OK/Apply. The mask only *fires* while the
    /// debugger window is open (see `dbg_armed`); a `0` mask is golden-safe.
    pub(crate) fn apply_exceptions(&mut self) {
        self.session
            .gb
            .set_exceptions(self.settings.exception_mask());
    }

    fn apply_window_size(&mut self, choice: WindowSizeChoice) {
        self.window_size = choice;
        // Remember the last windowed scale so leaving fullscreen-stretched (via
        // Options) restores it rather than the launch scale.
        if let WindowSizeChoice::Scale(n) = choice {
            self.last_scale = n;
        }
        // Keep the Options `stretch` setting in lock-step with the menu-chosen
        // mode, so a later Options OK/Apply (which reconciles stretch ↔ window
        // size) can't silently revert a fullscreen-stretched choice made here.
        self.settings.stretch = choice == WindowSizeChoice::FullscreenStretched;
        let Some(window) = &self.window else {
            return;
        };
        match choice {
            WindowSizeChoice::Scale(n) => {
                window.set_fullscreen(None);
                let _ = window.request_inner_size(LogicalSize::new(
                    f64::from(SCREEN_W as u32 * n),
                    f64::from(SCREEN_H as u32 * n),
                ));
            }
            WindowSizeChoice::Fullscreen | WindowSizeChoice::FullscreenStretched => {
                window.set_fullscreen(Some(Fullscreen::Borderless(None)));
            }
        }
        self.request_game_redraw();
    }
}

/// The Other → "About..." info box.
fn about_box() -> InfoBox {
    InfoBox::new(
        "About slopgb",
        vec![
            format!("slopgb {}", env!("CARGO_PKG_VERSION")),
            "cycle-accurate GB/GBC emulator".into(),
            "bgb-style debugger UI clone".into(),
        ],
    )
}

#[cfg(test)]
#[path = "app_menu_tests.rs"]
mod tests;
