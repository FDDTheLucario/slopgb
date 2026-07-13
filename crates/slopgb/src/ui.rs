//! Software-rendered UI toolkit for the bgb-style debugger/viewer windows
//! (Layer B of `docs/bgb-clone-plan.md`): pure pixel drawing into softbuffer
//! XRGB8888 buffers, no GUI dependency. Composed into the tool windows in
//! Layer C.

pub mod canvas;
pub mod dialog;
pub mod font;
pub mod menu;
pub mod registry;
pub mod text;
pub mod theme;
pub mod widgets;

pub use canvas::Canvas;
pub use registry::{ToolWindow, WindowRegistry};
// `ThemeParseError` is re-exported for API symmetry with `Theme`/`ThemeChoice`
// (a future custom-theme UI/CLI would surface it); nothing in-crate names it
// via this path yet (only via `theme::ThemeParseError` internally).
#[allow(unused_imports)]
pub use theme::{CustomThemes, Theme, ThemeChoice, ThemeParseError};

#[cfg(test)]
#[path = "ui/font_tests.rs"]
mod font_tests;
