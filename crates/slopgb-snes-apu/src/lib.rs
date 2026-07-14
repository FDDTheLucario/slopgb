//! SNES-side audio subsystem for the Super Game Boy: the [`spc700`] S-SMP CPU
//! and the [`dsp`] S-DSP sample synthesizer, plus the little-endian [`state`]
//! serializer they save through.
//!
//! Extracted from `slopgb-core` so the exact same logic backs both the built-in
//! SGB audio path (core's `SgbApu` wires an `Spc700` + `SDsp` off the Game Boy
//! clock) and a wasm coprocessor plugin (compiled to `wasm32`) — no
//! duplication. Std-only, `forbid(unsafe_code)`; it does not touch the Game Boy
//! core. See `docs/hardware-state/sgb-audio.md` + `spc700.md`.

pub mod dsp;
pub mod spc700;
pub mod state;

pub use state::StateError;
