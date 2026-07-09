//! Super Game Boy SNES-side subsystems.
//!
//! The SGB cartridge embeds an SNES: a Super Famicom running the Game Boy in a
//! window. slopgb needs the SNES audio path for SGB sound. Phase 2 (here) is the
//! [`spc700`] APU CPU; Phase 3 adds the S-DSP and wires both to the SGB command
//! stream. This module is self-contained and does not touch the Game Boy core.

pub mod spc700;
