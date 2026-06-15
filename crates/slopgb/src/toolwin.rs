//! Multi-window support (B12b): owns the open bgb tool windows (Debugger / VRAM
//! viewer / I/O map) alongside the always-present game window, which `main`
//! keeps handling itself. Each tool window is a winit window + its own
//! softbuffer surface; redraw renders the live machine through
//! [`windows::render`]. This is the winit plumbing around the already-tested
//! pure content; the routing in `main` checks [`owns`](ToolWindows::owns)
//! first, so the game window's path is untouched.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;

use slopgb_core::GameBoy;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::dbg::{Breakpoints, DebugAction};
use crate::ui::canvas::Rect;
use crate::ui::{Canvas, Theme, ToolWindow, WindowRegistry};
use crate::windows::{self, WinState, debugger, vram};

struct ToolView {
    window: Rc<Window>,
    // Kept alive alongside the surface (softbuffer's display connection).
    _ctx: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    kind: ToolWindow,
    /// Persistent interactive UI state (active tab, checkboxes, hovered cell).
    state: WinState,
    /// Last cursor position (physical pixels) over this window, for click
    /// hit-testing — `MouseInput` itself carries no coordinates.
    cursor: Option<(i32, i32)>,
}

impl ToolView {
    /// The window's content rect in physical pixels — matches the `Canvas`
    /// bounds `render` draws into, so hit-rects line up with the cursor.
    fn area(&self) -> Rect {
        let size = self.window.inner_size();
        Rect::new(0, 0, size.width as i32, size.height as i32)
    }
}

/// The set of open tool windows.
pub struct ToolWindows {
    views: HashMap<WindowId, ToolView>,
    reg: WindowRegistry<WindowId>,
    theme: Theme,
}

fn title(kind: ToolWindow) -> &'static str {
    match kind {
        ToolWindow::Debugger => "slopgb — debugger",
        ToolWindow::Vram => "slopgb — VRAM viewer",
        ToolWindow::IoMap => "slopgb — I/O map",
    }
}

fn default_size(kind: ToolWindow) -> LogicalSize<f64> {
    let (w, h) = match kind {
        ToolWindow::Debugger => (760.0, 560.0),
        // Wide enough for the 16×24 tile grid at 2× (256×384) plus the details
        // panel ([`vram::PANEL_W`]); tall enough for the grid + tab strip.
        ToolWindow::Vram => (560.0, 470.0),
        // Four register columns + the decoded LCDC/STAT/vector/wave panels.
        ToolWindow::IoMap => (600.0, 400.0),
    };
    LogicalSize::new(w, h)
}

impl ToolWindows {
    #[must_use]
    pub fn new() -> Self {
        Self {
            views: HashMap::new(),
            reg: WindowRegistry::new(),
            theme: Theme::BGB,
        }
    }

    /// Toggle a tool window: create it if closed, else close it. Window/surface
    /// creation failures are logged and left closed rather than crashing the
    /// emulator.
    pub fn toggle(&mut self, el: &ActiveEventLoop, kind: ToolWindow) {
        if let Some(id) = self.reg.id_of(kind) {
            self.views.remove(&id);
            self.reg.forget(id);
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(title(kind))
            .with_inner_size(default_size(kind));
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("slopgb: cannot open {} window: {e}", title(kind));
                return;
            }
        };
        let ctx = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("slopgb: {} surface init failed: {e}", title(kind));
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&ctx, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("slopgb: {} surface init failed: {e}", title(kind));
                return;
            }
        };
        let id = window.id();
        window.request_redraw();
        self.views.insert(
            id,
            ToolView {
                window,
                _ctx: ctx,
                surface,
                kind,
                state: WinState::new(kind),
                cursor: None,
            },
        );
        self.reg.register(id, kind);
    }

    /// Whether `id` is one of our tool windows (so `main` can route its events).
    #[must_use]
    pub fn owns(&self, id: WindowId) -> bool {
        self.reg.kind_of(id).is_some()
    }

    /// Whether a tool window of `kind` is currently open (gates the debugger
    /// hotkeys to when its window is up).
    #[must_use]
    pub fn is_open(&self, kind: ToolWindow) -> bool {
        self.reg.id_of(kind).is_some()
    }

    /// Render the tool window `id` from the live machine. `bps` (the App-owned
    /// breakpoint set) feeds the debugger's gutter dots; other windows ignore it.
    pub fn redraw(&mut self, id: WindowId, gb: &GameBoy, bps: &Breakpoints) {
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        let size = view.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return; // minimized
        };
        if view.surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buf) = view.surface.buffer_mut() else {
            return;
        };
        {
            let mut canvas = Canvas::new(&mut buf, size.width as usize, size.height as usize);
            windows::render(view.kind, gb, &mut canvas, &self.theme, &view.state, bps);
        }
        // Force opaque alpha: softbuffer leaves the top byte 0, which a 32-bit
        // ARGB compositor reads as fully transparent (the window would show the
        // desktop through it). softbuffer itself ignores the top byte.
        for px in buf.iter_mut() {
            *px |= 0xFF00_0000;
        }
        view.window.pre_present_notify();
        let _ = buf.present();
    }

    /// Close the tool window `id` (its close button); returns whether it was
    /// one of ours.
    pub fn close(&mut self, id: WindowId) -> bool {
        if self.views.remove(&id).is_some() {
            self.reg.forget(id);
            true
        } else {
            false
        }
    }

    /// Record the cursor moving to physical `(x, y)` over tool window `id`;
    /// updates the hovered-cell details and redraws only if the hover changed.
    pub fn on_cursor_moved(&mut self, id: WindowId, x: f64, y: f64) {
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        let (px, py) = (x as i32, y as i32);
        view.cursor = Some((px, py));
        let area = view.area();
        match &mut view.state {
            WinState::Vram(s) => {
                if vram::on_hover(s, area, px, py) {
                    view.window.request_redraw();
                }
            }
            // Track the hovered row of an open context menu.
            WinState::Debugger(s) => {
                if let Some(om) = &mut s.menu {
                    if om.hover_at(px, py) {
                        view.window.request_redraw();
                    }
                }
            }
            WinState::Stateless => {}
        }
    }

    /// Handle a left-button press on tool window `id` (uses the last cursor
    /// position): switches a VRAM control, selects a debugger menu item, or sets
    /// the debugger cursor. Returns an execution [`DebugAction`] for `main` to
    /// apply (debugger only), redrawing on any change.
    pub fn on_mouse_left(&mut self, id: WindowId, gb: &GameBoy) -> Option<DebugAction> {
        let view = self.views.get_mut(&id)?;
        let (px, py) = view.cursor?;
        let area = view.area();
        match &mut view.state {
            WinState::Vram(s) => {
                if vram::on_click(s, area, px, py) {
                    view.window.request_redraw();
                }
                None
            }
            WinState::Debugger(s) => {
                let action = debugger_left_click(s, area, gb, px, py);
                view.window.request_redraw();
                action
            }
            WinState::Stateless => None,
        }
    }

    /// Handle a right-button press on tool window `id`: on the debugger, open the
    /// context menu for the clicked pane (or dismiss an open one). Returns `None`
    /// — opening a menu has no immediate machine effect.
    pub fn on_mouse_right(&mut self, id: WindowId, gb: &GameBoy) -> Option<DebugAction> {
        let view = self.views.get_mut(&id)?;
        let (px, py) = view.cursor?;
        let area = view.area();
        if let WinState::Debugger(s) = &mut view.state {
            debugger_right_click(s, area, gb, px, py);
            view.window.request_redraw();
        }
        None
    }

    /// Clear the remembered cursor when it leaves tool window `id`, so a stale
    /// position can't drive a click and the hover details clear.
    pub fn on_cursor_left(&mut self, id: WindowId) {
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        view.cursor = None;
        let area = view.area();
        if let WinState::Vram(s) = &mut view.state {
            if vram::on_hover(s, area, -1, -1) {
                view.window.request_redraw();
            }
        }
    }

    /// Ask every open tool window to redraw (after emulation advances, so they
    /// track live state).
    pub fn request_redraw_all(&self) {
        for v in self.views.values() {
            v.window.request_redraw();
        }
    }
}

/// Glue the live machine onto the pure [`debugger::on_left_click`] (the register
/// snapshot + `debug_read` closure the resolver needs).
fn debugger_left_click(
    s: &mut debugger::DebuggerState,
    area: Rect,
    gb: &GameBoy,
    px: i32,
    py: i32,
) -> Option<DebugAction> {
    let r = gb.cpu_regs();
    debugger::on_left_click(|a| gb.debug_read(a), area, s, r.pc, r.sp, px, py)
}

/// Glue for [`debugger::on_right_click`] (opens / dismisses the context menu).
fn debugger_right_click(
    s: &mut debugger::DebuggerState,
    area: Rect,
    gb: &GameBoy,
    px: i32,
    py: i32,
) {
    let r = gb.cpu_regs();
    debugger::on_right_click(|a| gb.debug_read(a), area, s, r.pc, r.sp, px, py);
}
