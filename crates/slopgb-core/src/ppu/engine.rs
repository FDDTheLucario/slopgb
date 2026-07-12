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
            leading_edge_reads: false,
            tier2_reclock: false,
            eager_value: false,
            m0_rise: false,
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
            flip_dot: 0,
            vis_early: false,
            vis_hold_until: 0,
            render_finished: true,
            hdma_lead: false,
            pal_open_dot: 0,
            wy_latch: false,
            wy2: 0,
            wy2_delay: 0,
            wy_trig_sb: false,
            wy_trig_sb_line: 0,
            wy_trig_sb_dot: 0,
            wy_trig_sb_raw: false,
            stop_anchor_set: false,
            stop_anchor_midframe: false,
            stop_leave_lcd_on: false,
            stop_leave_k: 2,
            lcd_enable_in_ds: false,
            wy_xline_trig: false,
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
            render: Render::new(),
            front: pixel_buffer(0xFF_FFFF),
            back: pixel_buffer(0xFF_FFFF),
            dmg_palette: [0xFF_FFFF, 0xAA_AAAA, 0x55_5555, 0x00_0000],
        }
    }

    /// Advance one dot. Returns IF bits to request
    /// (bit 0 = vblank, bit 1 = STAT), 0 if none.
    pub fn tick(&mut self) -> u8 {
        self.strobe_tick();
        // The deferred BG-fetcher LCDC render view catches up
        // (like the `stat_ev` staged copies): applied before this dot's
        // `render_step` so a bit3/bit4 write staged K dots ago drives the fetch
        // grid from dot W+K. Only scheduled under the tier2 reclock during an
        // active render (byte-identical OFF).
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
            // A mid-mode-3 LCDC.5 clear's RENDER re-anchor fires HERE, at
            // the deferred render frame, when the deferred bit5 view falls 1→0
            // (the read-law flag half already fired eagerly in
            // `regs.rs::commit_eff`). So the drawn window ends at the render dot,
            // not the eager cc+0 (`m3_lcdc_win_en_change_multiple`). Only reached
            // under the tier2 `render_lcdc` defer (production sets the view in
            // lockstep with no pending) — byte-identical OFF.
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
        if self.wy2_delay > 0 {
            self.wy2_delay -= 1;
            if self.wy2_delay == 0 {
                self.wy2 = self.wy;
            }
        }
        if !self.enabled {
            // Flag-on engine: with the LCD off `GB_STAT_update` returns
            // early (`display.c:525`) and the interrupt line is held low, so a
            // re-enable edge-detects from a clean low. Inert flag-off (the
            // fields are unread), so this changes nothing in production.
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
        let len = self.line_len();
        if self.dot == len {
            self.dot = 0;
            self.glitch_line = false;
            // The window line counter advances at window *activation*
            // (see `win_line`), not at line end.
            self.render.win_active = false;
            self.line = if self.line == 153 { 0 } else { self.line + 1 };
            self.start_line();
        }
        self.step_dot();
        // Maintain the decoupled interrupt-facing mode (inert — not yet
        // consulted; the STAT engine that reads it runs on the flag-on path).
        // Runs after step_dot so it sees this dot's `line_render_done` flip.
        self.update_mode_for_interrupt();
        if self.leading_edge_reads {
            // Flag-on path: the SameBoy `GB_STAT_update` rising-edge engine
            // off the decoupled `mode_for_interrupt` + the LYC latch.
            self.stat_update_tick();
        } else {
            // Production path: the gambatte-derived per-source event engine.
            self.stat_events_tick();
        }
        // Age the dispatch-ack squash window (armed only on
        // the tier2 path; a saturating decrement of an always-0 counter is
        // byte-identical flag-off).
        self.ack_squash_ppu = self.ack_squash_ppu.saturating_sub(1);
        self.ly0_pulse_age = self.ly0_pulse_age.saturating_sub(1);
        self.m0sh_age = self.m0sh_age.saturating_sub(1);
        std::mem::take(&mut self.pending_if)
    }

    /// Advance one 8 MHz HALF-dot. The pixel-pipe
    /// reclock's grain: two half-dots per whole dot (single speed = 2 half-dots
    /// per CPU-T; double speed = 1). The first half of a dot (`dhalf 0→1`) does
    /// no structural work and the second (`dhalf 1→0`) runs the whole-dot
    /// [`Self::tick`] body, so a run of aligned half-dots is byte-identical to
    /// the whole-dot advance; the seam is that later stages move a
    /// mode-3-exit / read boundary onto the odd half-dot. Called only on the
    /// tier2 deferred path ([`Interconnect::advance_machine_t`]); production
    /// never calls it, so the flag-off path is untouched. Returns the IF bits
    /// produced (0 on the non-completing half).
    pub(crate) fn tick_half(&mut self) -> u8 {
        if self.dhalf == 0 {
            self.dhalf = 1;
            // HALFDOT Part A-render (eager): advance the write STROBE on the
            // non-completing half too, so a staged mid-mode-3 register commit
            // lands at its true HALF-dot instead of only whole-dot boundaries.
            // `stage_write` doubles `dots_left` under `eager_value` (the ×2
            // grid conversion), so a run of aligned half-dots still commits at
            // the same whole dot (byte-identical on the aligned grid); the seam
            // is the per-register SameBoy half-dot offset. Tier2 keeps the
            // whole-dot strobe (this half is inert with `eager_value` false).
            if self.eager_value {
                self.strobe_tick();
                // HALFDOT (#11dw): the odd-half STAT-engine level re-eval, so a
                // coincident FF41 write-commit / LYC re-latch / mode-0 rise
                // resolves at its true sub-dot phase. Idempotent on the aligned
                // grid → byte-identical (see `stat_update_half`).
                self.stat_update_half();
            }
            return 0;
        }
        self.dhalf = 0;
        self.tick()
    }

    /// Whether the half-dot just advanced by [`Self::tick_half`] completed a
    /// whole dot (the whole-dot body ran). The caller folds the PPU's IF /
    /// accessibility edges only on a completing half.
    pub(crate) fn dot_completed(&self) -> bool {
        self.dhalf == 0
    }

    /// The deferred read's EXACT half-dot
    /// position within the current line: `2*dot + dhalf` on the 8 MHz grid.
    /// [`Interconnect::read_deferred`] advances the machine T-granularly to the
    /// read's leading edge (the `GB_display_sync` analogue), so at the sample
    /// instant this IS the read's true half-dot — a DS read landing on an odd
    /// CPU-T resolves mid-dot (`dhalf == 1`), which the whole-dot `self.dot`
    /// alone cannot represent (the "+3 not +4" DS ISR read offset). Every
    /// half-dot read-position law compares against this ONE value; the per-ISR
    /// sub-M-cycle carry is [`Self::isr_read_carry_hd`], kept separate so
    /// polled reads stay uncarried. Production reads never resolve mid-dot
    /// (`dhalf` stays 0 flag-off) and no flag-off law consumes this.
    pub(crate) fn read_pos_hd(&self) -> i32 {
        // The eager cc+0 → deferred read-debt in 8 MHz half-dots. Single speed:
        // an M-cycle is 4 dots (8 hd), so the deferred read lands 4 dots ahead of
        // the eager cc+0. DOUBLE SPEED: the CPU M-cycle is 2 dots (4 hd — the CPU
        // runs 2×), so the deferred DS read lands only 2 dots (4 hd) ahead; the
        // tier2 DS exit constants (`vis_exit_hd`'s `ds1`/DS arms) are calibrated
        // to that +2-dot deferred position, so the eager DS read must advance the
        // matching +4 hd to resolve them on the same frame.
        const EAGER_READ_DEBT_HD_SS: i32 = 8;
        const EAGER_READ_DEBT_HD_DS: i32 = 4;
        let base = 2 * i32::from(self.dot) + i32::from(self.dhalf);
        // Eager-clock read-debt: the eager `Bus::read` samples FF41 at cc+0 (this
        // M-cycle's leading edge), one M-cycle (SS 4 dots / DS 2 dots) BEFORE the
        // deferred read (`read_deferred` pays the previous M-cycle's parked debt,
        // landing at the cc+4-equivalent position) that the tier2 exit constants
        // in [`Ppu::vis_exit_hd`] are calibrated against. Advance the eager read
        // position by that debt (SS +8 hd / DS +4 hd) so the exit constants
        // resolve at the same frame — the coupled render-length + read-exit laws
        // then separate the window `_1`/`_2` pairs on the eager clock at BOTH
        // speeds (measured: SS coupling CGB two-bin 578→553, DS debt 553→525).
        // The residual DS sub-dot (`sb_dsa8` mid-dot / `read_carried` ISR carry)
        // is not reconstructed on the eager whole-dot clock, so a handful of DS
        // pre-draw-abort / STOP-shift legs stay parked. Never fires flag-off
        // (`eager_value` false) → production byte-identical.
        base + if self.eager_value {
            if self.ds { EAGER_READ_DEBT_HD_DS } else { EAGER_READ_DEBT_HD_SS }
        } else {
            0
        }
    }

    /// The per-ISR deferred-read sub-M-cycle carry (8 MHz half-dots),
    /// applied ON TOP of [`Self::read_pos_hd`] by the laws that model a
    /// STAT-ISR handler's first FF41 read. The measured offsets:
    /// a carried (`read_carried`) mode-2 OAM-ISR read sits +4 hd late of the
    /// polled frame at single speed, a mode-0 HBlank-ISR read +2 hd; in double
    /// speed only the mode-0-ISR read differs (−4 hd — the full-carry
    /// law's `off = m0 ? 2 : 4` rewritten on the half-dot grid, exit-folded).
    /// 0 for polled/uncarried reads. Byte-identical OFF (`read_carried` is
    /// only armed on the tier2 dispatch path).
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

    /// Apply the per-STOP-pause alignment correction (−4 mod 8, the
    /// measured SameBoy-vs-slopgb pause delta). Tier2 STOP path only.
    pub(crate) fn dsa_pause_correction(&mut self) {
        self.sb_dsa8 = (self.sb_dsa8 + 4) & 7;
    }

    /// Record a machine STOPADV advance (see [`Self::lcd_shift_dots`]).
    pub(crate) fn add_lcd_shift(&mut self, dots: u16) {
        self.lcd_shift_dots += dots;
    }

    /// Latch the post-switch exit-table anchor at a switching STOP
    /// (see [`Self::stop_anchor_midframe`]). Called at the STOP decision
    /// point, tier2 only; the FIRST LCD-on switching STOP since the last
    /// LCD enable pins the dance's calibration class.
    pub(crate) fn note_switch_stop(&mut self) {
        if self.enabled && !self.stop_anchor_set {
            self.stop_anchor_set = true;
            self.stop_anchor_midframe = self.line < 144;
        }
    }

    /// Record a DS→SS STOP leave (see [`Self::stop_leave_lcd_on`]);
    /// `k` = the applied leave advance in half-dots. Tier2 only.
    pub(crate) fn note_switch_leave(&mut self, k: u8) {
        if self.enabled {
            self.stop_leave_lcd_on = true;
            self.stop_leave_k = k;
        }
    }

    /// The current access position mapped back onto the un-shifted
    /// calibrated frame (see [`Self::lcd_shift_dots`]): subtract the machine
    /// advance, wrapping across the line boundary. Identity when no advance
    /// was applied (never-switched ROMs, production).
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

    /// Forward the interconnect's `leading_edge_reads` master flag to the PPU,
    /// selecting the [`StatUpdate`](crate::stat_update) engine. Off in
    /// production until the atomic flip (which flips the default in `new`, not
    /// via this hook); driven by [`Interconnect::set_leading_edge_reads`] (the
    /// unit tests + the kernel-pair acceptance spec).
    pub(crate) fn set_leading_edge_reads(&mut self, on: bool) {
        self.leading_edge_reads = on;
    }

    /// Forward the interconnect's `tier2_reclock` flag. Gates
    /// the mode-0 IRQ dispatch move; implies `leading_edge_reads`.
    pub(crate) fn set_tier2_reclock(&mut self, on: bool) {
        self.tier2_reclock = on;
    }

    /// Forward the interconnect's `eager_value` flag. Implies
    /// `leading_edge_reads` (set on the same hook) but NOT `tier2_reclock`.
    // ponytail: only reachable via the port-probe-gated interconnect hook;
    // slice #2 adds the law reads. Drop the allow then.
    #[allow(dead_code)]
    pub(crate) fn set_eager_value(&mut self, on: bool) {
        self.eager_value = on;
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
        // Frame-sticky WY condition (gambatte weMaster): sampled at
        // discrete dots, not compared continuously — see `wy_latch`.
        // gambatte's line-cycle anchors translate to our dot convention
        // with a +1 shift on DMG (m3StartLineCycle is 83+cgb against our
        // model-independent mode-3 start at dot 84).
        let win_en = self.eff.lcdc & LCDC_WIN_ENABLE != 0;
        let late = u16::from(!self.model.is_cgb());
        if self.dot == 4 {
            // The mode-0 IRQ source level (raised by the previous line's
            // `m0_flip_events`) drops when the mode-2 window becomes
            // visible.
            self.m0_src = false;
        }
        // Shadow WY-trigger (tier2-only; byte-identical OFF).
        // SameBoy's `wy_triggered` is a continuous `WY == LY` latch, sticky for
        // the frame; reset it at the frame top (line 0 dot 0) and set it the
        // first dot the compare holds on any visible line. See `wy_trig_sb`.
        // Recording widened to DMG (was CGB-only) for the DMG window
        // law port — the DMG arms in `read_laws.rs` read the same latches.
        if self.tier2_reclock || self.eager_value {
            if self.line == 0 && self.dot == 0 {
                self.wy_trig_sb = false;
                self.wy_trig_sb_raw = false;
                self.wy_xline_trig = false;
            }
            // The raw-WY sticky latch (immediate `self.wy`, SameBoy's
            // `wy_check` input), the un-trigger discriminator. Gated `dot >= 4`
            // (the mode-2 OAM-scan compare window SameBoy runs `wy_check` in):
            // a line-start (dot 0) WY write commits AFTER slopgb's dot-0 PPU tick
            // (the tick runs before `write_no_tick`), so a dot-0 compare would
            // read the OLD WY and mis-latch; `dot >= 4` samples the settled WY,
            // matching SameBoy's post-write compare (`late_wy_1toFF_1` WY→FF at
            // dot 0 → WY=FF by dot 4 → never latches → SameBoy-bare).
            if self.line < 144
                && self.dot >= 4
                && !self.wy_trig_sb_raw
                && win_en
                && self.wy == self.ly
            {
                self.wy_trig_sb_raw = true;
            }
            if self.line < 144 && !self.wy_trig_sb && win_en && self.wy2 == self.ly {
                self.wy_trig_sb = true;
                self.wy_trig_sb_line = self.ly;
                self.wy_trig_sb_dot = self.dot;
                probe!(if crate::probe::s5dbg_on() {
                    eprintln!(
                        "SLOPGB wytrigset ly={} dot={} wy2={}",
                        self.ly, self.dot, self.wy2
                    );
                });
            }
        }
        if self.line == 0 && self.dot == 2 {
            // Line 0: assignment, not OR — this is the frame reset
            // (gambatte M2_Ly0::f0).
            self.wy_latch = win_en && self.eff.wy == 0;
        } else if self.line < 143 && !self.glitch_line {
            if self.dot == 450 + late {
                self.wy_latch |= win_en && self.ly == self.eff.wy;
            } else if self.dot == 454 + late {
                // Just before the LY increment the comparison already
                // sees the upcoming line (gambatte M2_LyNon0::f1).
                self.wy_latch |= win_en && self.ly + 1 == self.eff.wy;
            }
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
            // Trace the EFFECTIVE CPU-visible mode-3→0 EXIT dot — the dot
            // `vis_mode_read()` (what the FF41 register read returns, incl.
            // the window law / vis_hold / m0_unflip re-projection) actually
            // flips 3→0, to line up against SameBoy's mode trace.
            probe!(if crate::probe::s5dbg_on() {
                use std::cell::Cell;
                thread_local!(static PREV: Cell<u8> = const { Cell::new(255) });
                let vm = self.vis_mode_read();
                PREV.with(|p| {
                    if p.get() == 3 && vm == 0 {
                        eprintln!("SLOPGB visexit ly={} dot={}", self.line, self.dot);
                    }
                    p.set(vm);
                });
            });
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
            // VBlank interrupt: 4 dots after LY becomes 144, together with
            // the visible mode 1 (TCAGBD; `vblank_stat_intr-GS`).
            // A vblank-vector ack 1-2 dots earlier (SS)
            // merges this raise into the dispatch it interrupted
            // (`lycint_vblankirq_late_retrigger_2` want 0: ack 144:2, raise
            // 144:4 consumed; the `_ds_1` ack at 144:3 DELIVERS — DS window
            // 0). Never armed flag-off → production byte-identical.
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
