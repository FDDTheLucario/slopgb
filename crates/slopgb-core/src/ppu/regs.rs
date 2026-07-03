//! PPU register read/write dispatch (FF40-FF4B) + the mode-3 write strobe staging (stage_write/commit_eff/strobe_tick) + LCDC.7 enable/disable. docs/ARCHITECTURE.md §Mode-3 write strobe. Oracle: mealybug m3_*, gambatte scx/scy/dmgpalette during_m3.

use super::*;

impl Ppu {
    /// Stage a rendering-register write `dots` PPU dots before its
    /// architectural commit. The interconnect calls this *before* ticking
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
        // `ds` flag instead — #11bb).
        self.staged_ds = dots <= 1 || (self.tier2_reclock && self.ds);
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
    fn commit_eff(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF40 => {
                let old = self.eff.lcdc;
                self.eff.lcdc = value;
                // LCDC.5 cleared while the window machine is drawing:
                // the window aborts at the pipeline view's commit point
                // (gambatte ppu.cpp setLcdc clears win_draw_started
                // immediately; the tile data already latched still ships
                // — see `window_abort`).
                if old & LCDC_WIN_ENABLE != 0 && value & LCDC_WIN_ENABLE == 0 && self.render.active
                {
                    // C2 #11at — a mid-mode-3 LCDC.5 clear: `window_abort` flags
                    // a PRE-DRAW abort (window disabled before its first fetch)
                    // for the CGB shadow bare-exit law (`stat_irq.rs::
                    // vis_mode_read`). See `window_abort` + `win_predraw_abort`.
                    self.window_abort();
                }
                // C2 #11au — latch a mid-mode-3 LCDC.5 RE-enable dot for the CGB
                // shadow window-REENABLE mode-3 length law (`stat_irq.rs::
                // vis_mode_read`). A window disabled then re-enabled mid-line
                // (`late_reenable`) redraws from the re-enable point; whether its
                // mode-3 EXTENDS past the read depends on the re-enable dot vs the
                // WX match (the redraw start): re-enable at/before the match →
                // extends (mode3); after → the redraw starts too late, bare exit
                // (mode0). slopgb's whole-dot render collapses both to mode3 at
                // the read dot. Latched tier2+CGB while `render.active`.
                if old & LCDC_WIN_ENABLE == 0
                    && value & LCDC_WIN_ENABLE != 0
                    && self.render.active
                    && self.tier2_reclock
                    && self.model.is_cgb()
                {
                    self.render.win_reenable_dot = self.dot;
                    // #11bf item 3a — a FIRST enable (window neither active
                    // nor aborted this line) IS the window trigger; see
                    // `Render::win_enable_dot`.
                    if !self.render.win_active && !self.render.win_aborted {
                        self.render.win_enable_dot = self.dot;
                    }
                }
            }
            0xFF42 => self.eff.scy = value,
            0xFF43 => self.eff.scx = value,
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
            0xFF41 => 0x80 | self.stat_en | (u8::from(self.cmp) << 2) | self.vis_mode_read(),
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

    /// Write counterpart of [`Self::read`]. Returns IF bits raised by the
    /// write itself (same encoding as [`Self::tick`]): STAT/LYC/LCDC writes
    /// can raise the STAT line in the very M-cycle of the write —
    /// `stat_lyc_onoff` round 4 needs that interrupt to dispatch before the
    /// next instruction — so the caller must OR the returned bits into IF
    /// immediately, like a `tick` result.
    pub fn write(&mut self, addr: u16, value: u8) -> u8 {
        // Architectural commit point: converge the pipeline view with the
        // registers (the staged copy of this same write may already have
        // expired into it — see `stage_write`; writes that never went
        // through the staging path land in both views here).
        //
        // Half-dot reclock Part A (write-strobe, #11bb): on the tier2
        // deferred path the stage SURVIVES the architectural write and the
        // pipeline view commits via `strobe_tick` at SameBoy's frame instead
        // (the io write lands at the write M-cycle's END — `GB_advance_cycles`
        // runs the display coroutine BEFORE the write commits, memory.c /
        // sm83_cpu.c — so the pixel pipe sees the new value only from the
        // next dot after the M-cycle). The eager production path ticks the
        // machine BEFORE this call, so its stage has already expired and this
        // immediate convergence is what commits it; the deferred path calls
        // this at the leading edge with zero dots ticked, so the immediate
        // convergence was collapsing every mid-mode-3 register write onto the
        // write's leading edge — the measured "deferred WRITE collapse"
        // behind the late_scx/late_disable/late_wx render-length pairs
        // (`late_scx4`: SameBoy separates the legs by whether the SCX commit
        // lands before/after the fine-scroll comparator's first sample;
        // slopgb collapsed both legs onto the leading edge). Production
        // (`!tier2_reclock`) is byte-identical.
        let staged_pending = self.tier2_reclock
            && addr == 0xFF43
            && !self.glitch_line
            && self
                .staged
                .as_ref()
                .is_some_and(|s| s.addr == addr && s.value == value);
        if !staged_pending {
            if self.staged.as_ref().is_some_and(|s| s.addr == addr) {
                self.staged = None;
            }
            self.commit_eff(addr, value);
        }
        match addr {
            0x8000..=0x9FFF => {
                // #11bd item 5: record the attempt for the DS line-end VRAM
                // read release (blocked attempts too — the M-cycle cost is
                // what SameBoy spreads).
                self.vram_wr_line = self.line;
                self.vram_wr_dot = self.dot;
                // S5 accessibility write-attempt tracer (`SLOPGB_S5DBG`,
                // byte-identical unset): the vramw/oam postwrite families'
                // measurement WRITE dot + blocked verdict.
                if crate::ppu::s5dbg_on() && self.line < 144 {
                    eprintln!(
                        "SLOPGB vramw ly={} dot={} blk={}",
                        self.line,
                        self.dot,
                        u8::from(self.vram_write_blocked())
                    );
                }
                if !self.vram_write_blocked() {
                    self.vram[self.vram_index(addr)] = value;
                }
            }
            0xFE00..=0xFE9F => {
                if crate::ppu::s5dbg_on() && self.line < 144 {
                    eprintln!(
                        "SLOPGB oamw ly={} dot={} blk={}",
                        self.line,
                        self.dot,
                        u8::from(self.oam_write_blocked())
                    );
                }
                if !self.oam_write_blocked() {
                    self.oam[usize::from(addr - 0xFE00)] = value;
                }
            }
            0xFF40 => self.write_lcdc(value),
            0xFF41 => {
                let old = self.stat_en;
                let data = value & STAT_SRC_ALL;
                if self.enabled {
                    let fire = if self.model.is_cgb() {
                        // Retroactive pulse reach: the CGB line-start m2
                        // pulse sits a sub-cycle after our dot-0 tick, so
                        // a write committing in that same M-cycle still
                        // decides it (the un-fire direction is
                        // unrepresentable — m2enable disable_1 stays a
                        // documented swap).
                        let retro = self.dot == 0
                            && !self.glitch_line
                            && (1..=143).contains(&self.line)
                            && old & STAT_SRC_HBLANK == 0
                            && !self.m2_pulse_fires(old)
                            && self.m2_pulse_fires(data);
                        // (The FF45-write edge-only engine-line guard does
                        // NOT port here: the FF41 retro/m2 pulse reach is
                        // event-like in the pinned m2enable cells — the guard
                        // was built + measured +3 fails there.)
                        retro || self.stat_write_trigger_cgb(old, data)
                    } else {
                        // The glitch trigger, plus the DMG pulse reach:
                        // an m2 enable committing at the pulse's M-cycle
                        // or the one after re-decides a pulse that did
                        // not exist under the old enables (old m2en off),
                        // blocked by the held LYC match — through the
                        // *new* lyc enable at dot 0, either enable at
                        // dot 4 (the m2enable late_enable /
                        // late_enable_after_lycint(_disable) dmg08 cell
                        // grids pin all eleven cells).
                        let retro = (self.dot == 0 || self.dot == 4)
                            && !self.glitch_line
                            && (1..=144).contains(&self.line)
                            && old & (STAT_SRC_OAM | STAT_SRC_HBLANK) == 0
                            && data & STAT_SRC_OAM != 0
                            && data & STAT_SRC_HBLANK == 0
                            && {
                                let lycen = if self.dot == 0 { data } else { data | old };
                                !(lycen & STAT_SRC_LYC != 0 && self.lyc_ev_m == self.line - 1)
                            };
                        retro || self.stat_write_trigger_dmg(old)
                    };
                    if fire {
                        self.pending_if |= IF_STAT;
                    }
                    self.stat_en = data;
                    self.stage_stat_copies();
                    self.refresh_cmp(false);
                    if self.leading_edge_reads && fire {
                        // Port Stage A11 — when the gambatte write-trigger
                        // fired (`fire`), re-sync the flag-on [`StatUpdate`]
                        // line to the post-write level so the next dot-clocked
                        // `stat_update_tick` does NOT re-fire the SAME edge.
                        // Without this, enabling a source whose condition is
                        // already met fires IF twice flag-on: once here, again
                        // when the dot engine re-sees the new enable as a 0→1
                        // rise (`ff41_enable_lyc_fires_once_flag_on`). The edge
                        // is discarded — the write-trigger keeps gambatte's
                        // position-exact fire (replacing it wholesale with the
                        // rising edge is net-negative in our cc+4 frame,
                        // `ppu-subdot-ladder.md` "A11"); this only seeds the
                        // line level. Gated on `fire`: a write that does NOT
                        // trigger here must leave the line untouched so a
                        // legitimate dot-engine rise next tick still fires (the
                        // un-gated sync suppressed 15 such lifts — measured).
                        // Read-frame-independent, flag-gated → byte-identical
                        // flag-OFF.
                        let _ = self.stat_update.update(
                            self.mode_for_interrupt,
                            data,
                            self.lyc_interrupt_line,
                        );
                    }
                } else {
                    self.stat_en = data;
                    self.flush_stat_copies();
                    self.legacy_level_edge();
                }
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only.
            0xFF4A => {
                self.wy = value;
                // #11bd item 4 — the boundary-WY cross-line latch (see
                // `Ppu::wy_xline_trig`): a tail/head write matching the
                // current line, window enabled at the commit.
                if self.tier2_reclock
                    && self.model.is_cgb()
                    && self.enabled
                    && (self.dot >= 452 || self.dot < 4)
                    && self.line < 144
                    && value == self.ly
                    && self.eff.lcdc & LCDC_WIN_ENABLE != 0
                {
                    self.wy_xline_trig = true;
                }
                // The live window-trigger comparison uses a delayed WY
                // copy — see `wy2`.
                if self.enabled {
                    // CGB: ~6 dots after the architectural commit (5 in
                    // double speed); DMG: 2 (gambatte wyChange: wy2 at
                    // cc+6-ds on CGB with the LCD on, cc+2 otherwise,
                    // one cycle later than the wx commit; calibrated
                    // against the gambatte window/arg/late_wy_* rounds).
                    self.wy2_delay = if !self.model.is_cgb() {
                        2
                    } else if self.staged_ds {
                        5
                    } else {
                        6
                    };
                } else {
                    self.wy2 = value;
                }
            }
            0xFF45 => {
                let old = self.lyc;
                self.lyc = value;
                // The comparison retriggers immediately on LYC writes while
                // the comparison clock runs (`stat_lyc_onoff`).
                if self.enabled && old != value {
                    let before = self.pending_if;
                    if self.model.is_cgb() {
                        self.write_lyc_cgb(old, value);
                    } else {
                        self.write_lyc_dmg(old, value);
                    }
                    if self.leading_edge_reads && (self.pending_if & !before) & IF_STAT != 0 {
                        // `& !before` keys on a NEWLY-set STAT bit (the trigger
                        // fired this call), not one already pending from an
                        // earlier tick this M-cycle — so the sync only fires for
                        // the double-fire case and never over-suppresses a
                        // legitimate dot-engine rise (the un-gated form dropped
                        // 15 SameBoy-passing rows, A11).
                        // Port Stage A12 — the FF45 analogue of A11. The
                        // gambatte LYC-write trigger above just fired; re-derive
                        // `lyc_interrupt_line` for the NEW LYC (the engine's LYC
                        // input, normally latched in `stat_update_tick`) and
                        // re-sync the `StatUpdate` line so the next dot-clocked
                        // tick does NOT re-fire the same match as a 0→1 rise
                        // (`ff45_match_fires_once_flag_on`). Gated on the
                        // trigger having fired — a write that does not trigger
                        // here leaves the line for the dot engine to raise
                        // legitimately next tick. The edge is discarded.
                        // Read-frame-independent, flag-gated → byte-identical
                        // flag-OFF.
                        let ly = self.ly_for_comparison();
                        if ly != -1 {
                            self.lyc_interrupt_line = ly == i16::from(self.lyc);
                        }
                        let _ = self.stat_update.update(
                            self.mode_for_interrupt,
                            self.stat_en,
                            self.lyc_interrupt_line,
                        );
                    }
                } else {
                    self.lyc_event = value;
                    self.lyc_ev_m = value;
                    self.legacy_level_edge();
                }
            }
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
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
        std::mem::take(&mut self.pending_if)
    }

    fn write_lcdc(&mut self, value: u8) {
        let was_on = self.lcdc & LCDC_ENABLE != 0;
        self.lcdc = value;
        let now_on = value & LCDC_ENABLE != 0;
        if was_on && !now_on {
            // LCD off: LY=0, mode 0, instantly; the comparison clock stops
            // with the flag frozen (`stat_lyc_onoff`); the displayed frame
            // goes white.
            self.enabled = false;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            self.glitch_line = false;
            // Invariant hygiene: frame_skip only matters while enabled and
            // every enable re-arms it; don't leave it stale across off.
            self.frame_skip = false;
            self.line_render_done = true;
            self.flip_dot = 0;
            self.vis_early = false;
            self.vis_hold_until = 0;
            self.render_finished = true;
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.hdma_lead = false;
            // An in-flight CGB FF45-write IRQ dies with the LCD
            // (gambatte: disabling cancels every scheduled memevent).
            self.lyc_if_delay = 0;
            self.flush_stat_copies();
            self.render.active = false;
            self.render.win_active = false;
            self.win_start_pending = false;
            let white = self.white();
            self.front.fill(white);
            self.legacy_level_edge();
        } else if !was_on && now_on {
            // LCD on: glitched first line (`lcdon_timing-GS`); the LYC
            // comparison restarts against LY=0 immediately and can raise
            // the STAT line in this very cycle (`stat_lyc_onoff` round 4).
            self.enabled = true;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            // #11bd: the alignment shadow re-anchors at enable, like
            // SameBoy's `double_speed_alignment = 0` (memory.c:1510).
            self.sb_dsa8 = 0;
            // The event comparator's delayed FF45 copy restarts in sync
            // (gambatte lycIrq.lcdReset).
            self.lyc_event = self.lyc;
            self.glitch_line = true;
            // Hardware keeps the panel blank for the whole first frame
            // after enabling (see `frame_skip`).
            self.frame_skip = true;
            self.line_render_done = false;
            self.flip_dot = 0;
            self.vis_early = false;
            self.vis_hold_until = 0;
            self.render_finished = false;
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.hdma_lead = false;
            self.flush_stat_copies();
            self.render.active = false;
            self.wy_latch = false;
            self.win_line = 0xFF;
            self.win_start_pending = false;
            self.legacy_level_edge();
        }
    }
}
