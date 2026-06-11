//! Mode 3 pixel pipeline: BG/window fetcher, pixel FIFO, sprite fetcher.
//!
//! Timing model: the pipeline starts at line dot 84 (78 on the glitched
//! LCD-enable line) and performs one step per dot. The fetcher needs 12 dots
//! before the first pixel ships (a discarded first tile fetch plus a real
//! one), each shipped pixel takes one dot, and SCX%8 leading pixels are
//! popped and discarded, so an unobstructed line finishes mode 3 after
//! 172 + SCX%8 dots — pixel 159 ships at line dot 256 + SCX%8, matching
//! `hblank_ly_scx_timing-GS`.
//!
//! Sprite fetches stall the pipeline for 6 dots each, plus a first-per-tile
//! alignment penalty of max(0, 5 - (x + SCX) % 8) dots (the BG fetcher must
//! finish its current tile row first). This reproduces every case table in
//! `intr_2_mode0_timing_sprites` exactly (Pan Docs "Mode 3 length" OBJ
//! penalty algorithm).

use super::Ppu;
use crate::SCREEN_W;

#[derive(Clone, Copy, Default)]
pub(super) struct Sprite {
    y: u8,
    x: u8,
    tile: u8,
    flags: u8,
    idx: u8,
}

/// One pending sprite pixel, aligned to upcoming output positions.
#[derive(Clone, Copy)]
struct SpritePixel {
    color: u8,
    /// DMG: OBP number (0/1); CGB: palette index 0-7.
    palette: u8,
    /// OAM attribute bit 7 (BG-over-OBJ).
    bg_priority: bool,
    /// OAM index, for CGB priority resolution.
    oam_idx: u8,
}

const EMPTY_SPRITE_PIXEL: SpritePixel = SpritePixel {
    color: 0,
    palette: 0,
    bg_priority: false,
    oam_idx: 0xFF,
};

pub(super) struct Render {
    pub(super) active: bool,
    /// Next output pixel x (0-159).
    lx: u8,
    /// Leading pixels still to discard (SCX%8, or 7-WX for WX<7).
    discard: u8,
    /// Pipeline frozen for this many dots (sprite fetches).
    stall: u16,

    // BG FIFO: 8 pixels as shift registers, all from one tile (pushes only
    // happen into an empty FIFO).
    bg_lo: u8,
    bg_hi: u8,
    bg_attr: u8,
    bg_count: u8,

    // Fetcher.
    /// 0-5: fetch steps (tile#, lo, hi); 6: push-retry hold.
    phase: u8,
    /// Tile column counter (BG: added to SCX/8; window: from 0).
    fetch_x: u8,
    /// Fetching window tiles instead of BG.
    win_mode: bool,
    /// First fetch of the line is thrown away (12-dot mode 3 startup).
    first_discard: bool,
    t_no: u8,
    t_attr: u8,
    t_fine: u8,
    t_lo: u8,
    t_hi: u8,

    // Sprites (selected during OAM scan).
    sprites: [Sprite; 10],
    n_sprites: u8,
    fetched: u16,
    /// BG tiles that already paid the first-sprite alignment penalty,
    /// keyed by (x + SCX) / 8.
    penalty_tiles: u64,
    sp_fifo: [SpritePixel; 8],

    pub(super) win_active: bool,
}

impl Render {
    pub(super) fn new() -> Self {
        Self {
            active: false,
            lx: 0,
            discard: 0,
            stall: 0,
            bg_lo: 0,
            bg_hi: 0,
            bg_attr: 0,
            bg_count: 0,
            phase: 0,
            fetch_x: 0,
            win_mode: false,
            first_discard: true,
            t_no: 0,
            t_attr: 0,
            t_fine: 0,
            t_lo: 0,
            t_hi: 0,
            sprites: [Sprite::default(); 10],
            n_sprites: 0,
            fetched: 0,
            penalty_tiles: 0,
            sp_fifo: [EMPTY_SPRITE_PIXEL; 8],
            win_active: false,
        }
    }
}

impl Ppu {
    /// Select up to 10 sprites for this line, in OAM order, by Y only
    /// (X — even 0 or ≥168 — does not affect selection; it only affects
    /// fetching: see `intr_2_mode0_timing_sprites`).
    pub(super) fn oam_scan(&mut self) {
        let h = if self.lcdc & 0x04 != 0 { 16u16 } else { 8 };
        let row = u16::from(self.ly) + 16;
        self.render.n_sprites = 0;
        for i in 0..40 {
            let y = u16::from(self.oam[i * 4]);
            if row >= y && row < y + h {
                let n = usize::from(self.render.n_sprites);
                self.render.sprites[n] = Sprite {
                    y: self.oam[i * 4],
                    x: self.oam[i * 4 + 1],
                    tile: self.oam[i * 4 + 2],
                    flags: self.oam[i * 4 + 3],
                    idx: i as u8,
                };
                self.render.n_sprites += 1;
                if self.render.n_sprites == 10 {
                    break;
                }
            }
        }
    }

    pub(super) fn render_init(&mut self) {
        let r = &mut self.render;
        r.active = true;
        r.lx = 0;
        r.discard = self.scx & 7;
        r.stall = 0;
        r.bg_count = 0;
        r.phase = 0;
        r.fetch_x = 0;
        r.win_mode = false;
        r.first_discard = true;
        r.fetched = 0;
        r.penalty_tiles = 0;
        r.sp_fifo = [EMPTY_SPRITE_PIXEL; 8];
        r.win_active = false;
        if self.glitch_line {
            // No OAM scan ran on the glitched LCD-enable line: no sprites.
            r.n_sprites = 0;
        }
    }

    /// One mode-3 dot.
    pub(super) fn render_step(&mut self) {
        if self.render.stall > 0 {
            self.render.stall -= 1;
            return;
        }

        // Sprite fetch triggers at the current output position, but only
        // once the pipeline is actually about to ship that pixel (the FIFO
        // holds pixels and SCX discarding is done) — the alignment penalty
        // is the BG fetcher finishing the tile row it is mid-way through.
        if self.lcdc & 0x02 != 0 && self.render.bg_count > 0 && self.render.discard == 0 {
            for i in 0..usize::from(self.render.n_sprites) {
                if self.render.fetched & (1 << i) != 0 {
                    continue;
                }
                let s = self.render.sprites[i];
                if s.x >= 168 {
                    continue;
                }
                let trigger = s.x.saturating_sub(8);
                if trigger != self.render.lx {
                    continue;
                }
                self.render.fetched |= 1 << i;
                let wait = self.sprite_penalty(s.x);
                // The BG fetcher keeps working during the alignment wait
                // (that wait *is* the fetcher finishing its tile row).
                for _ in 0..wait {
                    self.fetcher_step();
                }
                self.fetch_sprite(i);
                self.render.stall += 6 + wait;
            }
            if self.render.stall > 0 {
                self.render.stall -= 1;
                return;
            }
        }

        // Window trigger: WX matches the next output pixel (WX-7..WX-1
        // hang off the left edge for WX<7; WX=166 starts at pixel 159).
        if !self.render.win_active
            && self.win_enabled_now()
            && self.wy_latch
            && ((self.wx >= 7 && self.render.lx == self.wx - 7)
                || (self.wx < 7 && self.render.lx == 0))
        {
            let r = &mut self.render;
            r.win_active = true;
            r.win_mode = true;
            r.bg_count = 0;
            r.phase = 0;
            r.fetch_x = 0;
            r.first_discard = false;
            // Window pixels are not subject to SCX fine scroll; WX<7 cuts
            // the leading 7-WX window columns instead.
            r.discard = 7u8.saturating_sub(self.wx);
        }

        // Pop one BG/window pixel.
        if self.render.bg_count > 0 {
            let r = &mut self.render;
            let c = ((r.bg_hi >> 7) << 1) | (r.bg_lo >> 7);
            r.bg_lo <<= 1;
            r.bg_hi <<= 1;
            r.bg_count -= 1;
            let attr = r.bg_attr;
            if r.discard > 0 {
                r.discard -= 1;
            } else {
                self.output_pixel(c, attr);
                self.render.lx += 1;
                if self.render.lx == 160 {
                    self.render.active = false;
                    self.line_render_done = true;
                    return;
                }
            }
        }

        self.fetcher_step();
    }

    /// First-per-BG-tile sprite alignment penalty (Pan Docs OBJ penalty
    /// algorithm; verified against intr_2_mode0_timing_sprites).
    fn sprite_penalty(&mut self, x: u8) -> u16 {
        let v = u16::from(x) + u16::from(self.scx);
        let key = v >> 3;
        if self.render.penalty_tiles & (1u64 << key) != 0 {
            0
        } else {
            self.render.penalty_tiles |= 1u64 << key;
            5u16.saturating_sub(v & 7)
        }
    }

    fn win_enabled_now(&self) -> bool {
        // DMG: LCDC bit 0 gates the window as well; CGB: bit 0 is only
        // priority (Pan Docs LCDC.0).
        self.lcdc & 0x20 != 0 && (self.model.is_cgb() || self.lcdc & 0x01 != 0)
    }

    fn fetcher_step(&mut self) {
        match self.render.phase {
            1 => {
                // Tile number (+ attributes on CGB) from the tile map.
                let (map_bit, row, col, fine) = if self.render.win_mode {
                    (
                        0x40,
                        self.win_line >> 3,
                        self.render.fetch_x & 31,
                        self.win_line & 7,
                    )
                } else {
                    let y = self.ly.wrapping_add(self.scy);
                    (
                        0x08,
                        y >> 3,
                        (self.scx / 8).wrapping_add(self.render.fetch_x) & 31,
                        y & 7,
                    )
                };
                let base = if self.lcdc & map_bit != 0 {
                    0x1C00
                } else {
                    0x1800
                };
                let map = base + usize::from(row) * 32 + usize::from(col);
                self.render.t_no = self.vram[map];
                self.render.t_attr = if self.model.is_cgb() {
                    self.vram[0x2000 + map]
                } else {
                    0
                };
                self.render.t_fine = if self.render.t_attr & 0x40 != 0 {
                    7 - fine // Y flip (CGB BG attribute bit 6).
                } else {
                    fine
                };
                self.render.phase = 2;
            }
            3 => {
                self.render.t_lo = self.vram[self.bg_tile_addr()];
                self.render.phase = 4;
            }
            5 => {
                self.render.t_hi = self.vram[self.bg_tile_addr() + 1];
                if self.render.first_discard {
                    // The first tile fetch of the line is thrown away and
                    // restarted: 12 dots of mode 3 before the first pixel.
                    self.render.first_discard = false;
                    self.render.phase = 0;
                } else if self.render.bg_count == 0 {
                    self.push_bg_row();
                } else {
                    self.render.phase = 6;
                }
            }
            6 => {
                if self.render.bg_count == 0 {
                    self.push_bg_row();
                }
            }
            _ => self.render.phase += 1,
        }
    }

    fn push_bg_row(&mut self) {
        let r = &mut self.render;
        let (lo, hi) = if r.t_attr & 0x20 != 0 {
            // X flip (CGB BG attribute bit 5).
            (r.t_lo.reverse_bits(), r.t_hi.reverse_bits())
        } else {
            (r.t_lo, r.t_hi)
        };
        r.bg_lo = lo;
        r.bg_hi = hi;
        r.bg_attr = r.t_attr;
        r.bg_count = 8;
        r.fetch_x = r.fetch_x.wrapping_add(1);
        r.phase = 0;
    }

    fn bg_tile_addr(&self) -> usize {
        let r = &self.render;
        let bank = if self.model.is_cgb() && r.t_attr & 0x08 != 0 {
            0x2000
        } else {
            0
        };
        let base = if self.lcdc & 0x10 != 0 {
            usize::from(r.t_no) * 16
        } else {
            (0x1000i32 + i32::from(r.t_no as i8) * 16) as usize
        };
        bank + base + usize::from(r.t_fine) * 2
    }

    /// Fetch sprite `i`'s row and merge it into the sprite FIFO.
    fn fetch_sprite(&mut self, i: usize) {
        let s = self.render.sprites[i];
        let tall = self.lcdc & 0x04 != 0;
        let h: u8 = if tall { 16 } else { 8 };
        let mut row = self.ly.wrapping_add(16).wrapping_sub(s.y);
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

    fn output_pixel(&mut self, bg_c: u8, bg_attr: u8) {
        // Shift the sprite FIFO in step with shipped pixels.
        let sp = self.render.sp_fifo[0];
        self.render.sp_fifo.copy_within(1.., 0);
        self.render.sp_fifo[7] = EMPTY_SPRITE_PIXEL;

        let cgb = self.model.is_cgb();
        // DMG LCDC bit 0: BG and window disabled — they show as white
        // (color 0 for sprite priority purposes). DMG compatibility mode on
        // CGB behaves the same way (integration addition).
        let bg_off = (!cgb || self.dmg_compat) && self.lcdc & 0x01 == 0;
        let bg_c = if bg_off { 0 } else { bg_c };

        let sprite_wins = sp.color != 0
            && if cgb {
                // CGB: BG color 0 always loses; LCDC bit 0 clear strips all
                // BG priority; else BG attribute bit 7 or OAM bit 7 wins.
                bg_c == 0 || self.lcdc & 0x01 == 0 || !(bg_attr & 0x80 != 0 || sp.bg_priority)
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
                        self.obp1
                    } else {
                        self.obp0
                    };
                    (obp >> (sp.color * 2)) & 3
                } else {
                    sp.color
                };
                self.cgb_color(&self.obj_pal_ram, sp.palette, c)
            } else {
                let obp = if sp.palette == 1 {
                    self.obp1
                } else {
                    self.obp0
                };
                self.dmg_palette[usize::from((obp >> (sp.color * 2)) & 3)]
            }
        } else if cgb {
            // Integration addition: compat mode remaps BG pixels through
            // BGP; BG attributes are all zero (VRAM bank 1 is locked), so
            // palette 0 is used either way.
            let c = if self.dmg_compat && !bg_off {
                (self.bgp >> (bg_c * 2)) & 3
            } else {
                bg_c
            };
            self.cgb_color(&self.bg_pal_ram, bg_attr & 0x07, c)
        } else if bg_off {
            self.dmg_palette[0]
        } else {
            self.dmg_palette[usize::from((self.bgp >> (bg_c * 2)) & 3)]
        };

        let idx = usize::from(self.ly) * SCREEN_W + usize::from(self.render.lx);
        self.back[idx] = color;
    }

    /// RGB555 palette RAM entry to XRGB8888: straight 5→8 bit expansion
    /// ((c << 3) | (c >> 2)), no color correction in the core.
    fn cgb_color(&self, ram: &[u8; 64], palette: u8, color: u8) -> u32 {
        let i = usize::from(palette) * 8 + usize::from(color) * 2;
        let raw = u16::from(ram[i]) | (u16::from(ram[i + 1]) << 8);
        let expand = |c: u16| -> u32 { u32::from(((c << 3) | (c >> 2)) & 0xFF) };
        let r = expand(raw & 0x1F);
        let g = expand((raw >> 5) & 0x1F);
        let b = expand((raw >> 10) & 0x1F);
        (r << 16) | (g << 8) | b
    }
}

#[cfg(test)]
mod tests {
    use super::super::Ppu;
    use crate::Model;

    const WHITE: u32 = 0xFF_FFFF;
    const LIGHT: u32 = 0xAA_AAAA;
    const DARK: u32 = 0x55_5555;
    const BLACK: u32 = 0x00_0000;

    fn run_to(p: &mut Ppu, line: u8, dot: u16) {
        let mut guard = 0u32;
        while !(p.line == line && p.dot == dot) {
            p.tick();
            guard += 1;
            assert!(guard < 200_000, "run_to({line},{dot}) never reached");
        }
    }

    /// Render the given line to completion; returns the dot at which mode 3
    /// ended (V0).
    fn render_line(p: &mut Ppu, line: u8) -> u16 {
        run_to(p, line, 84);
        let mut guard = 0u32;
        while !p.line_render_done {
            p.tick();
            guard += 1;
            assert!(guard < 2_000, "mode 3 never finished");
        }
        p.dot
    }

    fn px(p: &Ppu, line: usize, x: usize) -> u32 {
        p.back[line * crate::SCREEN_W + x]
    }

    fn dmg_on(lcdc: u8) -> Ppu {
        let mut p = Ppu::new(Model::Dmg);
        p.write(0xFF47, 0xE4); // identity BGP
        p.write(0xFF48, 0xE4);
        p.write(0xFF49, 0xE4);
        p.write(0xFF40, lcdc);
        p
    }

    fn set_tile_row(p: &mut Ppu, bank: usize, tile: usize, row: usize, lo: u8, hi: u8) {
        p.vram[bank * 0x2000 + tile * 16 + row * 2] = lo;
        p.vram[bank * 0x2000 + tile * 16 + row * 2 + 1] = hi;
    }

    fn set_map(p: &mut Ppu, base: usize, row: usize, col: usize, tile: u8) {
        p.vram[base + row * 32 + col] = tile;
    }

    fn sprite(p: &mut Ppu, i: u8, y: u8, x: u8, tile: u8, flags: u8) {
        p.oam_dma_write(i * 4, y);
        p.oam_dma_write(i * 4 + 1, x);
        p.oam_dma_write(i * 4 + 2, tile);
        p.oam_dma_write(i * 4 + 3, flags);
    }

    // --- BG rendering ---

    #[test]
    fn bg_tile_pixels_and_bgp() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), LIGHT);
        assert_eq!(px(&p, 2, 3), LIGHT);
        assert_eq!(px(&p, 2, 4), DARK);
        assert_eq!(px(&p, 2, 7), DARK);
        assert_eq!(px(&p, 2, 8), WHITE); // tile 0 = color 0

        // Remap shades through BGP.
        let mut p = dmg_on(0x91);
        p.write(0xFF47, 0x1B); // 0->3, 1->2, 2->1, 3->0
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), DARK);
        assert_eq!(px(&p, 2, 4), LIGHT);
        assert_eq!(px(&p, 2, 8), BLACK);
    }

    #[test]
    fn bg_scx_fine_scroll_shifts_pixels() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_map(&mut p, 0x1800, 0, 1, 1);
        p.write(0xFF43, 3);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), LIGHT); // bg col 3
        assert_eq!(px(&p, 2, 1), DARK); // bg col 4
        assert_eq!(px(&p, 2, 4), DARK); // bg col 7
        assert_eq!(px(&p, 2, 5), LIGHT); // bg col 8 = next tile col 0
    }

    #[test]
    fn bg_scy_selects_row() {
        let mut p = dmg_on(0x91);
        p.write(0xFF42, 5);
        set_tile_row(&mut p, 0, 1, 7, 0xFF, 0xFF); // line 2 + scy 5 = row 7
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLACK);
    }

    #[test]
    fn bg_signed_tile_addressing() {
        let mut p = dmg_on(0x81); // LCDC bit 4 clear: 0x8800 signed mode
                                  // Tile 0x80 lives at 0x9000 + (-128)*16 = 0x8800.
        p.vram[0x0800 + 2 * 2] = 0xFF;
        p.vram[0x0800 + 2 * 2 + 1] = 0xFF;
        set_map(&mut p, 0x1800, 0, 0, 0x80);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLACK);
    }

    #[test]
    fn bg_map_select_bit3() {
        let mut p = dmg_on(0x99); // bit 3: map at 0x9C00
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0xFF);
        set_map(&mut p, 0x1C00, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLACK);
    }

    #[test]
    fn dmg_lcdc0_blanks_bg_to_white() {
        let mut p = dmg_on(0x90); // BG disabled
        set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), WHITE);
        assert_eq!(px(&p, 2, 159), WHITE);
    }

    // --- Mode 3 length ---

    #[test]
    fn mode3_length_scx() {
        for scx in 0u8..=8 {
            let mut p = dmg_on(0x91);
            p.write(0xFF43, scx);
            let v0 = render_line(&mut p, 1);
            assert_eq!(v0, 256 + u16::from(scx & 7), "scx {scx}");
        }
    }

    fn penalty(xs: &[u8]) -> u16 {
        let mut p = dmg_on(0x93);
        for (i, &x) in xs.iter().enumerate() {
            sprite(&mut p, i as u8, 19, x, 0, 0); // row 0 on line 3
        }
        render_line(&mut p, 3) - 256
    }

    /// Mooneye intr_2_mode0_timing_sprites measures floor(penalty/4) as
    /// "extra cycles"; the dot counts come from the Pan Docs OBJ penalty
    /// algorithm which reproduces that whole table.
    #[test]
    fn sprite_penalty_table() {
        // 1-N sprites at X=0 -> extra cycles 2,4,5,7,8,10,11,13,14,16.
        let expect = [2, 4, 5, 7, 8, 10, 11, 13, 14, 16];
        for n in 1..=10usize {
            let dots = penalty(&vec![0u8; n]);
            assert_eq!(dots, 6 * n as u16 + 5, "{n} sprites at x=0");
            assert_eq!(dots / 4, expect[n - 1], "{n} sprites at x=0 (cycles)");
        }
        // 10 sprites at X=N.
        for (x, cycles) in [
            (1u8, 16),
            (2, 15),
            (5, 15),
            (7, 15),
            (8, 16),
            (16, 16),
            (160, 16),
            (167, 15),
        ] {
            assert_eq!(penalty(&[x; 10]) / 4, cycles, "10 sprites at x={x}");
        }
        // Off-screen X >= 168: selected but never fetched.
        assert_eq!(penalty(&[168; 10]), 0);
        assert_eq!(penalty(&[169; 10]), 0);
        // Two groups on different BG tiles both pay the alignment penalty.
        assert_eq!(penalty(&[0, 0, 0, 0, 0, 160, 160, 160, 160, 160]) / 4, 17);
        assert_eq!(penalty(&[4, 4, 4, 4, 4, 164, 164, 164, 164, 164]) / 4, 15);
        // Single sprite at X=N.
        for (x, cycles) in [(0u8, 2), (3, 2), (4, 1), (7, 1), (8, 2), (164, 1)] {
            assert_eq!(penalty(&[x]) / 4, cycles, "1 sprite at x={x}");
        }
        // Two sprites 8 apart.
        assert_eq!(penalty(&[0, 8]) / 4, 5);
        assert_eq!(penalty(&[4, 12]) / 4, 3);
        // 10 sprites 8 apart.
        assert_eq!(penalty(&[0, 8, 16, 24, 32, 40, 48, 56, 64, 72]) / 4, 27);
        assert_eq!(penalty(&[1, 9, 17, 25, 33, 41, 49, 57, 65, 73]) / 4, 25);
        assert_eq!(penalty(&[4, 12, 20, 28, 36, 44, 52, 60, 68, 76]) / 4, 17);
        assert_eq!(penalty(&[5, 13, 21, 29, 37, 45, 53, 61, 69, 77]) / 4, 15);
        // Reverse OAM order: identical timing.
        assert_eq!(penalty(&[72, 64, 56, 48, 40, 32, 24, 16, 8, 0]) / 4, 27);
    }

    #[test]
    fn sprites_disabled_no_penalty() {
        let mut p = dmg_on(0x91); // OBJ off
        for i in 0..10 {
            sprite(&mut p, i, 19, 0, 0, 0);
        }
        assert_eq!(render_line(&mut p, 3), 256);
    }

    #[test]
    fn window_costs_6_dots() {
        let mut p = dmg_on(0xB1); // window on, map 0x9800 for both
        p.write(0xFF4A, 0); // WY=0
        p.write(0xFF4B, 87); // WX: window from pixel 80
        let v0 = render_line(&mut p, 2);
        assert_eq!(v0, 262);
    }

    // --- Window rendering ---

    #[test]
    fn window_pixels_and_line_counter() {
        let mut p = dmg_on(0xF1); // win map 0x9C00, win on, bg map 0x9800
        p.write(0xFF4A, 1);
        p.write(0xFF4B, 15); // window from pixel 8
        set_map(&mut p, 0x1C00, 0, 0, 2);
        set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF); // window line 0: color 3
        set_tile_row(&mut p, 0, 2, 1, 0x00, 0xFF); // window line 1: color 2
        render_line(&mut p, 1);
        assert_eq!(px(&p, 1, 7), WHITE);
        assert_eq!(px(&p, 1, 8), BLACK, "first window line uses row 0");
        render_line(&mut p, 2);
        assert_eq!(
            px(&p, 2, 8),
            DARK,
            "window line counter advances independently of LY/SCY"
        );
    }

    #[test]
    fn window_wx0_starts_at_left_edge() {
        let mut p = dmg_on(0xB1);
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 0);
        set_map(&mut p, 0x1800, 0, 0, 0); // bg tile 0 (white)
        set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF);
        for col in 0..21 {
            set_map(&mut p, 0x1800, 0, col, 2); // window map = bg map here
        }
        render_line(&mut p, 0);
        // WX=0: the leading 7 window pixels fall off the left edge but the
        // window occupies the whole line.
        assert_eq!(px(&p, 0, 0), BLACK);
    }

    #[test]
    fn window_disabled_by_lcdc5() {
        let mut p = dmg_on(0x91); // bit 5 clear
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 7);
        set_map(&mut p, 0x1C00, 0, 0, 2);
        set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF);
        let v0 = render_line(&mut p, 2);
        assert_eq!(v0, 256, "no window penalty");
        assert_eq!(px(&p, 2, 0), WHITE);
    }

    // --- Sprite rendering ---

    #[test]
    fn sprite_pixels_palettes_transparency() {
        let mut p = dmg_on(0x93);
        p.write(0xFF48, 0xE4);
        p.write(0xFF49, 0x1B);
        set_tile_row(&mut p, 0, 4, 0, 0x0F, 0x00); // right half color 1
        sprite(&mut p, 0, 18, 16, 4, 0x00); // line 2, screen 8-15, OBP0
        sprite(&mut p, 1, 18, 40, 4, 0x10); // screen 32-39, OBP1
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), WHITE, "transparent sprite pixel shows BG");
        assert_eq!(px(&p, 2, 12), LIGHT, "OBP0 color 1");
        assert_eq!(px(&p, 2, 15), LIGHT);
        assert_eq!(px(&p, 2, 16), WHITE);
        assert_eq!(px(&p, 2, 36), DARK, "OBP1 maps 1 -> 2");
    }

    #[test]
    fn sprite_bg_priority_flag() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00); // bg: cols 0-3 color 1
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0xFF); // sprite solid color 3
        sprite(&mut p, 0, 18, 8, 4, 0x80); // behind BG, screen 0-7
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), LIGHT, "BG color 1-3 beats OBJ-behind-BG");
        assert_eq!(px(&p, 2, 4), BLACK, "BG color 0 shows the sprite");
    }

    #[test]
    fn sprite_x_flip() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0x80, 0x00); // only leftmost pixel
        sprite(&mut p, 0, 18, 16, 4, 0x00);
        sprite(&mut p, 1, 18, 40, 4, 0x20); // X-flipped
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), LIGHT);
        assert_eq!(px(&p, 2, 9), WHITE);
        assert_eq!(px(&p, 2, 32), WHITE);
        assert_eq!(px(&p, 2, 39), LIGHT);
    }

    #[test]
    fn sprite_y_flip() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // row 0: color 1
        set_tile_row(&mut p, 0, 4, 7, 0xFF, 0xFF); // row 7: color 3
        sprite(&mut p, 0, 18, 16, 4, 0x40); // Y-flipped: line 2 = row 7
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), BLACK);
    }

    #[test]
    fn sprite_8x16_tile_masking() {
        let mut p = dmg_on(0x97); // 8x16
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // top tile row 0: color 1
        set_tile_row(&mut p, 0, 5, 0, 0xFF, 0xFF); // bottom tile row 0: color 3
                                                   // Line 2 hits row 8 of a sprite at y=10 -> bottom tile.
        sprite(&mut p, 0, 10, 16, 5, 0x00); // tile 5: bit 0 ignored -> 4/5
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), BLACK, "row 8 comes from tile|1");

        let mut p = dmg_on(0x97);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        set_tile_row(&mut p, 0, 5, 0, 0xFF, 0xFF);
        sprite(&mut p, 0, 18, 16, 5, 0x00); // line 2 = row 0 -> top tile 4
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), LIGHT, "row 0 comes from tile&0xFE");
    }

    #[test]
    fn sprite_priority_dmg_lower_x_wins() {
        let mut p = dmg_on(0x93);
        p.write(0xFF49, 0x1B);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
        sprite(&mut p, 0, 18, 20, 4, 0x00); // idx 0, screen 12-19, OBP0
        sprite(&mut p, 1, 18, 18, 4, 0x10); // idx 1, screen 10-17, OBP1
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 10), DARK, "lower-X sprite only");
        assert_eq!(px(&p, 2, 14), DARK, "lower X wins overlap on DMG");
        assert_eq!(px(&p, 2, 18), LIGHT, "higher-X sprite tail");
    }

    #[test]
    fn sprite_priority_same_x_oam_order() {
        let mut p = dmg_on(0x93);
        p.write(0xFF49, 0x1B);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 20, 4, 0x00); // idx 0, OBP0
        sprite(&mut p, 1, 18, 20, 4, 0x10); // idx 1, OBP1, same X
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 14), LIGHT, "lower OAM index wins at equal X");
    }

    #[test]
    fn ten_sprite_limit_by_oam_order() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        // 11 sprites on the line; the 11th (highest OAM index) is dropped.
        for i in 0..11u8 {
            sprite(&mut p, i, 18, 8 + i * 12, 4, 0);
        }
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 9 * 12), LIGHT, "10th sprite renders");
        assert_eq!(px(&p, 2, 10 * 12), WHITE, "11th sprite dropped");
    }

    // --- CGB ---

    fn cgb_on(lcdc: u8) -> Ppu {
        let mut p = Ppu::new(Model::Cgb);
        // BG palette 0 color 0 = white, identity-ish grayscale for colors.
        for pal in 0..2usize {
            for (c, raw) in [(0usize, 0x7FFFu16), (1, 0x294A), (2, 0x14A5), (3, 0x0000)] {
                p.bg_pal_ram[pal * 8 + c * 2] = raw as u8;
                p.bg_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
                p.obj_pal_ram[pal * 8 + c * 2] = raw as u8;
                p.obj_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
            }
        }
        // Make palette 1 color 1 pure red, obj palette 1 color 1 pure blue.
        p.bg_pal_ram[8 + 2] = 0x1F;
        p.bg_pal_ram[8 + 3] = 0x00;
        p.obj_pal_ram[8 + 2] = 0x00;
        p.obj_pal_ram[8 + 3] = 0x7C;
        p.write(0xFF40, lcdc);
        p
    }

    const CGB_WHITE: u32 = 0xFF_FFFF;
    const RED: u32 = 0xFF_0000;
    const BLUE: u32 = 0x00_00FF;

    #[test]
    fn cgb_color_expansion() {
        let p = cgb_on(0x91);
        assert_eq!(p.cgb_color(&p.bg_pal_ram, 0, 0), CGB_WHITE);
        assert_eq!(p.cgb_color(&p.bg_pal_ram, 1, 1), RED);
        // 5->8 bit expansion: (c << 3) | (c >> 2).
        let mut q = cgb_on(0x91);
        q.bg_pal_ram[0] = 0x10; // red = 16
        q.bg_pal_ram[1] = 0x00;
        assert_eq!(q.cgb_color(&q.bg_pal_ram, 0, 0), 0x84_0000);
    }

    #[test]
    fn cgb_bg_attributes_palette_bank_flips() {
        let mut p = cgb_on(0x91);
        // Tile 1 data in bank 1 only; bank 0 left zero.
        set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00); // leftmost pixel color 1
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x09; // palette 1, bank 1
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), RED, "bank 1 data, palette 1");
        assert_eq!(px(&p, 2, 1), CGB_WHITE);

        // X flip.
        let mut p = cgb_on(0x91);
        set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00);
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x29; // + X flip
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), CGB_WHITE);
        assert_eq!(px(&p, 2, 7), RED);

        // Y flip: line 2 fetches tile row 5.
        let mut p = cgb_on(0x91);
        set_tile_row(&mut p, 1, 1, 5, 0x80, 0x00);
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x49; // + Y flip
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), RED);
    }

    #[test]
    fn cgb_sprite_priority_by_oam_index() {
        let mut p = cgb_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
        sprite(&mut p, 0, 18, 20, 4, 0x01); // idx 0, obj palette 1 (blue)
        sprite(&mut p, 1, 18, 18, 4, 0x00); // idx 1, palette 0, lower X
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 14), BLUE, "CGB: lower OAM index wins overlap");
        // OPRI bit 0 set: DMG-style X priority.
        let mut p = cgb_on(0x93);
        p.write(0xFF6C, 1);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 20, 4, 0x01);
        sprite(&mut p, 1, 18, 18, 4, 0x00);
        render_line(&mut p, 2);
        assert_ne!(px(&p, 2, 14), BLUE, "OPRI=1: lower X wins");
    }

    #[test]
    fn cgb_bg_priority_and_master_priority() {
        // BG attr bit 7 set, BG color nonzero: BG wins...
        let mut p = cgb_on(0x93);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00); // bg cols 0-3 color 1
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x81; // priority + palette 1
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 8, 4, 0x01); // obj palette 1 (blue)
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), RED, "BG attr priority beats sprite");
        assert_eq!(px(&p, 2, 4), BLUE, "BG color 0 always loses");

        // ...unless LCDC bit 0 is clear: master priority off.
        let mut p = cgb_on(0x92);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00);
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_map(&mut p, 0x1800, 0, 2, 1);
        p.vram[0x2000 + 0x1800] = 0x81;
        p.vram[0x2000 + 0x1802] = 0x81;
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 8, 4, 0x81); // even OAM bit 7 set
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLUE, "LCDC0=0 strips all BG priority");
        // And the BG itself still renders (not blanked like DMG).
        assert_eq!(px(&p, 2, 9), CGB_WHITE);
        assert_eq!(px(&p, 2, 16), RED, "BG drawn where no sprite covers it");
    }

    #[test]
    fn cgb_vbk_banks() {
        let mut p = cgb_on(0x91);
        run_to(&mut p, 145, 0); // vblank: VRAM accessible
        assert_eq!(p.read(0xFF4F), 0xFE);
        p.write(0x8000, 0x11);
        p.write(0xFF4F, 1);
        assert_eq!(p.read(0xFF4F), 0xFF);
        assert_eq!(p.read(0x8000), 0);
        p.write(0x8000, 0x22);
        assert_eq!(p.read(0x8000), 0x22);
        assert_eq!(p.vram_read_raw(0x8000), 0x22);
        p.vram_write_raw(0x9FFF, 0x33);
        assert_eq!(p.vram[0x3FFF], 0x33);
        p.write(0xFF4F, 0xFE); // only bit 0 counts
        assert_eq!(p.read(0x8000), 0x11);
        assert_eq!(p.vram_read_raw(0x8000), 0x11);
    }

    #[test]
    fn cgb_palette_registers() {
        let mut p = cgb_on(0x91);
        run_to(&mut p, 145, 0);
        p.write(0xFF68, 0x80); // index 0, auto-increment
        p.write(0xFF69, 0x1F);
        p.write(0xFF69, 0x00);
        assert_eq!(p.read(0xFF68), 0x40 | 0x82);
        assert_eq!(p.bg_pal_ram[0], 0x1F);
        assert_eq!(p.bg_pal_ram[1], 0x00);
        p.write(0xFF68, 0x00);
        assert_eq!(p.read(0xFF69), 0x1F, "read back without increment");
        assert_eq!(p.read(0xFF68), 0x40, "reads have bit 6 set");

        p.write(0xFF6A, 0x80 | 0x10);
        p.write(0xFF6B, 0xAA);
        assert_eq!(p.obj_pal_ram[0x10], 0xAA);
        assert_eq!(p.read(0xFF6A), 0x40 | 0x91);
    }

    #[test]
    fn cgb_palette_ram_blocked_in_mode3() {
        let mut p = cgb_on(0x91);
        p.bg_pal_ram[0] = 0x12;
        run_to(&mut p, 1, 100); // mode 3
        assert_eq!(p.read(0xFF41) & 3, 3);
        p.write(0xFF68, 0x80);
        assert_eq!(p.read(0xFF69), 0xFF, "reads blocked during mode 3");
        p.write(0xFF69, 0x77);
        assert_eq!(p.bg_pal_ram[0], 0x12, "write dropped during mode 3");
        assert_eq!(
            p.read(0xFF68) & 0x3F,
            1,
            "auto-increment still happens on a blocked write (Pan Docs)"
        );
    }

    #[test]
    fn dmg_cgb_registers_unmapped() {
        let mut p = dmg_on(0x91);
        assert_eq!(p.read(0xFF4F), 0xFF);
        assert_eq!(p.read(0xFF68), 0xFF);
        assert_eq!(p.read(0xFF69), 0xFF);
        assert_eq!(p.read(0xFF6C), 0xFF);
        p.write(0xFF4F, 1); // ignored
        p.write(0x9000, 0x55);
        run_to(&mut p, 150, 0);
        assert_eq!(p.read(0x9000), 0x55);
    }

    #[test]
    fn set_dmg_palette_applies() {
        let mut p = dmg_on(0x91);
        p.set_dmg_palette([0x11, 0x22, 0x33, 0x44]);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), 0x22);
        assert_eq!(px(&p, 2, 4), 0x33);
        assert_eq!(px(&p, 2, 8), 0x11);
    }

    #[test]
    fn frame_buffer_double_buffering() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 0, 0xFF, 0xFF);
        set_map(&mut p, 0x1800, 0, 0, 1);
        run_to(&mut p, 143, 455);
        assert_eq!(p.frame()[0], WHITE, "frame() is the completed frame");
        p.tick(); // 144:0 -> swap
        assert_eq!(p.frame()[0], BLACK);
    }
}
