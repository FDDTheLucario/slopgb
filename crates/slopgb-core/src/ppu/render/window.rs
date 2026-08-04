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
        // The WY condition: the frame-sticky window-Y latch (SameBoy
        // `check_window = wy_triggered && LCDC.5`, display.c:1315).
        let mut wy_ok = self.wy_triggered_for_activation();
        if self.model.is_cgb() {
            // The stale-hunt skew: a mid-line SCX rewrite the fine-scroll hunt
            // never absorbed leaves the window's screen position that many dots
            // ahead of slopgb's match, so a compare that fired inside the gap
            // arrived after the real activation instant and does not count.
            // Pins gambatte/window/arg/late_scx_late_wy_FFto4_ly4_wx20_{1,2,3}
            // [Cgb]: WX 32, hunt_fine 0 against SCX&7 4, match dot 122, compares
            // at 116 / 120 / 124 — hardware puts the boundary at 118, not 122.
            // CGB only, as with the WX < 7 column cut in `win_activation_lead`.
            let skew = u16::from((self.eff.scx & 7).saturating_sub(self.render.hunt_fine & 7));
            if skew > 0 && self.wy_triggered && self.wy_trig_dot + skew > self.dot {
                wy_ok = false;
            }
        }
        // Rising edge only: the match level holds while lx is frozen
        // through the start stall and must not re-fire.
        let prev_match = std::mem::replace(&mut self.render.win_match_prev, win_match);
        let win_match = win_match && !prev_match;
        // A match that failed only on the window-Y latch stays live until
        // SameBoy's own activation instant (see `Render::win_pending_until`),
        // so a WY write landing in between still catches this line.
        let win_match = if win_match {
            if !wy_ok {
                self.render.win_pending_until = self.dot + self.win_activation_lead();
            }
            true
        } else {
            self.render.win_pending_until != 0
                && self.dot < self.render.win_pending_until
                && self.wy_triggered
        };
        // The activation gate reads the ARCHITECTURAL LCDC, like SameBoy's
        // `check_window` (`display.c:1315` reads `io_registers[GB_IO_LCDC]`) and
        // like `Ppu::wy_check`. The pipeline view (`eff.lcdc`) sees a write ~2
        // dots early, which is right for the fetch/addressing side but not for
        // the window's enable test.
        let win_en_now = self.lcdc & LCDC_WIN_ENABLE != 0;
        // Record the raw WX-comparator match dot for the shadow WY-trigger's
        // activation deadline — *before* the `wy_ok`/`win_en` gate, so a bare
        // line the window never enters still pins the dot the window *would*
        // have activated. CGB only.
        if win_match && self.render.wx_match_dot == 0 {
            self.render.wx_match_dot = self.dot;
            self.render.wx_match_scx = self.eff.scx & 7;
        }
        if win_match
            && !win_en_now
            && self.wy_triggered
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
            // A window disabled and RE-enabled mid-mode-3 redraws from the
            // re-enable point, but the fetcher needs 5 dots to get that redraw
            // moving: a re-enable landing later than that misses the redraw
            // start — the WX match, pushed out by the SCX fine scroll — and the
            // window never draws, leaving the line bare for the rest of mode 3.
            // Pins gambatte/window/late_reenable{,_scx2,_scx3,_wx0f}_{1,2} on
            // both models and both speeds (`late_reenable_ds_2` too): the
            // deadline is a fetcher cost, so it does not scale with the CPU
            // clock.
            // A mid-line WX rewrite committing at or before the match dot
            // un-catches the window: the rewritten comparator value reaches the
            // fetcher before it acts on the match, so no window fetch starts and
            // the line runs bare. Whether the rewrite wins that race is set by
            // the BG fine-scroll phase — the fetch runs further ahead at high
            // SCX&7 — and the phase boundary sits lower on DMG (3) than on CGB
            // (5). Pins gambatte/window/late_wx{,_scx2,_scx3,_scx5}_{1,2}: the
            // scx0/2 legs still catch the same write-before-match race.
            // Single speed: in double speed the write lands after the match and
            // withdraws the start instead (`window_wx_uncatch`).
            let phase_bound = if self.model.is_cgb() { 5 } else { 3 };
            if !self.ds
                && self.eff.scx & 7 >= phase_bound
                && self.render.wx_write_dot != 0
                && self.render.wx_write_dot <= self.dot
                && self.render.n_sprites == 0
            {
                self.render.win_pending_until = 0;
                return false;
            }
            if self.render.win_reenable_dot != 0
                && self.render.win_disabled_line
                && i32::from(self.render.win_reenable_dot) + 5
                    > i32::from(self.dot) + i32::from(self.eff.scx & 7)
            {
                self.render.win_pending_until = 0;
                return false;
            }
            self.render.win_pending_until = 0;
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
                    // A WX <= 7 window activates inside the prefill, while the
                    // BG fine-scroll discard is still being paid out. Those
                    // pixels do not vanish when the window takes over the
                    // fetcher — carrying them is what makes a triggering
                    // window's mode 3 end at SameBoy's `263 + SCX&7` instead of
                    // a flat 261. (A WX >= 8 window activates after the discard
                    // has already been spent, so `discard` is 0 and nothing
                    // carries.)
                    let bg_left = if r.hunt_done {
                        r.discard
                    } else {
                        self.eff.scx & 7
                    };
                    r.hunt_done = true;
                    r.discard = 7u8.saturating_sub(wx) + bg_left;
                    if wx == 0 && self.eff.scx & 7 > 0 {
                        // WX=0 pays one extra dot on top (SameBoy display.c's
                        // WX=0/SCX&7 extra cycle; the mealybug
                        // m3_window_timing_wx_0 photos pin pixel 0 at
                        // dot 103 + SCX&7 + 1 on both DMG and CGB-C).
                        r.discard += 1;
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
    /// runs at the control commit (`regs.rs::commit_eff`), NEVER deferred: the
    /// late_disable read laws are calibrated to the write's cc+0 dot. The RENDER
    /// re-anchor (`window_abort_render`) is a separate, deferrable half so the
    /// drawn window ends at the render frame, not cc+0.
    pub(in crate::ppu) fn window_abort_flags(&mut self) {
        if !self.render.win_mode {
            // PRE-DRAW abort: LCDC.5 cleared before the window's first fetch
            // (`win_mode` not yet set — `late_disable_early_*_1`). SameBoy
            // renders BARE but DROPS the SCX fine-scroll penalty → exit
            // cfl257. `!win_mode` is the pre-draw discriminator. DMG too.
            self.render.win_predraw_abort = true;
            self.render.win_predraw_abort_dot = self.dot;
        } else if !self.model.is_cgb() {
            self.render.win_aborted = true;
        }
    }

    /// The RENDER half of a mid-mode-3 LCDC.5 clear: end the drawn window and
    /// re-anchor the BG fetch to a tile boundary. Fires at the deferred render
    /// frame (the `render_lcdc` bit5 1→0 catch-up, `ppu/mod.rs`), not the cc+0
    /// control commit — so the window stops at the same column SameBoy draws
    /// (`m3_lcdc_win_en_change_multiple`: a cc+0 clear would end it 2 dots /
    /// 2 pixels early). Idempotent: a no-op if the window already left
    /// `win_mode` (a natural end in the defer gap).
    /// A WX rewrite landing within a dot of the match withdraws a window start
    /// the fetcher has not committed yet: the new WX no longer matches, so no
    /// window fetch ever runs and the line finishes bare. Double speed only —
    /// its 2-dot M-cycle puts the write one dot PAST the match, where single
    /// speed lands before it and the comparator gate in `window_trigger_step`
    /// catches it. Pins gambatte/window/late_wx_scx5_ds_1 (write 98, match 97)
    /// against `_2` (write 100, fetch committed). Same SCX&7 >= 5 fine-scroll
    /// phase bound as the single-speed comparator gate: below it the fetch has
    /// not run far enough ahead for the rewrite to win the race.
    pub(in crate::ppu) fn window_wx_uncatch(&mut self) {
        if self.eff.scx & 7 < 5
            || !self.ds
            || !self.render.win_active
            || !self.render.win_mode
            || self.render.wx_match_dot == 0
            || self.dot > self.render.wx_match_dot + 1
        {
            return;
        }
        self.render.win_active = false;
        self.render.win_mode = false;
        self.render.win_stalled = false;
        self.window_abort_render();
    }

    pub(in crate::ppu) fn window_abort_render(&mut self) {
        if !self.render.win_mode {
            // PRE-DRAW abort. Same rule as the drawn case below: a clear
            // landing while the fetch holds no latched row abandons it, and the
            // line then keeps nothing of the window — the SCX fine-scroll
            // penalty included (mattcurrie §WIN_EN drops it, exit cfl257 rather
            // than 257 + SCX&7), so give that discard back.
            //
            // The fetch phase is the whole discriminator, and it replaces the
            // dot threshold the CGB read-law arm used to carry: across
            // `late_disable_early_scx03_wx{0f,10,11,12}` every `_1` leg aborts
            // at `LoWait` and wants the short exit while every `_2` aborts at
            // `Push` and wants the long one — both with `wx_match_dot == 0`, so
            // the match dot cannot separate them and the phase can. CGB only:
            // the DMG legs of the same ladder still need their own arm.
            let incomplete = !matches!(self.render.phase_of(), FetchPhase::Push | FetchPhase::Hi);
            // A clear landing after the WX match while a SPRITE fetch holds the
            // fetcher cannot take the window's start back: the object fetch
            // occupies the slot the abort would have used, so the start stays
            // committed and the line still pays the window's 6-dot restart
            // instead of re-anchoring to a tile boundary.
            // Pins gambatte/window/late_disable_spx10_wx0f_2 (object at the
            // window's screen X, clear one dot past the match) against its `_1`
            // sibling, which clears before the match and genuinely aborts.
            if self.model.is_cgb()
                && self.render.obj_abort != 0
                && self.render.wx_match_dot != 0
                && self.dot > self.render.wx_match_dot
            {
                self.render.add_stall(6);
                return;
            }
            if self.model.is_cgb() {
                if self.render.active && incomplete {
                    let hf = self.render.hunt_fine & 7;
                    self.render.lx_add(hf);
                }
                if self.ds && self.render.active && self.render.wx_match_dot == 0 {
                    // DS: a clear landing before the window ever matched WX
                    // leaves the fetcher with nothing of the window at all, so
                    // the line ends exactly where a window-free line does — one
                    // dot earlier than slopgb's hunt otherwise carries.
                    // Pins gambatte/window/late_disable_early_scx00_wx{0f..12}_ds_1.
                    self.render.lx_add(1);
                }
            } else if self.render.active {
                // DMG: a pre-draw window abort re-anchors like CGB, but the BG
                // fetcher keeps the fetch it already had in flight. A latched
                // fetch (Push/Hi) or a tile-aligned hunt that had already matched
                // WX pays the full 6-dot refetch; anything else only skips the
                // fine-scroll discard it never consumed. A mid-line SCX rewrite
                // invalidates the in-flight fetch's own fine scroll, so an
                // unlatched fetch pays the refetch there too.
                // Pins gambatte/window/late_scx_late_disable_{0,1,2},
                // late_scx03_wx10_{1,2}, early_scx03_wx12_{1,2} on DMG.
                let hf = self.render.hunt_fine & 7;
                let latched = matches!(self.render.phase_of(), FetchPhase::Push | FetchPhase::Hi);
                let rewritten = self.render.scx_write_dot != 0;
                let extend = (self.render.wx_match_dot != 0 && (hf == 0 || latched))
                    || (rewritten && !latched);
                if extend {
                    self.render.add_stall(6);
                } else {
                    self.render.lx_add(hf);
                }
            }
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
        // The BG resumes "on a tile boundary — the low 3 bits of SCX have no
        // effect" (mattcurrie comprehensive-ppu-doc §WIN_EN). `fetch_x` above is
        // one half of that re-anchor; the OUTPUT position is the other, and it
        // was missing. When the clear catches the window fetch mid-flight (the
        // tile-number read past but the row not yet latched) that fetch is
        // abandoned, and output resumes at the next tile boundary rather than
        // finishing the abandoned row. A latched row (`Push`) still ships, so
        // its line is untouched.
        //
        // Scoped to a fetch still owing a discard: `discard > 0` means the
        // window had not begun putting its own tile out, so nothing of it
        // survives the clear. `late_disable_scx5_ds_1` aborts at `HiWait` with
        // lx 0, SCX&7 5 and one pixel still to drop, giving a boundary of lx 3;
        // `late_disable_scx{2,3}_2` abort at the same phase with the discard
        // already spent and keep their full line.
        if !tileno_pending && !matches!(r.phase, FetchPhase::Push | FetchPhase::Hi) && r.discard > 0
        {
            let fine = self.eff.scx & 7;
            let to_boundary = (8 - (fine.wrapping_add(r.lx) & 7)) & 7;
            r.lx = r.lx.saturating_add(to_boundary);
            r.win_stalled = false;
            r.discard = 0;
        }
    }
}
