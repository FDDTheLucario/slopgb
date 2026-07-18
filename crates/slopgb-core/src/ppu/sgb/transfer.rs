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
    /// Latch a `*_TRN` destination (`TR_*`) and open its capture window: the
    /// screen is captured [`TRN_CAPTURE_DELAY`] cycles later, by
    /// [`Self::tick_trn`]. That models the real capture clock — the SNES
    /// side's own following frame. The GB's line-144 boundary is wrong (an
    /// LCD-off window can skip it entirely, silently losing a latched
    /// screen), and command time is wrong too (a game may still be streaming
    /// the payload when the command completes — Space Invaders sends DATA_TRN
    /// mid-redraw and relies on the following-frame capture).
    pub(super) fn latch_transfer(&mut self, dest: u8) {
        if self.pending_transfer.is_some() {
            // A second command inside an open window: consume the pending
            // capture rather than losing it.
            if std::env::var_os("SLOPGB_TRNDBG").is_some() {
                eprintln!("TRNDBG latch collision: early capture");
            }
            self.run_pending_transfer();
        }
        self.pending_transfer = Some(dest);
        self.trn_countdown = TRN_CAPTURE_DELAY;
    }

    /// Advance the `*_TRN` capture clock by `cycles` (ticked from the machine
    /// step on SGB models, whatever the GB LCD is doing); an expiring window
    /// captures the screen.
    pub(crate) fn tick_trn(&mut self, cycles: u32) {
        if self.trn_countdown > 0 {
            self.trn_countdown = self.trn_countdown.saturating_sub(cycles);
            if self.trn_countdown == 0 {
                self.run_pending_transfer();
            }
        }
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
                self.fade_pending = true; // cross-fade the new border in
            }
            TR_PCT => {
                let d = decode_tiles(&self.shade_buf, 136); // 2176 B = map + palettes
                self.border_raw.copy_from_slice(&d[..2176]);
                self.has_pct = true;
                self.fade_pending = true; // cross-fade the new border in
            }
            TR_OBJ => self.obj_data = Some(capture4096(&self.shade_buf)),
            TR_SOU => self.sou_trn = Some(capture4096(&self.shade_buf)),
            TR_DATA => self.data_trn = Some(capture4096(&self.shade_buf)),
            _ => {}
        }
    }

    /// Stream one completed 8-line band of the rendered screen into the
    /// ICD2 character-row queue: 20 tiles × 16 bytes of standard GB 2bpp
    /// (fullsnes "SGB Port 7800h" — the format the SNES DMAs straight into
    /// VRAM). `band` is the character row, 0-17.
    pub(super) fn stream_char_row(&mut self, band: u8) {
        let mut data = Box::new([0u8; 320]);
        let y0 = usize::from(band) * 8;
        for tile in 0..20 {
            for ry in 0..8 {
                let (mut lo, mut hi) = (0u8, 0u8);
                for x in 0..8 {
                    let px = self.shade_buf[(y0 + ry) * 160 + tile * 8 + x] & 3;
                    lo |= (px & 1) << (7 - x);
                    hi |= (px >> 1) << (7 - x);
                }
                data[tile * 16 + ry * 2] = lo;
                data[tile * 16 + ry * 2 + 1] = hi;
            }
        }
        if self.char_rows.len() >= 8 {
            self.char_rows.pop_front();
        }
        self.char_rows.push_back((band, data));
    }

    pub(super) fn take_char_row(&mut self) -> Option<(u8, Box<[u8; 320]>)> {
        self.char_rows.pop_front()
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
    /// Advance the SGB `*_TRN` capture window (machine-clocked; see
    /// [`SgbView::tick_trn`]). No-op off SGB.
    pub(crate) fn sgb_tick_trn(&mut self, cycles: u32) {
        if let Some(s) = self.sgb.as_mut() {
            s.tick_trn(cycles);
        }
    }

    /// At a line-8k boundary on SGB, stream the just-completed character
    /// row (the ICD2 `$7800` feed). No-op off SGB — golden-safe.
    pub(crate) fn sgb_stream_char_row(&mut self, line: u8) {
        if line % 8 == 0 && (8..=144).contains(&line) {
            if let Some(s) = self.sgb.as_mut() {
                s.stream_char_row(line / 8 - 1);
            }
        }
    }

    /// Drain one streamed ICD2 character row (`(row 0-17, 320 bytes)`).
    pub(crate) fn sgb_take_char_row(&mut self) -> Option<(u8, Box<[u8; 320]>)> {
        self.sgb.as_mut().and_then(SgbView::take_char_row)
    }

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
