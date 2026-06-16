//! The main game-window right-click menu (bgb's `rc-main.png`) and its submenus.
//! Unlike the debugger panes this isn't a tool window — it's an app-owned popup
//! drawn as an overlay over the live LCD. Pure state + hit-tests here (unit-
//! tested headless); `main` owns the `Option<MainMenu>` / `Option<SubMenu>`,
//! routes the mouse, and renders the overlay.
//!
//! Item-for-item from the captures (15 rows, no separators). The five already-
//! supported actions (Pause / Enable sound / Reset / Debugger / Exit) run via
//! `main`'s shared `run_action`; **Window size** opens its submenu (MN2). The
//! rest render greyed (the project's "not-yet-wired = greyed" convention) —
//! Load ROM / Options / Cheat / Save screenshot land in MN4/later, and the other
//! submenu rows (State / Other / Sound channel / Link / Recent ROMs) keep their
//! `▶` arrow but don't open until MN3–MN7.

use crate::input::Action;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::menu::{self, MenuItem};
use crate::ui::{Theme, ToolWindow};

/// Which submenu a main-menu row opens. `WindowSize` (MN2) and `SoundChannel`
/// (MN3) are wired; the rest of bgb's submenus arrive in MN4–MN7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubKind {
    WindowSize,
    SoundChannel,
}

/// What clicking a main-menu row does: run a shared frontend [`Action`], open a
/// [`SubKind`] submenu, or nothing (a greyed / not-yet-wired stub).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEffect {
    Run(Action),
    Submenu(SubKind),
    None,
}

/// An open main-window popup: its box origin, the rendered rows, the parallel
/// effect per row, and the hovered row. Drawn through the shared [`menu`] widget
/// so it matches every other bgb menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainMenu {
    pub origin: (i32, i32),
    pub items: Vec<MenuItem>,
    pub effects: Vec<MenuEffect>,
    pub hovered: Option<usize>,
}

impl MainMenu {
    /// Build the menu with its top-left at `origin`; `sound_on` check-marks the
    /// "Enable sound" row (reflecting the runtime mute state).
    #[must_use]
    pub fn open(origin: (i32, i32), sound_on: bool) -> Self {
        let (items, effects) = entries(sound_on).into_iter().unzip();
        Self {
            origin,
            items,
            effects,
            hovered: None,
        }
    }

    /// The effect of the row under `(px, py)` (enabled rows only; greyed rows and
    /// points outside the box resolve to [`MenuEffect::None`]).
    #[must_use]
    pub fn effect_at(&self, px: i32, py: i32) -> MenuEffect {
        menu::item_at(self.origin, &self.items, px, py)
            .map_or(MenuEffect::None, |i| self.effects[i])
    }

    /// The hit-rect of the row carrying `effect` (for positioning its submenu to
    /// the right of that row).
    #[must_use]
    pub fn row_rect(&self, effect: MenuEffect) -> Option<Rect> {
        let i = self.effects.iter().position(|&e| e == effect)?;
        menu::menu_rects(self.origin, &self.items)
            .into_iter()
            .nth(i)
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

/// The `rc-main.png` rows paired with each row's effect. `None` is a greyed stub
/// (Load ROM / Options / Cheat / Save screenshot → MN4/later) or a not-yet-wired
/// submenu row (State / Other / Sound channel / Link / Recent ROMs → MN3–MN7),
/// which renders its `▶` arrow greyed. **Window size** is live (MN2).
fn entries(sound_on: bool) -> Vec<(MenuItem, MenuEffect)> {
    vec![
        (MenuItem::new("Pause"), MenuEffect::Run(Action::Pause)),
        (MenuItem::new("Load ROM...").disabled(), MenuEffect::None),
        (
            MenuItem::new("Enable sound").checked(sound_on),
            MenuEffect::Run(Action::ToggleSound),
        ),
        (
            MenuItem::new("Options...").shortcut("F11").disabled(),
            MenuEffect::None,
        ),
        (
            MenuItem::new("Cheat...").shortcut("F10").disabled(),
            MenuEffect::None,
        ),
        (
            MenuItem::new("Reset gameboy").shortcut("*"),
            MenuEffect::Run(Action::Reset),
        ),
        (
            MenuItem::new("Save screenshot"),
            MenuEffect::Run(Action::SaveScreenshot),
        ),
        (
            MenuItem::new("Debugger").shortcut("Esc"),
            MenuEffect::Run(Action::ToggleTool(ToolWindow::Debugger)),
        ),
        (
            MenuItem::new("State").submenu().disabled(),
            MenuEffect::None,
        ),
        (
            MenuItem::new("Other").submenu().disabled(),
            MenuEffect::None,
        ),
        (
            MenuItem::new("Sound channel").submenu(),
            MenuEffect::Submenu(SubKind::SoundChannel),
        ),
        (
            MenuItem::new("Window size").submenu(),
            MenuEffect::Submenu(SubKind::WindowSize),
        ),
        (MenuItem::new("Link").submenu().disabled(), MenuEffect::None),
        (
            MenuItem::new("Recent ROMs").submenu().disabled(),
            MenuEffect::None,
        ),
        (MenuItem::new("Exit"), MenuEffect::Run(Action::Quit)),
    ]
}

// --- Submenus (MN2 Window size, MN3 Sound channel) -------------------------

/// A window-size choice from the "Window size" submenu
/// (`main-sub-windowsize.png`): an integer pixel scale, or a borderless
/// fullscreen mode (letterboxed vs aspect-stretched).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowSizeChoice {
    Scale(u32),
    Fullscreen,
    FullscreenStretched,
}

/// What activating a submenu row does. One variant per wired submenu so a
/// single [`SubMenu`] type backs them all (the parent dispatches on the
/// variant): a window-size change, or a toggle of sound channel 1-4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubChoice {
    WindowSize(WindowSizeChoice),
    /// Toggle mute on the given sound channel (1-4).
    SoundChannel(u8),
}

/// An open child submenu (Window size or Sound channel): its kind, box origin
/// (right of its parent row), rows, the parallel choice per row, and the hovered
/// row. Drawn through the same [`menu`] widget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubMenu {
    pub kind: SubKind,
    pub origin: (i32, i32),
    pub items: Vec<MenuItem>,
    pub choices: Vec<Option<SubChoice>>,
    pub hovered: Option<usize>,
}

impl SubMenu {
    /// Open the [`SubKind::WindowSize`] submenu to the right of `parent_row`,
    /// check-marking whichever row matches the `active` window size.
    #[must_use]
    pub fn window_size(parent_row: Rect, active: WindowSizeChoice) -> Self {
        let (items, choices) = window_size_items(active).into_iter().unzip();
        Self::hang(SubKind::WindowSize, parent_row, items, choices)
    }

    /// Open the [`SubKind::SoundChannel`] submenu (`main-sub-soundchannel.png`)
    /// to the right of `parent_row`. `muted[i]` mutes channel `i+1`; a row is
    /// check-marked when its channel is *audible* (bgb checks the live ones).
    #[must_use]
    pub fn sound_channel(parent_row: Rect, muted: [bool; 4]) -> Self {
        let (items, choices) = sound_channel_items(muted).into_iter().unzip();
        Self::hang(SubKind::SoundChannel, parent_row, items, choices)
    }

    /// Shared constructor: hang a submenu off the right edge of `parent_row`,
    /// top-aligned (bgb's layout).
    fn hang(
        kind: SubKind,
        parent_row: Rect,
        items: Vec<MenuItem>,
        choices: Vec<Option<SubChoice>>,
    ) -> Self {
        Self {
            kind,
            origin: (parent_row.right(), parent_row.y),
            items,
            choices,
            hovered: None,
        }
    }

    /// The choice under `(px, py)` if it lands on a row (all submenu rows are
    /// enabled); points outside the box resolve to `None`.
    #[must_use]
    pub fn choice_at(&self, px: i32, py: i32) -> Option<SubChoice> {
        menu::item_at(self.origin, &self.items, px, py).and_then(|i| self.choices[i])
    }

    /// Update the hovered row; returns whether it changed.
    pub fn hover_at(&mut self, px: i32, py: i32) -> bool {
        let new = menu::item_at(self.origin, &self.items, px, py);
        let changed = self.hovered != new;
        self.hovered = new;
        changed
    }
}

/// Draw the submenu via the shared popup widget.
pub fn render_sub(c: &mut Canvas, s: &SubMenu, theme: &Theme) {
    menu::render(c, s.origin, &s.items, s.hovered, theme);
}

/// The Window-size rows (`main-sub-windowsize.png`): 1x1‥6x6 then Full screen /
/// Fullscreen stretched, with the row matching `active` check-marked.
fn window_size_items(active: WindowSizeChoice) -> Vec<(MenuItem, Option<SubChoice>)> {
    let mut v = Vec::with_capacity(8);
    let mut push = |label: String, choice: WindowSizeChoice| {
        v.push((
            MenuItem::new(label).checked(active == choice),
            Some(SubChoice::WindowSize(choice)),
        ));
    };
    for n in 1..=6u32 {
        push(format!("{n}x{n}"), WindowSizeChoice::Scale(n));
    }
    push("Full screen".into(), WindowSizeChoice::Fullscreen);
    push(
        "Fullscreen stretched".into(),
        WindowSizeChoice::FullscreenStretched,
    );
    v
}

/// The Sound-channel rows (`main-sub-soundchannel.png`): channels 1-4 with
/// hotkeys F5-F8, each check-marked while audible (`!muted[i]`).
fn sound_channel_items(muted: [bool; 4]) -> Vec<(MenuItem, Option<SubChoice>)> {
    (1..=4u8)
        .map(|ch| {
            let item = MenuItem::new(ch.to_string())
                .shortcut(format!("F{}", ch + 4))
                .checked(!muted[usize::from(ch - 1)]);
            (item, Some(SubChoice::SoundChannel(ch)))
        })
        .collect()
}

#[cfg(test)]
#[path = "mainwin_tests.rs"]
mod tests;
