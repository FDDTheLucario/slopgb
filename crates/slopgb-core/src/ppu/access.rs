use super::*;

impl Ppu {
    // --- CPU access blocking (boundaries from lcdon_timing-GS /
    // --- lcdon_write_timing-GS; see module docs) ---

    // --- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ---

    pub(super) fn white(&self) -> u32 {
        if self.model.is_cgb() {
            0xFF_FFFF
        } else {
            self.dmg_palette[0]
        }
    }

    pub(super) fn vram_index(&self, addr: u16) -> usize {
        usize::from(self.vbk) * 0x2000 + usize::from(addr & 0x1FFF)
    }

    /// The live CGB VRAM bank (FF4F bit 0; always 0 on DMG), for the bank-aware
    /// CDL layout. Side-effect-free.
    pub(crate) fn vram_bank(&self) -> usize {
        usize::from(self.vbk & 1)
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
    /// `mgb_dma_freeze_glitch_entry` in render.rs).
    /// Interconnect wiring: CGB double speed engaged/left (see `ds`).
    pub(crate) fn set_double_speed(&mut self, ds: bool) {
        self.ds = ds;
    }

    pub fn set_oam_dma_freeze(&mut self, freeze: Option<(u8, u8)>) {
        self.dma_freeze = freeze;
    }

    /// Interconnect wiring: the OAM DMA controller owns (true) or released
    /// (false) OAM for the coming M-cycle's dots — see the
    /// [`Self::oam_dma_active`] field docs for the scan semantics and the
    /// gambatte derivation of the level's edges.
    pub(crate) fn set_oam_dma_active(&mut self, active: bool) {
        self.oam_dma_active = active;
    }

    /// Test hook for the interconnect wiring tests.
    #[cfg(test)]
    pub(crate) fn oam_dma_freeze(&self) -> Option<(u8, u8)> {
        self.dma_freeze
    }

    /// Test hook for the interconnect wiring tests: the scan's OAM view is
    /// disconnected for the current M-cycle's dots.
    #[cfg(test)]
    pub(crate) fn oam_dma_scan_disconnected(&self) -> bool {
        self.oam_dma_active
    }

    /// Test hook: raw (BG, OBJ) palette RAM. FF69/FF6B reads are gated on
    /// CGB mode by the interconnect and on mode 3 here, so the post-boot
    /// palette tests need an ungated view.
    #[cfg(test)]
    pub(crate) fn palette_ram(&self) -> (&[u8; 64], &[u8; 64]) {
        (&self.bg_pal_ram, &self.obj_pal_ram)
    }

    /// VRAM read for CGB HDMA (no mode blocking — the engine is responsible
    /// for scheduling). Doubles as the active-bank view for the
    /// interconnect's side-effect-free `peek`.
    pub fn vram_read_raw(&self, addr: u16) -> u8 {
        self.vram[self.vram_index(addr)]
    }

    /// OAM read ignoring mode-based and DMA blocking, for the
    /// interconnect's side-effect-free `peek`.
    pub(crate) fn oam_read_raw(&self, addr: u16) -> u8 {
        self.oam[usize::from(addr - 0xFE00)]
    }

    /// Power-on init for VRAM (both CGB banks) + OAM: overwrite every byte with
    /// `f()`. Used by [`crate::GameBoy::init_ram`] for the seeded-random power-on
    /// (authentic garbage tiles at boot). Golden-safe: never on a `new` machine.
    pub(crate) fn fill_video_ram(&mut self, mut f: impl FnMut() -> u8) {
        for b in self.vram.iter_mut().chain(self.oam.iter_mut()) {
            *b = f();
        }
    }

    /// Whole 16 KiB VRAM for the debug VRAM viewer (bank 0 in `[..0x2000]`,
    /// bank 1 in `[0x2000..]`). Side-effect-free.
    pub(crate) fn debug_vram(&self) -> &[u8; 0x4000] {
        &self.vram
    }

    /// Raw 160-byte OAM (40 sprites x 4 bytes), for the debug OAM viewer.
    /// Side-effect-free.
    pub(crate) fn debug_oam(&self) -> &[u8; 0xA0] {
        &self.oam
    }

    /// Raw CGB palette RAM `(BG, OBJ)`, for the debug I/O viewer. Unlike
    /// [`Self::palette_ram`] (test-only) this is available in non-test builds.
    /// Side-effect-free.
    pub(crate) fn debug_palette_ram(&self) -> (&[u8; 64], &[u8; 64]) {
        (&self.bg_pal_ram, &self.obj_pal_ram)
    }

    /// VRAM write for CGB HDMA.
    pub fn vram_write_raw(&mut self, addr: u16, value: u8) {
        let i = self.vram_index(addr);
        self.vram[i] = value;
    }

    /// Write an **explicit** VRAM bank (0/1) for the debug memory editor's
    /// bank browser, independent of the live VBK. Side-effect-free aside from
    /// the poked byte; debug-only. Mirrors [`Self::debug_vram`]'s layout.
    pub(crate) fn debug_vram_write(&mut self, bank: u16, addr: u16, value: u8) {
        self.vram[usize::from(bank & 1) * 0x2000 + usize::from(addr & 0x1FFF)] = value;
    }

    /// True while the PPU is in a real hblank (mode 3 finished on a visible
    /// line); the visible STAT mode-0 window at line starts is excluded.
    /// The HBlank DMA engine edge-detects [`Self::hdma_trigger_level`]
    /// (this level led by one dot) instead.
    pub fn hblank_active(&self) -> bool {
        self.enabled && self.line <= 143 && self.render_finished
    }

    /// The HBlank DMA trigger level: the real hblank of a visible line,
    /// led by one dot (see [`Self::hdma_lead`]). The interconnect's
    /// per-dot edge detector flags one block request per rising edge.
    /// Anchored at the render end (dot D−1 via the lead), independent of
    /// the visible mode-0 read flip at D−3 (gambatte-core derives
    /// `predictedNextM0Time` from the pixel pipe, and the dma/hdma_start
    /// `_1`/`_2` pairs pin it there).
    pub(crate) fn hdma_trigger_level(&self) -> bool {
        self.enabled && self.line <= 143 && (self.render_finished || self.hdma_lead)
    }

    /// The HBlank DMA trigger window: inside a visible line's hblank (as
    /// [`Self::hdma_trigger_level`] sees it), ending 3 dots before the
    /// line ends (gambatte-core video.cpp `isHdmaPeriod`:
    /// `ly < 144 && cc + 3 + 3 * ds < lyCounter.time() && cc >= m0Time` —
    /// the cc margin is 3 dots at either speed, and the m0 time derives
    /// from the same led `predictedNextM0Time` anchor). The interconnect
    /// consults this when HBlank DMA is enabled mid-window and when a
    /// halt/stop wake re-evaluates a pending block.
    /// This line's dot length: 452 on the LCD-enable glitch line, else 456.
    #[inline]
    pub(super) fn line_len(&self) -> u16 {
        if self.glitch_line {
            GLITCH_LINE_DOTS
        } else {
            LINE_DOTS
        }
    }

    pub(crate) fn hdma_period(&self) -> bool {
        let len = self.line_len();
        self.hdma_trigger_level() && self.dot + 3 < len
    }

    /// [`Self::hdma_period`] classified on the un-shifted frame for
    /// CPU-instant consults (FF55 arming, halt-entry snapshot, wake re-eval,
    /// STOP window): a shifted entry near the line end mis-reads the 3-dot
    /// margin (`hdma_late_m0halt_lcdoffset3_1` enters halt at dot 455 where
    /// the un-shifted frame is dot 452 — still inside). Cross-line law
    /// positions keep the conservative false. Identity when unshifted; the
    /// per-dot machine edge detector keeps the real [`Self::hdma_period`].
    pub(crate) fn hdma_period_law(&self) -> bool {
        if self.lcd_shift_dots == 0 {
            return self.hdma_period();
        }
        let len = self.line_len();
        let (ll, ld) = self.law_pos();
        self.hdma_trigger_level() && ll == self.line && ld + 3 < len
    }

    /// LCDC bit 7 as committed (architectural view).
    pub(crate) fn lcd_enabled(&self) -> bool {
        self.enabled
    }

    /// The PPU's current `(line, dot)` scan position. Pure accessor (no
    /// behaviour); the deferred-read position laws line slopgb's read dot up
    /// against SameBoy's `cycles_for_line`.
    pub(crate) fn scan_pos(&self) -> (u8, u16) {
        (self.line, self.dot)
    }

    /// Whether the LCD-enable sub-dot offset is active (`lcd_shift_dots != 0`):
    /// the CPU/PPU whole-dot grid is shifted, so a whole-dot write-commit
    /// borrow does not map cleanly. Pure accessor.
    pub(crate) fn lcd_shift_active(&self) -> bool {
        self.lcd_shift_dots != 0
    }

    /// Whether the PPU is on the LCD-enable glitch line (452 dots, dot-82
    /// pipe). The SCX write-strobe staging commits immediately there rather than
    /// deferring — the glitch line's render geometry carries its own calibrated
    /// offsets (`GLITCH_MODE3_START` 78 entry, +2 `early_lead`), and the +4
    /// render-frame lag mis-frames its fine-scroll hunt
    /// (`enable_display/ly0_late_scx7_m3stat_*`).
    pub(crate) fn glitch_active(&self) -> bool {
        self.glitch_line
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

    /// Graphics → "disable SGB colors": render the SGB game screen in plain DMG
    /// palette instead of the SGB per-cell colors. Default off (golden-safe).
    pub fn set_sgb_mono(&mut self, on: bool) {
        self.sgb_mono = on;
    }

    /// Integration addition: enable DMG compatibility rendering on a CGB
    /// model (CGB hardware running a non-CGB cart). Set once by the
    /// interconnect at power-on; no effect on DMG models.
    pub fn set_dmg_compat(&mut self, compat: bool) {
        self.dmg_compat = compat;
    }
}
