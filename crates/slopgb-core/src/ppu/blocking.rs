//! Mode/DMA access-blocking predicates (OAM/VRAM/palette read+write) + the DMG OAM-corruption bug. Oracle: gbtr oamdma/late_sp*, blargg oam_bug, mooneye sprite/access.

use super::*;

/// Tier-2 CGB single-speed line-start OAM-read window length (T-cycles), the
/// dots `0..N` over which SameBoy keeps OAM readable before the mode-2 lock
/// engages (`display.c:1805-1810`). See [`Ppu::cgb_linestart_oam_open`].
const CGB_LINESTART_OAM_OPEN: u16 = 4;

/// Tier-2 CGB single-speed palette-RAM accessible window INTO mode 3
/// (T-cycles), the extra dots past the dot-84 mode-3 anchor over which SameBoy
/// keeps `cgb_palettes_blocked = false` before the lock engages
/// (`display.c:1867` false → `:1877` true). See [`Ppu::pal_ram_blocked`].
const PAL_M3START_OPEN: u16 = 3;

impl Ppu {
    pub(crate) fn oam_read_blocked(&self) -> bool {
        self.enabled
            && self.line <= 143
            && !self.line_render_done
            && (!self.glitch_line || self.dot >= GLITCH_MODE3_START)
            // Tier-2 (cc+0 leading-edge): SameBoy unblocks OAM/VRAM reads
            // COINCIDENT with the visible mode→0 flip (`vis_early`), not at the
            // render-done dispatch (`line_render_done`) 1 dot later. The deferred
            // cc+0 read then sees mode 0 yet OAM still locked, rendering "3" where
            // SameBoy reads accessible (oam_access/vram_m3 postread_scx3). Release
            // on `vis_early`. Never set in production → byte-identical OFF.
            && !(self.tier2_reclock && self.vis_early)
            // Tier-2 CGB line-start OAM-read window: SameBoy keeps
            // `oam_read_blocked = false` for the first few T-cycles of each
            // visible line (`display.c:1805-1810` — the mode-0/HBlank tail runs
            // 2+1 cycles before the mode-2 OAM lock engages at state 7, on CGB
            // single-speed where `oam_read_blocked = !cgb_double_speed`). A
            // deferred cc+0 read landing at line start (the lcd-offset-shifted
            // `oam_access/preread_lcdoffset1_1` read, slopgb `ly2 dot2` vs
            // SameBoy `ly2 cfl0 blk0`) then sees OAM accessible; slopgb locks
            // from dot 0. Release dots `0..K` on CGB single-speed under tier2.
            // Line 0 excluded (post-enable FSM has its own window). Never set in
            // production -> byte-identical OFF.
            && !self.cgb_linestart_oam_open()
    }

    /// Tier-2 (cc+0) CGB single-speed line-start OAM-read window — see
    /// [`Self::oam_read_blocked`]. SameBoy carries `oam_read_blocked = false`
    /// from the previous HBlank across the first `CGB_LINESTART_OAM_OPEN`
    /// T-cycles of a visible line until the mode-2 lock engages. Never true in
    /// production / LE-only (`tier2_reclock` gate) -> byte-identical OFF.
    fn cgb_linestart_oam_open(&self) -> bool {
        self.tier2_reclock
            && self.model.is_cgb()
            && !self.ds
            && self.line != 0
            && self.dot < CGB_LINESTART_OAM_OPEN
    }

    pub(crate) fn oam_write_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 {
            return false;
        }
        if self.glitch_line {
            return self.dot >= GLITCH_MODE3_START && !self.line_render_done;
        }
        if self.model.is_cgb() {
            // CGB: line-start dots 0-3 block writes too, unless the
            // previous line was a vblank line (line 0 here — gambatte
            // oamWritable's `lineCycles + 3 + cgb >= 456` arm falls back
            // to `ly >= 143`, and lyCounter still reads 153 there), and
            // the DMG dots-80-83 writable gap does not exist (the
            // `lineCycles == 76 && !cgb` escape; SameBoy raises
            // oam_write_blocked at CGB line starts; age oam-write-cgbBCE).
            return if self.dot < 4 {
                self.line != 0
            } else {
                // Tier-2: the mode3→0 write-unblock coincides with the visible
                // mode→0 flip (`vis_early`), one dot before `line_render_done` —
                // the same coupling as the read side (see `oam_read_blocked`).
                self.dot < 84 || (!self.line_render_done && !self.write_unblocked_early())
            };
        }
        // Writes pass during dots 0-3 and 80-83 (`lcdon_write_timing-GS`).
        (4..80).contains(&self.dot)
            || (self.dot >= 84 && !self.line_render_done && !self.write_unblocked_early())
    }

    /// Tier-2 mode3→0 write-unblock coincides with the visible mode→0 flip
    /// (`vis_early`), one dot before the render-done dispatch (`line_render_done`)
    /// — the same coupling as [`Self::oam_read_blocked`], for writes. Glitch
    /// lines excluded so `lcdon_write_timing-GS` (the line-start dots 80-83 gap)
    /// is untouched. Never set in production / LE-only → byte-identical OFF.
    fn write_unblocked_early(&self) -> bool {
        self.tier2_reclock && self.vis_early && !self.glitch_line
    }

    pub(crate) fn vram_read_blocked(&self) -> bool {
        if !self.enabled
            || self.line > 143
            || self.line_render_done
            // Tier-2: VRAM unblocks coincident with the visible mode→0 flip
            // (`vis_early`); see `oam_read_blocked`. Byte-identical OFF.
            || (self.tier2_reclock && self.vis_early)
        {
            return false;
        }
        // CGB read locking starts 3 dots later than DMG — a read at
        // state(80) still returns data (gambatte vramReadable
        // `lineCycles + ds < 76 + 3*cgb`; SameBoy keeps vram_read_blocked
        // false through the OAM scan on CGB; age vram-read-cgbBCE).
        let late = if self.model.is_cgb() { 3 } else { 0 };
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START + late
        } else {
            self.dot >= 80 + late
        }
    }

    pub(super) fn vram_write_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.line_render_done {
            return false;
        }
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START
        } else {
            // Write locking begins 4 dots after read locking
            // (`lcdon_write_timing-GS`: a write at line dot 80 still lands), and
            // ends on the visible mode→0 flip under Tier-2 (`write_unblocked_early`).
            self.dot >= 84 && !self.write_unblocked_early()
        }
    }

    /// Palette RAM (BCPD/OCPD) is inaccessible while the PPU is reading
    /// palettes, i.e. during mode 3 (Pan Docs). Anchored at the *render*
    /// end (dot D), not the visible mode-0 read flip 3 dots earlier — the
    /// gambatte cgbpal_m3 write-window calibration sits on this anchor.
    pub(super) fn pal_ram_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.render_finished {
            return false;
        }
        let lock = if self.glitch_line {
            GLITCH_MODE3_START
        } else if self.tier2_reclock && self.model.is_cgb() && !self.ds {
            // Tier-2 CGB m3-start palette window: SameBoy keeps
            // `cgb_palettes_blocked = false` for `PAL_M3START_OPEN` T-cycles
            // INTO mode 3 (`display.c:1867` false → `:1877` true, a 3-cycle
            // SLEEP between), so a deferred read/write landing at the mode-3
            // entry (the lcd-offset-shifted `cgbpal_m3/*_m3start_lcdoffset1_1`
            // access — slopgb `ly1 dot86` vs SameBoy's ~cfl87 lock) still sees
            // palettes accessible. slopgb locks at dot 84; extend the lock.
            // Never extended in production / LE-only → byte-identical OFF.
            84 + PAL_M3START_OPEN
        } else {
            84
        };
        self.dot >= lock
    }

    /// Byte base (8..=0x98) of the OAM row the mode-2 scan makes
    /// vulnerable to the DMG OAM corruption bug for an access observing
    /// the current state, or `None` outside the scan.
    ///
    /// Anchoring (the one free parameter, calibrated against blargg's
    /// oam_bug ROMs, which are the only hardware oracle in the corpus):
    /// under tick-then-access an access at state(T) covers dots T-4..T.
    /// 4-scanline_timing pins the first corrupting INC DE of a visible
    /// line to the cycle covering dots 0-3 and the last to 72-75, with
    /// 76-79 already clean; 5-timing_bug confirms dots 0-3 on lines 0, 1
    /// and 143; 6-timing_no_bug brackets every visible line and hammers
    /// vblank. That is 19 corruptible M-cycles for the 19 corruptible
    /// rows 1..=19, so the access at state(T) corrupts row T/4, base
    /// (T/4)*8, for T in 4..80. The row-per-cycle mapping is pinned by
    /// 8-instr_effect's OAM-dump CRCs and by 7-timing_effect's expected
    /// CRC $7D792E7C, which is reproduced exactly by simulating the
    /// ROM's checksummed output for this mapping (the shipped single
    /// itself self-destructs — see the baseline note in
    /// tests/gbtr/blargg.rs). No scan runs on vblank lines or the
    /// 452-dot LCD-enable glitch line (lcdon_timing-GS), and rows 0xA0
    /// bytes apart never reach row 0 (Pan Docs: the first row is never
    /// the corrupted row; SameBoy guards `accessed_oam_row >= 8`).
    pub(crate) fn oam_bug_row(&self) -> Option<u8> {
        if !self.enabled || self.line > 143 || self.glitch_line || !(4..80).contains(&self.dot) {
            return None;
        }
        Some((self.dot / 4 * 8) as u8)
    }

    /// Apply the DMG OAM corruption bug for an access of the given kind
    /// happening this M-cycle. The interconnect gates on model family,
    /// address range, halt state and OAM DMA; everything PPU-positional
    /// is decided here via [`Self::oam_bug_row`].
    pub(crate) fn oam_bug(&mut self, kind: OamBugKind) {
        let Some(row) = self.oam_bug_row() else {
            return;
        };
        let row = usize::from(row);
        match kind {
            OamBugKind::Write => oam_bug_write_pattern(&mut self.oam, row),
            OamBugKind::Read => oam_bug_read_pattern(&mut self.oam, row),
            OamBugKind::ReadIncrease => {
                // The special pattern only fires for rows 4..=18 (SameBoy
                // v0.12.1 guards 0x20 <= row < 0x98); the plain read
                // corruption of the read itself applies regardless — a
                // no-op when the special pattern's row copies ran.
                if (0x20..0x98).contains(&row) {
                    oam_bug_read_increase_pattern(&mut self.oam, row);
                }
                oam_bug_read_pattern(&mut self.oam, row);
            }
        }
    }
}
