//! Dot-accurate PPU with pixel FIFO. PPU work package.
//!
//! Stepped one dot (T-cycle) at a time by the interconnect. Mode timing must
//! be exact: variable-length mode 3 (SCX fine scroll, window, sprite fetch
//! stalls), STAT interrupt line blocking, LY=153→0 early wrap, LCD-enable
//! first-frame quirks (mooneye `acceptance/ppu/*`, `lcdon_*`).
//!
//! Renders DMG (4-shade via BGP/OBP through a configurable RGB palette) and
//! CGB (BG/OBJ palette RAM, VRAM bank 1 attributes, master priority via OPRI).
//!
//! # Scanline timeline (derived from mooneye test ROM sources)
//!
//! All positions are dots within a 456-dot line, with dot 0 = the dot where
//! LY changes (the convention `lcdon_timing-GS` measurements decode to).
//! "state(T)" below means the state a CPU read observes after T dots have
//! been ticked.
//!
//! | dot          | event |
//! |--------------|-------|
//! | 0            | LY := line; OAM reads blocked; LYC compare invalid (flag 0); STAT mode reads 0 |
//! | 4            | STAT mode reads 2; OAM writes blocked; LYC compare valid; STAT mode-2 (OAM) interrupt source asserts (one M-cycle after the LY change, together with the readable mode and the compare — `intr_2_mode3_timing` puts the first mode-3 STAT read exactly 80 dots after the IRQ becomes visible, and `lcdon_timing-GS` pins that read flip at dot 84) |
//! | 80           | VRAM reads blocked; OAM scan complete |
//! | 84           | STAT mode reads 3; VRAM writes blocked; OAM source drops (follows the readable mode-2 window) |
//! | V0           | mode 0: STAT reads 0, mode-0 IRQ source asserts, OAM+VRAM unblock. V0 = 256 + SCX%8 + sprite/window penalties (`hblank_ly_scx_timing-GS`: LY increments 51/50/49 cycles after the mode-0 IRQ for SCX%8 = 0/1-4/5-7; `intr_2_mode0_timing`/`intr_2_oam_ok_timing`: 252 dots after the mode-2 IRQ) |
//!
//! VBlank: line 144 dots 0-3 still read STAT mode 0 (the mode-0 IRQ source
//! stays asserted, keeping the STAT line gapless for `stat_irq_blocking`);
//! mode 1 and the VBlank IF bit assert at 144:4. The OAM IRQ source also
//! pulses at 144:4 on DMG (`vblank_stat_intr-GS`: simultaneous with VBlank)
//! and at 144:0 on CGB (`misc/ppu/vblank_stat_intr-C`: one M-cycle earlier),
//! and on DMG it pulses again at dot 12 of every later vblank line
//! (`intr_1_2_timing-GS` measures mode1→mode2 IRQ distance = 464 dots, i.e.
//! one line + 8 dots — the next pulse is on line 145, sitting 8 dots after
//! the dot-4 position the OAM source rises at on visible lines).
//!
//! Line 153: LY reads 153 during dots 0-3 only, then 0; the LYC compare sees
//! 153 during dots 4-7, is invalid during 8-11, and sees 0 from dot 12
//! (TCAGBD §8.9).
//!
//! LCD enable starts a glitched line 0 (`lcdon_timing-GS`): 452 dots long,
//! no OAM scan (STAT reads mode 0, OAM/VRAM accessible), mode 3 (and all
//! read+write blocking) during dots 78..250, then a real hblank.

mod render;

use crate::SCREEN_PIXELS;
use crate::model::Model;

use render::Render;

/// Dots per normal scanline.
const LINE_DOTS: u16 = 456;
/// The glitched first line after LCD enable is 4 dots short: LY=1 appears at
/// dot 452 in `lcdon_timing-GS` (state(448) reads LY=0, state(452) reads 1).
const GLITCH_LINE_DOTS: u16 = 452;
/// Mode 3 / blocking start on the glitched LCD-enable line.
const GLITCH_MODE3_START: u16 = 78;

pub struct Ppu {
    model: Model,
    frame_count: u64,

    // Registers.
    lcdc: u8,
    /// STAT bits 3-6 (interrupt source enables).
    stat_en: u8,
    scy: u8,
    scx: u8,
    /// LY as read through FF44 (153-quirk aware).
    ly: u8,
    lyc: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    wy: u8,
    wx: u8,
    /// CGB VRAM bank select (bit 0).
    vbk: u8,
    /// CGB object priority mode (FF6C bit 0: 1 = DMG-style X priority).
    opri: u8,
    /// Integration addition: CGB hardware running a DMG cart ("DMG
    /// compatibility mode"). Rendering remaps pixels through BGP/OBP0/OBP1
    /// into the boot-installed compat palettes, and OAM flag bit 4 selects
    /// the object palette (Pan Docs "DMG compatibility mode").
    dmg_compat: bool,
    bcps: u8,
    ocps: u8,
    bg_pal_ram: [u8; 64],
    obj_pal_ram: [u8; 64],

    vram: Box<[u8; 0x4000]>,
    oam: [u8; 0xA0],
    /// OAM DMA transfer frozen mid-byte by the HALT/STOP core clock gate,
    /// as (OAM index about to be replaced, in-flight source byte). Set by
    /// the interconnect; while set, the MGB OAM scan sees glitched data
    /// (madness/mgb_oam_dma_halt_sprites.s — see `oam_scan` in render.rs).
    dma_freeze: Option<(u8, u8)>,

    // Timing state.
    enabled: bool,
    /// Internal line counter 0..=153 (the visible LY differs on line 153).
    line: u8,
    /// Dot within the line; the value is the "current time" T so that after
    /// D calls to [`Self::tick`] the observable state is state(D).
    dot: u16,
    /// First line after LCD enable (no OAM scan, shifted mode 3, 452 dots).
    glitch_line: bool,
    /// LY=LYC comparison flag (STAT bit 2). Frozen while the LCD is off
    /// (`stat_lyc_onoff`).
    cmp: bool,
    /// Current level of the shared STAT interrupt line (IRQ on rising edge:
    /// `stat_irq_blocking`).
    stat_line: bool,
    /// IF bits produced but not yet handed to the interconnect.
    pending_if: u8,
    /// Mode 3 finished on the current line (pixel 160 shipped).
    line_render_done: bool,

    // Window state.
    /// WY==LY matched somewhere this frame while the window was enabled.
    wy_latch: bool,
    /// Window internal line counter.
    win_line: u8,

    render: Render,

    front: Box<[u32; SCREEN_PIXELS]>,
    back: Box<[u32; SCREEN_PIXELS]>,
    dmg_palette: [u32; 4],
}

fn pixel_buffer(fill: u32) -> Box<[u32; SCREEN_PIXELS]> {
    vec![fill; SCREEN_PIXELS]
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

impl Ppu {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            frame_count: 0,
            lcdc: 0,
            stat_en: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0,
            obp0: 0,
            obp1: 0,
            wy: 0,
            wx: 0,
            vbk: 0,
            opri: 0,
            dmg_compat: false,
            bcps: 0,
            ocps: 0,
            bg_pal_ram: [0xFF; 64],
            obj_pal_ram: [0xFF; 64],
            vram: vec![0u8; 0x4000]
                .into_boxed_slice()
                .try_into()
                .unwrap_or_else(|_| unreachable!()),
            oam: [0; 0xA0],
            dma_freeze: None,
            enabled: false,
            line: 0,
            dot: 0,
            glitch_line: false,
            cmp: false,
            stat_line: false,
            pending_if: 0,
            line_render_done: true,
            wy_latch: false,
            win_line: 0,
            render: Render::new(),
            front: pixel_buffer(0xFF_FFFF),
            back: pixel_buffer(0xFF_FFFF),
            dmg_palette: [0xFF_FFFF, 0xAA_AAAA, 0x55_5555, 0x00_0000],
        }
    }

    /// Advance one dot. Returns IF bits to request
    /// (bit 0 = vblank, bit 1 = STAT), 0 if none.
    pub fn tick(&mut self) -> u8 {
        if !self.enabled {
            return std::mem::take(&mut self.pending_if);
        }
        self.dot += 1;
        let len = if self.glitch_line {
            GLITCH_LINE_DOTS
        } else {
            LINE_DOTS
        };
        if self.dot == len {
            self.dot = 0;
            self.glitch_line = false;
            if self.render.win_active {
                self.win_line = self.win_line.wrapping_add(1);
            }
            self.render.win_active = false;
            self.line = if self.line == 153 { 0 } else { self.line + 1 };
            self.start_line();
        }
        self.step_dot();
        self.refresh_stat();
        std::mem::take(&mut self.pending_if)
    }

    /// Write-induced IF bits (STAT/LYC/LCDC writes can raise the STAT line
    /// in the same M-cycle as the write — `stat_lyc_onoff` round 4 needs the
    /// interrupt to dispatch before the next instruction). The interconnect
    /// must OR these into IF after every PPU register write. `tick` drains
    /// the same accumulator, so a missed call only delays the bit by one
    /// M-cycle.
    pub fn consume_pending_irq(&mut self) -> u8 {
        std::mem::take(&mut self.pending_if)
    }

    fn start_line(&mut self) {
        match self.line {
            0 => {
                self.ly = 0;
                self.wy_latch = false;
                self.win_line = 0;
                self.line_render_done = false;
                self.render.active = false;
            }
            1..=143 => {
                self.ly = self.line;
                self.line_render_done = false;
                self.render.active = false;
            }
            144 => {
                self.ly = 144;
                self.frame_count += 1;
                std::mem::swap(&mut self.front, &mut self.back);
            }
            _ => self.ly = self.line,
        }
    }

    fn step_dot(&mut self) {
        if self.line <= 143 {
            // WY latch: the window activates for the rest of the frame once
            // LY==WY is observed while the window is enabled (Pan Docs).
            if self.lcdc & 0x20 != 0 && self.ly == self.wy {
                self.wy_latch = true;
            }
            if self.glitch_line {
                if self.dot == GLITCH_MODE3_START {
                    self.render_init();
                } else if self.render.active {
                    self.render_step();
                }
            } else {
                match self.dot {
                    80 => self.oam_scan(),
                    84 => self.render_init(),
                    d => {
                        if self.render.active && d > 84 {
                            self.render_step();
                        }
                    }
                }
            }
        }
        if self.line == 153 && self.dot == 4 {
            // Line 153 quirk: LY reads 0 from dot 4 (TCAGBD §8.9).
            self.ly = 0;
        }
        if self.line == 144 && self.dot == 4 {
            // VBlank interrupt: 4 dots after LY becomes 144, together with
            // the visible mode 1 (TCAGBD; `vblank_stat_intr-GS`).
            self.pending_if |= 0x01;
        }
    }

    /// LY value the LYC comparator sees, or None while the delayed-LY value
    /// is invalid (comparison flag forced to 0). See module docs.
    fn compare_ly(&self) -> Option<u8> {
        if self.glitch_line {
            // LCD enable: the comparison runs immediately with LY=0
            // (`stat_lyc_onoff` rounds 1-4).
            return Some(0);
        }
        match self.line {
            0 => Some(0),
            153 => match self.dot {
                0..=3 => None,
                4..=7 => Some(153),
                8..=11 => None,
                _ => Some(0),
            },
            _ => {
                if self.dot < 4 {
                    None
                } else {
                    Some(self.line)
                }
            }
        }
    }

    /// STAT mode bits as read through FF41. This is *not* the rendering
    /// state machine: mode reads 0 during the first 4 dots of every line
    /// (and during 144:0-3), and mode 3 appears 4 dots after VRAM read
    /// locking (`lcdon_timing-GS` tables).
    fn vis_mode(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.line >= 144 {
            if self.line == 144 && self.dot < 4 {
                0
            } else {
                1
            }
        } else if self.glitch_line {
            if self.dot < GLITCH_MODE3_START || self.line_render_done {
                0
            } else {
                3
            }
        } else if self.dot < 4 {
            0
        } else if self.dot < 84 {
            2
        } else if !self.line_render_done {
            3
        } else {
            0
        }
    }

    /// Level of the shared STAT interrupt line for the given enable bits.
    fn stat_line_level(&self, en: u8) -> bool {
        let mut high = en & 0x40 != 0 && self.cmp;
        if !self.enabled {
            // With the LCD off only the (frozen) LYC source persists
            // (`stat_lyc_onoff` round 2: no edge across off/on with cmp=1).
            return high;
        }
        let vm = self.vis_mode();
        // HBlank source: follows the visible mode-0 window (including line
        // starts and 144:0-3) so consecutive sources overlap and block each
        // other (`stat_irq_blocking`). The glitched post-enable prefix is
        // not a real hblank.
        high |= en & 0x08 != 0 && vm == 0 && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
        high |= en & 0x10 != 0 && (self.line >= 145 || (self.line == 144 && self.dot >= 4));
        if en & 0x20 != 0 {
            // The OAM source follows the readable mode-2 window: it rises at
            // dot 4 (one M-cycle after the LY change, simultaneous with the
            // LYC compare turning valid) and drops with the mode-3 read flip
            // at dot 84. `intr_2_mode3_timing`/`intr_2_mode0_timing`/
            // `intr_2_oam_ok_timing` measure their events 80/252/252 dots
            // after this IRQ becomes CPU-visible (see module docs).
            let oam_window = self.line <= 143 && !self.glitch_line && (4..84).contains(&self.dot);
            let cgb = self.model.is_cgb();
            // OAM pulse at vblank start: `vblank_stat_intr-GS` (DMG: with
            // vblank), `misc/ppu/vblank_stat_intr-C` (CGB: 4 dots earlier).
            let pulse144 = self.line == 144 && self.dot == if cgb { 0 } else { 4 };
            // DMG: the OAM source also pulses on every later vblank line
            // (`intr_1_2_timing-GS`: mode1→mode2 IRQ distance is 464 dots —
            // one line + 8 dots, i.e. the pulse sits 8 dots after the point
            // where the OAM source rises on visible lines).
            let vblank_pulse = !cgb && (145..=153).contains(&self.line) && self.dot == 12;
            high |= oam_window || pulse144 || vblank_pulse;
        }
        high
    }

    /// Recompute the comparison flag and STAT line; emit IF bit 1 on a
    /// rising edge of the shared line.
    fn refresh_stat(&mut self) {
        if self.enabled {
            self.cmp = self.compare_ly() == Some(self.lyc);
        }
        let level = self.stat_line_level(self.stat_en);
        if level && !self.stat_line {
            self.pending_if |= 0x02;
        }
        self.stat_line = level;
    }

    // --- CPU access blocking (boundaries from lcdon_timing-GS /
    // --- lcdon_write_timing-GS; see module docs) ---

    fn oam_read_blocked(&self) -> bool {
        self.enabled
            && self.line <= 143
            && !self.line_render_done
            && (!self.glitch_line || self.dot >= GLITCH_MODE3_START)
    }

    fn oam_write_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 {
            return false;
        }
        if self.glitch_line {
            return self.dot >= GLITCH_MODE3_START && !self.line_render_done;
        }
        // Writes pass during dots 0-3 and 80-83 (`lcdon_write_timing-GS`).
        (4..80).contains(&self.dot) || (self.dot >= 84 && !self.line_render_done)
    }

    fn vram_read_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.line_render_done {
            return false;
        }
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START
        } else {
            self.dot >= 80
        }
    }

    fn vram_write_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.line_render_done {
            return false;
        }
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START
        } else {
            // Write locking begins 4 dots after read locking
            // (`lcdon_write_timing-GS`: a write at line dot 80 still lands).
            self.dot >= 84
        }
    }

    /// Palette RAM (BCPD/OCPD) is inaccessible while the PPU is reading
    /// palettes, i.e. during (visible) mode 3 (Pan Docs).
    fn pal_ram_blocked(&self) -> bool {
        self.vis_mode() == 3
    }

    /// Read VRAM (0x8000-0x9FFF, current bank), OAM (0xFE00-0xFE9F), or a
    /// PPU register (FF40-FF4B, FF4F, FF68-FF6B). Mode-based access blocking
    /// applies to VRAM/OAM.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                if self.vram_read_blocked() {
                    0xFF
                } else {
                    self.vram[self.vram_index(addr)]
                }
            }
            0xFE00..=0xFE9F => {
                if self.oam_read_blocked() {
                    0xFF
                } else {
                    self.oam[usize::from(addr - 0xFE00)]
                }
            }
            0xFF40 => self.lcdc,
            0xFF41 => 0x80 | self.stat_en | (u8::from(self.cmp) << 2) | self.vis_mode(),
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F if self.model.is_cgb() => 0xFE | self.vbk,
            0xFF68 if self.model.is_cgb() => 0x40 | self.bcps,
            0xFF69 if self.model.is_cgb() => {
                if self.pal_ram_blocked() {
                    0xFF
                } else {
                    self.bg_pal_ram[usize::from(self.bcps & 0x3F)]
                }
            }
            0xFF6A if self.model.is_cgb() => 0x40 | self.ocps,
            0xFF6B if self.model.is_cgb() => {
                if self.pal_ram_blocked() {
                    0xFF
                } else {
                    self.obj_pal_ram[usize::from(self.ocps & 0x3F)]
                }
            }
            0xFF6C if self.model.is_cgb() => 0xFE | self.opri,
            _ => 0xFF,
        }
    }

    /// Write counterpart of [`Self::read`].
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                if !self.vram_write_blocked() {
                    self.vram[self.vram_index(addr)] = value;
                }
            }
            0xFE00..=0xFE9F => {
                if !self.oam_write_blocked() {
                    self.oam[usize::from(addr - 0xFE00)] = value;
                }
            }
            0xFF40 => self.write_lcdc(value),
            0xFF41 => {
                // DMG STAT write bug: the write behaves as if 0xFF were
                // written first for one cycle, enabling every source
                // momentarily (Pan Docs "STAT bug"; CGB is unaffected).
                if !self.model.is_cgb() && self.enabled {
                    let level = self.stat_line_level(0x78);
                    if level && !self.stat_line {
                        self.pending_if |= 0x02;
                    }
                    self.stat_line = level;
                }
                self.stat_en = value & 0x78;
                self.refresh_stat();
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only.
            0xFF45 => {
                self.lyc = value;
                // The comparison retriggers immediately on LYC writes while
                // the comparison clock runs (`stat_lyc_onoff`).
                self.refresh_stat();
            }
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            0xFF4F if self.model.is_cgb() => self.vbk = value & 1,
            0xFF68 if self.model.is_cgb() => self.bcps = value & 0xBF,
            0xFF69 if self.model.is_cgb() => {
                if !self.pal_ram_blocked() {
                    self.bg_pal_ram[usize::from(self.bcps & 0x3F)] = value;
                }
                // Auto-increment happens even when the write is blocked
                // (Pan Docs, "LCD Color Palettes (CGB only)").
                if self.bcps & 0x80 != 0 {
                    self.bcps = 0x80 | (self.bcps.wrapping_add(1) & 0x3F);
                }
            }
            0xFF6A if self.model.is_cgb() => self.ocps = value & 0xBF,
            0xFF6B if self.model.is_cgb() => {
                if !self.pal_ram_blocked() {
                    self.obj_pal_ram[usize::from(self.ocps & 0x3F)] = value;
                }
                if self.ocps & 0x80 != 0 {
                    self.ocps = 0x80 | (self.ocps.wrapping_add(1) & 0x3F);
                }
            }
            0xFF6C if self.model.is_cgb() => self.opri = value & 1,
            _ => {}
        }
    }

    fn write_lcdc(&mut self, value: u8) {
        let was_on = self.lcdc & 0x80 != 0;
        self.lcdc = value;
        let now_on = value & 0x80 != 0;
        if was_on && !now_on {
            // LCD off: LY=0, mode 0, instantly; the comparison clock stops
            // with the flag frozen (`stat_lyc_onoff`); the displayed frame
            // goes white.
            self.enabled = false;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            self.glitch_line = false;
            self.line_render_done = true;
            self.render.active = false;
            self.render.win_active = false;
            let white = self.white();
            self.front.fill(white);
            self.refresh_stat();
        } else if !was_on && now_on {
            // LCD on: glitched first line (`lcdon_timing-GS`); the LYC
            // comparison restarts against LY=0 immediately and can raise
            // the STAT line in this very cycle (`stat_lyc_onoff` round 4).
            self.enabled = true;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            self.glitch_line = true;
            self.line_render_done = false;
            self.render.active = false;
            self.wy_latch = false;
            self.win_line = 0;
            self.refresh_stat();
        }
    }

    fn white(&self) -> u32 {
        if self.model.is_cgb() {
            0xFF_FFFF
        } else {
            self.dmg_palette[0]
        }
    }

    fn vram_index(&self, addr: u16) -> usize {
        usize::from(self.vbk) * 0x2000 + usize::from(addr & 0x1FFF)
    }

    /// OAM write from the DMA engine: ignores mode-based blocking.
    pub fn oam_dma_write(&mut self, index: u8, value: u8) {
        if usize::from(index) < self.oam.len() {
            self.oam[usize::from(index)] = value;
        }
    }

    /// Interconnect wiring: an OAM DMA transfer is frozen mid-byte because
    /// HALT/STOP gated the core clock the DMA controller runs on
    /// (`Some((oam_index, in_flight_source_byte))`), or the freeze ended /
    /// no transfer was in flight (`None`). While frozen, the MGB PPU's OAM
    /// scan sees glitched data derived from the frozen access instead of
    /// real OAM entries (madness/mgb_oam_dma_halt_sprites.s; see
    /// `oam_scan` in render.rs).
    pub fn set_oam_dma_freeze(&mut self, freeze: Option<(u8, u8)>) {
        self.dma_freeze = freeze;
    }

    /// Test hook for the interconnect wiring tests.
    #[cfg(test)]
    pub(crate) fn oam_dma_freeze(&self) -> Option<(u8, u8)> {
        self.dma_freeze
    }

    /// VRAM read for CGB HDMA (no mode blocking — the engine is responsible
    /// for scheduling).
    pub fn vram_read_raw(&self, addr: u16) -> u8 {
        self.vram[self.vram_index(addr)]
    }

    /// VRAM write for CGB HDMA.
    pub fn vram_write_raw(&mut self, addr: u16, value: u8) {
        let i = self.vram_index(addr);
        self.vram[i] = value;
    }

    /// True while the PPU is in a real hblank (mode 3 finished on a visible
    /// line). The interconnect's HDMA engine edge-detects this; the visible
    /// STAT mode-0 window at line starts must not retrigger HBlank DMA.
    pub fn hblank_active(&self) -> bool {
        self.enabled && self.line <= 143 && self.line_render_done
    }

    /// XRGB8888 pixels of the most recently *completed* frame.
    pub fn frame(&self) -> &[u32; SCREEN_PIXELS] {
        &self.front
    }

    /// Completed frames since power-on. With the LCD off this stops
    /// advancing; `GameBoy::run_frame` falls back to a cycle deadline.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Map DMG shades 0..=3 to XRGB8888 (frontend palette option).
    pub fn set_dmg_palette(&mut self, palette: [u32; 4]) {
        self.dmg_palette = palette;
    }

    /// Integration addition: enable DMG compatibility rendering on a CGB
    /// model (CGB hardware running a non-CGB cart). Set once by the
    /// interconnect at power-on; no effect on DMG models.
    pub fn set_dmg_compat(&mut self, compat: bool) {
        self.dmg_compat = compat;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dmg() -> Ppu {
        Ppu::new(Model::Dmg)
    }

    fn cgb() -> Ppu {
        Ppu::new(Model::Cgb)
    }

    /// Tick `n` dots, OR-ing the returned IF bits.
    fn tick_n(p: &mut Ppu, n: u32) -> u8 {
        let mut ifs = 0;
        for _ in 0..n {
            ifs |= p.tick();
        }
        ifs
    }

    /// Tick until the PPU sits at (line, dot); returns OR of IF bits seen.
    fn run_to(p: &mut Ppu, line: u8, dot: u16) -> u8 {
        let mut ifs = 0;
        let mut guard = 0u32;
        while !(p.line == line && p.dot == dot) {
            ifs |= p.tick();
            guard += 1;
            assert!(guard < 200_000, "run_to({line},{dot}) never reached");
        }
        ifs
    }

    // --- lcdon_timing-GS: read state at 4*(c+2) dots after LCD enable ---

    const LCDON_CYCLES: [[u32; 8]; 3] = [
        [0, 17, 60, 110, 130, 174, 224, 244],
        [1, 18, 61, 111, 131, 175, 225, 245],
        [2, 19, 62, 112, 132, 176, 226, 246],
    ];

    fn lcdon_case(lyc: u8, pass: usize, col: usize) -> Ppu {
        let mut p = dmg();
        p.write(0xFF45, lyc);
        p.write(0xFF40, 0x81);
        tick_n(&mut p, 4 * (LCDON_CYCLES[pass][col] + 2));
        p
    }

    fn check_lcdon_table(lyc: u8, addr: u16, expect: &[[u8; 8]; 3]) {
        for pass in 0..3 {
            for col in 0..8 {
                let p = lcdon_case(lyc, pass, col);
                assert_eq!(
                    p.read(addr),
                    expect[pass][col],
                    "pass {pass} col {col} (cycle {})",
                    LCDON_CYCLES[pass][col]
                );
            }
        }
    }

    #[test]
    fn lcdon_ly_table() {
        check_lcdon_table(
            0,
            0xFF44,
            &[
                [0, 0, 0, 0, 1, 1, 1, 2],
                [0, 0, 0, 1, 1, 1, 2, 2],
                [0, 0, 0, 1, 1, 1, 2, 2],
            ],
        );
    }

    #[test]
    fn lcdon_stat_lyc0_table() {
        check_lcdon_table(
            0,
            0xFF41,
            &[
                [0x84, 0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82],
                [0x84, 0x87, 0x84, 0x80, 0x82, 0x80, 0x80, 0x82],
                [0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82, 0x83],
            ],
        );
    }

    #[test]
    fn lcdon_stat_lyc1_table() {
        check_lcdon_table(
            1,
            0xFF41,
            &[
                [0x80, 0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82],
                [0x80, 0x83, 0x80, 0x80, 0x86, 0x84, 0x80, 0x82],
                [0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82, 0x83],
            ],
        );
    }

    #[test]
    fn lcdon_oam_read_table() {
        check_lcdon_table(
            0,
            0xFE00,
            &[
                [0x00, 0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF],
                [0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF, 0xFF],
                [0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF, 0xFF],
            ],
        );
    }

    #[test]
    fn lcdon_vram_read_table() {
        check_lcdon_table(
            0,
            0x8000,
            &[
                [0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00],
                [0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF],
                [0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF],
            ],
        );
    }

    // --- lcdon_write_timing-GS ---

    const WRITE_NOPS: [u32; 19] = [
        0, 17, 18, 60, 61, 110, 111, 112, 130, 131, 132, 174, 175, 224, 225, 226, 244, 245, 246,
    ];

    #[test]
    fn lcdon_oam_write_table() {
        let expect: [u8; 19] = [
            0x81, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81, 0x00, 0x00, 0x81, 0x00, 0x00, 0x81, 0x81,
            0x81, 0x00, 0x00, 0x81, 0x00,
        ];
        for (i, &nops) in WRITE_NOPS.iter().enumerate() {
            let mut p = dmg();
            p.write(0xFF40, 0x81);
            tick_n(&mut p, 4 * (nops + 2));
            p.write(0xFE00, 0x81);
            assert_eq!(p.oam[0], expect[i], "nops {nops}");
        }
    }

    #[test]
    fn lcdon_vram_write_table() {
        let expect: [u8; 19] = [
            0x81, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81, 0x81, 0x81, 0x81, 0x00, 0x00, 0x81, 0x81,
            0x81, 0x81, 0x81, 0x81, 0x00,
        ];
        for (i, &nops) in WRITE_NOPS.iter().enumerate() {
            let mut p = dmg();
            p.write(0xFF40, 0x81);
            tick_n(&mut p, 4 * (nops + 2));
            p.write(0x8000, 0x81);
            assert_eq!(p.vram[0], expect[i], "nops {nops}");
        }
    }

    // --- Line lengths and LY=153 quirk ---

    #[test]
    fn steady_line_boundaries() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        tick_n(&mut p, 451);
        assert_eq!(p.read(0xFF44), 0); // glitch line 0 is 452 dots
        p.tick();
        assert_eq!(p.read(0xFF44), 1);
        tick_n(&mut p, 455);
        assert_eq!(p.read(0xFF44), 1); // state(907)
        p.tick();
        assert_eq!(p.read(0xFF44), 2); // state(908)
    }

    #[test]
    fn ly153_reads_zero_from_dot_4() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 3);
        assert_eq!(p.read(0xFF44), 153);
        p.tick();
        assert_eq!(p.read(0xFF44), 0);
        run_to(&mut p, 0, 0);
        assert_eq!(p.read(0xFF44), 0);
    }

    #[test]
    fn ly153_lyc153_compare_window() {
        let mut p = dmg();
        p.write(0xFF45, 153);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 3);
        assert_eq!(p.read(0xFF41) & 4, 0); // compare invalid dots 0-3
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 4); // dots 4-7 compare vs 153
        tick_n(&mut p, 3);
        assert_eq!(p.read(0xFF41) & 4, 4);
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 0); // dots 8-11 invalid
        tick_n(&mut p, 4);
        assert_eq!(p.read(0xFF41) & 4, 0); // dot 12+: compare vs 0
    }

    #[test]
    fn ly153_lyc0_compare_from_dot_12() {
        let mut p = dmg();
        p.write(0xFF45, 0);
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 11);
        assert_eq!(p.read(0xFF41) & 4, 0);
        assert_eq!(p.tick(), 0x02, "LYC=0 IRQ fires at 153:12");
        assert_eq!(p.read(0xFF41) & 4, 4);
        // The compare stays set through line 0; no further edge.
        assert_eq!(run_to(&mut p, 1, 0) & 2, 0);
    }

    #[test]
    fn lyc_compare_invalid_first_4_dots_of_line() {
        let mut p = dmg();
        p.write(0xFF45, 2);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 2, 0);
        assert_eq!(p.read(0xFF41) & 4, 0);
        tick_n(&mut p, 3);
        assert_eq!(p.read(0xFF41) & 4, 0); // state(2,3)
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 4); // state(2,4)
    }

    // --- VBlank / frame ---

    #[test]
    fn vblank_if_at_144_dot4_and_frame_count_at_dot0() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        assert_eq!(p.frame_count(), 0);
        let ifs = run_to(&mut p, 144, 0);
        assert_eq!(ifs & 1, 0, "no vblank IF before 144:4");
        assert_eq!(p.frame_count(), 1);
        tick_n(&mut p, 3);
        assert_eq!(p.tick() & 1, 1, "vblank IF at state(144,4)");
        // Exactly one vblank IF per frame.
        let ifs = run_to(&mut p, 144, 3);
        assert_eq!(ifs & 1, 0);
        assert_eq!(p.tick() & 1, 1);
        assert_eq!(p.frame_count(), 2);
    }

    #[test]
    fn stat_mode_during_vblank() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 3);
        assert_eq!(p.read(0xFF41) & 3, 0, "144:0-3 still reads mode 0");
        p.tick();
        assert_eq!(p.read(0xFF41) & 3, 1);
        run_to(&mut p, 150, 100);
        assert_eq!(p.read(0xFF41) & 3, 1);
        // OAM and VRAM accessible during vblank (mem_oam).
        p.write(0xFE05, 0x5A);
        assert_eq!(p.read(0xFE05), 0x5A);
        p.write(0x9000, 0xA5);
        assert_eq!(p.read(0x9000), 0xA5);
    }

    // --- STAT interrupt sources ---

    #[test]
    fn mode2_irq_asserts_at_dot_4() {
        let mut p = dmg();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        // No mode-2 source on the glitched line; first edge at line 1,
        // dot 4: the OAM IRQ rises one M-cycle after the LY change,
        // together with the readable mode 2 (see module docs).
        let ifs = run_to(&mut p, 1, 3);
        assert_eq!(ifs & 2, 0);
        assert_eq!(p.tick(), 0x02, "OAM IRQ at state(1,4)");
        // Source stays high through dot 83: no second edge.
        assert_eq!(run_to(&mut p, 1, 200) & 2, 0);
    }

    #[test]
    fn mode0_irq_at_256_plus_scx_fine() {
        for scx in [0u8, 1, 4, 5, 7, 8, 13] {
            let mut p = dmg();
            p.write(0xFF41, 0x08);
            p.write(0xFF43, scx);
            p.write(0xFF40, 0x81);
            run_to(&mut p, 1, 4); // line start: hblank source dropped
            let v0 = 256 + u16::from(scx & 7);
            let ifs = run_to(&mut p, 1, v0 - 1);
            assert_eq!(ifs & 2, 0, "scx {scx}: no hblank IRQ before {v0}");
            assert_eq!(p.tick(), 0x02, "scx {scx}: hblank IRQ at {v0}");
        }
    }

    #[test]
    fn lyc_edge_at_dot4_then_mode2_blocked() {
        let mut p = dmg();
        p.write(0xFF45, 2);
        p.write(0xFF41, 0x60);
        p.write(0xFF40, 0x81);
        // Drain everything up to just before (2,4): with bit5 + bit6, edges
        // happen earlier (line 1 dot 4). At (2,0..3) the line is low (OAM
        // source down, compare invalid).
        run_to(&mut p, 2, 3);
        assert_eq!(p.tick() & 2, 2, "LYC + OAM raise the line at (2,4)");
        let mut got = 0;
        for _ in 0..400 {
            got |= p.tick();
        }
        assert_eq!(got & 2, 0, "one shared edge; no second one this line");
    }

    #[test]
    fn hblank_to_oam_handover_gapless() {
        let mut p = dmg();
        p.write(0xFF41, 0x28); // hblank + OAM sources
        p.write(0xFF40, 0x81);
        run_to(&mut p, 1, 300); // line-1 hblank: line is high
        // The hblank source covers dots 0-3 of the next line and the OAM
        // source rises at dot 4: consecutive sources overlap, so the
        // handover produces no edge (`stat_irq_blocking` semantics), and
        // neither does the OAM drop at 84 (falling). The next edge is the
        // mode-0 source rising when mode 3 ends at dot 256.
        let ifs = run_to(&mut p, 2, 255);
        assert_eq!(ifs & 2, 0, "no edge across the hblank->OAM handover");
        assert_eq!(p.tick() & 2, 2, "edge at the line-2 mode-0 rise (256)");
    }

    #[test]
    fn oam_pulse_at_vblank_start_dmg() {
        let mut p = dmg();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 3);
        let got = p.tick();
        assert_eq!(got, 0x03, "OAM pulse simultaneous with vblank IF (GS)");
    }

    #[test]
    fn oam_pulse_one_cycle_early_cgb() {
        let mut p = cgb();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        let ifs = run_to(&mut p, 143, 455);
        let _ = ifs;
        assert_eq!(p.tick() & 2, 2, "CGB OAM pulse at 144:0");
        tick_n(&mut p, 3);
        assert_eq!(p.tick() & 1, 1, "vblank IF 4 dots later");
    }

    #[test]
    fn vblank_line_oam_pulses_dot12_dmg_only() {
        let mut p = dmg();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 145, 11);
        assert_eq!(p.tick() & 2, 2, "DMG: OAM pulse at 145:12");
        run_to(&mut p, 146, 11);
        assert_eq!(p.tick() & 2, 2, "DMG: OAM pulse at 146:12");

        let mut c = cgb();
        c.write(0xFF41, 0x20);
        c.write(0xFF40, 0x81);
        run_to(&mut c, 145, 0);
        let ifs = run_to(&mut c, 153, 450);
        assert_eq!(ifs & 2, 0, "CGB: no vblank-line OAM pulses");
    }

    #[test]
    fn vblank_source_continuous_through_vblank() {
        let mut p = dmg();
        p.write(0xFF41, 0x10);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 3);
        assert_eq!(p.tick() & 2, 2, "mode-1 source rises at 144:4");
        let ifs = run_to(&mut p, 153, 455);
        assert_eq!(ifs & 2, 0, "no further edge during vblank");
        // Next frame's vblank gives the next edge.
        let ifs = run_to(&mut p, 144, 4);
        assert_eq!(ifs & 2, 2);
    }

    // --- stat_lyc_onoff behaviours ---

    #[test]
    fn lyc_flag_frozen_while_lcd_off() {
        let mut p = dmg();
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 10);
        p.write(0xFF45, 0x90); // LY = LYC = 144
        p.tick();
        assert_eq!(p.read(0xFF41), 0xC5); // cmp set, mode 1 (vblank)
        p.write(0xFF40, 0x01); // LCD off
        assert_eq!(p.read(0xFF41), 0xC4, "flag retained");
        p.write(0xFF45, 0x01);
        assert_eq!(p.read(0xFF41), 0xC4, "comparison clock stopped");
        assert_eq!(p.consume_pending_irq(), 0);
        p.write(0xFF40, 0x81); // LCD on: LY=0 vs LYC=1
        assert_eq!(p.read(0xFF41), 0xC0);
        assert_eq!(p.consume_pending_irq(), 0);
    }

    #[test]
    fn lyc_no_edge_when_comparison_unchanged_across_off_on() {
        let mut p = dmg();
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 10);
        p.write(0xFF45, 0x90);
        p.tick();
        p.consume_pending_irq();
        p.write(0xFF40, 0x01);
        p.write(0xFF45, 0x00); // will match LY=0 on enable
        assert_eq!(p.read(0xFF41), 0xC4);
        p.write(0xFF40, 0x81);
        assert_eq!(p.read(0xFF41), 0xC4);
        assert_eq!(p.consume_pending_irq(), 0, "no edge: flag stayed set");
    }

    #[test]
    fn lyc_irq_on_lcd_enable() {
        let mut p = dmg();
        p.write(0xFF41, 0x40);
        p.write(0xFF45, 0x00);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 10);
        p.consume_pending_irq();
        p.write(0xFF40, 0x01); // off with cmp clear (LY=144 vs 0)
        assert_eq!(p.read(0xFF41), 0xC0);
        p.write(0xFF40, 0x81); // on: LY=0 vs LYC=0 -> rising edge
        assert_eq!(p.read(0xFF41), 0xC4);
        assert_eq!(
            p.consume_pending_irq(),
            0x02,
            "stat_lyc_onoff round 4: IRQ in the enabling write's cycle"
        );
    }

    #[test]
    fn stat_write_bug_dmg_only() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 1, 300); // real hblank, no sources enabled
        assert_eq!(p.read(0xFF41) & 3, 0);
        p.write(0xFF41, 0x00);
        assert_eq!(
            p.consume_pending_irq(),
            0x02,
            "DMG STAT write momentarily enables every source"
        );

        let mut c = cgb();
        c.write(0xFF40, 0x81);
        run_to(&mut c, 1, 300);
        c.write(0xFF41, 0x00);
        assert_eq!(c.consume_pending_irq(), 0, "CGB lacks the STAT write bug");
    }

    // --- LCD off ---

    #[test]
    fn lcd_off_state() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 50, 100);
        p.write(0xFF40, 0x01);
        assert_eq!(p.read(0xFF44), 0);
        assert_eq!(p.read(0xFF41) & 3, 0);
        assert!(p.frame().iter().all(|&px| px == 0xFF_FFFF));
        let fc = p.frame_count();
        tick_n(&mut p, 100_000);
        assert_eq!(p.frame_count(), fc, "frame counter frozen while off");
        assert_eq!(p.read(0xFF44), 0);
        // OAM/VRAM freely accessible.
        p.write(0xFE10, 0x12);
        assert_eq!(p.read(0xFE10), 0x12);
    }

    #[test]
    fn frame_count_steady_period() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 0);
        assert_eq!(p.frame_count(), 1);
        tick_n(&mut p, 70_224);
        assert_eq!(p.frame_count(), 2, "70224 dots per steady frame");
    }
}
