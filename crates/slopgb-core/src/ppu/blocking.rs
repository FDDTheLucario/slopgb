//! Mode/DMA access-blocking predicates (OAM/VRAM/palette read+write) + the DMG OAM-corruption bug. Oracle: gbtr oamdma/late_sp*, blargg oam_bug, mooneye sprite/access.

use super::*;

/// Tier-2 CGB single-speed line-start OAM-read window length (T-cycles), the
/// dots `0..N` over which SameBoy keeps OAM readable before the mode-2 lock
/// engages (`display.c:1805-1810`). See [`Ppu::cgb_linestart_oam_open`].
const CGB_LINESTART_OAM_OPEN: u16 = 4;

/// Tier-2 CGB double-speed line-start OAM-read window length (dots). Under
/// double speed the deferred cc+0 read lands 2 dots earlier in the dot grid
/// (the CPU runs at 2×), so the slopgb-side window shifts down with it to keep
/// the read-position calibration: the accessible `oam_access/preread_ds*_1`
/// read lands `dot0` (open `<2`) while its render-floor `_2` sibling lands
/// `dot2` and stays blocked. SameBoy itself keeps `oam_read_blocked = false`
/// further into DS (`display.c:1789` `!cgb_double_speed` is false, so the lock
/// only engages at the unconditional `:1804`, ~dot8), reading the `_2` access
/// accessible too — but the `_2` digit is the lcd-offset RENDER shift, not the
/// OAM read, so matching SameBoy's wider DS accessibility would corrupt that
/// render-floor output; the window tracks the slopgb read grid that reproduces
/// the SameBoy-passing digits (`_1` → 0, `_2` → 3). See
/// [`Ppu::cgb_linestart_oam_open`].
const CGB_LINESTART_OAM_OPEN_DS: u16 = 2;

/// Tier-2 CGB single-speed palette-RAM accessible window INTO mode 3
/// (T-cycles), the extra dots past the dot-84 mode-3 anchor over which SameBoy
/// keeps `cgb_palettes_blocked = false` before the lock engages
/// (`display.c:1867` false → `:1877` true). See [`Ppu::pal_ram_blocked`].
const PAL_M3START_OPEN: u16 = 3;

impl Ppu {
    /// A CPU VRAM write ATTEMPT landed on this line within the last
    /// 8 dots (the co-temporality discriminator, also used by the DS
    /// line-END release arm in [`Self::vram_read_blocked`]): the write's
    /// M-cycle cost is what SameBoy spreads across the following readback, so
    /// a read this close behind a write keeps the straddle-blocked view (the
    /// `vramw_m3end` want-blocked readbacks) while write-free reads resolve at
    /// their exact deferred position. Consumed by the interconnect's tier2
    /// VRAM-read stamp bypass (`memory.rs`).
    pub(crate) fn vram_wr_recent(&self) -> bool {
        self.vram_wr_line == self.line
            && self.dot >= self.vram_wr_dot
            && self.dot - self.vram_wr_dot < 8
    }

    /// EAGER emergent-flip accessibility release (HALFDOT Part-A, #11dm): the
    /// OAM/VRAM read + write mode-3→0 unblock keyed to the render's OWN
    /// projected flip on the eager clock, REPLACING the tier2 `vis_early`
    /// case-tower boolean for the eager path. The eager `vis_early` fires off
    /// the LE `early_lead = 3` residue (`mode0.rs`) — 2 dots too early — so
    /// extending its release to eager over-releases the `_1` reads (#11dg: CGB
    /// +13/−9). Instead the read's exact half-dot position
    /// ([`Ppu::read_pos_hd`], +8 hd SS read-debt) is compared against the
    /// EMERGENT flip `2 * projected_flip_dot()` (the render's live projection —
    /// SS `dhalf` is always 0, so the whole-dot projection IS the half-dot
    /// flip) plus the 6-hd accessibility lag (SameBoy's OAM/VRAM `m0Time`
    /// trails the mode-0 flip). This resolves the `_1`/`_2` pairs whole-dot on
    /// the eager clock where the boolean cannot: `postread_scx3_2` (rphd 520 ≥
    /// 2·257+6 = 520 → open), `postread_scx5_1` (rphd 520 < 2·259+6 = 524 →
    /// blocked), `postwrite_2_scx3` (write lands) vs `postwrite_1` (rphd 512 <
    /// 2·254+6 = 514 → blocked). SS bare (sprite/window-free) visible lines
    /// only — sprite/window lines carry their own extended exit; the DS floor
    /// is a separate lever. `eager_value`-gated → tier2 + production
    /// byte-identical (never released here).
    fn eager_access_released(&self) -> bool {
        self.eager_value
            && !self.ds
            && !self.glitch_line
            && (1..144).contains(&self.line)
            && self.render.active
            && !self.render.win_active
            && !self.render.win_stalled
            && !self.render.win_aborted
            && self.render.n_sprites == 0
            && !self.wy_latch
            && self.wy2 != self.ly
            && self.read_pos_hd() >= 2 * i32::from(self.projected_flip_dot()) + 6
    }

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
            // EAGER emergent-flip release (the eager twin of the `vis_early`
            // line above; see [`Self::eager_access_released`]).
            && !self.eager_access_released()
            // Tier-2 CGB line-start OAM-read window: SameBoy keeps
            // `oam_read_blocked = false` for the first few T-cycles of each
            // visible line (`display.c:1805-1810` — the mode-0/HBlank tail runs
            // 2+1 cycles before the mode-2 OAM lock engages at state 7, where
            // `oam_read_blocked = !cgb_double_speed`). A deferred cc+0 read
            // landing at line start (the lcd-offset-shifted
            // `oam_access/preread_lcdoffset1_1` read, slopgb `ly2 dot2` vs
            // SameBoy `ly2 cfl0 blk0`) then sees OAM accessible; slopgb locks
            // from dot 0. Release dots `0..K` on CGB under tier2 (the DS window
            // is narrower — the read grid shifts earlier; see
            // `cgb_linestart_oam_open`). Line 0 excluded (post-enable FSM has its
            // own window). Never set in production -> byte-identical OFF.
            && !self.cgb_linestart_oam_open()
            // Tier-2 CGB double-speed line-END OAM-read release (the render
            // mode-3 LENGTH port). SameBoy releases oam/vram reads at
            // the mode-0 transition (`display.c:2118`, one cycle after the
            // `!cgb_double_speed` block SKIPPED under DS), which lands the
            // deferred cc+0 read's unblock at slopgb dot `254 + SCX&7`. slopgb's
            // production block ran to `line_render_done` (~2-3 dots later) so the
            // `postread_ds_2` read (`ly135 dot254`, SameBoy accessible) stayed
            // blocked. See [`Self::ds_lineend_read_open`]. `!ds` in production
            // + `tier2` gate → byte-identical OFF.
            && !self.ds_lineend_open()
    }

    /// Tier-2 CGB double-speed line-END OAM unblock window — see
    /// [`Self::oam_read_blocked`] + [`Self::oam_write_blocked`]. SameBoy's DS
    /// mode-3 lock releases one cycle after the SS release (it skips the
    /// `if (!cgb_double_speed)` early unblock at `display.c:2104-2111` and
    /// drops through to `:2118`), so the deferred cc+0 access observes OAM
    /// accessible from slopgb dot `254 + SCX&7`. slopgb's production block ran
    /// to `line_render_done` (~2 dots later), so the `oam_access/postread_ds_2`
    /// read (`ly135 dot254`, SameBoy accessible) stayed blocked while its `_1`
    /// sibling (dot252, blocked) passed. Extends the SAME boundary to
    /// OAM WRITES (`postwrite_ds_2` write@254 lands / `_1` @252 dropped;
    /// `postwrite_scx1_ds` 256/254 — the write release rides the identical
    /// `254 + SCX&7` law). Bare non-sprite non-window non-glitch lines only (a
    /// sprite/window mode-3 extension pushes the real release later — those
    /// keep `line_render_done`). `tier2_reclock` gate + `!leading_edge_reads`
    /// in production → byte-identical OFF.
    fn ds_lineend_open(&self) -> bool {
        (self.tier2_reclock || self.eager_value)
            && self.model.is_cgb()
            && self.ds
            && self.line >= 1
            && self.line <= 143
            && !self.glitch_line
            && self.render.n_sprites == 0
            && !self.render.win_active
            && self.dot >= 254 + u16::from(self.scx & 7)
    }

    /// Tier-2 (cc+0) CGB line-start OAM-read window — see
    /// [`Self::oam_read_blocked`]. SameBoy carries `oam_read_blocked = false`
    /// from the previous HBlank across the first few dots of a visible line until
    /// the mode-2 lock engages ([`CGB_LINESTART_OAM_OPEN`] single speed /
    /// [`CGB_LINESTART_OAM_OPEN_DS`] double speed). Never true in production /
    /// LE-only (`tier2_reclock` gate) -> byte-identical OFF.
    fn cgb_linestart_oam_open(&self) -> bool {
        // Double speed shifts the read grid 2 dots earlier, so the window does
        // too ([`CGB_LINESTART_OAM_OPEN_DS`]). Verified:
        // `oam_access/preread_ds_lcdoffset1_1` reads `ly2 dot0` (slopgb blocked)
        // where SameBoy reads `ly2 cfl0 rdblk=0` accessible; its render-floor
        // `_2` sibling reads `dot2` and must stay blocked (digit 3).
        // Single speed EXCLUDES dot 0: the base `oam_access/preread_2` reads
        // `ly2 dot0` and wants BLOCKED (out3) — SameBoy's mode-2 OAM lock has
        // engaged by then — while the lcd-offset variant `preread_lcdoffset1_1`
        // reads `ly2 dot2` and wants OPEN (the offset shifts its read off the
        // line start). Opening dots 0-3 served the offset read but wrongly opened
        // the base's dot-0 read; opening only dots 1-3 separates them. Double
        // speed keeps dot 0 open (the DS read grid is 2 dots earlier — the DS
        // `preread_ds_lcdoffset1_1` reads `ly2 dot0` and wants OPEN; the DS base
        // is its own grid).
        // Shifted ROMs classify on the un-shifted frame (law_pos).
        let (_, ld) = self.law_pos();
        let in_window = if self.ds {
            ld < CGB_LINESTART_OAM_OPEN_DS
        } else {
            (1..CGB_LINESTART_OAM_OPEN).contains(&ld)
        };
        (self.tier2_reclock || self.eager_value) && self.model.is_cgb() && self.line != 0 && in_window
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
                // the same coupling as the read side (see `oam_read_blocked`);
                // the DS line-END release rides the same `254 + SCX&7` law as
                // reads ([`Self::ds_lineend_open`]).
                self.dot < 84
                    || (!self.line_render_done
                        && !self.write_unblocked_early()
                        && !self.ds_lineend_open())
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
        (self.tier2_reclock && self.vis_early && !self.glitch_line)
            || self.eager_access_released()
    }

    pub(crate) fn vram_read_blocked(&self) -> bool {
        if !self.enabled
            || self.line > 143
            || self.line_render_done
            // Tier-2: VRAM unblocks coincident with the visible mode→0 flip
            // (`vis_early`); see `oam_read_blocked`. Byte-identical OFF.
            || (self.tier2_reclock && self.vis_early)
            // EAGER emergent-flip release (see [`Self::eager_access_released`]).
            || self.eager_access_released()
            // NOTE: the DS line-END read release is NOT applied to
            // VRAM. `vram_m3/postread_ds_2` (want accessible @dot254) is
            // CO-TEMPORAL with `vramw_m3end/vramw_m3end_ds_2` (want the readback
            // BLOCKED @dot254): the vramw write costs a CPU M-cycle that shifts
            // SameBoy's readback cfl vs the sprite-free postread, but slopgb's
            // deferred frame collapses both to the same dot254 read — so a VRAM
            // release is an A/B swap (+1 postread / −1 vramw). OAM has no
            // write-end readback at that dot, so its release ([`Self::
            // ds_lineend_read_open`], wired only into `oam_read_blocked`) is
            // clean. The VRAM DS read grid is the parked reclock.
        {
            return false;
        }
        // CGB read locking starts 3 dots later than DMG — a read at
        // state(80) still returns data (gambatte vramReadable
        // `lineCycles + ds < 76 + 3*cgb`; SameBoy keeps vram_read_blocked
        // false through the OAM scan on CGB; age vram-read-cgbBCE).
        let late = if self.model.is_cgb() { 3 } else { 0 };
        // Shifted ROMs classify the lock on the un-shifted frame
        // (`vram_m3/preread_lcdoffset1_1` reads dot83 after the +1-dot machine
        // advance where SameBoy still reads open; identity otherwise). The eager
        // clock takes the same STOP-shift frame as tier2 (and as `pal_ram_blocked`
        // already does): measured `preread_lcdoffset1_1` law-dot82 opens where the
        // raw dot83 blocked, `_2` law-dot86 stays blocked — a clean separation.
        let d = if self.tier2_reclock || self.eager_value {
            self.law_pos().1
        } else {
            self.dot
        };
        // The DS tier2 VRAM read frame: (a) the deferred cc+0
        // read's true DS sample sits +3 T late, so the mode-3 entry lock
        // covers it from dot 80 (the SS +3 CGB grace does not apply on the
        // DS grid: `preread_ds_2` reads dot82 wanting BLOCKED); (b) the
        // line-END release mirrors the OAM release at 254 + SCX&7,
        // EXCEPT within 8 dots of a same-line CPU VRAM write attempt (the
        // write's M-cycle cost SameBoy spreads across the readback —
        // `vramw_m3end_ds_2` stays blocked where the write-free
        // `prewrite_ds`/`postread_ds` readbacks are open).
        if (self.tier2_reclock || self.eager_value) && self.model.is_cgb() && !self.glitch_line {
            // Line-END VRAM read release at the bare exit, BOTH speeds: the
            // SS refusal ("co-temporal with vramw_m3end") is resolved
            // by the wr_recent discriminator — the vramw readback follows
            // its own VRAM write within 2 M-cycles (the write's M-cycle
            // cost SameBoy spreads across the readback) and stays blocked,
            // while the write-free postread/prewrite readbacks open
            // (`postread_scx5_ds_2` measures in SINGLE speed at dot260).
            let wr_recent = self.vram_wr_line == self.line
                && self.dot >= self.vram_wr_dot
                && self.dot - self.vram_wr_dot < 8;
            if self.line >= 1
                && self.line <= 143
                && self.render.n_sprites == 0
                && !self.render.win_active
                && d >= 254 + u16::from(self.scx & 7)
                && !wr_recent
            {
                return false;
            }
            // The wxA6 window line-END VRAM read release, SS. The
            // WX=166 quirk window "activates during HBlank" (SameBoy
            // `wx_166_interrupt_glitch`); its VRAM unblock CO-MOVES with the
            // CGB visible exit (asm_window_gdma Row 6: unblock ∈
            // (T0+256,T0+260], measured slopgb-frame 259) — one bucket LATER
            // than DMG, and NOT keyed to the m0 IF rise (which fires while
            // VRAM is still locked). slopgb held the lock to
            // `line_render_done` (261) so `m2int_wxA6_vrambusyread_3`'s
            // dot-260 read stayed blocked (want open, out5). wxA6-SCOPED —
            // the generic win-line release was the vramw A/B.
            if !self.ds
                && (1..=143).contains(&self.line)
                && self.render.win_active
                && self.eff.wx == 0xA6
                && d >= 259 + u16::from(self.scx & 7)
            {
                return false;
            }
            if self.ds {
                // The `preread_ds_1`/`_2` pair straddles the entry lock (dot80
                // open / dot82 blocked): lock from 82 on the DS grid. Shifted
                // ROMs' polls land +3 dots for the +1-dot machine advance (the
                // poll quantum), so their law frame under-corrects by 1: the
                // boundary moves to 83 (`preread_ds_lcdoffset1_1` law-dot82
                // wants OPEN where the unshifted `_2` real-dot82 wants BLOCKED).
                return d >= 82 + u16::from(self.lcd_shift_dots > 0);
            }
        }
        if self.glitch_line {
            d >= GLITCH_MODE3_START + late
        } else {
            d >= 80 + late
        }
    }

    pub(super) fn vram_write_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.line_render_done {
            return false;
        }
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START
        } else {
            // DS tier2: the deferred write's true DS sample
            // sits later on the dot grid, so the mode-3 write lock covers it
            // from dot 82 (`prewrite_ds_2` wants its ~dot82 write BLOCKED
            // while `_1`'s earlier write lands).
            if (self.tier2_reclock || self.eager_value) && self.model.is_cgb() && self.ds {
                // Same shifted-poll-quantum +1 as the read lock. (A line-END
                // write release twin was built + REVERTED: it fixed nothing
                // and broke a vramw_m3end want-dropped write, measured.)
                let (_, wd) = self.law_pos();
                return wd >= 82 + u16::from(self.lcd_shift_dots > 0)
                    && !self.write_unblocked_early();
            }
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
        if !self.enabled || self.line > 143 {
            return false;
        }
        if self.render_finished {
            // The palette-RAM unblock trails the pipe end
            // by 1 dot at single speed / 0 in double speed on the deferred
            // (cc+0) frame. The full `cgbpal_m3end` constraint table:
            // SS reads at
            // `pipe_end` are blocked (`m3end_1` @256, `scx2_1` @256,
            // `scx5_1` @260) while reads at/past `pipe_end + 1` are open
            // (`_2` @260 with pipe 256/258/259, `scx5_2` @264 with 261 —
            // Δ = +2 is REFUTED by `scx3_2` read@260 pipe 259); DS reads
            // open AT the pipe end (`m3end_ds_2` @256 pipe 256,
            // `scx5_ds_2` @262 pipe 261) while pre-pipe reads stay blocked
            // (`ds_1` @254, `scx5_ds_1` @260 < 261). The production path
            // returns open here (the whole-M-cycle `pal_access_edge` stamp in
            // `interconnect/memory.rs` carries its cc+4 straddle law);
            // `pal_open_dot` is never read flag-off → byte-identical.
            return (self.tier2_reclock || self.eager_value)
                && self.model.is_cgb()
                && !self.glitch_line
                && self.pal_open_dot != 0
                && self.dot < self.pal_open_dot + u16::from(!self.ds);
        }
        let lock = if self.glitch_line {
            GLITCH_MODE3_START
        } else if (self.tier2_reclock || self.eager_value) && self.model.is_cgb() && !self.ds {
            // The tier2 SS mode-3 entry lock is the
            // mode-3 anchor 84 itself: the `*_m3start_2` triplet (SameBoy-pass)
            // accesses at dot 84 and wants BLOCKED (read FF / write dropped)
            // while the `_1` legs at dot 80 land — so the base boundary is 84,
            // NOT the previous `84 + PAL_M3START_OPEN` grace, which sacrificed
            // the triplet to serve the STOP-shifted rows. The +3 grace applies
            // ONLY to shifted ROMs (`cgbpal_m3/*_m3start_lcdoffset1_1`'s
            // law-dot-85 access must stay open — the shifted poll lands +3
            // dots per +1-dot machine advance, the poll-quantum law, so
            // its law frame under-corrects and the boundary sits at 87 there).
            // This also subsumes the first-frame (`frame_skip`)
            // arm — 84 is now the base for every unshifted frame.
            if self.lcd_shift_dots > 0 {
                84 + PAL_M3START_OPEN
            } else {
                84
            }
        } else {
            84
        };
        // Shifted ROMs classify the access on the un-shifted frame
        // (the machine STOPADV advance; identity otherwise).
        if (self.tier2_reclock || self.eager_value) && self.model.is_cgb() && !self.ds {
            let (_, ld) = self.law_pos();
            return ld >= lock;
        }
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
