//! Read-only debug introspection for the bgb-style debugger/viewer UI.
//!
//! Everything here is side-effect-free: it observes emulator state through
//! `&self` snapshots and pure functions, never advancing a cycle or mutating
//! the machine, so enabling the UI cannot perturb emulation (the gbtr golden
//! frame-hash gate stays green). See `docs/bgb-reference/` for the functional
//! spec these mirror.

pub mod disasm;
pub mod palette;
pub mod vram;

pub use disasm::{Insn, Syntax, decode, decode_with};
pub use palette::{cgb_palette_words, dmg_palette_shades, rgb555_to_rgb888};
pub use vram::{MapCell, Sprite, bg_map, oam_sprites, tile_pixels};
