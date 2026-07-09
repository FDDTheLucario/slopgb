//! SGB VRAM transfers: the `*_TRN` commands capture the *rendered* Game Boy
//! screen (never VRAM) and read its 2-bit shades as packed 4bpp data — 4096
//! bytes per full-screen capture (Pan Docs "SGB Functions — VRAM Transfer";
//! SameBoy `GB_sgb_render`'s `pixel_to_bits` packing). A command latches a
//! destination; the capture + route happen at the next frame boundary, when the
//! just-rendered [`SgbView::shade_buf`] is complete.

use super::*;

/// Decode `n_tiles` 8×8 tiles of the captured screen into `n_tiles * 16` bytes
/// of standard 2bpp tile data (SameBoy `pixel_to_bits`): the screen is read as
/// a 20-tile-wide grid, each tile row → two bitplane bytes (low then high, x=0
/// = bit 7). This is the universal representation every consumer then reads.
fn decode_tiles(shade: &[u8; SCREEN_PIXELS], n_tiles: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n_tiles * 16);
    for tile in 0..n_tiles {
        let tx = (tile % 20) * 8;
        let ty = (tile / 20) * 8;
        for y in 0..8 {
            let (mut lo, mut hi) = (0u8, 0u8);
            for x in 0..8 {
                let px = shade.get((ty + y) * 160 + tx + x).copied().unwrap_or(0) & 3;
                lo |= (px & 1) << (7 - x);
                hi |= (px >> 1) << (7 - x);
            }
            out.push(lo);
            out.push(hi);
        }
    }
    out
}

impl SgbView {
    /// Latch a `*_TRN` destination (`TR_*`); the capture happens at the next
    /// frame boundary via [`Self::run_pending_transfer`]. Last write wins if two
    /// transfers are requested in the same frame.
    pub(super) fn latch_transfer(&mut self, dest: u8) {
        self.pending_transfer = Some(dest);
    }

    /// Consume a pending `*_TRN`: decode the just-rendered screen and route the
    /// 4096 bytes (2176 for PCT_TRN) into the destination buffer. Called once
    /// per frame boundary; a no-op when nothing is pending.
    pub(super) fn run_pending_transfer(&mut self) {
        let Some(dest) = self.pending_transfer.take() else {
            return;
        };
        match dest {
            TR_PAL => {
                let d = decode_tiles(&self.shade_buf, 256); // 4096 B = 2048 colors
                self.ram_palettes.copy_from_slice(&d[..4096]);
            }
            TR_ATTR => {
                let d = decode_tiles(&self.shade_buf, 254); // 4064 B; files use 4050
                self.attr_files.copy_from_slice(&d[..4050]);
            }
            TR_CHR0 | TR_CHR1 => {
                let d = decode_tiles(&self.shade_buf, 256); // 4096 B = 128 SNES tiles
                let bank = if dest == TR_CHR1 { 4096 } else { 0 };
                self.border_tiles[bank..bank + 4096].copy_from_slice(&d[..4096]);
                self.has_chr = true;
            }
            TR_PCT => {
                let d = decode_tiles(&self.shade_buf, 136); // 2176 B = map + palettes
                self.border_raw.copy_from_slice(&d[..2176]);
                self.has_pct = true;
            }
            TR_OBJ => self.obj_data = Some(capture4096(&self.shade_buf)),
            TR_SOU => self.sou_trn = Some(capture4096(&self.shade_buf)),
            TR_DATA => self.data_trn = Some(capture4096(&self.shade_buf)),
            _ => {}
        }
    }

    pub(super) fn take_sound_event(&mut self) -> Option<SgbSound> {
        if self.sound_events.is_empty() {
            None
        } else {
            Some(self.sound_events.remove(0))
        }
    }

    pub(super) fn take_data_snd(&mut self) -> Option<Vec<u8>> {
        if self.data_snd.is_empty() {
            None
        } else {
            Some(self.data_snd.remove(0))
        }
    }

    pub(super) fn sou_trn_data(&self) -> Option<&[u8]> {
        self.sou_trn.as_deref().map(|b| &b[..])
    }

    pub(super) fn obj_data(&self) -> Option<&[u8]> {
        self.obj_data.as_deref().map(|b| &b[..])
    }

    pub(super) fn data_trn_data(&self) -> Option<&[u8]> {
        self.data_trn.as_deref().map(|b| &b[..])
    }

    pub(super) fn flags(&self) -> crate::SgbFlags {
        crate::SgbFlags {
            atrc_en: self.atrc_en,
            test_en: self.test_en,
            icon_en: self.icon_en,
            pal_pri: self.pal_pri,
            jump: self.jump,
        }
    }
}

fn capture4096(shade: &[u8; SCREEN_PIXELS]) -> Box<[u8; 4096]> {
    decode_tiles(shade, 256)[..4096]
        .to_vec()
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

impl Ppu {
    /// Drain one queued SGB SOUND ($08) effect event. The Phase-3 S-DSP seam:
    /// the host pulls these and feeds them to the sound engine. `None` off SGB
    /// or when the queue is empty. (Pan Docs "SGB Command $08 — SOUND".)
    pub(crate) fn sgb_take_sound_event(&mut self) -> Option<SgbSound> {
        self.sgb.as_mut().and_then(SgbView::take_sound_event)
    }

    /// The most recent SOU_TRN ($09) SPC700 program upload (4096 bytes), or
    /// `None`. Phase-3 S-DSP seam.
    pub(crate) fn sgb_sou_trn_data(&self) -> Option<&[u8]> {
        self.sgb.as_ref().and_then(SgbView::sou_trn_data)
    }

    /// The most recent OBJ_TRN ($18) payload (SGB OBJ palettes/attributes).
    pub(crate) fn sgb_obj_trn_data(&self) -> Option<&[u8]> {
        self.sgb.as_ref().and_then(SgbView::obj_data)
    }

    /// The most recent DATA_TRN ($10) payload destined for SNES RAM.
    pub(crate) fn sgb_data_trn_data(&self) -> Option<&[u8]> {
        self.sgb.as_ref().and_then(SgbView::data_trn_data)
    }

    /// Drain one queued DATA_SND ($0F) inline SNES-RAM write. Phase-2/3 seam.
    pub(crate) fn sgb_take_data_snd(&mut self) -> Option<Vec<u8>> {
        self.sgb.as_mut().and_then(SgbView::take_data_snd)
    }

    /// The current SGB flag/JUMP state (ATRC_EN/TEST_EN/ICON_EN/PAL_PRI + JUMP
    /// target), or `None` off SGB.
    pub(crate) fn sgb_flags(&self) -> Option<crate::SgbFlags> {
        self.sgb.as_ref().map(SgbView::flags)
    }
}
