//! The main game-window right-click menu (bgb's `rc-main.png`). Unlike the
//! debugger panes this isn't a tool window — it's an app-owned popup drawn as an
//! overlay over the live LCD. Pure state + hit-tests here (unit-tested headless);
//! `main` owns the `Option<MainMenu>`, routes the mouse, and renders the overlay.
//!
//! Item-for-item from the capture (15 rows, no separators). The five already-
//! supported actions (Pause / Enable sound / Reset / Debugger / Exit) are
//! enabled and ride `main`'s shared `run_action`; everything else renders greyed
//! (the project's "not-yet-wired = greyed" convention) — Load ROM / Options /
//! Cheat / Save screenshot land in MN4/later, and the six submenu rows keep
//! their `▶` arrow but don't open until MN2–MN7.

use crate::input::Action;
use crate::ui::canvas::Canvas;
use crate::ui::menu::{self, MenuItem};
use crate::ui::{Theme, ToolWindow};

/// An open main-window popup: its box origin, the rendered rows, the parallel
/// action per row (`None` = greyed / submenu stub), and the hovered row. Drawn
/// through the shared [`menu`] widget so it matches every other bgb menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainMenu {
    pub origin: (i32, i32),
    pub items: Vec<MenuItem>,
    pub actions: Vec<Option<Action>>,
    pub hovered: Option<usize>,
}

impl MainMenu {
    /// Build the menu with its top-left at `origin`; `sound_on` check-marks the
    /// "Enable sound" row (reflecting the runtime mute state).
    #[must_use]
    pub fn open(origin: (i32, i32), sound_on: bool) -> Self {
        let (items, actions) = entries(sound_on).into_iter().unzip();
        Self {
            origin,
            items,
            actions,
            hovered: None,
        }
    }

    /// The action under `(px, py)` if it lands on an enabled, wired row (greyed,
    /// submenu, and not-yet-wired rows resolve to `None`).
    #[must_use]
    pub fn action_at(&self, px: i32, py: i32) -> Option<Action> {
        menu::item_at(self.origin, &self.items, px, py).and_then(|i| self.actions[i])
    }

    /// Update the hovered row; returns whether it changed (so `main` only
    /// redraws on a real change).
    pub fn hover_at(&mut self, px: i32, py: i32) -> bool {
        let new = menu::item_at(self.origin, &self.items, px, py);
        let changed = self.hovered != new;
        self.hovered = new;
        changed
    }
}

/// Draw the menu via the shared popup widget.
pub fn render(c: &mut Canvas, m: &MainMenu, theme: &Theme) {
    menu::render(c, m.origin, &m.items, m.hovered, theme);
}

/// The `rc-main.png` rows paired with each row's action. `None` is a greyed stub
/// (Load ROM / Options / Cheat / Save screenshot → MN4/later) or a submenu row
/// (State / Other / Sound channel / Window size / Link / Recent ROMs → MN2–MN7),
/// which renders its `▶` arrow greyed until its submenu is wired.
fn entries(sound_on: bool) -> Vec<(MenuItem, Option<Action>)> {
    vec![
        (MenuItem::new("Pause"), Some(Action::Pause)),
        (MenuItem::new("Load ROM...").disabled(), None),
        (
            MenuItem::new("Enable sound").checked(sound_on),
            Some(Action::ToggleSound),
        ),
        (MenuItem::new("Options...").shortcut("F11").disabled(), None),
        (MenuItem::new("Cheat...").shortcut("F10").disabled(), None),
        (
            MenuItem::new("Reset gameboy").shortcut("*"),
            Some(Action::Reset),
        ),
        (MenuItem::new("Save screenshot").disabled(), None),
        (
            MenuItem::new("Debugger").shortcut("Esc"),
            Some(Action::ToggleTool(ToolWindow::Debugger)),
        ),
        (MenuItem::new("State").submenu().disabled(), None),
        (MenuItem::new("Other").submenu().disabled(), None),
        (MenuItem::new("Sound channel").submenu().disabled(), None),
        (MenuItem::new("Window size").submenu().disabled(), None),
        (MenuItem::new("Link").submenu().disabled(), None),
        (MenuItem::new("Recent ROMs").submenu().disabled(), None),
        (MenuItem::new("Exit"), Some(Action::Quit)),
    ]
}

#[cfg(test)]
#[path = "mainwin_tests.rs"]
mod tests;
