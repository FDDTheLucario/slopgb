//! Software-rendered UI toolkit for the bgb-style debugger/viewer windows
//! (Layer B of `docs/bgb-clone-plan.md`): pure pixel drawing into softbuffer
//! XRGB8888 buffers, no GUI dependency. Composed into the tool windows in
//! Layer C.
// Scaffolding under construction; the re-exports and helpers are consumed by
// the tool windows in Layer C. Remove these allows once those land.
#![allow(dead_code, unused_imports)]

pub mod canvas;

pub use canvas::{Canvas, Rect};
