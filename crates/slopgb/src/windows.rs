//! Layer C: the bgb tool windows (Debugger / VRAM viewer / I/O map). Each is a
//! pure content renderer composing the `ui` widgets over `slopgb_core::debug`
//! introspection, unit-tested headless; the event loop (B12b) feeds each one a
//! real softbuffer surface and routes its input.
#![allow(dead_code, unused_imports)] // scaffolding; wired to winit surfaces in B12b.

pub mod debugger;
pub mod iomap;
pub mod vram;
