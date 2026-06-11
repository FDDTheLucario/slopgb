//! Dot-accurate PPU with pixel FIFO. PPU work package.
//!
//! Stepped one dot (T-cycle) at a time by the interconnect. Mode timing must
//! be exact: variable-length mode 3 (SCX fine scroll, window, sprite fetch
//! stalls), STAT interrupt line blocking, LY=153→0 early wrap, LCD-enable
//! first-frame quirks (mooneye `acceptance/ppu/*`, `lcdon_*`).
//!
//! Renders DMG (4-shade via BGP/OBP through a configurable RGB palette) and
//! CGB (BG/OBJ palette RAM, VRAM bank 1 attributes, master priority via OPRI).

use crate::SCREEN_PIXELS;
use crate::model::Model;

pub struct Ppu {
    frame_count: u64,
    // PPU work package owns all further state.
}

impl Ppu {
    pub fn new(model: Model) -> Self {
        let _ = model;
        Self { frame_count: 0 }
    }

    /// Advance one dot. Returns IF bits to request
    /// (bit 0 = vblank, bit 1 = STAT), 0 if none.
    pub fn tick(&mut self) -> u8 {
        todo!("PPU work package")
    }

    /// Read VRAM (0x8000-0x9FFF, current bank), OAM (0xFE00-0xFE9F), or a
    /// PPU register (FF40-FF4B, FF4F, FF68-FF6B). Mode-based access blocking
    /// applies to VRAM/OAM.
    pub fn read(&self, addr: u16) -> u8 {
        let _ = addr;
        todo!("PPU work package")
    }

    /// Write counterpart of [`Self::read`].
    pub fn write(&mut self, addr: u16, value: u8) {
        let _ = (addr, value);
        todo!("PPU work package")
    }

    /// OAM write from the DMA engine: ignores mode-based blocking.
    pub fn oam_dma_write(&mut self, index: u8, value: u8) {
        let _ = (index, value);
        todo!("PPU work package")
    }

    /// VRAM read for CGB HDMA (no mode blocking — the engine is responsible
    /// for scheduling).
    pub fn vram_read_raw(&self, addr: u16) -> u8 {
        let _ = addr;
        todo!("PPU work package")
    }

    /// VRAM write for CGB HDMA.
    pub fn vram_write_raw(&mut self, addr: u16, value: u8) {
        let _ = (addr, value);
        todo!("PPU work package")
    }

    /// XRGB8888 pixels of the most recently *completed* frame.
    pub fn frame(&self) -> &[u32; SCREEN_PIXELS] {
        todo!("PPU work package")
    }

    /// Completed frames since power-on. With the LCD off this stops
    /// advancing; `GameBoy::run_frame` falls back to a cycle deadline.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Map DMG shades 0..=3 to XRGB8888 (frontend palette option).
    pub fn set_dmg_palette(&mut self, palette: [u32; 4]) {
        let _ = palette;
        todo!("PPU work package")
    }
}
