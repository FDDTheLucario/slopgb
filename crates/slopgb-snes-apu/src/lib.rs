//! SNES-side audio subsystem for the Super Game Boy: the [`spc700`] S-SMP CPU
//! and the [`dsp`] S-DSP sample synthesizer, plus the little-endian [`state`]
//! serializer they save through.
//!
//! The chip cores the `slopgb-spc700-plugin` wasm coprocessor is built from
//! (`slopgb-core` itself emulates no SNES chip). Std-only,
//! `forbid(unsafe_code)`; it does not touch the Game Boy core. See
//! `docs/hardware-state/sgb-audio.md` + `spc700.md`.

pub mod dsp;
pub mod spc700;
pub mod spc_file;
pub mod state;

pub use spc_file::{SPC_FILE_LEN, build_spc_file};
pub use state::StateError;
