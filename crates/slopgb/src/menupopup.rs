//! The game-window right-click menu as its **own borderless window** (the QA
//! fix for the menu being clipped by the game window's edge). bgb draws this as
//! a native Win32 popup that can extend past the parent onto the desktop; the
//! closest match in our winit/softbuffer stack is a separate undecorated window
//! sized to the whole menu tree (main menu + the currently-open submenu, drawn
//! side-by-side in popup-local coordinates — a *single* window, so there is no
//! nested-window focus-dismissal problem).
//!
//! Positioning is `game-window client origin + cursor`, clamped to the monitor
//! bounds ([`mainwin::popup_screen_origin`]; winit exposes no work-area API, so a
//! panel/taskbar is not excluded). **Wayland caveat:** winit cannot
//! place a top-level at an arbitrary global position on Wayland, so there the
//! compositor chooses the spot — the menu is still an un-clipped separate window
//! (the actual fix), just not pixel-placed at the cursor.
//!
//! The pure geometry + the menu hit-tests live in [`crate::windows::mainwin`]
//! (unit-tested); this module is the thin winit glue, verified live.

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::input::Action;
use crate::ui::canvas::Rect;
use crate::ui::{Canvas, Theme};
use crate::windows::mainwin::{
    self, MainMenu, MenuEffect, SubChoice, SubKind, SubMenu, popup_content_size,
    popup_screen_origin,
};

/// A screen/popup-local point in physical pixels.
type Point = (i32, i32);
/// Monitor bounds `(x, y, w, h)` in physical pixels.
type MonitorBounds = (i32, i32, i32, i32);

/// What a click on the popup resolves to, for `App` to apply (it owns the live
/// state the submenus + actions need). `OpenSub` carries the parent row so `App`
/// can build the submenu and hand it back via [`MenuPopup::set_submenu`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupOutcome {
    /// Run a shared frontend action (a main-menu leaf row).
    Run(Action),
    /// Apply a submenu choice.
    Sub(SubChoice),
    /// Open the submenu for `kind`, hung off `row` (popup-local coordinates).
    OpenSub(SubKind, Rect),
    /// Dismiss the menu (clicked off it / a greyed row).
    Close,
}

/// The open right-click menu popup: its borderless window + surface, the main
/// menu, the open submenu (if any), and the last popup-local cursor position.
pub struct MenuPopup {
    window: Rc<Window>,
    // Kept alive alongside the surface (softbuffer's display connection).
    _ctx: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    menu: MainMenu,
    sub: Option<SubMenu>,
    cursor: Option<(i32, i32)>,
    /// Screen-space anchor (the right-click point in global coords) and the game
    /// monitor bounds, captured at open. Kept so a submenu that grows the window
    /// can re-clamp its position to stay on-screen (the open-time clamp only knew
    /// the main-menu size).
    anchor: Point,
    monitor: Option<MonitorBounds>,
    /// Whether the popup has ever held focus. Some WMs deliver a spurious
    /// `Focused(false)` right after mapping a borderless window (before it is
    /// ever focused); dismissing on that would close the menu instantly. So a
    /// focus-loss only dismisses once the popup has actually been focused.
    focused_once: bool,
    theme: Theme,
}

impl MenuPopup {
    /// Open the popup for a right-click at `cursor` (game-window-local physical
    /// px), sized to the main menu and positioned at the screen cursor (clamped
    /// to `game_window`'s monitor). Returns `None` if window/surface creation
    /// fails (logged) — the caller just leaves no menu open.
    #[must_use]
    pub fn open(
        el: &ActiveEventLoop,
        game_window: &Window,
        cursor: (i32, i32),
        sound_on: bool,
        paused: bool,
    ) -> Option<Self> {
        let menu = MainMenu::open((0, 0), sound_on, paused);
        let (pw, ph) = popup_content_size(&menu, None);
        let (anchor, monitor) = anchor_and_monitor(game_window, cursor);
        let (ox, oy) = popup_screen_origin(anchor, (0, 0), (pw, ph), monitor);
        let attrs = Window::default_attributes()
            .with_title("slopgb — menu")
            .with_decorations(false)
            .with_resizable(false)
            // Request activation so click-away anywhere dismisses via focus-loss
            // (the WM may still decline, e.g. focus-follows-mouse — handled by the
            // game-window click-away + Escape paths and the `focused_once` guard).
            .with_active(true)
            .with_inner_size(PhysicalSize::new(pw.max(1) as u32, ph.max(1) as u32))
            .with_position(PhysicalPosition::new(ox, oy));
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("slopgb: cannot open menu popup: {e}");
                return None;
            }
        };
        let ctx = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("slopgb: menu popup surface init failed: {e}");
                return None;
            }
        };
        let surface = match softbuffer::Surface::new(&ctx, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("slopgb: menu popup surface init failed: {e}");
                return None;
            }
        };
        window.request_redraw();
        Some(Self {
            window,
            _ctx: ctx,
            surface,
            menu,
            sub: None,
            cursor: None,
            anchor,
            monitor,
            focused_once: false,
            theme: Theme::BGB,
        })
    }

    /// The popup window's id, so `main` can route its events.
    #[must_use]
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Note a focus change; returns whether the popup should now be dismissed.
    /// A focus *gain* arms dismissal; a focus *loss* dismisses only once the
    /// popup has actually been focused (ignoring the spurious on-map
    /// `Focused(false)` some WMs deliver before the window is ever focused).
    pub fn note_focus(&mut self, focused: bool) -> bool {
        focus_dismiss(&mut self.focused_once, focused)
    }

    /// Render the menu tree into the borderless window.
    pub fn redraw(&mut self) {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return; // minimized / zero-sized
        };
        if self.surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buf) = self.surface.buffer_mut() else {
            return;
        };
        {
            let mut c = Canvas::new(&mut buf, size.width as usize, size.height as usize);
            let area = c.bounds();
            c.fill_rect(area, self.theme.bg);
            mainwin::render(&mut c, &self.menu, &self.theme);
            if let Some(s) = &self.sub {
                mainwin::render_sub(&mut c, s, &self.theme);
            }
        }
        // Force opaque alpha (softbuffer leaves the top byte 0 — transparent to
        // an ARGB compositor), matching the game/tool windows.
        for px in buf.iter_mut() {
            *px |= 0xFF00_0000;
        }
        self.window.pre_present_notify();
        let _ = buf.present();
    }

    /// Record the popup-local cursor and re-highlight the hovered row (redrawing
    /// only on a real change). The submenu takes hover priority when open (as
    /// bgb highlights the active child row).
    pub fn on_cursor_moved(&mut self, x: f64, y: f64) {
        let (px, py) = (x as i32, y as i32);
        self.cursor = Some((px, py));
        let changed = if let Some(s) = &mut self.sub {
            s.hover_at(px, py)
        } else {
            self.menu.hover_at(px, py)
        };
        if changed {
            self.window.request_redraw();
        }
    }

    /// Resolve a left-click at the last cursor position into a [`PopupOutcome`].
    pub fn on_click(&mut self) -> PopupOutcome {
        let Some((px, py)) = self.cursor else {
            return PopupOutcome::Close;
        };
        // A click on the open submenu applies its choice; off it, the submenu
        // closes and the main menu handles the click (bgb's behaviour).
        if let Some(sub) = &self.sub {
            if let Some(choice) = sub.choice_at(px, py) {
                return PopupOutcome::Sub(choice);
            }
            self.sub = None;
            self.resize_to_content();
            self.window.request_redraw();
        }
        match self.menu.effect_at(px, py) {
            MenuEffect::Run(act) => PopupOutcome::Run(act),
            MenuEffect::Submenu(kind) => self
                .menu
                .row_rect(MenuEffect::Submenu(kind))
                .map_or(PopupOutcome::Close, |row| PopupOutcome::OpenSub(kind, row)),
            MenuEffect::None => PopupOutcome::Close,
        }
    }

    /// Hang `sub` off its parent row, growing the window to cover the whole tree.
    pub fn set_submenu(&mut self, sub: SubMenu) {
        self.sub = Some(sub);
        self.resize_to_content();
        self.window.request_redraw();
    }

    /// Resize the window to the current menu-tree extent, re-clamping its screen
    /// position. A submenu hangs off the parent row's right edge, so opening one
    /// near the monitor's right/bottom edge would push the grown window
    /// off-screen — re-running the clamp against the new size pulls it back (the
    /// open-time clamp only knew the smaller main-menu size).
    fn resize_to_content(&self) {
        let (w, h) = popup_content_size(&self.menu, self.sub.as_ref());
        let _ = self
            .window
            .request_inner_size(PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
        let (ox, oy) = popup_screen_origin(self.anchor, (0, 0), (w, h), self.monitor);
        self.window
            .set_outer_position(PhysicalPosition::new(ox, oy));
    }
}

/// The screen-space anchor (the right-click point in global coords) and the game
/// monitor bounds, for positioning/re-clamping the popup. Uses the game window's
/// **inner** (client-area) origin — the cursor from `CursorMoved` is
/// client-relative, so adding it to the *outer* origin would offset the popup by
/// the title-bar/decoration height — falling back to the outer origin only where
/// `inner_position` is unsupported. Monitor is the full bounds (winit exposes no
/// work-area API, so a panel/taskbar is not excluded).
fn anchor_and_monitor(game: &Window, cursor: (i32, i32)) -> (Point, Option<MonitorBounds>) {
    let base = game
        .inner_position()
        .or_else(|_| game.outer_position())
        .map_or((0, 0), |p| (p.x, p.y));
    let monitor = game.current_monitor().map(|m| {
        let pos = m.position();
        let sz = m.size();
        (pos.x, pos.y, sz.width as i32, sz.height as i32)
    });
    ((base.0 + cursor.0, base.1 + cursor.1), monitor)
}

/// Decide whether a focus change dismisses the popup, updating the
/// "has ever been focused" latch `focused_once`. A focus *gain* arms it; a focus
/// *loss* dismisses only after a gain — ignoring the spurious on-map
/// `Focused(false)` some WMs deliver before the window is ever focused (which
/// would otherwise close the menu the instant it opens). Pure, so it is tested
/// headless (the rest of the winit glue is verified live).
#[must_use]
pub fn focus_dismiss(focused_once: &mut bool, focused: bool) -> bool {
    if focused {
        *focused_once = true;
        false
    } else {
        *focused_once
    }
}

#[cfg(test)]
#[path = "menupopup_tests.rs"]
mod tests;
