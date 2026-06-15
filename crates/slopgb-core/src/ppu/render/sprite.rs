//! OBJ pipeline: OAM scan (dot-serial + freeze-glitch), sprite fetch, pixel mix/priority, CGB color lookup. Oracle: gbtr sprites/*, mooneye intr_2_mode0_timing_sprites, mealybug obj photos.

use super::*;

impl Ppu {
    /// One dot of the serial OAM scan: latch + evaluate the entry whose
    /// slot this dot is, if any. Called for every dot below mode-3 start
    /// (84) of a visible non-glitch line.
    pub(in crate::ppu) fn oam_scan_step(&mut self) {
        let off = scan_latch_dot(0);
        if self.dot < off || (self.dot - off) % 2 != 0 {
            return;
        }
        let i = (self.dot - off) / 2;
        if i >= 40 {
            return;
        }
        if i == 0 {
            self.render.n_sprites = 0;
        }
        self.oam_scan_entry(i as u8);
    }

    /// Run a whole line's scan at once: selection equivalent of the serial
    /// grid on an undisturbed line. Test/diagnostic helper only — the dot
    /// path goes through [`Self::oam_scan_step`].
    #[cfg(test)]
    pub(super) fn oam_scan(&mut self) {
        self.render.n_sprites = 0;
        for i in 0..40 {
            self.oam_scan_entry(i);
        }
    }

    /// Latch OAM entry `i` from the PPU's OAM view and select it for this
    /// line if its Y matches, in OAM order, by Y only (X — even 0 or ≥168
    /// — does not affect selection; it only affects fetching: see
    /// `intr_2_mode0_timing_sprites`), capped at 10.
    ///
    /// The view is not always real OAM:
    ///
    /// * While an OAM DMA transfer sits frozen mid-byte on MGB (HALT gates
    ///   the core clock the DMA controller runs on), every entry reads as
    ///   the same glitched sprite — fully characterized by
    ///   madness/mgb_oam_dma_halt_sprites.s, hardware-verified by its
    ///   author (see [`Self::mgb_dma_freeze_glitch_entry`]).
    /// * While the OAM DMA controller owns OAM — running *or* frozen by
    ///   HALT/STOP on the other models ("DMG: A different sprite ... CGB:
    ///   Checkerboard without sprites" — the asm gives no reference data
    ///   for their glitches) — the scan is disconnected and latches $FF, a
    ///   disabled sprite (gambatte memory.cpp startOamDma/endOamDma switch
    ///   the OamReader source to rdisabledRam; the dmg08-verified
    ///   oamdma/late_sp* and oamdma_late_halt_stat families pin the
    ///   selection loss per slot).
    fn oam_scan_entry(&mut self, i: u8) {
        let (y, x, tile, flags) = if self.model == Model::Mgb && self.dma_freeze.is_some() {
            match self.mgb_dma_freeze_glitch_entry() {
                Some(e) => e,
                None => return,
            }
        } else if self.oam_dma_active {
            (0xFF, 0xFF, 0xFF, 0xFF)
        } else {
            let b = usize::from(i) * 4;
            (
                self.oam[b],
                self.oam[b + 1],
                self.oam[b + 2],
                self.oam[b + 3],
            )
        };
        let h = if self.eff.lcdc & LCDC_OBJ_SIZE != 0 {
            16u16
        } else {
            8
        };
        let row = u16::from(self.ly) + 16;
        if self.render.n_sprites < 10 && row >= u16::from(y) && row < u16::from(y) + h {
            let n = usize::from(self.render.n_sprites);
            self.render.sprites[n] = Sprite {
                y,
                x,
                tile,
                flags,
                idx: i,
            };
            self.render.n_sprites += 1;
        }
    }

    /// The glitched entry every OAM slot reads as while an OAM DMA
    /// transfer sits frozen mid-byte on MGB, or `None` when the magic
    /// enable is absent. Everything here implements the hardware behavior
    /// documented in madness/mgb_oam_dma_halt_sprites.s:
    ///
    /// With `new` = the in-flight DMA source byte, `old` = the OAM byte it
    /// was about to replace and `next` = the OAM byte after that one, every
    /// OAM entry is seen as the same glitched sprite
    ///
    /// ```text
    /// Y: (old | new) & $FC      C: (old | new) & $FC
    /// X:  next | new            F:  next | new
    /// ```
    ///
    /// ("Why & $FC? I have no idea, but it seems that the low two bits are
    /// always 0"). Selection then proceeds as normal — Y match, 10-sprite
    /// cap — so a matching line gets its sprite slots filled with identical
    /// copies, which render as a single sprite shape (the asm's expected
    /// image shows exactly one).
    fn mgb_dma_freeze_glitch_entry(&self) -> Option<(u8, u8, u8, u8)> {
        let (index, new) = self.dma_freeze?;
        // "This is the data that somehow enables sprite rendering": without
        // at least one aligned magic entry in OAM, no sprite renders.
        if !oam_glitch_magic_enable(&self.oam) {
            return None;
        }
        // The interconnect caps the in-flight index at 159, but the pub
        // freeze API accepts any u8: out-of-range degrades to the
        // undriven-bus value like `next` below (matching `oam_dma_write`'s
        // bounds check) instead of panicking.
        let old = self.oam.get(usize::from(index)).copied().unwrap_or(0xFF);
        // The byte after the in-flight one. A freeze on the final byte
        // (index 159) has no successor; the asm does not pin that case and
        // $FF is the usual undriven-bus value.
        let next = self
            .oam
            .get(usize::from(index) + 1)
            .copied()
            .unwrap_or(0xFF);
        let y = (old | new) & 0xFC;
        let x = next | new;
        Some((y, x, y, x))
    }

    /// First-per-BG-tile sprite alignment penalty (Pan Docs OBJ penalty
    /// algorithm; verified against intr_2_mode0_timing_sprites).
    pub(super) fn sprite_penalty(&mut self, x: u8) -> u16 {
        let v = u16::from(x) + u16::from(self.eff.scx);
        let key = v >> 3;
        if self.render.penalty_tiles & (1u64 << key) != 0 {
            0
        } else {
            self.render.penalty_tiles |= 1u64 << key;
            5u16.saturating_sub(v & 7)
        }
    }

    /// Fetch sprite `i`'s row and merge it into the sprite FIFO.
    pub(super) fn fetch_sprite(&mut self, i: usize) {
        let s = self.render.sprites[i];
        let tall = self.eff.lcdc & LCDC_OBJ_SIZE != 0;
        let h: u8 = if tall { 16 } else { 8 };
        // Selection bounded the row by the height LCDC.2 held at OAM-scan
        // time (dot 80), but LCDC.2 is re-read here at fetch time: a game
        // clearing it (16 -> 8) mid-mode-3 can leave row >= h. Mask into the
        // current height (h is a power of two) — the hardware row counter
        // feeds the tile-data address through these low bits either way —
        // so the Y flip below cannot underflow.
        let mut row = self.ly.wrapping_add(16).wrapping_sub(s.y) & (h - 1);
        if s.flags & 0x40 != 0 {
            row = h - 1 - row; // Y flip.
        }
        let tile = if tall {
            // 8x16: bit 0 of the tile index is ignored (Pan Docs).
            (s.tile & 0xFE) + (row >> 3)
        } else {
            s.tile
        };
        let bank = if self.model.is_cgb() && s.flags & 0x08 != 0 {
            0x2000
        } else {
            0
        };
        let addr = bank + usize::from(tile) * 16 + usize::from(row & 7) * 2;
        let mut lo = self.vram[addr];
        let mut hi = self.vram[addr + 1];
        if s.flags & 0x20 != 0 {
            lo = lo.reverse_bits();
            hi = hi.reverse_bits();
        }
        let cgb = self.model.is_cgb();
        // CGB with OPRI bit 0 clear: lower OAM index wins regardless of X;
        // otherwise (DMG, or OPRI=1) earlier-fetched (= leftmost, then
        // lowest OAM index) sprites keep their pixels.
        let index_priority = cgb && self.opri & 1 == 0;
        for px in 0..8u8 {
            let screen = i16::from(s.x) - 8 + i16::from(px);
            let slot = screen - i16::from(self.render.lx);
            if !(0..8).contains(&slot) {
                continue;
            }
            let bit = 7 - px;
            let c = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1);
            let entry = &mut self.render.sp_fifo[slot as usize];
            let replace = if entry.color == 0 {
                true
            } else {
                index_priority && c != 0 && s.idx < entry.oam_idx
            };
            if replace {
                *entry = SpritePixel {
                    // Integration addition: in DMG compatibility mode the
                    // CGB PPU uses the DMG palette bit (OAM flag bit 4,
                    // selecting OBP0/OBP1 -> obj palette 0/1), not the CGB
                    // palette bits (Pan Docs "DMG compatibility mode").
                    palette: if cgb && !self.dmg_compat {
                        s.flags & 0x07
                    } else {
                        (s.flags >> 4) & 1
                    },
                    color: c,
                    bg_priority: s.flags & 0x80 != 0,
                    oam_idx: s.idx,
                };
            }
        }
    }

    pub(super) fn output_pixel(&mut self, bg_c: u8, bg_attr: u8) {
        // Shift the sprite FIFO in step with shipped pixels.
        let mut sp = self.render.sp_fifo[0];
        self.render.sp_fifo.copy_within(1.., 0);
        self.render.sp_fifo[7] = EMPTY_SPRITE_PIXEL;
        // LCDC.1 also gates sprite pixels at the mix: pixels already in
        // the FIFO stop showing on dots where the OBJ enable reads low
        // (mealybug m3_lcdc_obj_en_change: sprites fetched during the
        // prefill turn into background mid-glyph at the disable commit).
        // The DMG mixer samples the bit one dot ahead of the eff view
        // (the fetch-lead timing — the blob photos put each band's
        // suppression boundary one column left of the eff commit);
        // CGB-C samples eff (its leg is pixel-exact on eff).
        if self.eff.lcdc & LCDC_OBJ_ENABLE == 0 {
            sp = EMPTY_SPRITE_PIXEL;
        }

        let cgb = self.model.is_cgb();
        // DMG LCDC bit 0: BG and window disabled — they show as white
        // (color 0 for sprite priority purposes). DMG compatibility mode on
        // CGB behaves the same way (integration addition).
        let bg_off = (!cgb || self.dmg_compat) && self.eff.lcdc & LCDC_BG_ENABLE == 0;
        let bg_c = if bg_off { 0 } else { bg_c };

        let sprite_wins = sp.color != 0
            && if cgb {
                // CGB: BG color 0 always loses; LCDC bit 0 clear strips all
                // BG priority; else BG attribute bit 7 or OAM bit 7 wins.
                bg_c == 0
                    || self.eff.lcdc & LCDC_BG_ENABLE == 0
                    || !(bg_attr & 0x80 != 0 || sp.bg_priority)
            } else {
                !(sp.bg_priority && bg_c != 0)
            };

        let color = if sprite_wins {
            if cgb {
                // Integration addition: DMG compatibility mode remaps the
                // pixel through OBP0/OBP1 before the (boot-installed)
                // compat palette (Pan Docs "DMG compatibility mode").
                let c = if self.dmg_compat {
                    let obp = if sp.palette == 1 {
                        self.eff.obp1
                    } else {
                        self.eff.obp0
                    };
                    (obp >> (sp.color * 2)) & 3
                } else {
                    sp.color
                };
                self.cgb_color(&self.obj_pal_ram, sp.palette, c)
            } else {
                let obp = if sp.palette == 1 {
                    self.eff.obp1
                } else {
                    self.eff.obp0
                };
                self.dmg_palette[usize::from((obp >> (sp.color * 2)) & 3)]
            }
        } else if cgb {
            // Integration addition: compat mode remaps BG pixels through
            // BGP; BG attributes are all zero (VRAM bank 1 is locked), so
            // palette 0 is used either way.
            let c = if self.dmg_compat && !bg_off {
                (self.eff.bgp >> (bg_c * 2)) & 3
            } else {
                bg_c
            };
            self.cgb_color(&self.bg_pal_ram, bg_attr & 0x07, c)
        } else if bg_off {
            self.dmg_palette[0]
        } else {
            self.dmg_palette[usize::from((self.eff.bgp >> (bg_c * 2)) & 3)]
        };

        let idx = usize::from(self.ly) * SCREEN_W + usize::from(self.render.lx);
        self.back[idx] = color;
    }

    /// RGB555 palette RAM entry to XRGB8888: straight 5→8 bit expansion
    /// ((c << 3) | (c >> 2)), no color correction in the core.
    pub(super) fn cgb_color(&self, ram: &[u8; 64], palette: u8, color: u8) -> u32 {
        let i = usize::from(palette) * 8 + usize::from(color) * 2;
        let raw = u16::from(ram[i]) | (u16::from(ram[i + 1]) << 8);
        let expand = |c: u16| -> u32 { u32::from(((c << 3) | (c >> 2)) & 0xFF) };
        let r = expand(raw & 0x1F);
        let g = expand((raw >> 5) & 0x1F);
        let b = expand((raw >> 10) & 0x1F);
        (r << 16) | (g << 8) | b
    }
}
