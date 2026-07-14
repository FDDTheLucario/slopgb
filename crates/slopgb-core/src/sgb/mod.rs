//! Super Game Boy SNES-side subsystems.
//!
//! The SGB cartridge embeds an SNES: a Super Famicom running the Game Boy in a
//! window. slopgb needs the SNES audio path for SGB sound. This module holds the
//! [`spc700`] APU CPU, the [`dsp`] S-DSP synthesizer, and the [`apu`] seam that
//! clocks both off the Game Boy's cycle stream and routes the SGB sound
//! commands. It is self-contained and does not touch the Game Boy core.

// The SPC700 (S-SMP) CPU and the S-DSP synthesizer live in the shared
// `slopgb-snes-apu` crate — the same logic backs both the built-in path here and
// a wasm coprocessor plugin. Re-exported so `crate::sgb::spc700`/`::dsp` keep
// naming them unchanged.
pub(crate) use slopgb_snes_apu::dsp;
pub use slopgb_snes_apu::spc700;

pub(crate) mod apu;

use crate::interconnect::Interconnect;
use crate::{SgbFlags, SgbSound};

/// The SGB SNES-side sound commands the Game Boy queues each step, handed to an
/// [`AudioCoprocessor`] without leaking the core-private bus. `GameBoy` drains
/// the PPU's SGB command seams through this trait; an out-of-core coprocessor
/// (e.g. a plugin adapter) receives it as a `&mut dyn` and pulls exactly the
/// commands the built-in [`apu::SgbApu`] does — SOUND / DATA_SND / SOU_TRN /
/// DATA_TRN / flags+JUMP.
pub trait SgbCommandSource {
    /// Drain one queued SOUND ($08) effect event, or `None` when the queue is
    /// empty.
    fn take_sound_event(&mut self) -> Option<SgbSound>;

    /// Drain one queued DATA_SND ($0F) inline SNES-RAM write, or `None`.
    fn take_data_snd(&mut self) -> Option<Vec<u8>>;

    /// The most recent SOU_TRN ($09) SPC700 program upload (it persists between
    /// transfers, so consumers edge-detect), or `None`.
    fn sou_trn_data(&self) -> Option<&[u8]>;

    /// The most recent DATA_TRN ($10) payload destined for SNES RAM, or `None`.
    fn data_trn_data(&self) -> Option<&[u8]>;

    /// The current SGB flag / JUMP snapshot, or `None`.
    fn flags(&self) -> Option<SgbFlags>;
}

/// The Game Boy bus is the live command source, forwarding to the PPU's SGB
/// command seams. `Interconnect` stays crate-private, so the public
/// `SgbCommandSource` trait object is the only handle an out-of-core
/// coprocessor sees — the bus type never leaks.
impl SgbCommandSource for Interconnect {
    fn take_sound_event(&mut self) -> Option<SgbSound> {
        self.ppu_mut().sgb_take_sound_event()
    }
    fn take_data_snd(&mut self) -> Option<Vec<u8>> {
        self.ppu_mut().sgb_take_data_snd()
    }
    fn sou_trn_data(&self) -> Option<&[u8]> {
        self.ppu().sgb_sou_trn_data()
    }
    fn data_trn_data(&self) -> Option<&[u8]> {
        self.ppu().sgb_data_trn_data()
    }
    fn flags(&self) -> Option<SgbFlags> {
        self.ppu().sgb_flags()
    }
}

/// The SGB SNES-side audio coprocessor (SPC700 + S-DSP), abstracted behind a
/// trait so the built-in [`apu::SgbApu`] can be swapped for an alternative
/// implementation (e.g. one backed by a wasm coprocessor plugin) via
/// [`crate::GameBoy::set_audio_coprocessor`] without touching `GameBoy`.
///
/// The trait is bus-agnostic: `poll` takes a [`SgbCommandSource`] instead of the
/// core-private `Interconnect`, so it can be implemented outside `slopgb-core`.
///
/// Only ever held on `Model::Sgb`/`Sgb2`; `Dmg`/`Cgb` never construct one, so
/// those paths never touch this trait and stay byte-identical (the golden-safe
/// law). The built-in `SgbApu` is the default implementation.
pub trait AudioCoprocessor {
    /// Advance the chip by `gb_cycles` Game Boy T-cycles, emitting output-rate
    /// samples.
    fn clock(&mut self, gb_cycles: u64);

    /// Drain the SGB sound commands the Game Boy queued (via `cmds`) and apply
    /// them to the chip (SOUND / SOU_TRN / DATA_TRN / DATA_SND / JUMP).
    fn poll(&mut self, cmds: &mut dyn SgbCommandSource);

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
