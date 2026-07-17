//! The BG scanline renderer (modes 0/1), authored from nocash *fullsnes*:
//! "2105h BGMODE", "2107h BGnSC", "210Bh BG12NBA", "VRAM 8x8 Pixel Tile
//! Data", "16x16 (and bigger) Tiles", and the BG-map entry layout. Modes
//! 2-7 (offset-per-tile, hires, mode 7) are unsupported — their BGs render
//! transparent (ceiling; nothing SGB-shaped needs them yet). Mosaic is not
//! modeled either.

use super::*;

impl SnesPpu {
    /// Bits per pixel for BG `bg` (0-based) in the current mode, `None`
    /// where the BG doesn't render (fullsnes 2105h mode table).
    fn bg_bpp(&self, bg: usize) -> Option<u16> {
        match (self.bgmode & 7, bg) {
            (0, 0..=3) => Some(2),
            (1, 0 | 1) => Some(4),
            (1, 2) => Some(2),
            _ => None,
        }
    }

    /// Render BG `bg`'s contribution to line `y`: per pixel the 15-bit
    /// CGRAM color + the map entry's priority bit, `None` where transparent
    /// (or the BG doesn't render in this mode). TM masking and priority
    /// merging are the frame assembler's job.
    pub fn bg_line(&self, bg: usize, y: u16, out: &mut [Option<(u16, bool)>; 256]) {
        out.fill(None);
        let Some(bpp) = self.bg_bpp(bg) else {
            return;
        };
        let tile16 = self.bgmode & 1 << (4 + bg) != 0;
        let map_base = usize::from(self.bgsc[bg] >> 2) << 10;
        let size = self.bgsc[bg] & 3;
        let char_base = usize::from(self.nba[bg / 2] >> (bg % 2 * 4) & 0xF) << 12;
        let words_per_tile = usize::from(bpp) * 4;
        let fine_mask = if tile16 { 15u16 } else { 7 };
        let shift = if tile16 { 4 } else { 3 };
        let vy = y.wrapping_add(self.vofs[bg]) & 0x3FF;
        for (x, slot) in out.iter_mut().enumerate() {
            let vx = (x as u16).wrapping_add(self.hofs[bg]) & 0x3FF;
            // Map entry: 32x32 entries per screen; screen-size bits place
            // extra 32x32 screens at +$400 (and +$800/$C00 for 64x64) —
            // fullsnes 2107h.
            let (tx, ty) = (vx >> shift, vy >> shift);
            let mut map = usize::from(ty & 31) << 5 | usize::from(tx & 31);
            if size & 1 != 0 && tx & 32 != 0 {
                map += 0x400;
            }
            if size & 2 != 0 && ty & 32 != 0 {
                map += 0x400 << (size & 1);
            }
            let entry = self.vram[(map_base + map) & 0x7FFF];
            let mut ch = usize::from(entry & 0x3FF);
            let pal = usize::from(entry >> 10 & 7);
            let prio = entry & 0x2000 != 0;
            // Flips mirror the fine coordinates across the whole (8 or 16
            // pixel) tile, so a flipped 16x16 also swaps its 8x8 quadrants.
            let mut fx = vx & fine_mask;
            let mut fy = vy & fine_mask;
            if entry & 0x4000 != 0 {
                fx = fine_mask - fx;
            }
            if entry & 0x8000 != 0 {
                fy = fine_mask - fy;
            }
            if tile16 {
                // The entry names the upper-left 8x8 char; right is N+1,
                // below is N+10h, BG chars carry across the 10-bit space
                // (fullsnes "16x16 (and bigger) Tiles").
                if fx >= 8 {
                    ch = (ch + 1) & 0x3FF;
                    fx -= 8;
                }
                if fy >= 8 {
                    ch = (ch + 0x10) & 0x3FF;
                    fy -= 8;
                }
            }
            // Tile rows: one word per row holds planes 0/1 (low/high byte);
            // 4bpp tiles append a second 8-word block for planes 2/3
            // (fullsnes "VRAM 8x8 Pixel Tile Data"). Bit 7 is leftmost.
            let row = char_base + ch * words_per_tile + usize::from(fy);
            let bit = 7 - fx;
            let w0 = self.vram[row & 0x7FFF];
            let mut idx = usize::from(w0 >> bit & 1 | (w0 >> 8 >> bit & 1) << 1);
            if bpp == 4 {
                let w1 = self.vram[(row + 8) & 0x7FFF];
                idx |= usize::from((w1 >> bit & 1) << 2 | (w1 >> 8 >> bit & 1) << 3);
            }
            if idx == 0 {
                continue; // color 0 is transparent
            }
            // CGRAM index: mode 0 slices CGRAM per BG (BG1/2/3/4 palettes
            // at 00h/20h/40h/60h — fullsnes CGRAM content); otherwise
            // palette * tile colors.
            let cg = if self.bgmode & 7 == 0 {
                bg * 0x20 + pal * 4 + idx
            } else {
                pal * usize::from(1u16 << bpp) + idx
            };
            *slot = Some((self.cgram[cg & 0xFF] & 0x7FFF, prio));
        }
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
