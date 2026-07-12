//! `App`'s winit [`ApplicationHandler`] impl: the event-loop trait driving the
//! app — window/surface/audio creation on `resumed`, the per-window event
//! router in `window_event` (modifiers, the right-click popup window, the debug
//! tool windows, then the game window), the pacing wake in `about_to_wait`, and
//! the save flush on `exiting`. The discrete-action dispatch is in
//! [`crate::app_run`], the game-window menu handling in [`crate::app_menu`], and
//! the emulation pacing drivers in [`crate::app_pacing`].

use std::env;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slopgb_core::{SCREEN_H, SCREEN_W};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::input::Focus;
use crate::pacing::audio_pacing;
use crate::video::Video;
use crate::{App, link, should_idle, ui, window_title};

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let scale = self.opts.scale;
        let attrs = Window::default_attributes()
            .with_title(window_title(self.rom_loaded, &self.session.title, ""))
            .with_inner_size(LogicalSize::new(
                f64::from(SCREEN_W as u32 * scale),
                f64::from(SCREEN_H as u32 * scale),
            ))
            .with_min_inner_size(LogicalSize::new(SCREEN_W as f64, SCREEN_H as f64));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("error: cannot create window: {e}");
                event_loop.exit();
                return;
            }
        };
        match Video::new(window.clone()) {
            Ok(v) => self.video = Some(v),
            Err(e) => {
                eprintln!("error: cannot create render surface: {e}");
                event_loop.exit();
                return;
            }
        }
        if !self.opts.mute {
            self.try_open_audio();
        }
        self.window = Some(window);
        // Optionally open debug tool windows at startup (comma-separated
        // `debugger,vram,iomap` in `SLOPGB_OPEN_TOOLS`) — handy for screenshot
        // verification and for users who always want them up.
        if let Ok(list) = env::var("SLOPGB_OPEN_TOOLS") {
            for kind in list.split(',').filter_map(|s| match s.trim() {
                "debugger" => Some(ui::ToolWindow::Debugger),
                "vram" => Some(ui::ToolWindow::Vram),
                "iomap" => Some(ui::ToolWindow::IoMap),
                "memory" => Some(ui::ToolWindow::MemoryViewer),
                _ => None,
            }) {
                self.tools.toggle(event_loop, kind);
            }
        }
        // Optionally start the serial link at startup — `SLOPGB_LINK_LISTEN=1`
        // listens, `SLOPGB_LINK_CONNECT=host:port` dials. The Connect path
        // otherwise needs the keyboard modal, so this enables scripted /
        // screenshot-verified two-instance linking (mirrors SLOPGB_OPEN_TOOLS).
        if env::var_os("SLOPGB_LINK_LISTEN").is_some() {
            if let Err(e) = self.link.listen() {
                eprintln!("slopgb: link listen failed: {e}");
            }
        }
        if let Ok(addr) = env::var("SLOPGB_LINK_CONNECT") {
            let (host, port) = link::parse_host_port(&addr);
            if let Err(e) = self.link.connect(host, port) {
                eprintln!("slopgb: link connect failed: {e}");
            }
        }
        // Optionally host the MCP debug server — `--mcp-port` or `SLOPGB_MCP_PORT`.
        // Guarded so a resume/suspend cycle doesn't restart it.
        if !self.mcp.is_active() {
            if let Some(port) = self.opts.mcp_port.or_else(|| {
                env::var("SLOPGB_MCP_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
            }) {
                match self.mcp.start(port) {
                    Ok(()) => eprintln!(
                        "slopgb: MCP server on http://127.0.0.1:{}/",
                        self.mcp.port().unwrap_or(port)
                    ),
                    Err(e) => eprintln!("slopgb: MCP server failed on port {port}: {e}"),
                }
            }
        }
        self.resync_pacing();
        self.update_title();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Track keyboard modifiers for the focus-dependent key map (Ctrl+G),
        // regardless of which window has focus.
        if let WindowEvent::ModifiersChanged(m) = &event {
            self.modifiers = m.state();
            return;
        }
        // The right-click menu popup (its own borderless window) owns its events.
        if self
            .menu_popup
            .as_ref()
            .is_some_and(|p| p.window_id() == window_id)
        {
            self.on_popup_event(event, event_loop);
            return;
        }
        // A debug tool window owns its events; the game window path below is
        // untouched. Its close button closes just that window (not the app).
        if self.tools.owns(window_id) {
            match event {
                WindowEvent::CloseRequested => {
                    // A user-closed memory window clears the Options setting so a
                    // later Apply doesn't reopen it.
                    if self.tools.kind_of(window_id) == Some(ui::ToolWindow::MemoryViewer) {
                        self.settings.memory_window = false;
                    }
                    self.tools.close(window_id);
                }
                WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {
                    self.tools
                        .redraw(window_id, &self.session.gb, self.dbg.breakpoints());
                }
                // Mouse drives the tool windows' tabs/checkboxes/details and the
                // debugger's context menu (left selects/acts, right opens it).
                WindowEvent::CursorMoved { position, .. } => {
                    self.tools
                        .on_cursor_moved(window_id, position.x, position.y);
                }
                WindowEvent::CursorLeft { .. } => self.tools.on_cursor_left(window_id),
                // Mouse wheel scrolls the debugger memory pane (bgb).
                WindowEvent::MouseWheel { delta, .. } => {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => (p.y / 16.0) as f32,
                    };
                    self.tools.on_wheel(window_id, lines, &self.session.gb);
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button,
                    ..
                } if matches!(button, MouseButton::Left | MouseButton::Right) => {
                    let outcome = if button == MouseButton::Left {
                        self.tools.on_mouse_left(window_id, &self.session.gb)
                    } else {
                        self.tools.on_mouse_right(window_id, &self.session.gb)
                    };
                    if let Some(outcome) = outcome {
                        self.apply_menu_outcome(outcome, event_loop);
                    }
                }
                // Left-release ends any in-progress scrollbar drag.
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                } => self.tools.on_mouse_up(),
                // Hotkeys route by focused window kind: the debugger window gets
                // bgb's debugger keys, the viewers keep the game keymap.
                WindowEvent::KeyboardInput { event, .. } => {
                    // The standalone memory window owns its arrow/Page nav keys
                    // (continuous scroll on hold); they must not reach the game
                    // joypad or the key-repeat guard.
                    let mem_win =
                        self.tools.kind_of(window_id) == Some(ui::ToolWindow::MemoryViewer);
                    if mem_win && event.state.is_pressed() {
                        // An open Go to… dialog captures every key (so typing an
                        // address can't scroll the pane or trigger a hotkey).
                        if self.tools.mem_dialog_active(window_id) {
                            if let Some(dk) = crate::dialog_key_from(&event) {
                                self.tools.feed_mem_dialog(window_id, dk);
                            }
                            return;
                        }
                        // Ctrl+G opens the Go to… dialog (bgb parity; the
                        // integrated pane already has this via input::map).
                        if self.modifiers.control_key()
                            && event.physical_key == PhysicalKey::Code(KeyCode::KeyG)
                        {
                            self.tools.open_mem_goto(window_id);
                            return;
                        }
                        // Esc cancels a pending in-place edit (before the global
                        // esc-shows-debugger toggle); if not editing, fall through.
                        if event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                            && self.tools.mem_cancel_edit(window_id)
                        {
                            return;
                        }
                        // A hex digit types over the cursor byte in place (never
                        // with Ctrl, so Ctrl+<letter> hotkeys are unaffected).
                        if !self.modifiers.control_key() {
                            if let Some(d) = event
                                .text
                                .as_ref()
                                .and_then(|t| t.chars().next())
                                .and_then(|ch| ch.to_digit(16))
                            {
                                if let Some((sel, addr, val)) =
                                    self.tools.mem_edit_digit(window_id, d as u8)
                                {
                                    crate::windows::banked_write(
                                        &mut self.session.gb,
                                        sel,
                                        addr,
                                        val,
                                    );
                                }
                                return;
                            }
                        }
                        // Otherwise the window owns its arrow/Page nav keys.
                        if let PhysicalKey::Code(code) = event.physical_key {
                            if self.tools.mem_window_key(window_id, code, &self.session.gb) {
                                return;
                            }
                        }
                    }
                    // The debugger window gets bgb's debugger keys; the other tool
                    // windows get the game hotkeys but NOT the joypad (Focus::Viewer).
                    let focus = if self.tools.kind_of(window_id) == Some(ui::ToolWindow::Debugger) {
                        Focus::Debugger
                    } else {
                        Focus::Viewer
                    };
                    self.handle_key(event_loop, &event, focus);
                }
                // No key-release events arrive after a tool window loses focus, so
                // forget held keys — else a later press reads as a stuck repeat and
                // is dropped by the key-repeat guard.
                WindowEvent::Focused(false) | WindowEvent::Occluded(true) => {
                    self.held_keys.clear();
                }
                _ => {}
            }
            return;
        }
        // Anything else must be the game window. A just-closed popup/tool window
        // can still have queued events (e.g. a late `Focused(false)` or
        // `CursorMoved` for the destroyed id); without this guard they'd fall
        // through and be reinterpreted as game-window focus/mouse/close events
        // (spuriously releasing input or pausing). Ignore stale ids.
        if self.window.as_ref().map(|w| w.id()) != Some(window_id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => self.load_dropped(&path),
            // Focus loss and occlusion both mean held keys won't get release
            // events, so drop all input before any button can stick. With
            // Options → Misc → "Pause if losing focus" set, also pause.
            WindowEvent::Focused(false) | WindowEvent::Occluded(true) => {
                self.release_all_input();
                if self.settings.pause_on_focus_loss && !self.paused {
                    self.paused = true;
                    self.paused_by_focus = true;
                    self.session.flush_save();
                    self.update_title();
                }
            }
            // Refocus auto-resumes, but only a pause we induced — a manual pause
            // (P) stays put (bgb's "Pause if losing focus" resume behaviour).
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                if self.paused_by_focus && self.paused {
                    self.paused = false;
                    self.resync_pacing();
                    self.update_title();
                }
                self.paused_by_focus = false;
            }
            // Track the pointer so a right-click can open the menu where it sits.
            // (The menu's own hover highlighting is handled by its own window.)
            WindowEvent::CursorMoved { position, .. } => {
                self.game_cursor = (position.x as i32, position.y as i32);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } if matches!(button, MouseButton::Left | MouseButton::Right) => {
                self.on_game_click(button, event_loop);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_key(event_loop, &event, Focus::Game)
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            return; // not resumed yet
        }
        // Serve any queued MCP tool calls first — before the idle guard, so an
        // agent can still inspect a paused / breakpoint-halted machine (that is
        // exactly when it wants to). A no-op when no server is running.
        self.mcp
            .pump(&self.session.gb, &mut self.dbg, &self.symbols);
        // Reconcile a pending Options "memory viewer in own window" change now
        // that the event loop is available (open/close the standalone window).
        if let Some(want) = self.pending_mem_window.take() {
            if self.tools.is_open(ui::ToolWindow::MemoryViewer) != want {
                self.tools.toggle(event_loop, ui::ToolWindow::MemoryViewer);
                self.tools.set_symbols(self.symbols.clone());
            }
        }
        // A debugger break freezes emulation exactly like pause: the LCD holds
        // its last frame and zero frames are emulated until F9/step. With no ROM
        // loaded the blank machine is likewise frozen (bgb's no-ROM screen).
        if should_idle(self.paused, self.dbg.is_broken(), self.rom_loaded) {
            // Frozen (paused / no ROM / debugger-broken): drop queued presses
            // (a press on a frozen machine shouldn't register, and applying it
            // on resume would use a stale offset) but still honor releases, so a
            // button released while paused doesn't stick held on resume.
            self.flush_idle_input();
            // With the MCP server up, poll instead of parking indefinitely so
            // queued tool calls are served within a few ms even while frozen;
            // otherwise wait for the next real event (no wasted wakeups).
            if self.mcp.is_active() {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(8),
                ));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            return;
        }
        // Apply deferred joypad input at its sub-frame offset before emulating,
        // so the joypad interrupt lands on a realistic, varied LCD line.
        self.apply_pending_input();
        let (frames, hit_bp) = if self.turbo {
            self.run_turbo()
        } else if audio_pacing(self.audio.is_some(), self.muted) {
            self.run_audio_paced()
        } else {
            self.run_timer_paced()
        };
        // A free-running breakpoint hit freezes the debugger; the top guard then
        // idles to `Wait` on the next wake (bgb's halt-at-breakpoint). Pop the
        // debugger window to the front so the halt is visible even if the game
        // window had focus (bgb does this).
        if hit_bp {
            self.dbg.set_broken(true);
            self.tools.focus_debugger();
            self.update_title();
        }
        // A dead stream would otherwise leave the queue pinned high forever.
        self.check_audio_health();
        if frames > 0 {
            self.session.autosave();
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            // Keep the open debug windows tracking live machine state.
            self.tools.request_redraw_all();
        }
        self.update_fps(frames);
        // Both audio and timer pacing now march the `next_frame` grid (audio
        // slews the interval; see run_audio_paced / run_timer_paced), so in the
        // Steady band one wake schedules one frame → one present. The audio
        // transient bands still burst (CatchUp) or skip (Hold), so `frames` may
        // be >1 or 0 on a given wake — the redraw below is gated on frames > 0.
        // Turbo free-runs.
        let flow = if self.turbo {
            ControlFlow::Poll
        } else {
            ControlFlow::WaitUntil(self.next_frame)
        };
        event_loop.set_control_flow(flow);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.session.flush_save();
    }
}
