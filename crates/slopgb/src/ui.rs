//! Software-rendered UI toolkit for the bgb-style debugger/viewer windows:
//! pure pixel drawing into softbuffer XRGB8888 buffers, no GUI dependency.
//! Composed into the tool windows.

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
pub use theme::{CustomThemes, Theme, ThemeChoice};
// Re-exported for API symmetry with `Theme`/`ThemeChoice` (a future
// custom-theme UI/CLI would surface it); nothing in-crate names it via this
// path yet (only via `theme::ThemeParseError` internally).
#[allow(unused_imports)]
pub use theme::ThemeParseError;

#[cfg(test)]
#[path = "ui/font_tests.rs"]
mod font_tests;
