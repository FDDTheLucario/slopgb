//! SGB border composite: the 256×224 SNES surface (32×28 tiles of 8×8) with the
//! colorized 160×144 Game Boy screen composited as an inset at (48, 40). Border
//! tiles with color 0 are transparent over the GB area (the screen shows
//! through) and the SGB backdrop (palette-0 color 0) elsewhere. A faithful port
//! of SameBoy `GB_sgb_render`'s border loop (`Core/sgb.c`).

use super::*;

impl SgbView {
    /// The border is displayable once both a CHR_TRN (tiles) and a PCT_TRN
    /// (tilemap + palettes) have landed.
    fn border_ready(&self) -> bool {
        self.has_chr && self.has_pct
    }

    /// Recomposite `border_fb` from the tilemap/tiles/palettes and the current
    /// colorized GB screen `front`. Backdrop-fill first so unused (`0x300`) and
    /// skipped tiles show the SGB backdrop, then the GB inset, then the border
    /// tiles (color-0 tiles over the GB area stay transparent).
    fn composite(&mut self, front: &[u32; SCREEN_PIXELS]) {
        let backdrop = self.pal[0][0];
        self.border_fb.fill(backdrop);
        for gy in 0..SCREEN_H {
            let dst = (INSET_Y + gy) * BORDER_W + INSET_X;
            let src = gy * SCREEN_W;
            self.border_fb[dst..dst + SCREEN_W].copy_from_slice(&front[src..src + SCREEN_W]);
        }

        // Border palettes 4-7 (16 BGR555 colors each) live at raw offset 0x800.
        let mut border_colors = [0u32; 64];
        for (i, c) in border_colors.iter_mut().enumerate() {
            let off = 2048 + i * 2;
            *c = bgr555(self.border_raw[off], self.border_raw[off + 1]);
        }

        for tile_y in 0..28usize {
            for tile_x in 0..32usize {
                let gb_area = (6..26).contains(&tile_x) && (5..23).contains(&tile_y);
                let m = (tile_x + tile_y * 32) * 2;
                let entry =
                    u16::from(self.border_raw[m]) | (u16::from(self.border_raw[m + 1]) << 8);
                if entry & 0x300 != 0 {
                    continue; // unused tile: leave the backdrop / GB inset
                }
                let tile_idx = usize::from(entry & 0xFF);
                let palette = usize::from((entry >> 10) & 3);
                let flip_x = entry & 0x4000 != 0;
                let flip_y = entry & 0x8000 != 0;
                for y in 0..8usize {
                    let srcy = if flip_y { 7 - y } else { y };
                    let base = tile_idx * 32 + srcy * 2;
                    for x in 0..8usize {
                        let bit = 1u8 << if flip_x { x } else { 7 - x };
                        let color = u8::from(self.border_tiles[base] & bit != 0)
                            | (u8::from(self.border_tiles[base + 1] & bit != 0) << 1)
                            | (u8::from(self.border_tiles[base + 16] & bit != 0) << 2)
                            | (u8::from(self.border_tiles[base + 17] & bit != 0) << 3);
                        let idx = (tile_y * 8 + y) * BORDER_W + tile_x * 8 + x;
                        if color == 0 {
                            if !gb_area {
                                self.border_fb[idx] = backdrop;
                            }
                            // gb_area color-0: transparent — the GB inset stays.
                        } else {
                            self.border_fb[idx] = border_colors[usize::from(color) + palette * 16];
                        }
                    }
                }
            }
        }
    }

    pub(super) fn border_fb(&self) -> Option<&[u32; BORDER_PIXELS]> {
        self.border_ready().then_some(&*self.border_fb)
    }
}

impl Ppu {
    /// Recomposite the SGB border surface from the just-presented `front` frame.
    /// A no-op off SGB or before a CHR_TRN+PCT_TRN pair has landed. Called at the
    /// frame boundary and after a save-state load.
    pub(in crate::ppu) fn sgb_composite_border(&mut self) {
        let front = &self.front;
        let Some(s) = self.sgb.as_mut() else {
            return;
        };
        if !s.border_ready() {
            return;
        }
        s.composite(front);
    }

    /// The 256×224 SNES border surface with the GB screen composited as a
    /// 160×144 inset, or `None` until a CHR_TRN+PCT_TRN pair has landed (or off
    /// SGB). The frontend renders this in place of the bare 160×144 frame.
    pub(crate) fn sgb_border(&self) -> Option<&[u32; BORDER_PIXELS]> {
        self.sgb.as_ref().and_then(SgbView::border_fb)
    }
}
