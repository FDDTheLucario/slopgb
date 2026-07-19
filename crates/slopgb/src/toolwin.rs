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
use std::time::{Duration, Instant};

use slopgb_core::GameBoy;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;
use winit::window::{Window, WindowId};

use crate::dbg::Breakpoints;
use crate::ui::canvas::Rect;
use crate::ui::dialog::{DialogKey, DialogResult, InputDialog};
use crate::ui::text::line_height;
use crate::ui::widgets::{vscroll_frac, vscroll_track};
use crate::ui::{Canvas, Theme, ToolWindow, WindowRegistry};
use crate::windows::{self, WinState, debugger, vram};
use debugger::MenuOutcome;

mod debugger_ctl;

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
    /// Time + position of the last left-press, for synthesizing double-clicks
    /// (winit delivers no double-click event).
    last_click: Option<(Instant, i32, i32)>,
}

impl ToolView {
    /// The window's content rect in physical pixels — matches the `Canvas`
    /// bounds `render` draws into, so hit-rects line up with the cursor.
    fn area(&self) -> Rect {
        let size = self.window.inner_size();
        Rect::new(0, 0, size.width as i32, size.height as i32)
    }

    /// Record a left-press at `(px, py)` and report whether it completes a
    /// double-click ([`is_double_click`]). A completed double resets the timer so
    /// a third press starts fresh.
    fn note_click(&mut self, px: i32, py: i32) -> bool {
        let now = Instant::now();
        let double = self
            .last_click
            .is_some_and(|(t, lx, ly)| is_double_click(now.duration_since(t), px - lx, py - ly));
        self.last_click = if double { None } else { Some((now, px, py)) };
        double
    }
}

/// A second left-press completes a double-click when it lands within 400 ms and
/// 3 px of the previous one (winit delivers no double-click event). `pub(crate)`
/// so `app_menu`'s game-window file-picker click routing can reuse the same
/// rule instead of re-deriving it.
#[must_use]
pub(crate) fn is_double_click(dt: Duration, dx: i32, dy: i32) -> bool {
    dt < Duration::from_millis(400) && dx.abs() <= 3 && dy.abs() <= 3
}

/// The standalone memory viewer's dump body (window minus the status-bar line),
/// the rect its scrollbar spans — matches `render_memory_window`.
fn mem_body(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.w, (area.h - line_height()).max(0))
}

/// The scrollbar track the point `(px, py)` lands in for a `kind` window of
/// content-rect `area`, or `None`. Shared by drag-start and drag-follow so both
/// hit the tracks the renderer drew (`vscroll_track` on the same pane rects).
fn scrollbar_at(kind: ToolWindow, area: Rect, px: i32, py: i32) -> Option<ScrollBar> {
    match kind {
        ToolWindow::Debugger => {
            let l = debugger::DebuggerLayout::for_size(area.w, area.h);
            if vscroll_track(l.disasm).contains(px, py) {
                Some(ScrollBar::Disasm)
            } else if vscroll_track(l.memory).contains(px, py) {
                Some(ScrollBar::Memory)
            } else if vscroll_track(l.stack).contains(px, py) {
                Some(ScrollBar::Stack)
            } else {
                None
            }
        }
        ToolWindow::MemoryViewer => vscroll_track(mem_body(area))
            .contains(px, py)
            .then_some(ScrollBar::MemViewer),
        _ => None,
    }
}

/// Full 16-byte rows visible in a pane `height` px tall (less the one-line
/// status bar) at the current line height — one page of PageUp/PageDown
/// scrolling (at least 1), used for cursor auto-scroll.
#[must_use]
fn mem_visible_rows(height: i32) -> i32 {
    let lh = line_height();
    ((height - lh) / lh.max(1)).max(1)
}

/// Which scrollable pane a scrollbar drag is manipulating.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScrollBar {
    Disasm,
    Memory,
    Stack,
    MemViewer,
}

/// The set of open tool windows.
pub struct ToolWindows {
    views: HashMap<WindowId, ToolView>,
    reg: WindowRegistry<WindowId>,
    theme: Theme,
    /// The scrollbar the user is dragging (window + which pane), if any — a
    /// left-press on a track starts it, cursor-move updates the offset, release
    /// ends it.
    scroll_drag: Option<(WindowId, ScrollBar)>,
}

fn title(kind: ToolWindow) -> &'static str {
    match kind {
        ToolWindow::Debugger => "slopgb — debugger",
        ToolWindow::Vram => "slopgb — VRAM viewer",
        ToolWindow::IoMap => "slopgb — I/O map",
        ToolWindow::MemoryViewer => "slopgb — memory",
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
        // A 16-byte-per-row hex dump + the status bar.
        ToolWindow::MemoryViewer => (430.0, 360.0),
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
            scroll_drag: None,
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
                last_click: None,
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

    /// Raise + focus the debugger window and repaint it — bgb pops the debugger to
    /// the front when a breakpoint is hit, so the halt is visible even if the game
    /// window had focus. No-op when the debugger window isn't open.
    pub fn focus_debugger(&self) {
        if let Some(id) = self.reg.id_of(ToolWindow::Debugger) {
            if let Some(view) = self.views.get(&id) {
                view.window.focus_window();
                view.window.request_redraw();
            }
        }
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
        // Keep the disasm view tracking PC (unless pinned) before drawing, so
        // single-stepping doesn't scroll the listing until PC leaves the pane.
        if let WinState::Debugger(s) = &mut view.state {
            let l = debugger::DebuggerLayout::for_size(size.width as i32, size.height as i32);
            let visible = (l.disasm.h / line_height()).max(0) as usize;
            let bank = s.disasm_bank;
            s.disasm_follow(
                gb.cpu_regs().pc,
                |a| crate::windows::banked_read(gb, bank, a),
                visible,
            );
        }
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
    /// Returns whether a VRAM hover changed — the game window then repaints its
    /// OAM sprite outline (which won't follow the cursor on its own while paused).
    pub fn on_cursor_moved(&mut self, id: WindowId, x: f64, y: f64) -> bool {
        let (px, py) = (x as i32, y as i32);
        {
            let Some(view) = self.views.get_mut(&id) else {
                return false;
            };
            view.cursor = Some((px, py));
        }
        // A scrollbar drag owns the cursor: update its offset, skip hover.
        if matches!(self.scroll_drag, Some((d, _)) if d == id) {
            self.apply_scroll_drag(id);
            return false;
        }
        let Some(view) = self.views.get_mut(&id) else {
            return false;
        };
        let area = view.area();
        match &mut view.state {
            WinState::Vram(s) => {
                let changed = vram::on_hover(s, area, px, py);
                if changed {
                    view.window.request_redraw();
                }
                changed
            }
            // Track the hovered row of an open context menu.
            WinState::Debugger(s) => {
                if let Some(om) = &mut s.menu {
                    if om.hover_at(px, py) {
                        view.window.request_redraw();
                    }
                }
                false
            }
            // The memory window has no hover state (just the cursor, tracked above).
            WinState::Stateless | WinState::Memory(_) => false,
        }
    }

    /// Emu-pixel bounding box of the sprite hovered in an open VRAM viewer's OAM
    /// tab, for the game window to outline over the live screen. `None` unless a
    /// VRAM window is open on the OAM tab with the cursor over a live sprite.
    #[must_use]
    pub fn oam_hover_rect(&self, gb: &GameBoy) -> Option<Rect> {
        let tall = gb.debug_read(0xFF40) & 0x04 != 0;
        self.views.values().find_map(|v| match &v.state {
            WinState::Vram(s) => vram::oam_hover_rect(s, v.area(), gb.oam(), tall),
            _ => None,
        })
    }

    /// If a left-press landed on a scrollbar track, begin dragging it and jump
    /// to that position. Returns whether the press was consumed (so normal click
    /// routing is skipped).
    fn begin_scroll_drag(&mut self, id: WindowId) -> bool {
        let Some(view) = self.views.get(&id) else {
            return false;
        };
        let Some((px, py)) = view.cursor else {
            return false;
        };
        let Some(bar) = scrollbar_at(view.kind, view.area(), px, py) else {
            return false;
        };
        self.scroll_drag = Some((id, bar));
        self.apply_scroll_drag(id);
        true
    }

    /// Update the dragged scrollbar's pane offset from the current cursor y.
    /// Redraws. No-op unless a drag on `id` is active.
    fn apply_scroll_drag(&mut self, id: WindowId) {
        let Some((_, bar)) = self.scroll_drag.filter(|&(d, _)| d == id) else {
            return;
        };
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        let Some((_, py)) = view.cursor else {
            return;
        };
        let area = view.area();
        let lh = line_height();
        match (&mut view.state, bar) {
            (WinState::Debugger(s), ScrollBar::Disasm) => {
                let l = debugger::DebuggerLayout::for_size(area.w, area.h);
                let (_, vis) = s.disasm_scroll((l.disasm.h / lh).max(0) as usize);
                s.set_disasm_scroll(vscroll_frac(vscroll_track(l.disasm), py, vis));
            }
            (WinState::Debugger(s), ScrollBar::Memory) => {
                let l = debugger::DebuggerLayout::for_size(area.w, area.h);
                let (_, vis) = s.mem_scroll((l.memory.h / lh).max(0) as usize);
                s.set_mem_scroll(vscroll_frac(vscroll_track(l.memory), py, vis));
            }
            (WinState::Debugger(s), ScrollBar::Stack) => {
                let l = debugger::DebuggerLayout::for_size(area.w, area.h);
                let (_, vis) = s.stack_scroll((l.stack.h / lh).max(0) as usize);
                s.set_stack_scroll(vscroll_frac(vscroll_track(l.stack), py, vis));
            }
            (WinState::Memory(s), ScrollBar::MemViewer) => {
                let body = mem_body(area);
                let (_, vis) = s.scroll_frac((body.h / lh).max(0) as usize);
                s.set_scroll(vscroll_frac(vscroll_track(body), py, vis));
            }
            _ => return,
        }
        view.window.request_redraw();
    }

    /// End any scrollbar drag (on left-release).
    pub fn on_mouse_up(&mut self) {
        self.scroll_drag = None;
    }

    /// Handle a left-button press on tool window `id` (uses the last cursor
    /// position): switches a VRAM control, selects a debugger menu item, or sets
    /// the debugger cursor. Returns a [`MenuOutcome`] for `main` to apply
    /// (debugger only), redrawing on any change.
    pub fn on_mouse_left(&mut self, id: WindowId, gb: &GameBoy) -> Option<MenuOutcome> {
        // A press on a scrollbar track starts a drag instead of a pane click.
        if self.begin_scroll_drag(id) {
            return None;
        }
        let view = self.views.get_mut(&id)?;
        let (px, py) = view.cursor?;
        let area = view.area();
        let double = view.note_click(px, py);
        match &mut view.state {
            WinState::Vram(s) => {
                if vram::on_click(s, area, px, py) {
                    view.window.request_redraw();
                }
                None
            }
            WinState::Debugger(s) => {
                // An open modal eats the click (OK/Cancel may yield a register
                // write); else normal routing. A double-click toggles a
                // breakpoint on the disasm line (bgb); a single click selects it.
                let (consumed, outcome) = debugger::dialog_click(s, area, px, py);
                let action = if consumed {
                    outcome
                } else if double {
                    // A double-click off a disasm line declines the breakpoint
                    // toggle; fall through to the normal single-click handling so
                    // the click still selects.
                    match debugger_double_click(s, area, gb, px, py) {
                        Some(o) => Some(o),
                        None => debugger_left_click(s, area, gb, px, py),
                    }
                } else {
                    debugger_left_click(s, area, gb, px, py)
                };
                view.window.request_redraw();
                action
            }
            WinState::Stateless | WinState::Memory(_) => None,
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

    /// A mouse-wheel notch over tool window `id` scrolls the pane under the cursor
    /// (`y_lines` > 0 = wheel up = toward lower addresses): the debugger's disasm,
    /// stack, or memory pane, or the standalone memory window; ignored elsewhere.
    pub fn on_wheel(&mut self, id: WindowId, y_lines: f32, gb: &GameBoy) {
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        let area = view.area();
        let cursor = view.cursor;
        let rows = -(y_lines.round() as i32) * 3;
        match &mut view.state {
            WinState::Debugger(s) => {
                if let Some((px, py)) = cursor {
                    let l = debugger::DebuggerLayout::for_size(area.w, area.h);
                    if l.memory.contains(px, py) {
                        s.scroll_memory(rows);
                    } else if l.disasm.contains(px, py) {
                        let bank = s.disasm_bank;
                        s.scroll_disasm(rows, |a| crate::windows::banked_read(gb, bank, a));
                    } else if l.stack.contains(px, py) {
                        s.scroll_stack(rows);
                    } else {
                        return;
                    }
                    view.window.request_redraw();
                }
            }
            // The standalone memory window is all memory: the wheel always scrolls.
            WinState::Memory(s) => {
                s.scroll(rows);
                view.window.request_redraw();
            }
            _ => {}
        }
    }

    /// Whether the standalone memory window `id` has an open `Go to…` dialog
    /// (so the key path routes keys to it instead of scrolling/hotkeys).
    #[must_use]
    pub fn mem_dialog_active(&self, id: WindowId) -> bool {
        matches!(
            self.views.get(&id).map(|v| &v.state),
            Some(WinState::Memory(s)) if s.goto.is_some()
        )
    }

    /// Open the `Go to…` dialog on the standalone memory window `id` (Ctrl+G).
    pub fn open_mem_goto(&mut self, id: WindowId) {
        if let Some(view) = self.views.get_mut(&id) {
            if let WinState::Memory(s) = &mut view.state {
                s.goto = Some(InputDialog::new("Go to address or symbol", false));
                view.window.request_redraw();
            }
        }
    }

    /// Feed a key to the standalone memory window's `Go to…` dialog: apply the
    /// address on Accept, close on Accept/Cancel. Redraws.
    pub fn feed_mem_dialog(&mut self, id: WindowId, key: DialogKey) {
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        let WinState::Memory(s) = &mut view.state else {
            return;
        };
        let Some(dlg) = &mut s.goto else {
            return;
        };
        match dlg.on_key(key) {
            DialogResult::Continue => {}
            DialogResult::Accept(text) => {
                s.apply_goto(&text);
                s.goto = None;
            }
            DialogResult::Cancel => s.goto = None,
        }
        view.window.request_redraw();
    }

    /// Handle a navigation key for the standalone memory window `id` (arrows by a
    /// row, PageUp/Down by a page); returns whether it was consumed (so the caller
    /// doesn't also route it as a game button). Repeats are welcome here, so it is
    /// not behind the key-repeat guard — holding an arrow scrolls continuously.
    pub fn mem_window_key(&mut self, id: WindowId, code: KeyCode, gb: &GameBoy) -> bool {
        let Some(view) = self.views.get_mut(&id) else {
            return false;
        };
        let area = view.area();
        let visible = mem_visible_rows(area.h);
        let WinState::Memory(s) = &mut view.state else {
            return false;
        };
        // `[` / `]` step the browsed bank of the region the view sits in.
        if matches!(code, KeyCode::BracketLeft | KeyCode::BracketRight) {
            let delta = if code == KeyCode::BracketLeft { -1 } else { 1 };
            let live = crate::windows::live_bank(gb, s.mem_base);
            s.step_bank(delta, live, gb.region_bank_count(s.mem_base));
            view.window.request_redraw();
            return true;
        }
        // Arrows move the byte-edit cursor (a row / a byte); Page by a window.
        let delta = match code {
            KeyCode::ArrowUp => -16,
            KeyCode::ArrowDown => 16,
            KeyCode::ArrowLeft => -1,
            KeyCode::ArrowRight => 1,
            KeyCode::PageUp => -visible * 16,
            KeyCode::PageDown => visible * 16,
            _ => return false,
        };
        s.move_cursor(delta, visible);
        view.window.request_redraw();
        true
    }

    /// Type a hex digit into the standalone memory window's in-place editor;
    /// returns `Some((sel, addr, value))` to write when the byte completes, where
    /// `sel` is the pane's bank selection — fed to `windows::banked_write` so the
    /// edit lands exactly where the dump shows it (`windows::banked_read`). Redraws.
    pub fn mem_edit_digit(&mut self, id: WindowId, d: u8) -> Option<(Option<u16>, u16, u8)> {
        let view = self.views.get_mut(&id)?;
        let visible = mem_visible_rows(view.area().h);
        let WinState::Memory(s) = &mut view.state else {
            return None;
        };
        let sel = s.bank;
        let out = s.edit_hex_digit(d);
        s.ensure_cursor_visible(visible);
        view.window.request_redraw();
        out.map(|(addr, val)| (sel, addr, val))
    }

    /// Cancel a pending in-place edit on the memory window (Esc). Returns whether
    /// an edit was in progress (so the key is consumed only then). Redraws.
    pub fn mem_cancel_edit(&mut self, id: WindowId) -> bool {
        let Some(view) = self.views.get_mut(&id) else {
            return false;
        };
        let WinState::Memory(s) = &mut view.state else {
            return false;
        };
        let cancelled = s.cancel_edit();
        if cancelled {
            view.window.request_redraw();
        }
        cancelled
    }

    /// Push the "8-bit tile hex" display option (Options → Debug) to the VRAM
    /// viewer, repainting it on change. Inert when no VRAM window is open.
    pub fn set_tile_hex_8bit(&mut self, on: bool) {
        for view in self.views.values_mut() {
            if let WinState::Vram(s) = &mut view.state {
                if s.tile_hex_8bit != on {
                    s.tile_hex_8bit = on;
                    view.window.request_redraw();
                }
            }
        }
    }

    /// Push a loaded `.sym` symbol table to the debugger view (shared `Rc`),
    /// repainting it so the disasm labels/operands update. Inert when no
    /// debugger window is open (the table is re-pushed when one opens).
    pub fn set_symbols(&mut self, symbols: Rc<crate::symbols::SymbolTable>) {
        for view in self.views.values_mut() {
            match &mut view.state {
                WinState::Debugger(s) => s.symbols = symbols.clone(),
                WinState::Memory(s) => s.symbols = symbols.clone(),
                _ => continue,
            }
            view.window.request_redraw();
        }
    }

    /// Clear the remembered cursor when it leaves tool window `id`, so a stale
    /// position can't drive a click and the hover details clear.
    pub fn on_cursor_left(&mut self, id: WindowId) {
        // End a scrollbar drag that leaves the window (a release off-window may
        // not reach us), so it can't resume as a no-button "drag" on re-entry.
        if matches!(self.scroll_drag, Some((d, _)) if d == id) {
            self.scroll_drag = None;
        }
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

    /// Per-frame auto-redraw honouring the "Live update memory viewer" option:
    /// every tool window repaints except the standalone memory viewer when
    /// `mem_live` is off (it then repaints only on interaction — scroll / Go-to).
    pub fn request_redraw_live(&self, mem_live: bool) {
        for v in self.views.values() {
            if auto_redraws(matches!(v.state, WinState::Memory(_)), mem_live) {
                v.window.request_redraw();
            }
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
    // Refresh the CDL-on flag so the Debug dropdown's "CDL logging" check-mark
    // reflects the live state (bgb's toggled-item tick).
    s.cdl_on = gb.cdl_flags().is_some();
    // Hit-test through the same pinned-bank read + bank the renderer used, so
    // symbol label lines land on the same rows and the click maps to the drawn
    // address (a live-bank read would shift rows when the view is bank-pinned).
    let bank = s.disasm_bank;
    debugger::on_left_click(
        |a| windows::banked_read(gb, bank, a),
        area,
        s,
        r,
        px,
        py,
        |a| windows::shown_bank(gb, bank, a),
    )
}

/// Glue for [`debugger::on_double_click`] (toggles a breakpoint on a
/// double-clicked disasm line).
fn debugger_double_click(
    s: &debugger::DebuggerState,
    area: Rect,
    gb: &GameBoy,
    px: i32,
    py: i32,
) -> Option<MenuOutcome> {
    let r = gb.cpu_regs();
    debugger::on_double_click(
        |a| windows::banked_read(gb, s.disasm_bank, a),
        area,
        s,
        r.pc,
        r.sp,
        px,
        py,
        |a| windows::shown_bank(gb, s.disasm_bank, a),
    )
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
    // Same renderer-matched read/bank as the left-click glue (row alignment).
    let bank = s.disasm_bank;
    debugger::on_right_click(
        |a| windows::banked_read(gb, bank, a),
        area,
        s,
        r.pc,
        r.sp,
        px,
        py,
        |a| windows::shown_bank(gb, bank, a),
    );
}

/// Whether a tool window should auto-refresh on the per-frame tick: everything
/// does, except the standalone memory viewer when "Live update memory viewer"
/// is off (it then refreshes only on interaction).
fn auto_redraws(is_memory_window: bool, mem_live: bool) -> bool {
    mem_live || !is_memory_window
}

#[cfg(test)]
mod tests {
    use super::{auto_redraws, debugger_double_click, is_double_click};
    use crate::dbg::DebugAction;
    use crate::symbols::SymbolTable;
    use crate::ui::canvas::Rect;
    use crate::ui::text::line_height;
    use crate::windows::debugger::{self, MenuOutcome};
    use slopgb_core::{GameBoy, Model};
    use std::rc::Rc;
    use std::time::Duration;

    /// A bank-pinned view must hit-test through the pinned bank, not the live
    /// one: the renderer names symbols per the *shown* bank, and a live-bank
    /// hit-test would miss the label row and toggle the breakpoint one line off.
    #[test]
    fn double_click_hits_the_drawn_row_in_a_pinned_bank() {
        // MBC1, 64 KiB (4 banks), all-nop body; live ROM bank is 1 at reset.
        let mut rom = vec![0u8; 0x10000];
        rom[0x0147] = 0x01;
        rom[0x0148] = 0x01;
        let gb = GameBoy::new(Model::Dmg, rom).unwrap();
        assert_ne!(gb.rom_bank(), 2, "live bank must differ from the pin");
        // View pinned to bank 2 with a symbol at its first shown row, so the
        // rendered pane is: row 0 `Foo:` label, row 1 the 02:4000 instruction.
        let st = debugger::DebuggerState {
            pinned: true,
            disasm_base: 0x4000,
            disasm_bank: Some(2),
            symbols: Rc::new(SymbolTable::parse("02:4000 Foo")),
            ..debugger::DebuggerState::default()
        };
        let area = Rect::new(0, 0, 760, 560);
        let l = debugger::DebuggerLayout::for_size(area.w, area.h);
        let out = debugger_double_click(
            &st,
            area,
            &gb,
            l.disasm.x + 9,
            l.disasm.y + line_height() + 1, // row 1: the labeled instruction
        );
        assert_eq!(
            out,
            Some(MenuOutcome::Act(DebugAction::ToggleBreakpoint(
                0x4000,
                Some(2)
            )))
        );
    }

    #[test]
    fn double_click_within_window_and_radius() {
        let ms = Duration::from_millis;
        assert!(is_double_click(ms(100), 2, 3), "fast + close = double");
        assert!(
            is_double_click(ms(399), -3, 0),
            "just inside the time/radius"
        );
        assert!(!is_double_click(ms(401), 0, 0), "too slow");
        assert!(!is_double_click(ms(50), 4, 0), "too far in x");
        assert!(!is_double_click(ms(50), 0, 4), "too far in y");
    }

    #[test]
    fn memory_window_skips_auto_redraw_only_when_live_update_is_off() {
        // Non-memory windows always auto-refresh; the memory window skips it only
        // when "Live update memory viewer" is off.
        assert!(auto_redraws(false, true));
        assert!(auto_redraws(false, false), "non-memory always refreshes");
        assert!(auto_redraws(true, true), "memory refreshes when live");
        assert!(
            !auto_redraws(true, false),
            "memory frozen when live-update off"
        );
    }
}
