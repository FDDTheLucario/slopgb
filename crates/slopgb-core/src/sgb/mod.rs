//! Super Game Boy SNES-side subsystems.
//!
//! The SGB cartridge embeds an SNES: a Super Famicom running the Game Boy in a
//! window. slopgb needs the SNES audio path for SGB sound. This module holds the
//! [`spc700`] APU CPU, the [`dsp`] S-DSP synthesizer, and the [`apu`] seam that
//! clocks both off the Game Boy's cycle stream and routes the SGB sound
//! commands. It is self-contained and does not touch the Game Boy core.

pub mod spc700;

pub(crate) mod apu;
pub(crate) mod dsp;
