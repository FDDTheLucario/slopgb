//! Window machine: WX comparator (pause-aware, edge-triggered), window line counter, mid-line LCDC.5 abort. Oracle: gbtr m3_wx_*/window/m0enable, mealybug m3_window_timing*.

use super::*;

impl Ppu {
    /// The window trigger: the WX position comparator runs every dot
    /// (gambatte ppu.cpp plotPixel: `wx == xpos`, xpos < 168), checked
    /// *before* the same-dot sprite trigger (see the call site). Returns
    /// true when the caller's render_step must end (a start stall or a
    /// reactivation pixel consumed the dot). The comparator also runs
    /// through the 8-dot prefill — so WX 0-7 match before any pixel
    /// pops; from the first pop on, a match at WX >= 8 lands the first
    /// window pixel at lx = WX-7. The wx+6 prefill anchor is pinned by
    /// the m3_window_timing reference photographs: every WX 0-7 line
    /// pops pixel 0 at dot 103 — the same 6-dot-delayed schedule as
    /// WX 8-10 — so trigger + 6-dot restart + (7-WX)-pixel discard must
    /// sum to 19 prefill dots. The machine is gated on LCDC.5 + the WY
    /// latch only: LCDC.0 blanks pixels at output but does not stop the
    /// window fetch (gambatte lcdcWinEn).
    pub(super) fn window_trigger_step(&mut self) -> bool {
        // The position counter the WX comparator runs against advances
        // only on dots the pipeline advances: sprite-fetch stalls freeze
        // it (and the stall returns in render_step skip this increment
        // on the trigger dot itself), so a WX 0-7 match shifts later by
        // the stall instead of skipping its comparison dot
        // (m3_lcdc_win_map_change2's per-line X<8 sprites).
        self.render.pos_dot += 1;
        let wx = self.eff.wx;
        let win_match = if wx <= 7 {
            self.render.pos_dot == u16::from(wx) + 6
        } else {
            wx <= 166 && self.render.lx == wx - 7
        };
        // The WY condition: the frame-sticky latch OR a live match
        // against the delayed WY copy (gambatte plotPixel:
        // `weMaster || (wy2 == ly && lcdcWinEn)`).
        let wy_ok = self.wy_latch || self.wy2 == self.ly;
        // Rising edge only: the match level holds while lx is frozen
        // through the start stall and must not re-fire.
        let prev_match = std::mem::replace(&mut self.render.win_match_prev, win_match);
        let win_match = win_match && !prev_match;
        let win_en_now = self.eff.lcdc & LCDC_WIN_ENABLE != 0;
        // Record the raw WX-comparator match dot for the shadow
        // WY-trigger's activation deadline — *before* the `wy_ok`/`win_en`
        // gate, so a bare line the window never enters still pins the dot the
        // window *would* have activated. Tier2 + CGB only; never read OFF.
        if win_match && self.render.wx_match_dot == 0 && self.tier2_reclock {
            self.render.wx_match_dot = self.dot;
            self.render.wx_match_scx = self.eff.scx & 7;
        }
        if win_match
            && !win_en_now
            && self.wy_latch
            && !self.model.is_cgb()
            && wx == 166
            && !self.win_start_pending
        {
            // DMG: a WX=166 match with the window *disabled* still
            // latches the start request when the frame's WY latch holds
            // (gambatte plotPixel's `!cgb` branch runs without lcdcWinEn
            // when weMaster is set; requests at any other WX are
            // consumed and dropped one dot later, but the xpos >= 167
            // bound leaves the 166 one pending into the next line --
            // on_screen/wxA6_weoff_at_xposA6). Honored at the next
            // mode-3 start only if the window is enabled by then.
            self.win_start_pending = true;
        }
        if win_match && wy_ok && win_en_now {
            if !self.render.win_active {
                // Activation: the window line counter advances *here*
                // (gambatte plotPixel: ++winYPos), which is what makes a
                // same-line retrigger draw the next row (mattcurrie
                // comprehensive-ppu-doc §WIN_EN).
                self.win_line = self.win_line.wrapping_add(1);
                if !self.model.is_cgb() && wx == 166 {
                    // DMG: the start request raised at a WX=166 match is
                    // never consumed in-line (gambatte
                    // handleWinDrawStartReq honors requests at
                    // xpos >= 167 only on CGB): no window pixel ships —
                    // the pipeline only freezes briefly for the aborted
                    // start (m2int_wxA6_m3stat_1/_2 bracket the DMG
                    // mode-3 end 1-4 dots past the unextended end) —
                    // and the request survives to the next line's
                    // mode-3 start (see `win_start_pending`). The line
                    // still counts as started (gambatte keeps
                    // win_draw_started set) — the comparator must not
                    // re-fire while lx sits at 159 through the stall.
                    self.win_start_pending = true;
                    self.render.win_active = true;
                    self.render.win_stalled = true;
                    // Freeze from the match dot: 2 dots total.
                    self.render.stall += 1;
                    self.m0_unflip();
                    return true;
                } else {
                    self.m0_unflip();
                    let r = &mut self.render;
                    r.win_active = true;
                    r.win_stalled = true;
                    r.win_mode = true;
                    r.bg_count = 0;
                    r.phase = FetchPhase::TileNoWait;
                    r.fetch_x = 0;
                    r.first_discard = false;
                    // Window pixels are not subject to SCX fine scroll;
                    // WX<7 cuts the leading 7-WX window columns instead,
                    // and the BG fine-scroll comparator hunt ends with
                    // the BG fetching.
                    r.hunt_done = true;
                    r.discard = 7u8.saturating_sub(wx);
                    if wx == 0 {
                        // WX=0 with a fine scroll: the start eats the
                        // SCX&7 discard plus one extra dot (SameBoy
                        // display.c WX=0/SCX&7 extra cycle; the mealybug
                        // m3_window_timing_wx_0 photos pin pixel 0 at
                        // dot 103 + SCX&7 + 1 on both DMG and CGB-C).
                        let fine = self.eff.scx & 7;
                        if fine > 0 {
                            r.discard += fine + 1;
                        }
                    }
                }
            } else if !self.model.is_cgb() && wx == 166 && !self.win_start_pending {
                // DMG: a WX=166 match with the window already drawing
                // re-arms the carryover without counting a new activation
                // (gambatte plotPixel else-branch: `xpos == lcd_hres + 6`
                // sets win_draw_start; M3Start::f0 increments winYPos
                // when it consumes the request), with the same short
                // aborted-start freeze. `win_start_pending` doubles as
                // the once-per-line guard while lx sits at 159.
                self.win_start_pending = true;
                self.render.win_stalled = true;
                self.render.stall += 1;
                self.m0_unflip();
                return true;
            } else if self.render.win_mode && self.render.bg_count == 8 {
                // Window *reactivation*: a WX match while the window is
                // already drawing, landing exactly on the dot that ships
                // the first pixel of a window tile, emits one color-0
                // pixel and delays the rest of the line by one dot
                // (mealybug m3_wx_5_change.asm: "Window reactivation
                // zero pixels should be present when window is already
                // activated and the pixel that the window reactivates on
                // is on the same cycle as the window tile nametable
                // read" -- its reference photos pin the inserted zero
                // pixel on exactly the rows where WX-7 falls on a window
                // tile boundary, and pin that off-boundary matches have
                // no visible effect).
                self.output_pixel(0, 0);
                self.advance_lx();
                return true;
            }
        }
        false
    }

    /// LCDC.5 cleared mid-line while the window is drawing. The disable
    /// "takes effect at the end of the current window tile being drawn"
    /// and the BG then resumes "on a tile boundary — the low 3 bits of
    /// SCX have no effect" (mattcurrie comprehensive-ppu-doc §WIN_EN).
    /// Mechanically (gambatte ppu.cpp setLcdc + Tile::f0): the started
    /// flag clears immediately, the FIFO/latched window tile row still
    /// ships, remaining reads of the in-flight fetch revert to BG
    /// addressing, and the next BG map read uses the live column
    /// `(scx + xpos + 1 - cgb) / 8` — re-anchoring the tile grid to the
    /// output position rather than re-showing skipped columns.
    /// A mid-mode-3 LCDC.5 clear: the read-law FLAG half of the abort (the
    /// cc+0-calibrated `win_predraw_abort` / DMG `win_aborted` inputs the FF41
    /// mode-3-length read laws consume — `stat_irq.rs::vis_mode_read`). Always
    /// runs at the eager control commit (`regs.rs::commit_eff`), NEVER deferred:
    /// the late_disable read laws are calibrated to the write's cc+0 dot. The
    /// RENDER re-anchor (`window_abort_render`) is a separate, deferrable half
    /// so the drawn window ends at the render frame, not cc+0.
    pub(in crate::ppu) fn window_abort_flags(&mut self) {
        if !self.render.win_mode {
            // PRE-DRAW abort: LCDC.5 cleared before the window's first fetch
            // (`win_mode` not yet set — `late_disable_early_*_1`). SameBoy
            // renders BARE but DROPS the SCX fine-scroll penalty → exit
            // cfl257. `!win_mode` is the pre-draw discriminator. DMG too.
            if self.tier2_reclock {
                self.render.win_predraw_abort = true;
                self.render.win_predraw_abort_dot = self.dot;
            }
        } else if !self.model.is_cgb() {
            self.render.win_aborted = true;
        }
    }

    /// The RENDER half of a mid-mode-3 LCDC.5 clear: end the drawn window and
    /// re-anchor the BG fetch to a tile boundary. Under tier2 this fires at the
    /// deferred render frame (the `render_lcdc` bit5 1→0 catch-up, `ppu/mod.rs`),
    /// not the eager cc+0 — so the window stops at the same column SameBoy/
    /// production draws (`m3_lcdc_win_en_change_multiple`: the eager clear ended
    /// it 2 dots / 2 pixels early). In production (no `render_lcdc` defer) it runs
    /// synchronously from `commit_eff`, byte-identical. Idempotent: a no-op if the
    /// window already left `win_mode` (a natural end in the defer gap).
    pub(in crate::ppu) fn window_abort_render(&mut self) {
        if !self.render.win_mode {
            return;
        }
        let cgb = self.model.is_cgb();
        let r = &mut self.render;
        r.win_mode = false;
        // Re-arms the trigger: re-enabling with WX pointing at a pixel
        // not yet drawn retriggers the window (doc §WIN_EN).
        r.win_active = false;
        // First screen pixel of the tile the *next* tile-number read
        // belongs to: the FIFO drains bg_count more pops (minus pending
        // discards), and a fetch already past its tile-number read ships
        // one more full row first.
        let tileno_pending = matches!(r.phase, FetchPhase::TileNoWait | FetchPhase::TileNo);
        let x = i32::from(r.lx) + i32::from(r.bg_count) - i32::from(r.discard)
            + if tileno_pending { 0 } else { 8 };
        let col = (i32::from(self.eff.scx) + x.max(0) + 1 - i32::from(cgb)) >> 3;
        r.fetch_x = (col as u8).wrapping_sub(self.eff.scx >> 3) & 31;
    }
}
