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

use crate::interconnect::Interconnect;

/// The SGB SNES-side audio coprocessor (SPC700 + S-DSP), abstracted behind a
/// trait so the built-in [`apu::SgbApu`] can be swapped for an alternative
/// implementation (e.g. one backed by a wasm coprocessor plugin) without
/// touching `GameBoy`.
///
/// Only ever held on `Model::Sgb`/`Sgb2`; `Dmg`/`Cgb` never construct one, so
/// those paths never touch this trait and stay byte-identical (the golden-safe
/// law). The built-in `SgbApu` is the default and only implementation today.
pub(crate) trait AudioCoprocessor {
    /// Advance the chip by `gb_cycles` Game Boy T-cycles, emitting output-rate
    /// samples.
    fn clock(&mut self, gb_cycles: u64);

    /// Drain the SGB sound commands the Game Boy queued on `bus` and apply them
    /// to the chip (SOUND / SOU_TRN / DATA_TRN / JUMP).
    fn poll(&mut self, bus: &mut Interconnect);

    /// Add the pending SNES-side samples into the Game Boy samples just drained,
    /// sample-for-sample.
    fn mix_into(&mut self, out: &mut [(f32, f32)]);

    /// Retarget the output sample rate (Hz) to mirror the Game Boy APU.
    fn set_output_rate(&mut self, hz: u32);

    /// Store an optional user-supplied SGB audio BIOS (SNES ROM image).
    fn load_bios(&mut self, bios: &[u8]);

    /// Serialize chip state into a save state.
    fn write_state(&self, w: &mut crate::state::Writer);

    /// Restore chip state from a save state.
    fn read_state(&mut self, r: &mut crate::state::Reader<'_>) -> Result<(), crate::StateError>;

    /// Deep-clone into a fresh box (trait objects can't derive `Clone`), for the
    /// atomic save-state restore that clones the whole `GameBoy`.
    fn clone_box(&self) -> Box<dyn AudioCoprocessor>;
}
