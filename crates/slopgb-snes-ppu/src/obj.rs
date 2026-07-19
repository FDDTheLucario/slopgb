//! The OBJ (sprite) scanline renderer, authored from nocash *fullsnes*:
//! "2101h OBSEL", "SNES PPU Sprites (OBJs)", the OAM entry/high-table
//! layout, and the OBJ tile-number arithmetic ("no carry-outs from x+1 to
//! y, nor from y+1 to Y" — each 4-bit field wraps in place, bit 8 fixed).
//!
//! Ceilings: the STAT77 range/time-over *flags* are not surfaced (the
//! limits themselves are enforced); the X=256 range quirk is not modeled.

use super::*;

/// OBSEL bits 7-5: (small w, small h, large w, large h) — fullsnes 2101h
/// (6/7 are the undocumented non-square pairs).
const OBJ_SIZES: [(u16, u16, u16, u16); 8] = [
    (8, 8, 16, 16),
    (8, 8, 32, 32),
    (8, 8, 64, 64),
    (16, 16, 32, 32),
    (16, 16, 64, 64),
    (32, 32, 64, 64),
    (16, 32, 32, 64),
    (16, 32, 32, 32),
];

impl SnesPpu {
    /// Render the OBJ contribution to line `y`: per pixel the 15-bit CGRAM
    /// color + the OAM priority (0-3), `None` where transparent. Among
    /// overlapping sprites the one earliest in evaluation order (from the
    /// priority-rotation start) wins regardless of its priority bits — the
    /// bits only order OBJ against BG (fullsnes priority chart). The
    /// 32-sprites/line and 34-tiles/line limits drop the additional ones.
    pub fn obj_line(&self, y: u16, out: &mut [Option<(u16, u8)>; 256]) {
        out.fill(None);
        let base = usize::from(self.obsel & 7) << 13;
        let gap = usize::from(self.obsel >> 3 & 3) << 12;
        let (sw, sh, lw, lh) = OBJ_SIZES[usize::from(self.obsel >> 5)];
        // Priority rotation (fullsnes 2102h bit 15): evaluation starts at
        // OBJ #N (the reload's bits 7-1) instead of #0.
        let first = if self.oam_priority {
            usize::from(self.oam_reload >> 1) & 0x7F
        } else {
            0
        };
        let mut range = 0; // sprites intersecting this line (cap 32)
        let mut slots = 34; // 8-pixel tile fetches this line (cap 34)
        for i in 0..128 {
            let n = (first + i) & 0x7F;
            let e = &self.oam[n * 4..n * 4 + 4];
            let hi = self.oam[0x200 + n / 4] >> (n % 4 * 2);
            let (w, h) = if hi & 2 != 0 { (lw, lh) } else { (sw, sh) };
            // OAM Y is the sprite top's framebuffer row (display lines are
            // 1-based, OBJ SCREEN.Y is 1..224 — fullsnes); vertical
            // position wraps through 256.
            let row = y.wrapping_sub(u16::from(e[1])) & 0xFF;
            if row >= h {
                continue;
            }
            if range == 32 {
                break; // range over: the additional sprites are dropped
            }
            range += 1;
            let x9 = u16::from(e[0]) | u16::from(hi & 1) << 8;
            let sx = if x9 >= 256 {
                i32::from(x9) - 512
            } else {
                i32::from(x9)
            };
            let attr = e[3];
            let tile = u16::from(e[2]) | u16::from(attr & 1) << 8;
            let pal = usize::from(attr >> 1 & 7);
            let prio = attr >> 4 & 3;
            let fy = if attr & 0x80 != 0 { h - 1 - row } else { row };
            // The sub-tile number: y advances bits 7-4, x advances bits
            // 3-0, each wrapping in place with bit 8 fixed (fullsnes "no
            // carry-outs"; OBJ $1FF's right half is $1F0, its lower half
            // $10F).
            let trow = tile & 0x100 | (tile >> 4).wrapping_add(fy / 8) << 4 & 0xF0 | tile & 0xF;
            for chunk in 0..w / 8 {
                let on_screen = (0..8).any(|p| {
                    let x = sx + i32::from(chunk * 8 + p);
                    (0..256).contains(&x)
                });
                if !on_screen {
                    continue;
                }
                if slots == 0 {
                    return; // time over: no tile fetches left this line
                }
                slots -= 1;
                // An X-flip mirrors the chunk onto one 8-aligned source
                // block (src/8 is constant across it), so the sub-tile and
                // its two plane words load once per chunk; only the bit
                // column walks, descending normally, ascending when
                // flipped.
                let src0 = if attr & 0x40 != 0 {
                    w - 1 - chunk * 8
                } else {
                    chunk * 8
                };
                let t = trow & 0x1F0 | trow.wrapping_add(src0 / 8) & 0xF;
                let word = base
                    + usize::from(t) * 16
                    + if t >= 0x100 { gap } else { 0 }
                    + usize::from(fy & 7);
                let w0 = self.vram[word & 0x7FFF];
                let w1 = self.vram[(word + 8) & 0x7FFF];
                let nibbles = crate::render::row_nibbles(w0, w1);
                for p in 0..8u16 {
                    let x = sx + i32::from(chunk * 8 + p);
                    if !(0..256).contains(&x) {
                        continue;
                    }
                    let slot = &mut out[x as usize];
                    if slot.is_some() {
                        continue; // an earlier sprite already owns the pixel
                    }
                    let col = if attr & 0x40 != 0 {
                        (src0 - p) & 7
                    } else {
                        (src0 + p) & 7
                    };
                    let idx = (nibbles >> (col * 4) & 0xF) as usize;
                    if idx != 0 {
                        // OBJs are always 16-color; palettes live in the
                        // CGRAM OBJ half at 80h+ (fullsnes CGRAM indices).
                        *slot = Some((self.cgram[0x80 + pal * 16 + idx] & 0x7FFF, prio));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "obj_tests.rs"]
mod tests;
