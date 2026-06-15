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

use crate::ui::{Canvas, Theme, ToolWindow, WindowRegistry};
use crate::windows;

struct ToolView {
    window: Rc<Window>,
    // Kept alive alongside the surface (softbuffer's display connection).
    _ctx: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    kind: ToolWindow,
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
        ToolWindow::Vram => (380.0, 420.0),
        ToolWindow::IoMap => (560.0, 360.0),
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
            },
        );
        self.reg.register(id, kind);
    }

    /// Whether `id` is one of our tool windows (so `main` can route its events).
    #[must_use]
    pub fn owns(&self, id: WindowId) -> bool {
        self.views.contains_key(&id)
    }

    /// Render the tool window `id` from the live machine.
    pub fn redraw(&mut self, id: WindowId, gb: &GameBoy) {
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
            windows::render(view.kind, gb, &mut canvas, &self.theme);
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

    /// Ask every open tool window to redraw (after emulation advances, so they
    /// track live state).
    pub fn request_redraw_all(&self) {
        for v in self.views.values() {
            v.window.request_redraw();
        }
    }
}
