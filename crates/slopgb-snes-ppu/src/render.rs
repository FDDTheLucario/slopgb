//! The BG scanline renderer (modes 0/1), authored from nocash *fullsnes*:
//! "2105h BGMODE", "2107h BGnSC", "210Bh BG12NBA", "VRAM 8x8 Pixel Tile
//! Data", "16x16 (and bigger) Tiles", and the BG-map entry layout. Modes
//! 2-7 (offset-per-tile, hires, mode 7) are unsupported — their BGs render
//! transparent (ceiling; nothing SGB-shaped needs them yet). Mosaic is not
//! modeled either.

use super::*;

/// Bit-spread: byte bit `b` lands in nibble `7-b` (bit 7 is the leftmost
/// pixel), so a plane byte expands to eight 1-bit pixel nibbles in screen
/// order — two (or four) spread planes OR together into eight color
/// indices at once.
pub(crate) const SPREAD: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut v = 0usize;
    while v < 256 {
        let mut b = 0;
        let mut acc = 0u32;
        while b < 8 {
            if v & 1 << b != 0 {
                acc |= 1 << ((7 - b) * 4);
            }
            b += 1;
        }
        t[v] = acc;
        v += 1;
    }
    t
};

/// The eight 2-bit (or 4-bit) indices of one char row, packed one nibble
/// per pixel in screen order: planes 0/1 from `w0`'s low/high bytes,
/// planes 2/3 from `w1`'s.
#[inline]
pub(crate) fn row_nibbles(w0: u16, w1: u16) -> u32 {
    SPREAD[usize::from(w0 & 0xFF)]
        | SPREAD[usize::from(w0 >> 8)] << 1
        | SPREAD[usize::from(w1 & 0xFF)] << 2
        | SPREAD[usize::from(w1 >> 8)] << 3
}

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
    /// merging are the frame assembler's job. Returns whether any pixel
    /// landed (the assembler skips this layer's rungs when nothing did).
    ///
    /// The line is walked in 8-pixel char-row runs (a run never crosses a
    /// char boundary: `vx & 7` is contiguous within it, and an X-flip
    /// mirrors the run onto a single 8-aligned block of the tile), so the
    /// map entry and the row's plane words load once per run instead of
    /// once per pixel.
    pub fn bg_line(&self, bg: usize, y: u16, out: &mut [Option<(u16, bool)>; 256]) -> bool {
        out.fill(None);
        let Some(bpp) = self.bg_bpp(bg) else {
            return false;
        };
        let tile16 = self.bgmode & 1 << (4 + bg) != 0;
        let map_base = usize::from(self.bgsc[bg] >> 2) << 10;
        let size = self.bgsc[bg] & 3;
        let char_base = usize::from(self.nba[bg / 2] >> (bg % 2 * 4) & 0xF) << 12;
        let words_per_tile = usize::from(bpp) * 4;
        let fine_mask = if tile16 { 15u16 } else { 7 };
        let shift = if tile16 { 4 } else { 3 };
        let vy = y.wrapping_add(self.vofs[bg]) & 0x3FF;
        // CGRAM base for color 1..N of a palette: mode 0 slices CGRAM per
        // BG (BG1/2/3/4 at 00h/20h/40h/60h — fullsnes CGRAM content);
        // otherwise palette * tile colors.
        let cg_base = |pal: usize| {
            if self.bgmode & 7 == 0 {
                bg * 0x20 + pal * 4
            } else {
                pal << bpp
            }
        };
        let mut wrote = false;
        let mut x = 0usize;
        while x < 256 {
            let vx = (x as u16).wrapping_add(self.hofs[bg]) & 0x3FF;
            let run = usize::from(8 - (vx & 7)).min(256 - x);
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
            let prio = entry & 0x2000 != 0;
            // Flips mirror the fine coordinates across the whole (8 or 16
            // pixel) tile, so a flipped 16x16 also swaps its 8x8 quadrants.
            let xflip = entry & 0x4000 != 0;
            let mut fx = vx & fine_mask;
            if xflip {
                fx = fine_mask - fx;
            }
            let mut fy = vy & fine_mask;
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
            let w0 = self.vram[row & 0x7FFF];
            let w1 = if bpp == 4 {
                self.vram[(row + 8) & 0x7FFF]
            } else {
                0
            };
            let nibbles = row_nibbles(w0, w1);
            let base = cg_base(usize::from(entry >> 10 & 7));
            // Within the run the char-space column steps +1 per screen
            // pixel, or -1 under X-flip (the mirror reverses direction).
            for k in 0..run {
                let cfx = if xflip { fx - k as u16 } else { fx + k as u16 };
                let idx = (nibbles >> (cfx * 4) & 0xF) as usize;
                if idx != 0 {
                    // color 0 is transparent
                    out[x + k] = Some((self.cgram[(base + idx) & 0xFF] & 0x7FFF, prio));
                    wrote = true;
                }
            }
            x += run;
        }
        wrote
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
