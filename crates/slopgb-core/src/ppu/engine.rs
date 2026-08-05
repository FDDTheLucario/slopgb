use super::*;

impl Ppu {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            frame_count: 0,
            lcd_regs_written: false,
            lcdc: 0,
            stat_en: 0,
            eng_stat: 0,
            eng_stat_pending: None,
            eng_stat_half: None,
            eng_mfi_prev: 0,
            ff41_ds_drop: None,
            stat_if_squash: 0,
            ack_squash_ppu_mask: 0,
            ack_squash_ppu: 0,
            ly0_pulse_age: 0,
            m0sh_age: 0,
            m0sh_dot: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0,
            obp0: 0,
            obp1: 0,
            wy: 0,
            wx: 0,
            vbk: 0,
            opri: 0,
            dmg_compat: false,
            bcps: 0,
            ocps: 0,
            bg_pal_ram: [0xFF; 64],
            obj_pal_ram: [0xFF; 64],
            vram: vec![0u8; 0x4000]
                .into_boxed_slice()
                .try_into()
                .unwrap_or_else(|_| unreachable!()),
            oam: [0; 0xA0],
            dma_freeze: None,
            oam_dma_active: false,
            enabled: false,
            line: 0,
            dot: 0,
            dhalf: 0,
            lcd_phase_hd: 0,
            sb_dsa8: 0,
            lcd_shift_dots: 0,
            glitch_line: false,
            frame_skip: false,
            cmp: false,
            stat_line: false,
            pending_if: 0,
            stat_late: false,
            m0_src: false,
            m0_rise_dot: false,
            mode_for_interrupt: 0,
            mfi_m0_prev: false,
            stat_update: crate::stat_update::StatUpdate::new(),
            lyc_interrupt_line: false,
            m0_rise: false,
            lyc_rise: false,
            m0_access_flip: None,
            pal_access_flip: None,
            m0_stat_flip: None,
            lyc_if_delay: 0,
            l153_lyc_write_dot: u16::MAX,
            lyc_event: 0,
            cmp_irq: false,
            stat_ev: 0,
            stat_ev_staged: None,
            lyc_ev_m: 0,
            lyc_ev_m_staged: None,
            stat_lyc_ev: 0,
            stat_lyc_ev_staged: None,
            stat_halt_late: false,
            stat_rise_oam: false,
            stat_rise_m0: false,
            read_carried: false,
            halt_refetch: false,
            line_render_done: true,
            irq_done: true,
            flip_dot: 0,
            vis_early: false,
            vis_hold_until: 0,
            render_finished: true,
            hdma_lead: false,
            pal_open_dot: 0,
            wy_triggered: false,
            wy_trig_dot: 0,
            wy_check_in: 0,
            stop_anchor_set: false,
            stop_anchor_midframe: false,
            stop_leave_lcd_on: false,
            stop_leave_k: 2,
            lcd_enable_in_ds: false,
            vram_wr_line: 0xFF,
            vram_wr_dot: 0,
            staged_ds: false,
            ds: false,
            win_line: 0xFF,
            win_start_pending: false,
            eff: PipeRegs {
                lcdc: 0,
                render_lcdc: 0,
                scy: 0,
                scx: 0,
                bgp: 0,
                obp0: 0,
                obp1: 0,
                wy: 0,
                wx: 0,
            },
            staged: None,
            render_lcdc_pending: None,
            obj_en_lag: 0,
            scan_obj_size: false,
            render: Render::new(),
            front: pixel_buffer(0xFF_FFFF),
            back: pixel_buffer(0xFF_FFFF),
            dmg_palette: [0xFF_FFFF, 0xAA_AAAA, 0x55_5555, 0x00_0000],
            sgb_mono: false,
            sgb: matches!(model, Model::Sgb | Model::Sgb2).then(SgbView::new),
        }
    }

    /// Advance one dot. Returns IF bits to request
    /// (bit 0 = vblank, bit 1 = STAT), 0 if none.
    pub fn tick(&mut self) -> u8 {
        self.strobe_tick();
        // The deferred BG-fetcher LCDC render view catches up (like the
        // `stat_ev` staged copies): applied before this dot's `render_step` so
        // a bit3/bit4 write staged K dots ago drives the fetch grid from dot
        // W+K.
        let apply_render_lcdc = if let Some((_, dots)) = &mut self.render_lcdc_pending {
            *dots -= 1;
            *dots == 0
        } else {
            false
        };
        if apply_render_lcdc {
            let value = self.render_lcdc_pending.take().map_or(0, |(v, _)| v);
            let old = self.eff.render_lcdc;
            self.eff.render_lcdc = value;
            // A mid-mode-3 LCDC.5 clear's RENDER re-anchor fires here, at the
            // deferred render frame, when the deferred bit5 view falls 1→0 (the
            // read-law half already fired in `regs.rs::commit_eff`). So the
            // drawn window ends at the render dot, not cc+0
            // (`m3_lcdc_win_en_change_multiple`).
            if old & LCDC_WIN_ENABLE != 0 && value & LCDC_WIN_ENABLE == 0 && self.render.active {
                self.window_abort_render();
            }
        }
        // Delayed event-register copies catch up (see `stat_ev`); applied
        // before this dot's events so a value staged K dots ago becomes
        // visible to events from dot W+K on.
        for (staged, cur) in [
            (&mut self.stat_ev_staged, &mut self.stat_ev),
            (&mut self.lyc_ev_m_staged, &mut self.lyc_ev_m),
            (&mut self.stat_lyc_ev_staged, &mut self.stat_lyc_ev),
        ] {
            if let Some((value, dots)) = staged {
                *dots -= 1;
                if *dots == 0 {
                    *cur = *value;
                    *staged = None;
                }
            }
        }
        if !self.enabled {
            // With the LCD off `GB_STAT_update` returns early
            // (`display.c:525`) and the interrupt line is held low, so a
            // re-enable edge-detects from a clean low.
            self.stat_update = crate::stat_update::StatUpdate::new();
            self.lyc_interrupt_line = false;
            // A staged FF41 engine view must not survive an LCD-off
            // gap and apply at a stale tick after re-enable.
            self.eng_stat = self.stat_en;
            self.eng_stat_pending = None;
            self.eng_stat_half = None;
            self.ff41_ds_drop = None;
            self.stat_if_squash = 0;
            self.ack_squash_ppu = 0;
            self.ack_squash_ppu_mask = 0;
            return std::mem::take(&mut self.pending_if);
        }
        if self.lyc_if_delay > 0 {
            self.lyc_if_delay -= 1;
            if self.lyc_if_delay == 0 {
                // CGB-deferred FF45-write STAT IRQ (see `lyc_if_delay`).
                self.pending_if |= IF_STAT;
            }
        }
        // The SameBoy `double_speed_alignment` shadow (see `sb_dsa8`).
        self.sb_dsa8 = (self.sb_dsa8 + 2) & 7;
        self.dot += 1;
        // A pending write-scheduled compare runs BEFORE the line wrap, like
        // SameBoy's (`display.c:1553-1579` sits at the top of
        // `GB_display_run`, ahead of the line-length rollover): a WY write in
        // a line's tail therefore compares against the OLD line and latches
        // there (`late_wy_FFto0_ly2_1` — SameBoy hits at `ly0 cmp=0`, then
        // line 1's own compares miss but the latch is already sticky).
        self.wy_check_scheduled_tick();
        let len = self.line_len();
        if self.dot == len {
            self.dot = 0;
            self.glitch_line = false;
            // The window line counter advances at window *activation*
            // (see `win_line`), not at line end.
            self.render.win_active = false;
            self.line = if self.line == 153 { 0 } else { self.line + 1 };
            // Lives across the whole line: a mode-2 clear has to still be
            // visible to the mode-3 re-enable deadline, which `render_init`
            // (mode-3 start) would otherwise wipe.
            self.render.win_disabled_line = false;
            self.start_line();
        }
        // One dot of LCDC.1 history for the object fetcher's delayed view,
        // pushed before this dot's render step so bit 0 is this dot's level.
        self.obj_en_lag = (self.obj_en_lag << 1) | u8::from(self.eff.lcdc & LCDC_OBJ_ENABLE != 0);
        self.step_dot();
        // Close this dot's LCDC.2 snapshot for the next dot's OAM scan step
        // (see `scan_obj_size`).
        self.scan_obj_size = self.eff.lcdc & LCDC_OBJ_SIZE != 0;
        // Maintain the decoupled interrupt-facing mode (`mode_for_interrupt`),
        // consulted by the STAT engine on the very next line. Runs after
        // step_dot so it sees this dot's `line_render_done` flip.
        self.update_mode_for_interrupt();
        // The SameBoy `GB_STAT_update` rising-edge engine (production STAT
        // dispatch), off the decoupled `mode_for_interrupt` + the LYC latch.
        self.stat_update_tick();
        // Age the dispatch-ack squash window.
        self.ack_squash_ppu = self.ack_squash_ppu.saturating_sub(1);
        self.ly0_pulse_age = self.ly0_pulse_age.saturating_sub(1);
        self.m0sh_age = self.m0sh_age.saturating_sub(1);
        std::mem::take(&mut self.pending_if)
    }

    /// Advance one 8 MHz HALF-dot — the PPU's tick grain: two half-dots
    /// per whole dot (single speed = 2 half-dots per CPU-T; double speed = 1).
    /// The first half of a dot (`dhalf 0→1`) does no structural work and the
    /// second (`dhalf 1→0`) runs the whole-dot [`Self::tick`] body, so a run of
    /// aligned half-dots is byte-identical to the whole-dot advance; the seam is
    /// that a mode-3-exit / read boundary can sit on the odd half-dot. This is
    /// the emulation loop's only PPU tick path: the per-M-cycle machine advance
    /// (`interconnect::tick`), the mid-M-cycle write strobe (`interconnect::bus`)
    /// and the STOP dance (`interconnect::speed`) all drive it. (The post-boot
    /// LCD warmup in `interconnect::boot` is the one caller that advances whole
    /// dots through [`Self::tick`] directly, since it only needs to reach a
    /// known line/dot.) Returns the IF bits produced (0 on the non-completing
    /// half).
    pub(crate) fn tick_half(&mut self) -> u8 {
        if self.dhalf == 0 {
            self.dhalf = 1;
            // The odd half-dot's position; the even one is taken by `tick`.
            self.wy_check_scheduled_tick();
            // Advance the write STROBE on the non-completing half too, so a
            // staged mid-mode-3 register commit lands at its true half-dot, not
            // only at whole-dot boundaries (`stage_write` doubles `dots_left`
            // for the ×2 grid).
            self.strobe_tick();
            // The odd-half STAT-engine level re-eval, so a coincident FF41
            // write-commit / LYC re-latch / mode-0 rise resolves at its true
            // sub-dot phase. Idempotent on the aligned grid (see
            // `stat_update_half`).
            self.stat_update_half();
            return 0;
        }
        self.dhalf = 0;
        self.tick()
    }

    /// SameBoy `wy_check` (`display.c:508`): latch the frame-sticky
    /// [`Self::wy_triggered`] when the window is enabled and WY equals the
    /// PPU's comparison line. The comparison is the raw line on CGB single
    /// speed and `ly_for_comparison` otherwise (DMG and double speed), which
    /// is what makes a line-boundary WY write land on the OLD line there.
    /// Both operands are architectural (SameBoy reads `io_registers`), so a
    /// write is visible to the very next compare.
    fn wy_check(&mut self) {
        if !self.enabled || self.lcdc & LCDC_WIN_ENABLE == 0 {
            return;
        }
        if i16::from(self.wy) == self.wy_comparison() {
            if !self.wy_triggered {
                self.wy_trig_dot = self.dot;
            }
            self.wy_triggered = true;
        }
    }

    /// The line [`Self::wy_check`] compares WY against: the raw line on CGB
    /// single speed, `ly_for_comparison` otherwise (DMG and double speed),
    /// which is what lets a line-boundary WY write land on the old line there.
    fn wy_comparison(&self) -> i16 {
        if !self.model.is_cgb() || self.ds {
            let lyfc = self.ly_for_comparison();
            if lyfc != -1 {
                return lyfc;
            }
        }
        i16::from(self.line)
    }

    /// The half-dot phase a write-scheduled [`Self::wy_check`] lands on:
    /// SameBoy's `K` in `8 - ((wy_check_modulo + K) & 7)`
    /// (`display.c:1560-1569`) — 0 on CGB single speed, 2 on DMG, 6 in double
    /// speed.
    fn wy_check_phase_hd(&self) -> i32 {
        match (self.model.is_cgb(), self.ds) {
            (true, false) => 0,
            (true, true) => 2,
            (false, _) => 0,
        }
    }

    /// Schedule a WY/LCDC write's deferred [`Self::wy_check`]: 1-8 half-dots
    /// out, landing on the model's 4-dot phase. The write commits at the
    /// M-cycle END while SameBoy's display coroutine has only run to the
    /// write's own T, so the schedule counts from `write_debt_hd` half-dots
    /// back.
    pub(in crate::ppu) fn schedule_wy_check_at(&mut self, extra_hd: u8) {
        self.schedule_wy_check();
        self.wy_check_in = self.wy_check_in.saturating_add(extra_hd);
    }

    pub(in crate::ppu) fn schedule_wy_check(&mut self) {
        let pos = 2 * i32::from(self.dot) + i32::from(self.dhalf);
        self.wy_check_in = (8 - ((pos + self.wy_check_phase_hd()).rem_euclid(8))) as u8;
    }

    /// Dots slopgb's WX comparator match leads SameBoy's window activation.
    /// A WX <= 7 window matches during the prefill, and SameBoy's
    /// `position_in_line` only reaches that position after the SCX fine-scroll
    /// discard has been waited out, so its activation sits `SCX & 7` dots later
    /// than slopgb's fixed `pos_dot == WX + 6` match. Dual-traced on
    /// `gambatte/window/arg/late_wy_FFto2_ly2{,_scx2,_scx3,_scx5}`: SameBoy
    /// activates a WX=7 window at dot 97, 99, 100, 102 for SCX&7 = 0, 2, 3, 5
    /// where slopgb matches at 97 throughout. A WX >= 8 window matches on the
    /// output position `lx`, which has already absorbed the discard, so it
    /// leads by nothing (`..._wx0f`: both activate at 105).
    pub(in crate::ppu) fn win_activation_lead(&self) -> u16 {
        if self.eff.wx <= 7 {
            // The discard the fine-scroll comparator actually locked in, not
            // the read-time SCX: a mid-line SCX rewrite that missed the hunt
            // does not move the window's fetch
            // (`late_scx_late_wy_FFto4_ly4_wx00`).
            let hf = u16::from(self.render.hunt_fine & 7);
            if self.model.is_cgb() {
                // A WX < 7 window cuts its leading `7 - WX` columns, and those
                // columns consume that much of the discard the activation is
                // waiting out — so they come off the lead. The law above was
                // measured at WX = 7, where the term is zero.
                // Pins gambatte/window/arg/late_scx_late_wy_FFto4_ly4_wx00_2
                // [Cgb] (hunt_fine 4, WX 0 → lead 0, not 4). CGB only: the same
                // subtraction on DMG un-triggers that row's DMG sibling.
                hf.saturating_sub(7 - u16::from(self.eff.wx))
            } else {
                hf
            }
        } else {
            0
        }
    }

    /// The window-Y latch as SameBoy's activation test sees it: already
    /// latched, or a write-scheduled compare ([`Self::wy_check_in`]) that comes
    /// due strictly before SameBoy's activation instant, which trails slopgb's
    /// WX match by [`Self::win_activation_lead`] (a compare landing ON that
    /// instant does not make it — `late_wy_FFto2_ly2_scx3_2`, write dot 96,
    /// compare dot 100, SameBoy activation dot 100, renders bare). Both compare operands are settled at
    /// the match, so resolving the pending compare here evaluates SameBoy's
    /// predicate at SameBoy's instant without moving slopgb's — the fine-scroll
    /// delay is spent as a pixel discard here, not as a later activation.
    pub(super) fn wy_triggered_for_activation(&self) -> bool {
        if self.wy_triggered {
            return true;
        }
        if self.wy_check_in == 0 || !self.enabled || self.lcdc & LCDC_WIN_ENABLE == 0 {
            return false;
        }
        u16::from(self.wy_check_in) < 2 * self.win_activation_lead()
            && i16::from(self.wy) == self.wy_comparison()
    }

    /// Count a pending write-scheduled [`Self::wy_check`] down one half-dot
    /// and run it when it comes due.
    pub(super) fn wy_check_scheduled_tick(&mut self) {
        if self.wy_check_in == 0 {
            return;
        }
        if self.wy_triggered {
            // Already latched: SameBoy drops the pending check outright
            // (`display.c:1554`).
            self.wy_check_in = 0;
            return;
        }
        self.wy_check_in -= 1;
        if self.wy_check_in == 0 {
            self.wy_check();
        }
    }

    /// Whether the half-dot just advanced by [`Self::tick_half`] completed a
    /// whole dot (the whole-dot body ran). The caller folds the PPU's IF /
    /// accessibility edges only on a completing half.
    pub(crate) fn dot_completed(&self) -> bool {
        self.dhalf == 0
    }

    /// The read's EXACT half-dot position within the current line:
    /// `2*dot + dhalf` on the 8 MHz grid. The machine is advanced T-granularly
    /// to the read's sample instant (the `GB_display_sync` analogue), so at
    /// that instant this IS the read's true half-dot — a DS read landing on an
    /// odd CPU-T resolves mid-dot (`dhalf == 1`), which the whole-dot
    /// `self.dot` alone cannot represent (the "+3 not +4" DS ISR read offset).
    /// Every half-dot read-position law compares against this ONE value; the
    /// per-ISR sub-M-cycle carry is [`Self::isr_read_carry_hd`], kept separate
    /// so polled reads stay uncarried.
    pub(crate) fn read_pos_hd(&self) -> i32 {
        // The cc+0 read → deferred read-debt in 8 MHz half-dots. Single speed:
        // an M-cycle is 4 dots (8 hd), so the deferred read lands 4 dots (8 hd)
        // ahead of cc+0. Double speed: the CPU M-cycle is 2 dots (4 hd — the
        // CPU runs 2×), so the deferred DS read lands only 2 dots (4 hd) ahead;
        // the DS exit constants (`vis_exit_hd`'s `ds1`/DS arms) are calibrated
        // to that +2-dot position, so the DS read must advance the matching
        // +4 hd to resolve them on the same frame.
        const EAGER_READ_DEBT_HD_SS: i32 = 8;
        const EAGER_READ_DEBT_HD_DS: i32 = 4;
        let base = 2 * i32::from(self.dot) + i32::from(self.dhalf);
        // `Bus::read` samples FF41 at cc+0, one M-cycle (SS 4 dots / DS 2 dots)
        // before the deferred read the [`Ppu::vis_exit_hd`] exit constants are
        // calibrated against. Advance the read position by that debt (SS +8 hd /
        // DS +4 hd) so the exit constants resolve on the same frame — the
        // coupled render-length + read-exit laws then separate the window
        // `_1`/`_2` pairs at both speeds. The residual DS sub-dot (`sb_dsa8`
        // mid-dot / `read_carried` ISR carry) is not reconstructed on the
        // whole-dot clock, so a handful of DS pre-draw-abort / STOP-shift legs
        // stay parked.
        base + if self.ds {
            EAGER_READ_DEBT_HD_DS
        } else {
            EAGER_READ_DEBT_HD_SS
        }
    }

    /// The per-ISR deferred-read sub-M-cycle carry (8 MHz half-dots), applied
    /// ON TOP of [`Self::read_pos_hd`] by the laws that model a STAT-ISR
    /// handler's first FF41 read. Measured offsets: a carried (`read_carried`)
    /// mode-2 OAM-ISR read sits +4 hd late of the polled frame at single speed,
    /// a mode-0 HBlank-ISR read +2 hd; in double speed only the mode-0-ISR read
    /// differs (−4 hd). 0 for polled/uncarried reads.
    pub(super) fn isr_read_carry_hd(&self) -> i32 {
        if !self.read_carried {
            return 0;
        }
        if self.ds {
            if self.stat_rise_m0 { -4 } else { 0 }
        } else if self.stat_rise_oam {
            4
        } else if self.stat_rise_m0 {
            2
        } else {
            0
        }
    }

    /// The SameBoy `double_speed_alignment` shadow, mod 8 (see
    /// [`Self::sb_dsa8`]). Read by the STOP leave shift; the −4-per-pause
    /// correction is applied by [`Self::dsa_pause_correction`].
    pub(crate) fn sb_dsa(&self) -> u8 {
        self.sb_dsa8
    }

    /// Apply the per-STOP-pause alignment correction (−4 mod 8, the measured
    /// SameBoy-vs-slopgb pause delta).
    pub(crate) fn dsa_pause_correction(&mut self) {
        self.sb_dsa8 = (self.sb_dsa8 + 4) & 7;
    }

    /// Record a machine STOPADV advance (see [`Self::lcd_shift_dots`]).
    pub(crate) fn add_lcd_shift(&mut self, dots: u16) {
        self.lcd_shift_dots += dots;
    }

    /// Latch the post-switch exit-table anchor at a switching STOP (see
    /// [`Self::stop_anchor_midframe`]). Called at the STOP decision point; the
    /// FIRST LCD-on switching STOP since the last LCD enable pins the dance's
    /// calibration class.
    pub(crate) fn note_switch_stop(&mut self) {
        if self.enabled && !self.stop_anchor_set {
            self.stop_anchor_set = true;
            self.stop_anchor_midframe = self.line < 144;
        }
    }

    /// Record a DS→SS STOP leave (see [`Self::stop_leave_lcd_on`]); `k` = the
    /// applied leave advance in half-dots.
    pub(crate) fn note_switch_leave(&mut self, k: u8) {
        if self.enabled {
            self.stop_leave_lcd_on = true;
            self.stop_leave_k = k;
        }
    }

    /// The current access position mapped back onto the un-shifted calibrated
    /// frame (see [`Self::lcd_shift_dots`]): subtract the machine advance,
    /// wrapping across the line boundary. Identity when no advance was applied
    /// (never-switched ROMs).
    pub(super) fn law_pos(&self) -> (u8, u16) {
        let s = self.lcd_shift_dots;
        if s == 0 {
            return (self.line, self.dot);
        }
        if self.dot >= s {
            (self.line, self.dot - s)
        } else {
            let prev = if self.line == 0 { 153 } else { self.line - 1 };
            (prev, LINE_DOTS - (s - self.dot))
        }
    }

    fn step_dot(&mut self) {
        // CGB: the line-start LYC event's delayed FF45 copy catches up
        // outside the 4-dot lead-in of each event — dot 4, and 153:12
        // for the LYC=0 event (see `lyc_event`; gambatte
        // LycIrq::regChange's `time_ - cc` windows).
        if self.model.is_cgb() {
            let protected =
                (1..=4).contains(&self.dot) || (self.line == 153 && (9..=12).contains(&self.dot));
            if !protected {
                self.lyc_event = self.lyc;
            }
        }
        if self.dot == 4 {
            // The mode-0 IRQ source level (raised by the previous line's
            // `m0_flip_events`) drops when the mode-2 window becomes
            // visible.
            self.m0_src = false;
        }
        // The frame-sticky window-Y latch clears at the frame top
        // (`display.c:1686`, the VBlank exit) and is compared at the two
        // per-line points SameBoy runs `wy_check` from: the line's start
        // (`display.c:1755`, before mode 2, against `current_line`) and the
        // mode-2 rise (`display.c:1815`, once `ly_for_comparison` holds the
        // line). The dot-4 rise is `ly_for_comparison`'s own latch dot
        // (`ly_for_comparison_at`).
        // Line 0's compare sits later than the visible-line one. Hardware
        // pins it through the `late_wy{,_ds}{,_lcdoffset1}` pairs, whose only
        // difference is one M-cycle in a WY->FF write that must either beat the
        // compare (no window that frame) or miss it: writes at dot 6 / 5 kill
        // the trigger while writes at 8 / 7 do not (double speed), and at
        // single speed a dot-7 write kills it where a dot-11 write does not.
        // That brackets the compare to dot 7 in double speed and 8..11 in
        // single, against dot 4 for lines 1-143. DMG keeps dot 4 on line 0 too:
        // its own pair puts the kill/no-kill boundary between a dot-0 and a
        // dot-4 write (`late_wy_{1,2}` [Dmg]).
        // Double speed runs the compare one dot earlier on EVERY line, not just
        // line 0: `late_wy_1toFF_ds_lcdoffset1_{1,2}` write WY->FF at line-1
        // dots 1 and 3, and only the dot-1 write may beat the compare, which
        // puts it at dot 3. Dots 1/2 drop five other rows, dot 4 drops this
        // pair — the bracket is exact.
        let base = if self.line == 0 && self.model.is_cgb() {
            8
        } else {
            4
        };
        let compare_dot = base - u16::from(self.ds);
        if self.dot == compare_dot {
            self.wy_check();
        }
        if self.line <= 143 {
            if self.glitch_line {
                if self.dot == GLITCH_MODE3_START {
                    self.render_init();
                } else if self.render.active {
                    self.render_step();
                }
            } else {
                match self.dot {
                    // Serial OAM scan: one entry latched + evaluated per
                    // 2 dots (see `scan_latch_dot` in render.rs); the last
                    // entry is consumed before mode 3 starts at dot 84.
                    d if d < 84 => self.oam_scan_step(),
                    84 => self.render_init(),
                    d => {
                        if self.render.active && d > 84 {
                            self.render_step();
                        }
                    }
                }
            }
            // Visible mode-0 flip + IRQ-source rise (after the dot's
            // render step so the projection sees this dot's state).
            self.m0_flip_events();
        }
        if self.model.is_cgb() && !self.ds && self.line == 152 && self.dot == 454 {
            // CGB-C single speed loads LY=153 two dots before line 153
            // starts: the readable window is dots -2..3 around the
            // boundary, which is how wilbertpol ly_new_frame-C's
            // frame-anchored reads (the boot grid sits 2 dots off the
            // M-cycle lattice, see Model::post_boot_state) catch 153 on
            // two consecutive M-cycles while age ly-dmgC-cgbBC's
            // enable-anchored ladder sees it exactly once.
            self.ly = 153;
        }
        if self.line == 153 {
            // Line 153 quirk: LY reads 0 from dot 4 (TCAGBD §8.9). In
            // CGB double speed the wrap comes 2 dots later — age
            // ly-dmgC-cgbBC's ds ladder reads 153 at three consecutive
            // 2-dot-spaced points; SameBoy display.c holds LY=153 for
            // the longer sleep when `cgb_double_speed`.
            let wrap = if self.model.is_cgb() && self.ds { 6 } else { 4 };
            if self.dot == wrap {
                self.ly = 0;
            }
        }
        if self.line == 144 && self.dot == 4 {
            // VBlank interrupt: 4 dots after LY becomes 144, together with the
            // visible mode 1 (TCAGBD; `vblank_stat_intr-GS`). A vblank-vector
            // ack 1-2 dots earlier (SS) merges this raise into the dispatch it
            // interrupted (`lycint_vblankirq_late_retrigger_2` want 0: ack
            // 144:2, raise 144:4 consumed; the `_ds_1` ack at 144:3 DELIVERS —
            // DS window 0).
            let w = if self.ack_squash_ppu_mask & IF_VBLANK != 0 && !self.ds {
                2
            } else {
                0
            };
            if w > 0 && self.ack_squash_ppu >= 3 - w {
                self.ack_squash_ppu = 0;
            } else {
                self.pending_if |= IF_VBLANK;
            }
        }
    }
}
