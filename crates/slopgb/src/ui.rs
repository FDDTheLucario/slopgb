//! Software-rendered UI toolkit for the bgb-style debugger/viewer windows
//! (Layer B of `docs/bgb-clone-plan.md`): pure pixel drawing into softbuffer
//! XRGB8888 buffers, no GUI dependency. Composed into the tool windows in
//! Layer C.
// Scaffolding under construction; the re-exports and helpers are consumed by
// the tool windows in Layer C. Remove these allows once those land.
#![allow(dead_code, unused_imports)]

pub mod canvas;
pub mod font;
pub mod text;
pub mod widgets;

pub use canvas::{Canvas, Rect};
pub use text::{draw_text, measure};

/// The bgb debugger colour scheme, as XRGB8888 (`0x00RRGGBB`). Values are bgb's
/// `bgb.ini` defaults converted from Windows `COLORREF` (`0x00BBGGRR`): bg white
/// `FFFFFF`, text black, current-PC line blue, breakpoint red, hilight grey,
/// freeze/locked yellow.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub bg: u32,
    pub text: u32,
    pub current: u32,
    pub breakpoint: u32,
    pub hilight: u32,
    pub freeze: u32,
    pub border: u32,
}

impl Theme {
    /// bgb's stock debugger palette.
    pub const BGB: Theme = Theme {
        bg: 0x00FF_FFFF,
        text: 0x0000_0000,
        current: 0x0000_00FF,
        breakpoint: 0x00FF_0000,
        hilight: 0x0080_8080,
        freeze: 0x00FF_FF00,
        border: 0x0080_8080,
    };
}

#[cfg(test)]
#[path = "ui/font_tests.rs"]
mod font_tests;
