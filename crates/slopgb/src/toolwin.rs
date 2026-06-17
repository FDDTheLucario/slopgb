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

use crate::dbg::Breakpoints;
use crate::ui::canvas::Rect;
use crate::ui::dialog::DialogKey;
use crate::ui::{Canvas, Theme, ToolWindow, WindowRegistry};
use crate::windows::{self, WinState, debugger, vram};
use debugger::{GotoTarget, MenuOutcome};

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
    /// the debugger cursor. Returns a [`MenuOutcome`] for `main` to apply
    /// (debugger only), redrawing on any change.
    pub fn on_mouse_left(&mut self, id: WindowId, gb: &GameBoy) -> Option<MenuOutcome> {
        let view = self.views.get_mut(&id)?;
        let (px, py) = view.cursor?;
        let area = view.area();
        match &mut view.state {
            WinState::Vram(s) => {
                if vram::on_click(s, area, px, py, gb.model().is_cgb()) {
                    view.window.request_redraw();
                }
                None
            }
            WinState::Debugger(s) => {
                // An open modal eats the click (OK/Cancel may yield a register
                // write); else normal routing.
                let (consumed, outcome) = debugger::dialog_click(s, area, px, py);
                let action = if consumed {
                    outcome
                } else {
                    debugger_left_click(s, area, gb, px, py)
                };
                view.window.request_redraw();
                action
            }
            WinState::Stateless => None,
        }
    }

    /// Handle a right-button press on tool window `id`: on the debugger, open the
    /// context menu for the clicked pane (or dismiss an open one). Returns `None`
    /// — opening a menu has no immediate machine effect.
    pub fn on_mouse_right(&mut self, id: WindowId, gb: &GameBoy) -> Option<MenuOutcome> {
        let view = self.views.get_mut(&id)?;
        let (px, py) = view.cursor?;
        let area = view.area();
        if let WinState::Debugger(s) = &mut view.state {
            debugger_right_click(s, area, gb, px, py);
            view.window.request_redraw();
        }
        None
    }

    /// The tool a window shows, for the focus-dependent key router in `main`.
    #[must_use]
    pub fn kind_of(&self, id: WindowId) -> Option<ToolWindow> {
        self.reg.kind_of(id)
    }

    /// The (single) open debugger window's view, by kind.
    fn debugger_view_mut(&mut self) -> Option<&mut ToolView> {
        let id = self.reg.id_of(ToolWindow::Debugger)?;
        self.views.get_mut(&id)
    }

    fn debugger_view(&self) -> Option<&ToolView> {
        let id = self.reg.id_of(ToolWindow::Debugger)?;
        self.views.get(&id)
    }

    /// The debugger's selected cursor address (keyboard breakpoint / run-to-cursor).
    #[must_use]
    pub fn debugger_cursor(&self) -> Option<u16> {
        match &self.debugger_view()?.state {
            WinState::Debugger(s) => s.cursor,
            _ => None,
        }
    }

    /// Whether the debugger window has an open modal, so `main` routes keys to it
    /// instead of the focus keymap.
    #[must_use]
    pub fn debugger_modal_active(&self) -> bool {
        if let Some(WinState::Debugger(s)) = self.debugger_view().map(|v| &v.state) {
            s.dialog.is_some()
        } else {
            false
        }
    }

    /// Feed one key to the debugger's open modal (Go to… / edit register);
    /// redraw if it consumed the key. Returns the modal's outcome (an `edit
    /// register` accept yields a register write) for `main` to apply.
    pub fn feed_debugger_dialog(&mut self, key: DialogKey) -> Option<MenuOutcome> {
        let view = self.debugger_view_mut()?;
        if let WinState::Debugger(s) = &mut view.state {
            let (consumed, outcome) = debugger::feed_dialog(s, key);
            if consumed {
                view.window.request_redraw();
            }
            return outcome;
        }
        None
    }

    /// Open a prebuilt context menu on the debugger window — the bp/wp manager
    /// list (RM15), which `main` builds from the App-owned breakpoint/watchpoint
    /// state and hands here. Redraws.
    pub fn set_debugger_menu(&mut self, menu: debugger::OpenMenu) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.menu = Some(menu);
            view.window.request_redraw();
        }
    }

    /// Re-center the disasm on PC (Search → "go to PC", Ctrl+A): drop the
    /// stay-on-bank pin so the pane follows PC again. Redraws.
    pub fn debugger_goto_pc(&mut self) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.pinned = false;
            view.window.request_redraw();
        }
    }

    /// Open the debugger's `Go to…` modal on the disasm pane (Ctrl+G).
    pub fn open_debugger_goto(&mut self) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            debugger::open_goto(s, GotoTarget::Disasm);
            view.window.request_redraw();
        }
    }

    /// Open the debugger's `Search string` modal (MB3, Ctrl+F).
    pub fn open_debugger_search(&mut self) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            debugger::open_search(s);
            view.window.request_redraw();
        }
    }

    /// Run the stored Search-string query over the machine (MB3): from just after
    /// the last hit, or the current disasm base for a fresh search. Pins the
    /// disasm to a match and remembers it for "Continue search"; a miss leaves
    /// the view unchanged.
    pub fn debugger_search(&mut self, gb: &GameBoy) {
        let pc = gb.cpu_regs().pc;
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            if s.search_query.is_empty() {
                return;
            }
            let from = s
                .search_hit
                .map_or_else(|| s.disasm_start(pc), |h| h.wrapping_add(1));
            if let Some(addr) = debugger::find_match(|a| gb.debug_read(a), from, &s.search_query) {
                s.disasm_base = addr;
                s.pinned = true;
                s.search_hit = Some(addr);
                view.window.request_redraw();
            }
        }
    }

    /// Set numbered bookmark `slot` (0-9) to `addr` (bgb Ctrl+Shift+digit).
    pub fn set_debugger_bookmark(&mut self, slot: u8, addr: u16) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            if let Some(b) = s.bookmarks.get_mut(slot as usize) {
                *b = Some(addr);
                view.window.request_redraw();
            }
        }
    }

    /// Jump the disasm to numbered bookmark `slot` if set (bgb Ctrl+digit).
    pub fn goto_debugger_bookmark(&mut self, slot: u8) {
        let addr = match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => s.bookmarks.get(slot as usize).copied().flatten(),
            _ => None,
        };
        if let Some(addr) = addr {
            self.debugger_goto_addr(addr);
        }
    }

    /// Pin the disasm to `addr` (go-to-bookmark + the next/previous-mark walk).
    pub fn debugger_goto_addr(&mut self, addr: u16) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.disasm_base = addr;
            s.pinned = true;
            view.window.request_redraw();
        }
    }

    /// The set bookmark addresses (input to the next/previous-mark walk).
    #[must_use]
    pub fn debugger_bookmarks(&self) -> Vec<u16> {
        match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => s.bookmarks.iter().flatten().copied().collect(),
            _ => Vec::new(),
        }
    }

    /// Where the next/previous-mark walk starts: the current disasm view base.
    #[must_use]
    pub fn debugger_disasm_ref(&self, pc: u16) -> u16 {
        match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => s.disasm_start(pc),
            _ => pc,
        }
    }

    /// Disassemble a fixed region from the current disasm base for the File →
    /// "save asm..." export (MB2): the formatted lines joined by newlines,
    /// honouring the pane's code/data hints.
    #[must_use]
    pub fn debugger_disasm_dump(&self, gb: &GameBoy) -> String {
        const COUNT: usize = 4096;
        let pc = gb.cpu_regs().pc;
        let (start, hints, fmt) = match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => (s.disasm_start(pc), s.data_hints.clone(), s.disasm_fmt),
            _ => (
                pc,
                std::collections::BTreeSet::new(),
                debugger::DisasmFmt::default(),
            ),
        };
        let rows = debugger::disasm_rows(|a| gb.debug_read(a), start, COUNT, &hints, fmt);
        rows.iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clipboard text for the debugger "Copy code" / "Copy data" items (RM10):
    /// 16 disassembled rows from `addr` (`code`) or 16 hex bytes from `addr`
    /// (data), honouring the pane's code/data hints + hex-case format.
    #[must_use]
    pub fn debugger_copy_text(&self, gb: &GameBoy, addr: u16, code: bool) -> String {
        let (hints, fmt) = match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => (s.data_hints.clone(), s.disasm_fmt),
            _ => (
                std::collections::BTreeSet::new(),
                debugger::DisasmFmt::default(),
            ),
        };
        if code {
            let rows = debugger::disasm_rows(|a| gb.debug_read(a), addr, 16, &hints, fmt);
            rows.iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            (0..16u16)
                .map(|i| {
                    let b = gb.debug_read(addr.wrapping_add(i));
                    if fmt.lowercase_hex {
                        format!("{b:02x}")
                    } else {
                        format!("{b:02X}")
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    /// Push the disasm display options (Options → Debug) to the debugger view,
    /// repainting it if open. Inert when the debugger window isn't built yet.
    pub fn set_disasm_fmt(&mut self, fmt: debugger::DisasmFmt) {
        if let Some(view) = self.debugger_view_mut() {
            if let WinState::Debugger(s) = &mut view.state {
                if s.disasm_fmt != fmt {
                    s.disasm_fmt = fmt;
                    view.window.request_redraw();
                }
            }
        }
    }

    /// Open the debugger's `Evaluate expression` modal (RM14).
    pub fn open_debugger_eval(&mut self) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            debugger::open_eval(s);
            view.window.request_redraw();
        }
    }

    /// Evaluate the stored expression against the live machine (RM14) and show
    /// the result (or the error) in a display-only box.
    pub fn debugger_eval(&mut self, gb: &GameBoy) {
        let regs = gb.cpu_regs();
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            let expr = s.eval_input.clone();
            let text = match debugger::eval_expr(&expr, &regs, |a| gb.debug_read(a)) {
                Ok(v) => format!("{expr} = {v:04X} ({v})"),
                Err(e) => format!("{expr}: {e}"),
            };
            debugger::show_eval_result(s, text);
            view.window.request_redraw();
        }
    }

    /// Zero the regs-pane `cnt` user-clock counter (RM14, "Set user clocks
    /// counter"): re-baseline it to the current cycle count.
    pub fn reset_debugger_clocks(&mut self, gb: &GameBoy) {
        let now = gb.cycles();
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.clock_base = now;
            view.window.request_redraw();
        }
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
) -> Option<MenuOutcome> {
    let r = gb.cpu_regs();
    // Refresh the cached profiler state so an opening Execution-profiler dropdown
    // shows the live mode + "N addresses seen" (MB5).
    s.prof = debugger::ProfilerView {
        logging: gb.profiling(),
        brk: gb.profile_break(),
        seen: gb.profile_seen(),
    };
    debugger::on_left_click(|a| gb.debug_read(a), area, s, r, px, py)
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
