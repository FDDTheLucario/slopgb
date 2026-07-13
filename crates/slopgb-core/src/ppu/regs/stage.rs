//! Mode-3 write strobe staging (stage_write / commit_eff / strobe_tick),
//! split from regs.rs (register read/write dispatch) to stay under the
//! 1000-line cap. Second `impl Ppu` block via `use super::*`; behaviour-
//! identical. See docs/ARCHITECTURE.md §Mode-3 write strobe.

use super::*;

impl Ppu {
    /// The per-register mid-mode-3 write-commit stage offset (in dots) — a pure
    /// function of `addr` / `scan_pos` / speed. `Bus::write` calls this with its
    /// live `double_speed`; the render-test harness calls it too, so both stage
    /// at the same offset (no duplicated timing constants).
    pub(crate) fn stage_write_dots(&self, addr: u16, double_speed: bool) -> u8 {
        if let (0xFF43, true, true) = (addr, !self.glitch_active(), double_speed) {
            // SCX in DOUBLE SPEED defers +2 (dots=2), not single speed's +4:
            // the DS M-cycle is 2 PPU dots (vs 4), so the write-commit-to-fetch-
            // grid offset halves. dots=2 fixes the 5 `scx_during_m3_ds`
            // fine-scroll pixel legs and holds `late_scx4`'s DS read law (the
            // fine-scroll comparator straddle). SCY/palette keep dots=3 in DS
            // (no DS pixel legs).
            2
        } else if !self.model.is_cgb() && matches!(addr, 0xFF47..=0xFF49) && !self.glitch_active() {
            // The DMG palette (BGP/OBP FF47-49) commit anchors to the EVEN
            // (CPU-M-cycle) dot grid. SameBoy commits the palette at the write
            // M-cycle's exact half-dot; single speed is whole-dot aligned so
            // the commit lands at a whole (EVEN) dot, from which the pop is
            // visible +2 dots — an ODD leading edge rounds up one dot so the
            // commit is visible +3 (round_up_even(LE)+2), an EVEN one +2. The
            // mealybug BGP/OBP legs land EVEN leading edges (want +2), the
            // gambatte dmgpalette legs ODD (want +3). DMG only — CGB has no
            // FF47-49 render path (its palettes are FF68-6B).
            2 + (self.scan_pos().1 & 1) as u8
        } else if addr == 0xFF42 && !double_speed && !self.glitch_active() {
            // SCY (FF42) takes the same EVEN-dot parity anchor as the DMG
            // palette (round_up_even(LE)+2: +2 from an EVEN leading edge, +3
            // from an ODD one). On a sprite-stalled line a tile's Lo/Hi data
            // read straddles the deferred SCY-commit dot, re-sampling the NEW
            // scroll while the latched tile NUMBER keeps the old (the mealybug
            // m3_scy_change mixed-fetch behaviour). objectless
            // scy_during_m3_{1,4,5,6} land ODD (want +3); the sprite leg EVEN
            // (want +2). SS only (DS keeps defer=3, the `else` below).
            2 + (self.scan_pos().1 & 1) as u8
        } else if matches!(addr, 0xFF42 | 0xFF43 | 0xFF47..=0xFF49) && !self.glitch_active() {
            // SCX takes the full +4 render-frame deferral: the fine-scroll
            // comparator hunt (dots 89-96) is calibrated to the production cc+4
            // frame, 4 dots late of the deferred write's true instant, so the
            // pipeline-view SCX must lag the same 4 dots for the straddle pairs
            // to separate (late_scx4 SS+DS + scx_m3_extend). WX/WY keep the
            // production frame; each register carries its own commit class,
            // mirroring SameBoy's per-register conflict maps.
            3
        } else if addr == 0xFF4B && !self.glitch_active() {
            // WX (FF4B) render-VIEW defer: `eff.wx` (the window activation
            // comparator) survives the arch write and strobe-commits at the
            // production frame — leading+2 at BOTH speeds. stage_write adds the
            // FF4B +1 (WX one dot later than the palette class) → final 2 ==
            // leading+2. The un-catch READ law (`wx_write_dot`, FF41 mode-3
            // length) keeps its cc+0 input in regs.rs (the split).
            0
        } else if double_speed {
            1
        } else {
            2
        }
    }

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
        self.staged_ds = dots <= 1;
        // The write strobe advances per half-dot ([`Ppu::tick_half`]), so the
        // staged commit debt is measured in 8-MHz half-dots — double the
        // whole-dot offset. A run of aligned half-dots then still commits at the
        // same whole dot as the whole-dot strobe.
        let dots = {
            // The write commit must land in the SAME cc+4 read frame the FF41/
            // accessibility reads observe the mode-3 length in ([`Ppu::read_pos_hd`]'s
            // +8hd SS / +4hd DS read-debt): the render latches (`wx_match_dot`/
            // `win_predraw_abort_dot`/`scx_write_dot`/…) are recorded at the render
            // dot (cc+0), but the reads sample the length +debt later, so add the
            // read-debt to separate the render-length pairs. DS is +4hd (its M-cycle
            // is 2 dots). The DMG window/palette render-length laws (arm D1/D3/D6
            // fetch phase + the palette pop-grid) are calibrated one fetch-step ahead
            // of CGB, so DMG carries a separate per-register debt below — a uniform
            // +8hd over-shifts 5 SameBoy-PASS rows (`late_enable_afterVblank`/
            // `late_disable`/`late_scx_late_disable`).
            let debt = if !self.model.is_cgb() {
                // DMG: give the mid-mode-3 render registers the render-frame debt
                // so their commit lands at the render position instead of ~4 dots
                // (8hd SS) early. The debt shifts only the pixel-view `eff` commit;
                // the FF41 mode-3-length OCR reads sample ARCH `self.scy` and the WX
                // un-catch read law's `wx_write_dot` is recorded at cc+0 in
                // `Ppu::write` (the split), so this is render-only. FF40 (LCDC) stays
                // at ZERO debt: it drives the window bit5 abort/reenable + FF41 read
                // laws calibrated to the cc+0 control commit, and a debt there breaks
                // the `late_enable_afterVblank` gambatte set. SCX (FF43) also stays
                // zero-debt — its render IS the length (below).
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
                    // WX SS: the render stage is the smallest (dots=0, +1 in
                    // `stage_write` for the FF4B palette-class offset), so it needs
                    // the largest frame debt (12hd) to reach the ~14hd absolute
                    // render-commit the WX activation comparator wants (pins
                    // m3_wx_4/5/6_change + _sprites). The un-catch READ law's
                    // `wx_write_dot` is recorded at cc+0 in `Ppu::write` (not
                    // `commit_eff`), so the debt shifts only the render view.
                    0xFF4B if !self.ds => 12,
                    // SCX (FF43) POST-match: the write lands after THIS line's
                    // fine-scroll comparator lock (`hunt_done && dot >
                    // hunt_match_dot`), so the discard is locked and the change is a
                    // pure COARSE/pixel tile shift with NO mode-3-length effect —
                    // give it the render-frame debt (6) so the commit lands the tile
                    // column at the fetch grid (m3_scx_high_5_bits). The `dot >
                    // hunt_match_dot` guard rejects the LINE-START write (dot 80)
                    // whose `hunt_done` is STALE from the previous line (match_dot ≥85).
                    0xFF43
                        if !self.ds
                            && self.render.hunt_done
                            && self.dot > self.render.hunt_match_dot =>
                    {
                        6
                    }
                    // SCX (FF43) PRE-match on a plain BG line (`!hunt_done`, NON-
                    // glitch, NON-window): NOT length-coupled — the bare line starts
                    // SCX=0 so the comparator locks (discard 0) at mode-3 dot 5
                    // BEFORE the write; a cc+0 commit lands BEFORE the lock →
                    // re-opens the comparator → wrong discard → 320px
                    // (`m3_scx_low_3_bits`). The `6` render-frame debt re-aligns to
                    // the post-lock dot. The excluded cases ARE genuinely length-
                    // coupled: `glitch_line` (SCX re-open, `ly0_late_scx7`) and
                    // `wy_trig_sb` (a WINDOW line masks the discard, `late_scx_late_
                    // disable`); the m2int length rows write at dot 152 with
                    // `hunt_done` → the post-match arm above, never here. Full A/B:
                    // `eager-scxlow-recheck-2026-07-12.md`.
                    0xFF43
                        if !self.ds
                            && !self.render.hunt_done
                            && !self.glitch_line
                            && !self.wy_trig_sb =>
                    {
                        6
                    }
                    _ => 0,
                }
            } else if self.ds {
                // CGB double-speed: the uniform render-frame read-debt is 4
                // half-dots (the DS M-cycle is 2 dots, so cc+0→cc+4 is 4hd) for the
                // pure-render registers. SCX (FF43) POST-match is the exception:
                // like the SS post-match arm above, a write landing AFTER the
                // fine-scroll comparator lock (`hunt_done && dot > hunt_match_dot`)
                // is a pure COARSE/tile shift with no mode-3-length effect, but on
                // the DS grid the uniform 4hd over-shoots the commit by exactly one
                // whole dot past the post-lock commit. Debt 2 → 6hd re-lands it,
                // recovering the 4 `scx_during_m3_ds` post-match fine-scroll pixel
                // legs (scx_0060c0/0063c0 `_5`/`_8`). Pre-match / line-start DS SCX
                // writes keep 4. See `eager-ds-scx-2026-07-12.md`.
                match addr {
                    0xFF43 if self.render.hunt_done && self.dot > self.render.hunt_match_dot => 2,
                    _ => 4,
                }
            } else {
                // CGB single-speed, per-register render-commit debt. A uniform 8
                // landed the mealybug/age DMG-compat m3_* palette/WX legs at the
                // wrong pixel column — CGB runs these DMG ROMs in compat mode and
                // shares the FF47-4B render path, so each register carries its own
                // commit class like the DMG calibration above.
                match addr {
                    // Palette (FF47-49): the DMG-compat BGP pop-grid. Its stage is
                    // the flat `3` (`stage_write_dots`, no CGB parity), so a
                    // `6 + 2*parity` debt reproduces the DMG palette even/odd anchor
                    // (12hd even / 14hd odd; pins m3_bgp_change + age m3-bg-bgp).
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
                // The BG fetcher's addressing view (bit3 BG map / bit4 tile-data
                // select) lags the control commit by the render frame, so a
                // mid-mode-3 bgtilemap/bgtiledata toggle reaches the fetch grid at
                // the SameBoy dot instead of the leading edge. Window bit5 (abort/
                // reenable/enable) + the FF41 read laws keep the cc+0 `eff.lcdc` set
                // above. Non-render / glitch lines set the view in lockstep.
                if self.render.active && !self.glitch_line {
                    self.render_lcdc_pending = Some((value, RENDER_LCDC_DELAY));
                } else {
                    self.eff.render_lcdc = value;
                    self.render_lcdc_pending = None;
                }
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
                    // The RENDER re-anchor (drawn-window end + BG-fetch tile-
                    // boundary) defers to the `render_lcdc` bit5 1→0 catch-up
                    // (`ppu/mod.rs`), so the window stops at the render frame
                    // (`m3_lcdc_win_en_change_multiple`: a synchronous clear ended
                    // it 2 dots early). Glitch lines (no `render_lcdc` defer) run it
                    // synchronously.
                    if self.glitch_line {
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
                if old & LCDC_WIN_ENABLE == 0 && value & LCDC_WIN_ENABLE != 0 && self.render.active
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
                if self.render.active && (self.eff.scx & 7) != (value & 7) {
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
