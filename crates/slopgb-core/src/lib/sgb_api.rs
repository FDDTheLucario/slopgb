//! Super Game Boy accessors on [`GameBoy`]: the colorized border surface and
//! the SNES-side command seams (SOUND / SOU_TRN / OBJ_TRN / DATA_TRN /
//! DATA_SND / flags + JUMP) the Phase-3 S-DSP consumes, plus the opt-in SGB
//! audio BIOS loader.
//!
//! A second `impl GameBoy` block, split out of `lib.rs` to keep it under the
//! 1000-line cap. `use super::*` pulls in
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

    /// Supply the optional, user-provided **SGB BIOS** (the SGB cartridge's
    /// SNES-side ROM image) — the single BIOS entry point. Mirrors the opt-in
    /// boot-ROM plumbing ([`Self::new_with_boot`]): absent it, SGB audio is
    /// silent for the default sound bank, the original default border and the
    /// neutral palette stand, and output stays byte-identical.
    ///
    /// It feeds two things:
    /// - **Audio** — the image is handed to the APU exactly as before (a
    ///   self-uploaded driver still plays with no BIOS; the *default* driver
    ///   needs the image, but without a 65816 core it is stored, not executed).
    /// - **Border + title→palette** — the two `Ppu` seams
    ///   ([`Ppu::sgb_install_border`] / [`Ppu::sgb_apply_bios_palette`]).
    ///
    /// **The honest limit:** slopgb is high-level — it never runs the SNES
    /// 65816 — so it can neither *execute* the firmware to build the
    /// border/palette nor trust a raw, firmware-revision-specific offset (an
    /// unverifiable guess would ship a wrong border dressed up as right). The
    /// two locators below therefore only trust a payload found by a documented,
    /// checked structure; none is verifiable from a bare SNES image, so today
    /// they find nothing and the default border + neutral palette stand. The
    /// seams are the wired upgrade path — a checked locator drops into
    /// [`sgb_bios_border`] / [`sgb_bios_palette`] with no other change.
    ///
    /// A no-op off `Model::Sgb`/`Sgb2`. See [`crate::sgb::apu`],
    /// `docs/hardware-state/sgb-audio.md` and `docs/hardware-state/sgb.md` for
    /// exactly what does and does not happen with and without it.
    pub fn load_sgb_bios(&mut self, bios: &[u8]) {
        if let Some(apu) = self.sgb_apu.as_mut() {
            apu.load_bios(bios);
        }
        if let Some((chr0, chr1, pct)) = sgb_bios_border(bios) {
            self.bus.ppu_mut().sgb_install_border(&chr0, &chr1, &pct);
        }
        if let Some(table) = sgb_bios_palette(bios) {
            // The palette-by-title hook hashes *this cart's* header title
            // (0x0134..0x0144) to index the BIOS table.
            let title: Vec<u8> = (0x0134..0x0144).map(|a| self.debug_read(a)).collect();
            self.bus.ppu_mut().sgb_apply_bios_palette(&title, &table);
        }
    }

    /// Install an alternative SGB SNES-side audio coprocessor in place of the
    /// built-in [`sgb::apu::SgbApu`] — the injection seam a frontend/host uses to
    /// run a plugin-backed [`sgb::AudioCoprocessor`] (e.g. a combined 65C816 +
    /// SPC700 + S-DSP chip forwarding to a wasm plugin's `drain_pcm`).
    ///
    /// Only meaningful on `Model::Sgb`/`Sgb2`: off SGB the machine holds no audio
    /// coprocessor slot, so there is nothing to replace — the passed `cop` is
    /// dropped and `Dmg`/`Cgb` stay byte-identical (golden-safe). Like
    /// [`Self::debug_set_reg`] / load-state, this is an explicit user-initiated
    /// mutation, never taken on the passive frame loop.
    ///
    /// The incoming `cop` keeps whatever output rate it was built with; call
    /// [`Self::set_sample_rate`] afterwards to align it with the host (it
    /// propagates to the newly installed coprocessor).
    pub fn set_audio_coprocessor(&mut self, cop: Box<dyn sgb::AudioCoprocessor>) {
        if self.sgb_apu.is_some() {
            self.sgb_apu = Some(cop);
        }
    }

    /// Attach the built-in default SGB border surface to a non-SGB machine (the
    /// fallback for "GBC + initial SGB border" when the game uploads no border).
    /// Presentation-only: `frame()`/cycles are byte-identical to the same model
    /// without a border. The golden path never calls this. Idempotent.
    pub fn enable_sgb_border(&mut self) {
        self.bus.enable_sgb_border();
    }

    /// Run `rom` on an SGB machine until it uploads its own border (bgb's
    /// "initial SGB" phase), then snapshot that border. `None` if the game
    /// uploads no SGB border within `max_frames`. Pair with
    /// [`Self::install_sgb_border`] to show it on a CGB machine while the game
    /// renders in GBC color — the faithful "GBC + initial SGB border" mode.
    #[must_use]
    pub fn capture_initial_sgb_border(rom: &[u8], max_frames: u32) -> Option<SgbBorder> {
        let mut sgb = GameBoy::new(Model::Sgb, rom.to_vec()).ok()?;
        for _ in 0..max_frames {
            sgb.run_frame();
            if let Some((tiles, raw)) = sgb.bus.ppu().sgb_border_snapshot() {
                return Some(SgbBorder { tiles, raw });
            }
        }
        None
    }

    /// Install a border captured by [`Self::capture_initial_sgb_border`]: the
    /// machine keeps rendering in its native (CGB) mode, now with this SGB border
    /// around the screen. Idempotent (replaces any prior border).
    pub fn install_sgb_border(&mut self, border: &SgbBorder) {
        self.bus
            .install_sgb_border(border.tiles.clone(), border.raw.clone());
    }
}

/// Locate the SGB firmware's real default border (two 4096-byte SNES-4bpp tile
/// banks + the 2176-byte tilemap/palette payload — the CHR_TRN/PCT_TRN formats
/// [`Ppu::sgb_install_border`] takes) inside a user-supplied BIOS image.
///
/// Returns `None`: slopgb never runs the SNES 65816, so it cannot execute the
/// firmware to produce the border, and the raw payload offset is
/// firmware-revision-specific — trusting one blind would ship a wrong border.
/// A locator that first *validates* a documented structure drops in here; until
/// then nothing is trusted and the original default border stands.
fn sgb_bios_border(_bios: &[u8]) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    None
}

/// Locate the SGB firmware's title→palette table (four BGR555 colours per
/// entry) for [`Ppu::sgb_apply_bios_palette`]. Returns `None` for the same
/// reason as [`sgb_bios_border`]: the table's location is revision-specific and
/// unverifiable from a bare image, so the neutral DMG palette stands.
fn sgb_bios_palette(_bios: &[u8]) -> Option<Vec<[u16; 4]>> {
    None
}
