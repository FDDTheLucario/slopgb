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
                0
            } else if self.ds {
                4
            } else {
                8
            };
            (i32::from(dots) * 2 + debt + crate::probe::tune_wcommit(0)).clamp(0, 255) as u8
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
    fn commit_eff(&mut self, addr: u16, value: u8) {
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
            0xFF41 => 0x80 | self.stat_en | (u8::from(self.read_cmp()) << 2) | self.vis_mode_read(),
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
        // On the tier2 deferred path the stage SURVIVES the architectural
        // write and the
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
        // The mode-3 render regs (SCY/SCX/BGP/OBP) survive the arch
        // write so they strobe-commit at the render frame instead of the
        // leading edge (see the dots calc in `cycle.rs::write_deferred`). LCDC
        // lands via the split `render_lcdc` view.
        // The eager render-frame debt ([`Ppu::stage_write`]) keeps the staged
        // commit alive past the write's own `tick_machine` (its `dots_left`
        // exceeds the M-cycle's half-dots), so — like tier2 at the leading edge
        // — the stage is still present here and this survive-check holds, letting
        // the debt-delayed strobe commit at the read frame instead of the
        // redundant M-cycle-END (D+4) re-commit clobbering it. On the un-shifted
        // DMG eager path (debt 0) the stage drains inside `tick_machine`, so this
        // is false and the M-cycle-END commit still runs — byte-identical to the
        // pre-slice DMG eager behaviour.
        let staged_pending = (self.tier2_reclock || self.eager_value)
            && matches!(addr, 0xFF42 | 0xFF43 | 0xFF47..=0xFF49 | 0xFF4B)
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
        // The glitch-line same-dot SCX-write hunt RE-OPEN: the
        // LCD-enable glitch line's fine-scroll hunt closes at
        // machine dot `83 + scx_init` on CGB, but a same-dot FF43 commit lands
        // AFTER that dot's render tick — hardware's comparator (SameBoy
        // `render_pixel_if_possible`, live per-dot compare) still honors the
        // new value: the CGB sample deadline is `D(scx_init) = 83 + scx_init`
        // inclusive (`ly0_late_scx7_m3stat_scx1_1` writes SCX=7 at dot 84 with
        // scx_init=1 — hunt matched dot 84 — and must read mode 3 at 252, exit
        // E(7)=257; `scx0_2` writes dot 84 with match dot 83, stays missed;
        // `scx1_2`/`scx3_2` write past the window). Re-opening lets the
        // comparator cycle to the new SCX&7 (re-match dot ~90, discard 7) —
        // the exit arithmetic is already right when honored. Glitch + Tier-2
        // + CGB only; production and non-glitch lines (the late_scx4
        // write-strobe calibration) untouched.
        if addr == 0xFF43
            && self.tier2_reclock
            && self.model.is_cgb()
            && self.glitch_line
            && self.render.hunt_done
            && self.render.hunt_match_dot == self.dot
            && value & 7 != self.render.hunt_idx
        {
            self.render.hunt_done = false;
        }
        match addr {
            0x8000..=0x9FFF => {
                // Record the attempt for the DS line-end VRAM read release
                // (blocked attempts too — the M-cycle cost is what SameBoy
                // spreads).
                self.vram_wr_line = self.line;
                self.vram_wr_dot = self.dot;
                probe!(if crate::probe::s5dbg_on() && self.line < 144 {
                    eprintln!(
                        "SLOPGB vramw ly={} dot={} blk={}",
                        self.line,
                        self.dot,
                        u8::from(self.vram_write_blocked())
                    );
                });
                if !self.vram_write_blocked() {
                    self.vram[self.vram_index(addr)] = value;
                }
            }
            0xFE00..=0xFE9F => {
                probe!(if crate::probe::s5dbg_on() && self.line < 144 {
                    eprintln!(
                        "SLOPGB oamw ly={} dot={} blk={}",
                        self.line,
                        self.dot,
                        u8::from(self.oam_write_blocked())
                    );
                });
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
                    // Seed the held-high engine level for
                    // the suppressed DS line-start carryover enable (the
                    // `lyc_carryover` DS dots-0-1 suppression in
                    // `stat_write_trigger_cgb`): hardware's line is still
                    // HIGH from the previous line's mode-0 there, so the
                    // post-write level (fresh bit6 + the held carryover
                    // match) continues it — without the seed the next dot's
                    // engine tick re-fires the same edge as a spurious 0→1.
                    if self.leading_edge_reads
                        && self.model.is_cgb()
                        && self.ds
                        && self.dot < 2
                        && old & STAT_SRC_HBLANK != 0
                        && old & STAT_SRC_LYC == 0
                        && data & STAT_SRC_LYC != 0
                        && (1..=143).contains(&self.line)
                        && self.lyc == self.line - 1
                    {
                        self.stat_update.force_level(true);
                    }
                    self.stat_en = data;
                    // The CGB FF41 two-phase write for the engine
                    // view (see `eng_stat_pending` for the full schedule):
                    // phase-1 (mode bits new, bit6 old) applies at engine
                    // tick commit+2 (= SameBoy T0), the final value at
                    // commit+4, externals in between edging against the
                    // armed phase-1.
                    if self.leading_edge_reads
                        && self.model.is_cgb()
                        && !self.ds
                        && self.lcd_shift_dots == 0
                    {
                        // Single speed: the two-phase engine view (see the
                        // `eng_stat_pending` field doc for the schedule).
                        // phase-1 = mode bits NEW, LYC enable (bit 6) OLD —
                        // SameBoy `GB_CONFLICT_STAT_CGB` holds bit 6 one T
                        // past the mode bits.
                        //
                        // Double speed: 1 T = half a dot — the whole window is
                        // sub-dot, and the deferred DS write commit (+1 dot)
                        // already lands at the hardware instant: the engine
                        // sees the new value from the next tick (immediate),
                        // with the write-after-tick order giving the
                        // display-step-first collision semantics the
                        // `lyc0_m1disable_ds` / `lyc153_m1disable_ds` pairs
                        // pin (together with the DS line-153 lyfc table).
                        let phase1 = (old & STAT_SRC_LYC) | (data & !STAT_SRC_LYC);
                        self.eng_stat_pending = Some(EngStatPending {
                            phase1,
                            fin: data,
                            pre_high: self.stat_update.line(),
                            mfi_t0: 0,
                            k: 0,
                        });
                    } else {
                        self.eng_stat = data;
                        self.eng_stat_pending = None;
                        // Record a DS LYC-enable DROP for the
                        // m0-flip dip (see `ff41_ds_drop`).
                        if self.leading_edge_reads
                            && self.model.is_cgb()
                            && self.ds
                            && old & STAT_SRC_LYC != 0
                            && data & STAT_SRC_LYC == 0
                        {
                            self.ff41_ds_drop = Some((self.line, self.dot));
                        }
                    }
                    self.stage_stat_copies();
                    self.refresh_cmp(false);
                    if self.leading_edge_reads && fire {
                        // When the gambatte write-trigger fired (`fire`),
                        // re-sync the flag-on [`StatUpdate`]
                        // line to the post-write level so the next dot-clocked
                        // `stat_update_tick` does NOT re-fire the SAME edge.
                        // Without this, enabling a source whose condition is
                        // already met fires IF twice flag-on: once here, again
                        // when the dot engine re-sees the new enable as a 0→1
                        // rise (`ff41_enable_lyc_fires_once_flag_on`). The edge
                        // is discarded — the write-trigger keeps gambatte's
                        // position-exact fire (replacing it wholesale with the
                        // rising edge is net-negative in our cc+4 frame);
                        // this only seeds the line level. Gated on `fire`: a
                        // write that does NOT
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
                    self.eng_stat = data;
                    self.eng_stat_pending = None;
                    self.flush_stat_copies();
                    self.legacy_level_edge();
                }
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only.
            0xFF4A => {
                let old_wy = self.wy;
                self.wy = value;
                probe!(if crate::probe::s5dbg_on() {
                    eprintln!(
                        "SLOPGB wwy ly={} dot={} old={old_wy:02x} new={value:02x}",
                        self.line, self.dot
                    );
                });
                // The boundary-WY cross-line latch (see `Ppu::wy_xline_trig`):
                // a tail/head write matching the current line, window enabled
                // at the commit (DMG too — the DMG arm-7 twin reads the same
                // latch). Also enabled under `eager_value`: this arch commit runs
                // at the eager M-cycle END, so the boundary window catches the
                // tail-write class (`late_wy_FFto0/FFto1/10to0/1toFF`) that the
                // read-frame WY laws pair with — measured +8 CGB, 0 drop.
                if (self.tier2_reclock || self.eager_value)
                    && self.enabled
                    && (self.dot >= 452 || self.dot < 4)
                    && self.line < 144
                    && value == self.ly
                    && self.eff.lcdc & LCDC_WIN_ENABLE != 0
                {
                    self.wy_xline_trig = true;
                }
                // DMG — a HEAD write (dot < 4) matching the JUST-FINISHED
                // line: slopgb's deferred frame lands a line-boundary WY write
                // a full line late (SameBoy applies it at `ly N−1 cfl0` and its
                // continuous `wy_check` triggers on line N−1; slopgb commits at
                // `ly N dot0`, past that line's weMaster sample). If the value
                // matches the previous line (`value + 1 == line`), SameBoy
                // triggered there → the window is sticky-active for every later
                // line → set the cross-line latch. `late_wy_10to0/FFto0/FFto1`
                // `_2` (commit ly1/ly2 dot0) extend; the `_3` siblings commit
                // at dot 4 (past the head) → no trigger, bare.
                if self.tier2_reclock
                    && !self.model.is_cgb()
                    && self.enabled
                    && self.dot < 4
                    && self.line >= 1
                    && self.line < 144
                    && u16::from(value) + 1 == u16::from(self.line)
                    && self.eff.lcdc & LCDC_WIN_ENABLE != 0
                {
                    self.wy_xline_trig = true;
                }
                // The DS trigger-line WY un-latch: SameBoy's per-line
                // `wy_check` for line N runs ~dot 2-5, but slopgb's
                // production `wy_latch` pre-latches at the PREVIOUS line's
                // dot-450/454 samples — so an un-matching WY write landing
                // before the hardware check (commit dot <= 4 of the fresh
                // trigger line) must release the latch the check would never
                // have acquired (`late_wy_1toFF_ds_1` renders bare on
                // hardware AND SameBoy; its `_2` sibling commits at dot 5
                // and keeps the trigger). Tier-2 + CGB + DS; also `eager_value`
                // (the eager DS `late_wy_ds`/`late_wy_1toFF_ds` pairs recover on
                // the same latch, part of the +8 CGB WY slice).
                if (self.tier2_reclock || self.eager_value)
                    && self.model.is_cgb()
                    && self.ds
                    && self.enabled
                    && self.wy_latch
                    && !self.render.win_active
                    && self.line < 144
                    // The un-latch deadline is PER-TRIGGER-LINE:
                    // lines >= 1 run the lyfc-path check at the mode-2 entry
                    // (~internal dot 3-4) → a commit <= 2 beats it
                    // (`late_wy_1toFF_ds_1` dot 2 bare / `_ds_2` dot 4 keeps
                    // the latch, extended); line 0's check sits ~4 dots later
                    // (lyfc becomes 0 only at dot 3) → commit <= 6
                    // (`late_wy_ds_1` dot 6 bare on HARDWARE — SameBoy
                    // mis-times the line-0 check and fails that row itself /
                    // `_ds_2` dot 8 keeps). The old single `<= 4` split both
                    // brackets wrong.
                    && self.dot <= if self.ly == 0 { 6 } else { 2 }
                    && old_wy == self.ly
                    && value != self.ly
                {
                    self.wy_latch = false;
                    // The shadow latched the same pre-check compare
                    // (wy2 still old at line start) — release it with the
                    // render latch so the read law's extend arm follows, and
                    // commit the wy2 copy immediately (the write BEAT the
                    // hardware check: every later compare this line reads the
                    // new value; the stale copy re-latched the shadow at the
                    // next dot, measured).
                    if self.wy_trig_sb && self.wy_trig_sb_line == self.ly {
                        self.wy_trig_sb = false;
                    }
                    self.wy2 = value;
                    self.wy2_delay = 0;
                }
                // The DMG SS trigger-line WY→(non-LY) un-latch: a
                // WY write that MATCHED at the line's mode-2 compare then
                // flips away by dot 4 un-triggers the window on SameBoy
                // (`wy_check` reads the settled WY) while slopgb's raw sticky
                // latch (`wy_trig_sb_raw`, set at dot ≥ 4) already caught the
                // brief match. Releasing it lets the D6 un-trigger arm fire.
                // `late_wy_1toFF_2`/`2toFF_2` (FF at dot 4 → bare) vs `_3`
                // (FF at dot 8, past the compare → the window drew, extends).
                // SS + DMG; the CGB path is the DS
                // block above (`wy_latch`/`wy_trig_sb`).
                if self.tier2_reclock
                    && !self.model.is_cgb()
                    && !self.ds
                    && self.enabled
                    && self.line < 144
                    && self.dot <= 4
                    && old_wy == self.ly
                    && value != self.ly
                {
                    self.wy_trig_sb_raw = false;
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
                        // 15 SameBoy-passing rows).
                        // The FF45 analogue of the FF41 write-trigger re-sync
                        // above. The gambatte LYC-write trigger above just
                        // fired; re-derive
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
            0xFF4B => {
                // Latch a mid-line WX rewrite for the un-catch law (see
                // `Render::wx_write_dot`; DMG too) at the EAGER (cc+0) leading
                // edge here, NOT in `commit_eff`. The tier2 reclock defers the
                // render-VIEW `eff.wx` (a WX survive-stage, for the window
                // activation/reactivation comparator — `late_wx_ds`, m3_wx_*),
                // so `commit_eff` now runs at the deferred strobe dot; the
                // un-catch read law (`tier2_window_late_wx_uncatch_passes`) is
                // calibrated to the write's cc+0 dot, so its input stays here.
                // The SPLIT: length/read-law input eager, render view deferred.
                if self.render.active && (self.tier2_reclock || self.eager_value) {
                    self.render.wx_write_dot = self.dot;
                }
                self.wx = value;
            }
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
            // The post-switch exit-table latches die with the LCD
            // (the frame they classify is gone; SameBoy's freeze path is
            // LCD-bound). Inert flag-off: only tier2 STOPs set them.
            self.stop_anchor_set = false;
            self.stop_anchor_midframe = false;
            self.stop_leave_lcd_on = false;
            self.stop_leave_k = 2;
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
            // The alignment shadow re-anchors at enable, like
            // SameBoy's `double_speed_alignment = 0` (memory.c:1510).
            self.sb_dsa8 = 0;
            // An enable re-anchors the PPU frame (the e-law: the DS
            // enable quantizes the phase to the 4-dot grid), so the
            // post-switch exit-table latches restart; record the enable's
            // speed (the lcdoff-dance −4 rp class term). Inert flag-off.
            self.stop_anchor_set = false;
            self.stop_anchor_midframe = false;
            self.stop_leave_lcd_on = false;
            self.stop_leave_k = 2;
            self.lcd_enable_in_ds = self.ds;
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
