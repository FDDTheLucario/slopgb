//! VRAM capture for the MCP `vram` tool: builds a plain XRGB8888 bitmap for one
//! of the VRAM-viewer views (`bg` / `win` / `tile0` / `tile1` / `oam` /
//! `palette`) out of the core's read-only decoders (`debug::tile_pixels` /
//! `bg_map` / `oam_sprites` / palette words). It reuses the *decoders*, not the
//! window renderer (that draws chrome onto the live canvas), so the output is a
//! clean image the PNG encoder can serialize. `bg` / `win` are game-paletted;
//! the Tiles views use a neutral grey ramp (as bgb does).

use slopgb_core::GameBoy;
use slopgb_core::debug::{
    bg_map, bg_tile_index, cgb_palette_words, dmg_palette_shades, oam_sprites, rgb555_to_rgb888,
    tile_pixels,
};

/// A captured bitmap: `w×h` XRGB8888 (top-down, row-major) — the format the PNG
/// encoder and `GameBoy::frame` share.
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

/// The neutral 4-grey ramp — the "unpaletted" view, and what the Tiles views
/// always use (bgb never game-palettes the Tiles tab).
const GREYS: [u32; 4] = [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000];

fn xrgb(word: u16) -> u32 {
    let (r, g, b) = rgb555_to_rgb888(word);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

fn dmg_pal(reg: u8) -> [u32; 4] {
    dmg_palette_shades(reg).map(|s| GREYS[s as usize])
}

impl Bitmap {
    fn new(w: usize, h: usize) -> Self {
        Bitmap {
            w,
            h,
            px: vec![0; w * h],
        }
    }

    /// Blit an 8×8 index tile at `(ox, oy)` through `pal`, honoring x/y flip.
    fn blit(
        &mut self,
        ox: usize,
        oy: usize,
        tile: &[[u8; 8]; 8],
        pal: &[u32; 4],
        xflip: bool,
        yflip: bool,
    ) {
        for y in 0..8 {
            for x in 0..8 {
                let sy = if yflip { 7 - y } else { y };
                let sx = if xflip { 7 - x } else { x };
                let (px, py) = (ox + x, oy + y);
                if px < self.w && py < self.h {
                    self.px[py * self.w + px] = pal[tile[sy][sx] as usize];
                }
            }
        }
    }

    fn fill(&mut self, ox: usize, oy: usize, w: usize, h: usize, color: u32) {
        for y in oy..(oy + h).min(self.h) {
            for x in ox..(ox + w).min(self.w) {
                self.px[y * self.w + x] = color;
            }
        }
    }
}

/// Capture the named view, or `Err` for an unknown name. `no_palette` forces the
/// neutral grey ramp on the game-paletted views (bg/win/oam) — the raw tile
/// pixels, ignoring the live BG/OBJ palettes (the Tiles views are always grey).
pub fn capture(gb: &GameBoy, view: &str, no_palette: bool) -> Result<Bitmap, String> {
    match view {
        "tile0" => Ok(tiles(gb, 0)),
        "tile1" => Ok(tiles(gb, 1)),
        "bg" => Ok(tilemap(gb, 0x08, no_palette)), // LCDC bit3 selects the BG map base
        "win" => Ok(tilemap(gb, 0x40, no_palette)), // LCDC bit6 selects the window map base
        "oam" => Ok(oam(gb, no_palette)),
        "palette" => Ok(palette(gb)),
        other => Err(format!(
            "unknown vram view '{other}' — want bg|win|tile0|tile1|oam|palette|palreg"
        )),
    }
}

/// The palette registers as text (the `palreg` view): DMG BGP/OBP0/OBP1 shade
/// maps, or the CGB 8 BG + 8 OBJ palettes as raw BGR555 words + the auto-index
/// registers. Text because a colour word is more useful to read than a swatch.
pub fn palreg(gb: &GameBoy) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if gb.model().is_cgb() {
        let (bg, obj) = gb.cgb_palette_ram();
        let _ = writeln!(
            out,
            "CGB palettes  BGPI=${:02X} OBPI=${:02X}",
            gb.debug_read(0xFF68),
            gb.debug_read(0xFF6A)
        );
        for (tag, cram) in [("BG", bg), ("OB", obj)] {
            for pal in 0..8 {
                let w = cgb_palette_words(cram, pal);
                let _ = writeln!(
                    out,
                    "{tag}{pal}  {:04X} {:04X} {:04X} {:04X}",
                    w[0], w[1], w[2], w[3]
                );
            }
        }
    } else {
        let _ = writeln!(out, "DMG palettes (shade per colour id 0-3)");
        for (name, reg) in [("BGP ", 0xFF47u16), ("OBP0", 0xFF48), ("OBP1", 0xFF49)] {
            let v = gb.debug_read(reg);
            let s = dmg_palette_shades(v);
            let _ = writeln!(out, "{name} ${v:02X}  {} {} {} {}", s[0], s[1], s[2], s[3]);
        }
    }
    out
}

/// The 384 tiles of one VRAM bank, 16 across (128×192), grey ramp.
fn tiles(gb: &GameBoy, bank: usize) -> Bitmap {
    let vram = gb.vram();
    let mut bmp = Bitmap::new(128, 192);
    for t in 0..384 {
        let tile = tile_pixels(vram, bank, t);
        bmp.blit((t % 16) * 8, (t / 16) * 8, &tile, &GREYS, false, false);
    }
    bmp
}

/// A 32×32 tilemap (256×256), game-paletted. `base_bit` is the LCDC bit that
/// picks the 0x9800/0x9C00 base (bit3 for BG, bit6 for window).
fn tilemap(gb: &GameBoy, base_bit: u8, no_palette: bool) -> Bitmap {
    let vram = gb.vram();
    let lcdc = gb.debug_read(0xFF40);
    let base = if lcdc & base_bit != 0 { 0x9C00 } else { 0x9800 };
    let signed = lcdc & 0x10 == 0; // bit4: 1 → 0x8000 unsigned, 0 → 0x8800 signed
    let cgb = gb.model().is_cgb();
    let cells = bg_map(vram, base);
    let (bg_cram, _) = gb.cgb_palette_ram();
    let dmg_bgp = dmg_pal(gb.debug_read(0xFF47));
    let mut bmp = Bitmap::new(256, 256);
    for cy in 0..32 {
        for cx in 0..32 {
            let cell = cells[cy * 32 + cx];
            let idx = bg_tile_index(cell.tile, signed);
            let bank = if cgb {
                usize::from(cell.attr >> 3 & 1)
            } else {
                0
            };
            let tile = tile_pixels(vram, bank, idx);
            let (xflip, yflip) = if cgb {
                (cell.attr & 0x20 != 0, cell.attr & 0x40 != 0)
            } else {
                (false, false)
            };
            let pal = if no_palette {
                GREYS
            } else if cgb {
                cgb_palette_words(bg_cram, usize::from(cell.attr & 7)).map(xrgb)
            } else {
                dmg_bgp
            };
            bmp.blit(cx * 8, cy * 8, &tile, &pal, xflip, yflip);
        }
    }
    bmp
}

/// The 40 OAM sprites in an 8×5 grid, each with its own palette/bank/flip.
fn oam(gb: &GameBoy, no_palette: bool) -> Bitmap {
    let vram = gb.vram();
    let lcdc = gb.debug_read(0xFF40);
    let tall = lcdc & 0x04 != 0;
    let cgb = gb.model().is_cgb();
    let (_, obj_cram) = gb.cgb_palette_ram();
    let dmg = [
        dmg_pal(gb.debug_read(0xFF48)),
        dmg_pal(gb.debug_read(0xFF49)),
    ];
    let sprites = oam_sprites(gb.oam());
    let cellh = if tall { 16 } else { 8 };
    let mut bmp = Bitmap::new(8 * 8, 5 * cellh);
    for (i, sp) in sprites.iter().enumerate() {
        let (ox, oy) = ((i % 8) * 8, (i / 8) * cellh);
        let bank = if cgb {
            usize::from(sp.attr >> 3 & 1)
        } else {
            0
        };
        let pal = if no_palette {
            GREYS
        } else if cgb {
            cgb_palette_words(obj_cram, usize::from(sp.attr & 7)).map(xrgb)
        } else {
            dmg[usize::from(sp.attr >> 4 & 1)]
        };
        let (xflip, yflip) = (sp.attr & 0x20 != 0, sp.attr & 0x40 != 0);
        if tall {
            // 8×16: top tile = index & 0xFE, bottom = | 1 (y-flip swaps them).
            let (top, bot) = (usize::from(sp.tile & 0xFE), usize::from(sp.tile | 1));
            let (a, b) = if yflip { (bot, top) } else { (top, bot) };
            bmp.blit(ox, oy, &tile_pixels(vram, bank, a), &pal, xflip, yflip);
            bmp.blit(ox, oy + 8, &tile_pixels(vram, bank, b), &pal, xflip, yflip);
        } else {
            bmp.blit(
                ox,
                oy,
                &tile_pixels(vram, bank, usize::from(sp.tile)),
                &pal,
                xflip,
                yflip,
            );
        }
    }
    bmp
}

/// Palette swatches: CGB 8 BG + 8 OBJ rows of 4, or DMG BGP/OBP0/OBP1 rows.
fn palette(gb: &GameBoy) -> Bitmap {
    const S: usize = 16;
    if gb.model().is_cgb() {
        let (bg, obj) = gb.cgb_palette_ram();
        let mut bmp = Bitmap::new(4 * S, 16 * S);
        for row in 0..16 {
            let (cram, pal) = if row < 8 { (bg, row) } else { (obj, row - 8) };
            let words = cgb_palette_words(cram, pal);
            for (c, &word) in words.iter().enumerate() {
                bmp.fill(c * S, row * S, S, S, xrgb(word));
            }
        }
        bmp
    } else {
        let rows = [
            dmg_pal(gb.debug_read(0xFF47)),
            dmg_pal(gb.debug_read(0xFF48)),
            dmg_pal(gb.debug_read(0xFF49)),
        ];
        let mut bmp = Bitmap::new(4 * S, 3 * S);
        for (row, pal) in rows.iter().enumerate() {
            for (c, &color) in pal.iter().enumerate() {
                bmp.fill(c * S, row * S, S, S, color);
            }
        }
        bmp
    }
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
