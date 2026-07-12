//! Mode-3 write strobe staging (stage_write / commit_eff / strobe_tick),
//! split from regs.rs (register read/write dispatch) to stay under the
//! 1000-line cap. Second `impl Ppu` block via `use super::*`; behaviour-
//! identical. See docs/ARCHITECTURE.md §Mode-3 write strobe.

use super::*;

impl Ppu {
    /// the write M-cycle and commits via [`Self::write`] afterwards, so
    /// the pixel pipeline sees the new value land mid-cycle exactly as the
    /// bus drives it on hardware (gbctr "Memory access timing"), while
    /// everything the tick-then-access contract calibrates (STAT, IRQ,
    /// access blocking, LCDC.7 enable/disable) keeps the architectural
    /// commit point. `dots` is 2 at normal speed, 1 in double speed (the
    /// second half of the M-cycle either way).
    ///
    /// Non-rendering addresses are ignored; rendering registers are FF40
    /// (pipeline bits only — bit 7 acts at the commit), FF42/FF43 and
    /// FF47-FF4B.
    pub(crate) fn stage_write(&mut self, addr: u16, value: u8, dots: u8) {
        if !matches!(addr, 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B) {
            return;
        }
        // WX reaches the pixel pipeline one dot later than the palette
        // class — at the architectural tick's strobe point rather than
        // mid-cycle. Pinned by the mealybug m3_wx_4/5/6_change triple:
        // their shared WX=LY rewrite lands exactly between the WX=5 and
        // WX=6 prefill comparator dots (the WX=5 line still triggers,
        // the WX=6 line does not), which only the +1 commit satisfies
        // (gambatte wxChange likewise updates wx one cycle later than
        // the dmg palette path).
        let dots = if addr == 0xFF4B { dots + 1 } else { dots };
        // Speed hint for the FF4A wy2 scheduling below (1-dot staging
        // only happens in double speed; the tier2 write-strobe stages 3
        // dots at either speed, so there the hint comes from the live
        // `ds` flag instead).
        self.staged_ds = dots <= 1 || (self.tier2_reclock && self.ds);
        // HALFDOT Part A-render (eager): the write strobe advances per half-dot
        // under `eager_value` ([`Ppu::tick_half`]), so the staged commit debt is
        // measured in 8-MHz half-dots — double the whole-dot offset. A run of
        // aligned half-dots then still commits at the same whole dot as the
        // whole-dot strobe (byte-identical on the aligned grid). Tier2 keeps the
        // whole-dot strobe (1 per dot) → unchanged.
        let dots = if self.eager_value {
            // The eager write commit must land in the SAME cc+4 read frame the
            // FF41/accessibility reads observe the mode-3 length in
            // ([`Ppu::read_pos_hd`]'s +8hd SS / +4hd DS read-debt): the render
            // latches (`wx_match_dot`/`win_predraw_abort_dot`/`scx_write_dot`/…)
            // are recorded at the render dot (cc+0 frame), but the reads sample
            // the length +debt later, so the un-shifted eager commit lands the
            // mid-mode-3 register change `debt`-hd early of the read's view. Add
            // the read-debt so the render-length pairs separate on the eager
            // clock. Speed-dependent: DS is +4hd (its M-cycle is 2 dots).
            // CGB-scoped: the DMG window/palette render-length laws (arm
            // D1/D3/D6 fetch phase + the palette pop-grid) are calibrated one
            // fetch-step ahead of CGB — a uniform +8hd there over-shifts 5
            // SameBoy-PASS DMG rows (`late_enable_afterVblank`/`late_disable`/
            // `late_scx_late_disable`). The DMG write-commit frame is a separate
            // calibration (a later slice).
            let debt = if !self.model.is_cgb() {
                // DMG: give the mid-mode-3 render registers the render-frame debt
                // so their commit lands at the tier2 render position instead of ~4
                // dots (8hd SS) early — the eager stage starts at cc+0 while the
                // tier2 stage starts at the cc+4 leading edge (`write_deferred`
                // advances the machine first). The debt shifts only the pixel-view
                // `eff` commit; the FF41 mode-3-length OCR reads sample ARCH
                // `self.scy`, and the WX un-catch read law's `wx_write_dot` is
                // recorded at cc+0 in `Ppu::write` (the #11bq split), so this is
                // render-only — measured EV DMG two-bin 102 (palette) → 96 (+WX),
                // no OCR regression vs the pre-debt eager clock. FF40 (LCDC) stays
                // at ZERO debt: it drives the window bit5 abort/reenable + FF41
                // read laws calibrated to the cc+0 control commit, and a debt there
                // breaks the `late_enable_afterVblank` gambatte set (#11ck). SCX
                // (FF43) also stays zero-debt — its render IS the length (below).
                match addr {
                    // SCY / palette: the full cc+0→cc+4 frame debt. Their stage is
                    // dots ≈ 2 (`2 + parity`), so 8hd SS lands them at the render
                    // frame's ~12hd absolute (recovers m3_bgp/obp/scy).
                    0xFF42 | 0xFF47..=0xFF49 => {
                        if self.ds {
                            4
                        } else {
                            8
                        }
                    }
                    // WX SS: its render stage is dots=0 (+1 in `stage_write` for
                    // the FF4B palette-class offset) → 2hd of ×2 grid, the smallest
                    // of the render regs, so it needs the LARGEST frame debt (12hd)
                    // to reach the ~14hd absolute render-commit the WX
                    // activation/reactivation comparator wants. Swept: 12hd lands
                    // all of m3_wx_4/5/6_change + _sprites; 10hd lands 2, ≤8 lands
                    // 0. The un-catch READ law's `wx_write_dot` is recorded in
                    // `Ppu::write` at the eager cc+0 (not `commit_eff`), so the debt
                    // shifts only the render view — the split #11bq built for tier2.
                    0xFF4B if !self.ds => 12,
                    // SCX (FF43) POST-match: the write lands after THIS line's
                    // fine-scroll comparator lock (`hunt_done && dot >
                    // hunt_match_dot`), so the discard is locked and the change is
                    // a pure COARSE/pixel tile shift with NO mode-3-length effect →
                    // give it the render-frame debt so the eager cc+0 commit lands
                    // the tile column at the tier2 fetch grid. `6` swept
                    // unique-optimal (m3_scx_high_5_bits: 4→41px, 6→PASS, 8→35px).
                    // The `dot > hunt_match_dot` guard rejects the LINE-START write
                    // (dot 80) whose `hunt_done` is STALE from the previous line
                    // (match_dot ≥85). PRE-match writes (`!hunt_done`, `_ => 0`)
                    // feed the fine-scroll hunt the EMERGENT bare-line length grows
                    // from, so a debt there shifts the gambatte m3stat/late_scx
                    // length verdicts — genuine coupling, kept zero-debt. Full
                    // A/B: `measurements/eager-scx-adversarial-2026-07-12.md` (#11el).
                    0xFF43 if !self.ds
                        && self.render.hunt_done
                        && self.dot > self.render.hunt_match_dot => 6,
                    _ => 0,
                }
            } else if self.ds {
                4
            } else {
                // CGB single-speed, per-register eager render-commit debt
                // (#11ej). The uniform 8 landed the mealybug/age DMG-compat m3_*
                // palette/WX legs at the wrong pixel column — CGB runs these DMG
                // ROMs in compat mode and shares the FF47-4B render path, so each
                // register carries its own commit class like the DMG calibration
                // above. These rows pass tier2 (same whole-dot render) and fail
                // eager ONLY on this cc+0 commit position → `eager_value`-scoped,
                // tier2 byte-ident.
                match addr {
                    // Palette (FF47-49): the DMG-compat BGP pop-grid. Its stage is
                    // the flat `3` (`stage_write_dots`, no CGB parity), so a
                    // `6 + 2*parity` debt reproduces the DMG palette even/odd
                    // anchor (12hd even / 14hd odd). Swept unique-optimal: +-2
                    // regress m3_bgp_change + age m3-bg-bgp.
                    0xFF47..=0xFF49 => 6 + 2 * (self.scan_pos().1 & 1) as i32,
                    // WX (FF4B): like the DMG WX arm — its render stage is the
                    // smallest, so it needs the largest debt to reach the WX
                    // activation comparator. 12 recovers m3_window_timing (+_wx_0)
                    // + m3_wx_4_change_sprites (comparator slack 10-16, no drops).
                    0xFF4B => 12,
                    _ => 8,
                }
            };
            (i32::from(dots) * 2 + debt).clamp(0, 255) as u8
        } else {
            dots
        };
        // One bus op per M-cycle: a previous stage has always expired or
        // been architecturally committed by now; flush defensively if not.
        if let Some(s) = self.staged.take() {
            self.commit_eff(s.addr, s.value);
        }
        self.staged = Some(StagedWrite {
            addr,
            value,
            dots_left: dots,
        });
    }

    /// Fold an expired staged write into the pipeline-view registers.
    pub(super) fn commit_eff(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF40 => {
                let old = self.eff.lcdc;
                self.eff.lcdc = value;
                // The BG fetcher's addressing view (bit3 BG map /
                // bit4 tile-data select) lags the eager control commit by the
                // render frame under tier2, so a mid-mode-3 bgtilemap/bgtiledata
                // toggle reaches the fetch grid at the production/SameBoy dot
                // instead of the leading edge. Window bit5 (abort/reenable/
                // enable) + the FF41 read laws keep the eager `eff.lcdc` set
                // above — their tier2 pins are calibrated to the cc+0 control
                // commit. Production (and non-render / glitch lines) set the
                // view in lockstep — byte-identical OFF.
                if (self.tier2_reclock || self.eager_value)
                    && self.render.active
                    && !self.glitch_line
                {
                    self.render_lcdc_pending = Some((value, RENDER_LCDC_DELAY));
                } else {
                    self.eff.render_lcdc = value;
                    self.render_lcdc_pending = None;
                }
                probe!(if (old ^ value) & LCDC_WIN_ENABLE != 0 && crate::probe::s5dbg_on() {
                    eprintln!(
                        "SLOPGB wlcdc ly={} dot={} old={old:02x} new={value:02x}",
                        self.line, self.dot
                    );
                });
                // LCDC.5 cleared while the window machine is drawing:
                // the window aborts at the pipeline view's commit point
                // (gambatte ppu.cpp setLcdc clears win_draw_started
                // immediately; the tile data already latched still ships
                // — see `window_abort`).
                if old & LCDC_WIN_ENABLE != 0 && value & LCDC_WIN_ENABLE == 0 && self.render.active
                {
                    // A mid-mode-3 LCDC.5 clear: the read-law FLAG half
                    // (`win_predraw_abort` pre-draw / DMG `win_aborted`) fires
                    // eagerly here for the shadow bare-exit / length read laws
                    // (`stat_irq.rs::vis_mode_read`), calibrated to the cc+0 dot.
                    self.window_abort_flags();
                    // The RENDER re-anchor (drawn-window end + BG-fetch
                    // tile-boundary) defers to the `render_lcdc` bit5 1→0 catch-up
                    // (`ppu/mod.rs`) under the tier2 reclock, so the window stops
                    // at the render frame (`m3_lcdc_win_en_change_multiple`: the
                    // eager clear ended it 2 dots early). Production / glitch lines
                    // (no `render_lcdc` defer) run it synchronously — byte-identical.
                    if !(self.tier2_reclock || self.eager_value) || self.glitch_line {
                        self.window_abort_render();
                    }
                }
                // Latch a mid-mode-3 LCDC.5 RE-enable dot for the CGB
                // shadow window-REENABLE mode-3 length law (`stat_irq.rs::
                // vis_mode_read`). A window disabled then re-enabled mid-line
                // (`late_reenable`) redraws from the re-enable point; whether its
                // mode-3 EXTENDS past the read depends on the re-enable dot vs the
                // WX match (the redraw start): re-enable at/before the match →
                // extends (mode3); after → the redraw starts too late, bare exit
                // (mode0). slopgb's whole-dot render collapses both to mode3 at
                // the read dot. Latched tier2 while `render.active` (applies to
                // DMG as well as CGB).
                if old & LCDC_WIN_ENABLE == 0
                    && value & LCDC_WIN_ENABLE != 0
                    && self.render.active
                    && (self.tier2_reclock || self.eager_value)
                {
                    self.render.win_reenable_dot = self.dot;
                    // A FIRST enable (window neither active nor aborted this
                    // line) IS the window trigger; see `Render::win_enable_dot`.
                    if !self.render.win_active && !self.render.win_aborted {
                        self.render.win_enable_dot = self.dot;
                    }
                }
            }
            0xFF42 => self.eff.scy = value,
            0xFF43 => {
                // Flag a mid-mode-3 SCX rewrite (`late_scx_*`); see
                // `Render::scx_write_dot`.
                if self.render.active
                    && (self.tier2_reclock || self.eager_value)
                    && (self.eff.scx & 7) != (value & 7)
                {
                    self.render.scx_write_dot = self.dot;
                }
                self.eff.scx = value;
            }
            0xFF47 => self.eff.bgp = value,
            0xFF48 => self.eff.obp0 = value,
            0xFF49 => self.eff.obp1 = value,
            0xFF4A => self.eff.wy = value,
            0xFF4B => self.eff.wx = value,
            _ => {}
        }
    }

    /// Advance the in-flight write strobe by one dot. The dot on which
    /// `dots_left` hits 0 is the transition dot: on pre-CGB models the DMG
    /// palette registers read old OR new for that single dot (mealybug
    /// README, m3_bgp_change: "BGP takes the value old OR new for one
    /// cycle"; the CGB-C reference shows a clean switch); from the next
    /// dot on, the new value drives the pipeline view.
    pub(super) fn strobe_tick(&mut self) {
        let Some(s) = &mut self.staged else { return };
        if s.dots_left > 0 {
            s.dots_left -= 1;
            if s.dots_left == 0 && !self.model.is_cgb() {
                match s.addr {
                    0xFF47 => self.eff.bgp |= s.value,
                    0xFF48 => self.eff.obp0 |= s.value,
                    0xFF49 => self.eff.obp1 |= s.value,
                    _ => {}
                }
            }
        } else {
            let (addr, value) = (s.addr, s.value);
            self.staged = None;
            self.commit_eff(addr, value);
        }
    }
}
