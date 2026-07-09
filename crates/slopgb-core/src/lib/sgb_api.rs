//! Super Game Boy accessors on [`GameBoy`]: the colorized border surface and
//! the SNES-side command seams (SOUND / SOU_TRN / OBJ_TRN / DATA_TRN /
//! DATA_SND / flags + JUMP) the Phase-3 S-DSP consumes, plus the opt-in SGB
//! audio BIOS loader.
//!
//! A second `impl GameBoy` block, split out of `lib.rs` to keep it under the
//! 1000-line cap (see `docs/tdd-split-plan.md`). `use super::*` pulls in
//! `GameBoy`, `SgbSound`, `SgbFlags`, and the border constants; as a child
//! module it reaches `GameBoy`'s private `bus`/`sgb_apu` fields directly.
//!
//! All of this is `Model::Sgb`/`Sgb2`-scoped: off SGB the seams return `None`
//! and the audio path is inert, so `Dmg`/`Cgb` output stays byte-identical
//! (the golden-safe law).

use super::*;

impl GameBoy {
    /// The 256×224 SGB border surface (32×28 tiles) with the colorized 160×144
    /// GB screen composited as an inset at (48, 40), or `None` until a
    /// CHR_TRN+PCT_TRN border pair has landed (and always `None` off
    /// `Model::Sgb`/`Sgb2`). The frontend renders this in place of [`Self::frame`]
    /// when present. ([`Self::frame`] itself stays 160×144 — the golden hash reads it.)
    pub fn sgb_border(&self) -> Option<&[u32; SGB_BORDER_PIXELS]> {
        self.bus.ppu().sgb_border()
    }

    /// Drain one queued SGB SOUND ($08) effect event — the Phase-3 S-DSP seam.
    /// `None` off SGB or when the queue is empty.
    pub fn sgb_take_sound_event(&mut self) -> Option<SgbSound> {
        self.bus.ppu_mut().sgb_take_sound_event()
    }

    /// The most recent SOU_TRN ($09) SPC700 program upload (4096 bytes captured
    /// from the screen), or `None`. Phase-3 S-DSP seam.
    pub fn sgb_sou_trn_data(&self) -> Option<&[u8]> {
        self.bus.ppu().sgb_sou_trn_data()
    }

    /// The most recent OBJ_TRN ($18) payload (SGB OBJ palettes / attributes), or
    /// `None`. Phase-2/3 seam.
    pub fn sgb_obj_trn_data(&self) -> Option<&[u8]> {
        self.bus.ppu().sgb_obj_trn_data()
    }

    /// The most recent DATA_TRN ($10) payload destined for SNES RAM, or `None`.
    /// Phase-2/3 seam.
    pub fn sgb_data_trn_data(&self) -> Option<&[u8]> {
        self.bus.ppu().sgb_data_trn_data()
    }

    /// Drain one queued DATA_SND ($0F) inline SNES-RAM write, or `None`.
    /// Phase-2/3 seam.
    pub fn sgb_take_data_snd(&mut self) -> Option<Vec<u8>> {
        self.bus.ppu_mut().sgb_take_data_snd()
    }

    /// The current SGB flag / JUMP state, or `None` off SGB. Read-only.
    pub fn sgb_flags(&self) -> Option<SgbFlags> {
        self.bus.ppu().sgb_flags()
    }

    /// Supply the optional, user-provided **SGB audio BIOS** (the SGB
    /// cartridge's SNES-side ROM image). Mirrors the opt-in boot-ROM plumbing
    /// ([`Self::new_with_boot`], `docs/bootrom-plan.md`): absent it, SGB audio
    /// is silent for the default sound bank but every other subsystem works and
    /// output stays byte-identical.
    ///
    /// A no-op off `Model::Sgb`/`Sgb2`. See [`crate::sgb::apu`] and
    /// `docs/hardware-state/sgb-audio.md` for exactly what does and does not
    /// produce sound with and without it.
    pub fn load_sgb_bios(&mut self, bios: &[u8]) {
        if let Some(apu) = self.sgb_apu.as_mut() {
            apu.load_bios(bios);
        }
    }
}
