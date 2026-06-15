//! Bookkeeping for the multi-window frontend: which bgb tool windows are open
//! and which OS window id shows which, so the event loop can route redraw/input
//! to the right one. Pure state — creating/destroying the actual winit windows
//! is the caller's job; it tells the registry the id it got
//! ([`register`](WindowRegistry::register)) and the id it closed
//! ([`forget`](WindowRegistry::forget)). Generic over the id type so it tests
//! headless (real code uses `winit::window::WindowId`).

use std::collections::HashMap;
use std::hash::Hash;

/// A toggleable debug tool window. The always-present game LCD is not one of
/// these.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ToolWindow {
    Debugger,
    Vram,
    IoMap,
}

/// Maps each open window's id to the tool it shows. At most one window per
/// [`ToolWindow`] kind.
pub struct WindowRegistry<Id> {
    open: HashMap<Id, ToolWindow>,
}

impl<Id: Eq + Hash + Copy> WindowRegistry<Id> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: HashMap::new(),
        }
    }

    /// Whether a window of this kind is currently open.
    #[must_use]
    pub fn is_open(&self, kind: ToolWindow) -> bool {
        self.open.values().any(|&k| k == kind)
    }

    /// What a toggle of `kind` should do: `true` = open it (it's closed),
    /// `false` = close it. Pure decision; the caller then creates/destroys the
    /// winit window and calls [`register`](Self::register)/[`forget`](Self::forget).
    #[must_use]
    pub fn toggle_opens(&self, kind: ToolWindow) -> bool {
        !self.is_open(kind)
    }

    /// Record a freshly-created window `id` showing `kind`.
    pub fn register(&mut self, id: Id, kind: ToolWindow) {
        self.open.insert(id, kind);
    }

    /// The tool window `id` shows, if it is one of ours.
    #[must_use]
    pub fn kind_of(&self, id: Id) -> Option<ToolWindow> {
        self.open.get(&id).copied()
    }

    /// Drop a window that closed; returns the kind it was showing.
    pub fn forget(&mut self, id: Id) -> Option<ToolWindow> {
        self.open.remove(&id)
    }

    /// The id of the open window of `kind`, if any (to raise/redraw it).
    #[must_use]
    pub fn id_of(&self, kind: ToolWindow) -> Option<Id> {
        self.open
            .iter()
            .find_map(|(&id, &k)| (k == kind).then_some(id))
    }

    /// Every open tool window's id, for redraw-all / close-all sweeps.
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.open.keys().copied()
    }
}

impl<Id: Eq + Hash + Copy> Default for WindowRegistry<Id> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
