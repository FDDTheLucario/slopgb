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

/// Which submenu a main-menu row opens. `WindowSize` (MN2), `SoundChannel`
/// (MN3) and `Other` (MN5) are wired; the rest of bgb's submenus arrive later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubKind {
    WindowSize,
    SoundChannel,
    Other,
    State,
    RecentRoms,
    Link,
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
        (
            MenuItem::new("Load ROM..."),
            MenuEffect::Run(Action::MainLoadRom),
        ),
        (
            MenuItem::new("Enable sound").checked(sound_on),
            MenuEffect::Run(Action::ToggleSound),
        ),
        (
            MenuItem::new("Options...").shortcut("F11"),
            MenuEffect::Run(Action::MainOptions),
        ),
        (
            MenuItem::new("Cheat...").shortcut("F10"),
            MenuEffect::Run(Action::MainCheats),
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
            MenuItem::new("State").submenu(),
            MenuEffect::Submenu(SubKind::State),
        ),
        (
            MenuItem::new("Other").submenu(),
            MenuEffect::Submenu(SubKind::Other),
        ),
        (
            MenuItem::new("Sound channel").submenu(),
            MenuEffect::Submenu(SubKind::SoundChannel),
        ),
        (
            MenuItem::new("Window size").submenu(),
            MenuEffect::Submenu(SubKind::WindowSize),
        ),
        (
            MenuItem::new("Link").submenu(),
            MenuEffect::Submenu(SubKind::Link),
        ),
        (
            MenuItem::new("Recent ROMs").submenu(),
            MenuEffect::Submenu(SubKind::RecentRoms),
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

/// What activating a submenu row does. One variant per wired submenu row so a
/// single [`SubMenu`] type backs them all (the parent dispatches on the
/// variant): a window-size change, a sound-channel mute toggle, or an "Other"
/// submenu action (open the VRAM viewer / show an info box).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubChoice {
    WindowSize(WindowSizeChoice),
    /// Toggle mute on the given sound channel (1-4).
    SoundChannel(u8),
    /// Other → "VRAM viewer": open the VRAM tool window.
    OpenVram,
    /// Other → "Cart info": show the cartridge-header info box.
    CartInfo,
    /// Other → "System info": show the emulated-model info box.
    SystemInfo,
    /// Other → "About...": show the version info box.
    About,
    /// State → "Quick Save": snapshot the machine.
    QuickSave,
    /// State → "Quick Load": restore the last snapshot.
    QuickLoad,
    /// State → "Load state...": open the on-disk load-state path modal.
    LoadState,
    /// Recent ROMs → load the recent-list entry at this index (MN4).
    LoadRecent(usize),
    /// Link → "Listen": bind the link port and wait for a peer.
    LinkListen,
    /// Link → "Connect": open the host:port modal to dial a peer.
    LinkConnect,
    /// Link → "Disconnect": tear down a connected link.
    LinkDisconnect,
    /// Link → "Cancel listen": stop a listening (not-yet-connected) link.
    LinkCancelListen,
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

    /// Open the [`SubKind::Other`] submenu (`main-sub-other.png`) to the right of
    /// `parent_row`. Cart info / System info / VRAM viewer / About are live; the
    /// rest (cheat searcher / Camera / clear-recent / debug-mode / Close screen)
    /// stay greyed (their subsystems aren't built).
    #[must_use]
    pub fn other(parent_row: Rect) -> Self {
        let (items, choices) = other_items().into_iter().unzip();
        Self::hang(SubKind::Other, parent_row, items, choices)
    }

    /// Open the [`SubKind::State`] submenu (`main-sub-state.png`) to the right of
    /// `parent_row`. Quick Save / Quick Load (an in-memory snapshot) + Load
    /// state... (on-disk, via a path modal) are live; Select / Load recovery
    /// stay greyed (their subsystems aren't built — see `docs/bgb-menu-design.md`).
    #[must_use]
    pub fn state(parent_row: Rect) -> Self {
        let (items, choices) = state_items().into_iter().unzip();
        Self::hang(SubKind::State, parent_row, items, choices)
    }
    pub fn recent_roms(parent_row: Rect, names: &[String]) -> Self {
        let (items, choices) = recent_roms_items(names).into_iter().unzip();
        Self::hang(SubKind::RecentRoms, parent_row, items, choices)
    }

    /// Open the [`SubKind::Link`] submenu (`main-sub-link.png`): Listen /
    /// Connect / Disconnect / Cancel listen. Rows grey by link state —
    /// Listen/Connect only while idle, Cancel listen only while `listening`,
    /// Disconnect whenever a socket is `active` but not listening (so a pending
    /// dial can be aborted as well as a live connection torn down).
    #[must_use]
    pub fn link(parent_row: Rect, active: bool, listening: bool) -> Self {
        let (items, choices) = link_items(active, listening).into_iter().unzip();
        Self::hang(SubKind::Link, parent_row, items, choices)
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

/// The Other rows (`main-sub-other.png`), item-for-item; the wired ones carry a
/// [`SubChoice`], the not-built ones render greyed with no choice.
/// The Recent ROMs submenu rows (MN4): one per remembered ROM (most-recent
/// first), each loading that entry; an empty list shows a single greyed row.
fn recent_roms_items(names: &[String]) -> Vec<(MenuItem, Option<SubChoice>)> {
    if names.is_empty() {
        return vec![(MenuItem::new("(no recent ROMs)").disabled(), None)];
    }
    names
        .iter()
        .enumerate()
        .map(|(i, n)| (MenuItem::new(n.clone()), Some(SubChoice::LoadRecent(i))))
        .collect()
}

fn other_items() -> Vec<(MenuItem, Option<SubChoice>)> {
    let live = |label: &str, c: SubChoice| (MenuItem::new(label), Some(c));
    let greyed = |label: &str| (MenuItem::new(label).disabled(), None);
    vec![
        live("Cart info", SubChoice::CartInfo),
        live("System info", SubChoice::SystemInfo),
        live("VRAM viewer", SubChoice::OpenVram),
        greyed("cheat searcher"),
        greyed("Camera control..."),
        greyed("clear recent roms list"),
        greyed("debug mode enabled: *"),
        greyed("Close screen"),
        live("About...", SubChoice::About),
    ]
}

/// The State rows (`main-sub-state.png`): Quick Save (F2) / Quick Load (F4) are
/// the in-memory snapshot; Select / Load recovery / Load state... are greyed
/// (the on-disk format isn't built).
fn state_items() -> Vec<(MenuItem, Option<SubChoice>)> {
    vec![
        (
            MenuItem::new("Quick Save").shortcut("F2"),
            Some(SubChoice::QuickSave),
        ),
        (
            MenuItem::new("Quick Load").shortcut("F4"),
            Some(SubChoice::QuickLoad),
        ),
        (
            MenuItem::new("Select").shortcut("F3").submenu().disabled(),
            None,
        ),
        (MenuItem::new("Load recovery state").disabled(), None),
        (MenuItem::new("Load state..."), Some(SubChoice::LoadState)),
    ]
}

/// The Link rows (`main-sub-link.png`): Listen / Connect / Disconnect / Cancel
/// listen. A row is enabled (carries its [`SubChoice`]) only when meaningful:
/// Listen/Connect while idle (no socket), Cancel listen while `listening`,
/// Disconnect whenever a socket is `active` but not listening (a pending dial
/// or a live connection); the rest render greyed — bgb's enable/grey behavior.
fn link_items(active: bool, listening: bool) -> Vec<(MenuItem, Option<SubChoice>)> {
    let idle = !active;
    let row = |label: &str, enabled: bool, choice: SubChoice| {
        if enabled {
            (MenuItem::new(label), Some(choice))
        } else {
            (MenuItem::new(label).disabled(), None)
        }
    };
    vec![
        row("Listen", idle, SubChoice::LinkListen),
        row("Connect", idle, SubChoice::LinkConnect),
        row(
            "Disconnect",
            active && !listening,
            SubChoice::LinkDisconnect,
        ),
        row("Cancel listen", listening, SubChoice::LinkCancelListen),
    ]
}

// --- Popup-window geometry (RM-QA: the menu is its own borderless window) ---

/// The popup window's content size in pixels: the bounding box of the main menu
/// plus the currently-open submenu, both laid out in popup-local coordinates
/// (the main menu hung at its `origin`, the submenu off its parent row). Reuses
/// the shared [`menu`] geometry so the borderless window is sized exactly to the
/// whole menu tree — which can then extend past the game window onto the desktop
/// (bgb's native-popup behaviour) instead of being clipped by the game window.
#[must_use]
pub fn popup_content_size(menu: &MainMenu, sub: Option<&SubMenu>) -> (i32, i32) {
    let mb = menu::menu_bounds(menu.origin, &menu.items);
    let (mut right, mut bottom) = (mb.right(), mb.bottom());
    if let Some(s) = sub {
        let sb = menu::menu_bounds(s.origin, &s.items);
        right = right.max(sb.right());
        bottom = bottom.max(sb.bottom());
    }
    (right.max(1), bottom.max(1))
}

/// Screen-space top-left for the popup window: the game-window-local pointer
/// `cursor` offset by the game window's `window_outer` position, clamped so a
/// `popup` (w, h) box stays inside `monitor` (`Some((x, y, w, h))`, the full
/// monitor bounds — winit exposes no work-area API) when that is known — so the
/// menu never opens half-off the screen edge.
/// Unknown monitor → unclamped (the raw window + cursor sum).
#[must_use]
pub fn popup_screen_origin(
    cursor: (i32, i32),
    window_outer: (i32, i32),
    popup: (i32, i32),
    monitor: Option<(i32, i32, i32, i32)>,
) -> (i32, i32) {
    let mut x = window_outer.0 + cursor.0;
    let mut y = window_outer.1 + cursor.1;
    if let Some((mx, my, mw, mh)) = monitor {
        // Pull back to fit, then clamp to the near edge (handles popup > monitor).
        x = x.min(mx + mw - popup.0).max(mx);
        y = y.min(my + mh - popup.1).max(my);
    }
    (x, y)
}

// --- Info box (MN5): a centred message overlay over the LCD -----------------

/// A read-only info box (Other → Cart info / System info / About): a centred
/// titled panel of text lines drawn over the LCD. Any click or Escape closes it
/// (`main` owns the `Option<InfoBox>` and routes those, like the menus).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoBox {
    pub title: String,
    pub lines: Vec<String>,
}

impl InfoBox {
    #[must_use]
    pub fn new(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            lines,
        }
    }
}

/// Draw the info box centred in the canvas: a bordered panel, the title, then
/// each line, then an `OK` hint at the bottom.
pub fn render_info(c: &mut Canvas, info: &InfoBox, theme: &Theme) {
    use crate::ui::text::{draw_text, line_height, measure};
    let lh = line_height();
    let pad = 8;
    // Size the box to the widest of the title / lines / "OK", plus padding.
    let widest = std::iter::once(info.title.as_str())
        .chain(info.lines.iter().map(String::as_str))
        .chain(std::iter::once("OK"))
        .map(measure)
        .max()
        .unwrap_or(0);
    let w = widest + 2 * pad;
    // title + blank + lines + blank + OK row.
    let rows = info.lines.len() as i32 + 3;
    let h = rows * lh + 2 * pad;
    let area = c.bounds();
    let x = area.x + (area.w - w) / 2;
    let y = area.y + (area.h - h) / 2;
    let boxr = Rect::new(x, y, w, h);
    c.fill_rect(boxr, theme.bg);
    c.outline_rect(boxr, theme.border);
    draw_text(c, x + pad, y + pad, &info.title, theme.text);
    for (i, line) in info.lines.iter().enumerate() {
        draw_text(c, x + pad, y + pad + (i as i32 + 2) * lh, line, theme.text);
    }
    // A simple OK affordance bottom-right (any click closes the box).
    draw_text(
        c,
        boxr.right() - pad - measure("OK"),
        boxr.bottom() - pad - lh,
        "OK",
        theme.text,
    );
}

#[cfg(test)]
#[path = "mainwin_tests.rs"]
mod tests;
