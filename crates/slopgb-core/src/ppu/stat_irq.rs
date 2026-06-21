//! STAT IRQ event engine: per-source predicates (m0/m1/m2/LYC) with delayed FF41/FF45 copies, mode readout, FF41-write trigger tables, edge/IF takers. Port of gambatte mstat_irq.h. Oracle: gbtr m2int/m0irq/lycm2int, gbmicrotest hblank_int/oam_int, mooneye intr_2_*/stat_irq_blocking.

use super::*;

impl Ppu {
    /// STAT mode bits as read through FF41. This is *not* the rendering
    /// state machine: mode reads 0 during the first 4 dots of every line
    /// (and during 144:0-3), and mode 3 appears 4 dots after VRAM read
    /// locking (`lcdon_timing-GS` tables).
    pub(super) fn vis_mode(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.line >= 144 {
            if self.line == 144 && self.dot < 4 {
                0
            } else {
                1
            }
        } else if self.glitch_line {
            if self.dot < GLITCH_MODE3_START || self.line_render_done {
                0
            } else {
                3
            }
        } else if self.dot < 4 {
            // CGB line 0: the vblank's mode 1 persists through dots 0-3
            // — there is no mode-0 gap before the OAM scan (wilbertpol
            // ly00_mode1_2-C vs ly00_mode1_0-GS; SameBoy display.c only
            // clears the mode bits at the line-0 LY-write dot on DMG;
            // gambatte getStat's mode-1 window runs to 3 cycles before
            // line 0's mode 2).
            u8::from(self.model.is_cgb() && self.line == 0)
        } else if self.dot < 84 {
            2
        } else if !self.line_render_done {
            3
        } else {
            0
        }
    }

    /// STAT mode bits (FF41 bits 0-1) as currently visible to the CPU, for
    /// the interconnect (FEA0-FEFF prohibited-area reads key on OAM locking).
    pub(crate) fn mode_bits(&self) -> u8 {
        self.vis_mode()
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] came
    /// from the line-0 OAM rise and must miss the CPU's interrupt sample
    /// for the current M-cycle (see `stat_events_tick`).
    pub(crate) fn take_stat_late(&mut self) -> bool {
        std::mem::take(&mut self.stat_late)
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] was a
    /// second-half commit that the halt-exit sampler must miss for one
    /// M-cycle (see the `stat_halt_late` field docs). Drained by the
    /// interconnect into its `if_late` halt-wake mask.
    pub(crate) fn take_stat_halt_late(&mut self) -> bool {
        std::mem::take(&mut self.stat_halt_late)
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] came
    /// from the mode-0 source rise (`m0_flip_events`). The interconnect
    /// drains this and, when the rise landed in the second half of the
    /// CPU's M-cycle, masks it from the halt-exit sampler for one
    /// M-cycle (see the `m0_rise` field docs).
    pub(crate) fn take_m0_rise(&mut self) -> bool {
        std::mem::take(&mut self.m0_rise)
    }

    /// The mode-3→mode-0 OAM/VRAM accessibility unblock's `lead_eighths`
    /// `Some(_)` if it fired on the dot just stepped, else `None` (see the
    /// `m0_access_flip` field docs). The interconnect stamps the edge at
    /// `event_phase(M0Access, cc, lead)` so a cc+2 MID-phase OAM read still
    /// sees mode 3 when the unblock lands in the cycle's second half.
    pub(crate) fn take_m0_access_flip(&mut self) -> Option<i8> {
        self.m0_access_flip.take()
    }

    /// The CGB palette-RAM unblock's `lead_eighths` `Some(_)` if it fired on
    /// the dot just stepped, else `None` (see the `pal_access_flip` field
    /// docs). The interconnect stamps the edge at
    /// `event_phase(PalAccess, cc, lead)` so a cc+2 MID-phase FF69/FF6B read
    /// still reads $FF while the palette is blocked.
    pub(crate) fn take_pal_access_flip(&mut self) -> Option<i8> {
        self.pal_access_flip.take()
    }

    /// The mode-3→mode-0 STAT mode-bit flip's `lead_eighths` `Some(_)` if it
    /// fired on the dot just stepped, else `None` (see the `m0_stat_flip`
    /// field docs). The interconnect stamps the edge at
    /// `event_phase(StatMode, cc, lead)` so a cc+2 MID-phase FF41 read in
    /// double speed still reads mode 3 when the flip straddles the M-cycle.
    pub(crate) fn take_m0_stat_flip(&mut self) -> Option<i8> {
        self.m0_stat_flip.take()
    }

    /// Level of the shared STAT interrupt line for the given enable bits.
    /// The LYC source uses the IRQ-side comparison (`cmp_irq` — the
    /// delayed `lyc_event` copy on CGB); FF41 reads show the live `cmp`.
    pub(super) fn stat_line_level(&self, en: u8) -> bool {
        let mut high = en & STAT_SRC_LYC != 0 && self.cmp_irq;
        if !self.enabled {
            // With the LCD off only the (frozen) LYC source persists
            // (`stat_lyc_onoff` round 2: no edge across off/on with cmp=1).
            return high;
        }
        let vm = self.vis_mode();
        // HBlank source: rises at the mode-0 IRQ event (`m0_src`, one
        // dot *before* the visible flip — gambatte memevent_m0irq one
        // xpos ahead of its m0 anchor) and holds through the hblank and
        // the next line's dots 0-3 (and 144:0-3) so consecutive sources
        // overlap and block each other (`stat_irq_blocking`). The
        // glitched post-enable prefix is not a real hblank.
        high |= en & STAT_SRC_HBLANK != 0
            && ((self.line <= 143 && self.m0_src) || (vm == 0 && self.dot < 4))
            && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
        // Vblank source. On CGB the level extends through line 0 dots
        // 0-3 together with the persisting visible mode 1 (gambatte
        // getStat's mode-1 window + the lycEnable lyc0_m1disable
        // cgb04c_outE0 rows: edges under it stay blocked).
        high |= en & STAT_SRC_VBLANK != 0
            && (self.line >= 145
                || (self.line == 144 && self.dot >= 4)
                || (self.model.is_cgb() && self.line == 0 && !self.glitch_line && self.dot < 4));
        if en & STAT_SRC_OAM != 0 {
            // The OAM *blocking level* spans the whole scan+render of every
            // visible line, dots 0..the mode-0 source rise — one dot past
            // the visible flip, so the hblank source takes over without a
            // gap (gambatte mstat_irq.h doM0Event: the m2 enable blocks
            // the m0 IRQ — m2int_m0irq_*_out0; the level also blocks the
            // LYC dot-4 edge, lycm2int). The IRQ itself is an *event* at
            // the line-start dots — see `stat_events_tick` (SameBoy display.c
            // mode_for_interrupt pulse). Line 0's level starts at dot 4
            // with the LY/LYC validity.
            let oam_window = self.line <= 143
                && !self.glitch_line
                && if self.dot < 4 {
                    // Line-start dots 0-3: the previous line's `m0_src`
                    // is still set; the level is high here exactly as
                    // before (line 0's starts at dot 4).
                    self.line != 0
                } else {
                    !self.m0_src
                };
            let cgb = self.model.is_cgb();
            // OAM pulse at vblank entry: one M-cycle before the vblank IF
            // on *both* families (wilbertpol intr_2_timing rounds 5-7 pin
            // MGB and CGB alike; gbmicrotest line_144_oam_int_b/c/d pin
            // DMG — `vblank_stat_intr-GS` sees it together with the
            // vblank IF through the DMG halt-late commit, see
            // `stat_events_tick`).
            let pulse144 = self.line == 144 && self.dot == 0;
            // DMG: the OAM source also pulses on every later vblank line
            // (`intr_1_2_timing-GS`: mode1→mode2 IRQ distance is 464 dots —
            // one line + 8 dots).
            let vblank_pulse = !cgb && (145..=153).contains(&self.line) && self.dot == 12;
            high |= oam_window || pulse144 || vblank_pulse;
        }
        high
    }

    /// Recompute the readable comparison flag (`cmp`), the IRQ-side
    /// comparison (`cmp_irq`) and the legacy line level (`stat_line` —
    /// kept for the LCD-off edge path and the CGB FF45 trigger's level
    /// check; IF emission no longer hangs on it).
    pub(super) fn refresh_cmp(&mut self, from_tick: bool) {
        if self.enabled {
            self.cmp = self.compare_ly() == Some(self.lyc);
            if !self.model.is_cgb() {
                self.cmp_irq = self.cmp;
            } else if !from_tick
                || self.glitch_line
                || self.dot == 0
                || self.dot == 4
                || (self.line == 153 && (self.dot == 8 || self.dot == 12))
            {
                // The IRQ-side comparison is event-clocked on CGB: it
                // re-evaluates at the window-boundary dots and on
                // register writes, against the delayed `lyc_event` copy
                // — a copy that caught up *between* events changes the
                // line level only at the next event (no IRQ for an FF45
                // write that lands inside its line's protected window:
                // wilbertpol ly_lyc_write-C round 4).
                self.cmp_irq = self.compare_ly_irq() == Some(self.lyc_event);
            }
        }
        self.stat_line = self.stat_line_level(self.stat_en);
    }

    /// Register-write edge for the LCD-off state and LCDC transitions:
    /// with the LCD off only the frozen LYC source contributes
    /// (`stat_lyc_onoff`), and the enable transition can raise the line
    /// in its own cycle (round 4).
    pub(super) fn legacy_level_edge(&mut self) {
        let was = self.stat_line;
        self.refresh_cmp(false);
        if self.stat_line && !was {
            self.pending_if |= IF_STAT;
        }
    }

    /// Per-source STAT IRQ events, fired from the dot clock (gambatte
    /// mstat_irq.h `MStatIrqEvent` + lyc_irq.cpp `LycIrq`, ported
    /// function by function). There is no wired-OR STAT line on the IRQ
    /// side: each source is an *event* whose rise is allowed or
    /// suppressed by a predicate over the *other* sources' enables —
    /// sampled through delayed register copies — and the delayed LYC
    /// value. Truth table (live = `stat_en` at the event tick; ev/evl =
    /// the delayed [`Self::stat_ev`]/[`Self::stat_lyc_ev`] FF41 copies;
    /// lycm/lyce = the delayed [`Self::lyc_ev_m`]/[`Self::lyc_event`]
    /// FF45 copies):
    ///
    /// | event (line, dot) | exists iff | fires iff (provenance) |
    /// |---|---|---|
    /// | m2 pulse (N∈1-144, 0) | live m2en ∧ ¬live m0en | ¬(ev lycen ∧ lycm = N−1) — `doM2Event` blockedByLycIrq compares the *previous* line (its compare is still held at the pulse dot); the ¬m0en exists-gate is `mode2IrqSchedule` routing every per-line event to the line-0 slot while m0en is set |
    /// | m2 pulse (0, 4) | live m2en | ¬(ev m1en) ∧ ¬(ev lycen ∧ lycm = 0) — `doM2Event` blockedByM1Irq + blockedByLycIrq |
    /// | m2 pulse, DMG only (N∈145-153, 12) | live m2en | ¬(live m1en) ∧ ¬(live lycen ∧ cmp_irq) — no gambatte equivalent (mooneye `intr_1_2_timing-GS`); keeps the pre-port level blocking |
    /// | m0 rise (`m0_flip_events`) | (live ∨ ev) m0en | ¬(ev lycen ∧ lycm = N) — `doM0Event`: *not* blocked by m2en (lcdirq_precedence/m0irq_ly44_lcdstat28) |
    /// | m1 (144, 4) | live m1en | ¬(ev ∧ (m2en ∨ m0en)) — `doM1Event` |
    /// | LYC (N∈1-153, 4), lyce = N | (live ∨ evl) lycen | N ∈ 1-144: ¬(evl m2en); else ¬(evl m1en) — `LycIrq::doEvent` + `lycIrqBlockedByM2OrM1StatIrq` (keyed on the LYC *value*, so LYC=144 is m2-blocked and never m1-blocked) |
    /// | LYC=0 (153, 12), lyce = 0 | (live ∨ evl) lycen | ¬(evl m1en) |
    ///
    /// Emission masks: the (N,0) pulses are second-half commits
    /// (`stat_late` + `stat_halt_late`; the CGB 144 entry is exempt —
    /// misc/ppu/vblank_stat_intr-C), the (0,4) pulse is dispatch-late
    /// (`stat_late`; SameBoy "except on line 0", mealybug's "line 0
    /// timing is different by 4 cycles" handlers), the m0 rise carries
    /// the half-cycle halt law (`m0_rise`); LYC and m1 events commit
    /// plain.
    pub(super) fn stat_events_tick(&mut self) {
        self.refresh_cmp(true);
        let cgb = self.model.is_cgb();
        let live = self.stat_en;
        let ev = self.stat_ev;
        let evl = self.stat_lyc_ev;
        let mut fired = 0u8;

        // m2 line-start pulse (a CGB STAT write committing in this same
        // M-cycle still reaches the pulse — handled retroactively in the
        // FF41 write path, see `m2_pulse_fires`).
        if !self.glitch_line
            && self.dot == 0
            && (1..=144).contains(&self.line)
            && self.m2_pulse_fires(live)
        {
            fired |= IF_STAT;
            if !(cgb && self.line == 144) {
                self.stat_late = true;
                self.stat_halt_late = true;
            }
        }
        if !self.glitch_line && self.line == 0 && self.dot == 4 {
            // m2 line-0 pulse (the one slot that survives the m0en
            // schedule routing).
            if live & STAT_SRC_OAM != 0
                && ev & STAT_SRC_VBLANK == 0
                && !(ev & STAT_SRC_LYC != 0 && self.lyc_ev_m == 0)
            {
                fired |= IF_STAT;
                self.stat_late = true;
            }
        }
        if !cgb && (145..=153).contains(&self.line) && self.dot == 12 {
            // DMG vblank-line OAM pulses.
            if live & STAT_SRC_OAM != 0
                && live & STAT_SRC_VBLANK == 0
                && !(live & STAT_SRC_LYC != 0 && self.cmp_irq)
            {
                fired |= IF_STAT;
            }
        }
        if self.line == 144 && self.dot == 4 {
            // m1 event, one M-cycle after the 144:0 pulse, together with
            // the vblank IF.
            if live & STAT_SRC_VBLANK != 0 && ev & (STAT_SRC_OAM | STAT_SRC_HBLANK) == 0 {
                fired |= IF_STAT;
            }
        }
        if std::mem::take(&mut self.m0_rise_dot) {
            // m0 event on the visible flip's dot (incl. un-flip refires).
            // The m0 event's delayed view is one M-cycle *fresher* than
            // the m1/m2 events': the mstat_irq guards are uniform in
            // gambatte cc, but the m0 event dot carries a smaller
            // calibration skew on our grid, so a write in the preceding
            // M-cycle already lands (m0enable disable_1 out0 vs
            // disable_2 out2 pin the 2-dot cell) — take pending staged
            // values that are within their last 3 dots.
            let ev_m0 = self.stat_ev_fresh();
            let lyc_m0 = self.lyc_ev_m_fresh();
            if (live | ev_m0) & STAT_SRC_HBLANK != 0
                && !(ev_m0 & STAT_SRC_LYC != 0 && lyc_m0 == self.line)
            {
                fired |= IF_STAT;
                self.m0_rise = true;
            }
        }
        // LYC events: once per frame at the (delayed) LYC value's line.
        let lyc_val = if self.glitch_line {
            None
        } else if self.line >= 1 && self.dot == 4 && self.lyc_event == self.line {
            Some(self.line)
        } else if self.line == 153 && self.dot == 12 && self.lyc_event == 0 {
            Some(0)
        } else {
            None
        };
        if let Some(value) = lyc_val {
            let blocker = if (1..=144).contains(&value) {
                STAT_SRC_OAM
            } else {
                STAT_SRC_VBLANK
            };
            // The enable side ORs the live registers with the delayed
            // copy (gambatte's `(statReg_ | statRegSrc_) & lycirqen`):
            // both a just-enabled and a just-disabled source fire.
            if (live | evl) & STAT_SRC_LYC != 0 && evl & blocker == 0 {
                fired |= IF_STAT;
            }
        }
        self.pending_if |= fired;
    }

    /// gambatte `statChangeTriggersStatIrqDmg`: the DMG STAT-write glitch
    /// — the write momentarily enables every source (Pan Docs "STAT
    /// bug"), raising IF from the hblank/vblank levels and the held LYC
    /// match (never from the mode-2 condition), suppressed per source
    /// when the corresponding *old* enable already held the line high.
    /// Independent of the written value. gbmicrotest
    /// stat_write_glitch_l0/l1/l143/l154 pin the position grid.
    pub(super) fn stat_write_trigger_dmg(&self, old: u8) -> bool {
        let lyc_high = self.lyc_period();
        // Visible-line region (gambatte ly < 144: our dots 0-3 still
        // belong to the previous line on their grid, so line 144's first
        // M-cycle is still "line 143, hblank").
        if self.line <= 143 || (self.line == 144 && self.dot < 4) {
            // This line's mode-0 time passed = a real hblank (the
            // LCD-enable glitch prefix is not one).
            let hblank = (self.m0_src || self.dot < 4)
                && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
            if hblank {
                old & STAT_SRC_HBLANK == 0 && !(lyc_high && old & STAT_SRC_LYC != 0)
            } else {
                // Mode 2/3: only the LYC condition fires the glitch.
                lyc_high && old & STAT_SRC_LYC == 0
            }
        } else {
            old & STAT_SRC_VBLANK == 0 && !(lyc_high && old & STAT_SRC_LYC != 0)
        }
    }

    /// gambatte `statChangeTriggersStatIrqCgb` (+ the M2/M0LycOrM1
    /// helpers): CGB STAT writes raise IF only for newly-enabled
    /// sources —
    /// * lyc: enabling while the held compare matches fires anywhere
    ///   (an old lyc enable suppresses everything);
    /// * m0: enabling during mode 2/3 of a visible line fires at the
    ///   write; in the hblank it raises nothing;
    /// * m1: enabling during vblank fires, except in mode 1's last
    ///   M-cycle (line 0 dots 0-3, where only the lyc condition can
    ///   fire — the old `m1_tail_veto`);
    /// * m2: only in the last M-cycle before a visible line's pulse
    ///   (`statChangeTriggersM2IrqCgb`; the m2enable late_enable
    ///   ladders pin the window).
    pub(super) fn stat_write_trigger_cgb(&self, old: u8, data: u8) -> bool {
        if data & !old & STAT_SRC_ALL == 0 {
            return false;
        }
        // The CGB write's compare view: the trigger-side compare has
        // already switched to the new line at our dot 0 (gambatte's CGB
        // write cc sits later against getLycCmpLy's −2 switch:
        // miscmstatirq m1statwirq_trigger_ly94 round 2 fires its m1
        // enable at the line boundary because the LYC=148 period has
        // ended, while lycEnable lyc_ff41_enable_3's same-cell enable
        // still matches its own line and fires).
        let cmp_cgb = if self.glitch_line {
            0
        } else {
            match (self.line, self.dot) {
                (0, _) => 0,
                (153, 0..=7) => 153,
                (153, _) => 0,
                (line, _) => line,
            }
        };
        let lyc_high = self.lyc == cmp_cgb;
        if lyc_high && old & STAT_SRC_LYC != 0 {
            return false;
        }
        let lyc_fire = lyc_high && data & STAT_SRC_LYC != 0;
        // m2 sub-trigger window (kept from the pre-port calibration;
        // gambatte's ly==143 and ly==153 branches are empty at single
        // speed, so the (144,0) and (0,0) cells never fire it).
        let m2 = old & STAT_SRC_OAM == 0
            && data & (STAT_SRC_OAM | STAT_SRC_HBLANK) == STAT_SRC_OAM
            && (1..=143).contains(&self.line)
            && self.dot < 2 + 2 * u16::from(self.ds);
        // gambatte's ly-region split on our grid: dots 0-3 still belong
        // to the previous line, so (0, 0-3) is mode 1's tail and
        // (144, 0-3) line 143's hblank (an m0 enable written there still
        // fires: m1/ly143_late_m0enable_ds_1 cgb04c_out3).
        let vis = (self.line <= 143 && !(self.line == 0 && self.dot < 4))
            || (self.line == 144 && self.dot < 4);
        let main = if vis {
            // A scheduled mode-0 event still ahead within this line
            // (gambatte `eventTimes_(memevent_m0irq) <
            // lyCounter.time()`): the write trigger defers to it. The
            // m0irq event is (re)scheduled with the *new* enables before
            // the trigger check, so a fresh m0 enable during mode 2/3
            // stays silent (its event fires instead: m0enable
            // late_enable_1), while the same enable in the hblank — the
            // prediction then points at the next line, beyond the LY
            // increment — raises IF at the write (m1/m1irq_m0enable_1).
            let crossed = self.m0_src && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
            let m0_pending = !crossed && (old | data) & STAT_SRC_HBLANK != 0;
            // Line-boundary tail (`timeToNextLy <= 4 + 4*ds`).
            let tail = self.dot < 4;
            if m0_pending || tail {
                lyc_fire
            } else if old & STAT_SRC_HBLANK != 0 {
                false
            } else {
                data & STAT_SRC_HBLANK != 0 || lyc_fire
            }
        } else {
            // Vblank region. Mode 1's last M-cycle (line 0 dots 0-3)
            // doesn't fire a written m1 enable, and an old m1 enable
            // still suppresses a written lyc condition there (gambatte's
            // `old & m1irqen` arm; miscmstatirq
            // lycstatwirq_trigger_ly00_10_50_1 reads E0).
            let m1_tail = self.line == 0 && self.dot < 4;
            if old & STAT_SRC_VBLANK != 0 {
                false
            } else {
                (data & STAT_SRC_VBLANK != 0 && !m1_tail) || lyc_fire
            }
        };
        main || m2
    }

    /// Stage the delayed event-register FF41 copies after a write
    /// (gambatte statRegChange guards): CGB copies land 6 dots after the
    /// architectural commit — an event in the following M-cycle still
    /// sees the old enables — DMG copies update immediately.
    pub(super) fn stage_stat_copies(&mut self) {
        if self.model.is_cgb() {
            // The guard windows are in machine cycles (`cc + 2*cgb <
            // nextEventTime`), so the dot spans halve in double speed.
            let k = if self.ds { 2 } else { 6 };
            self.stat_ev_staged = Some((self.stat_en, k));
            self.stat_lyc_ev_staged = Some((self.stat_en, k));
        } else {
            self.flush_stat_copies();
        }
    }

    /// The m0 event's (and the CGB line-start pulses') *fresher* view of
    /// the delayed copies: those events carry a smaller calibration skew
    /// on our dot grid, so a staged write within its last few dots
    /// already counts for them (m0enable disable_1/2 and
    /// lyc1_m2irq_late_lycdisable_1 pin the cells).
    fn stat_ev_fresh(&self) -> u8 {
        match self.stat_ev_staged {
            Some((v, d)) if d <= 3 => v,
            _ => self.stat_ev,
        }
    }

    fn lyc_ev_m_fresh(&self) -> u8 {
        match self.lyc_ev_m_staged {
            Some((v, d)) if d <= 1 => v,
            _ => self.lyc_ev_m,
        }
    }

    /// Predicate of the line-start m2 pulse (lines 1-144 dot 0) for the
    /// given live enables: exists iff m2 enabled and m0 not (gambatte
    /// mode2IrqSchedule routes every per-line event to the line-0 slot
    /// while m0en is set), blocked by the previous line's still-held LYC
    /// compare through the delayed copies (doM2Event blockedByLycIrq).
    /// Also consulted retroactively by the CGB FF41 write path: a write
    /// committing at the pulse's own M-cycle reaches it on CGB
    /// (m2enable lyc1_late_m2enable_lycdisable_1 cgb04c_out2 vs the same
    /// row's dmg08_out0).
    pub(super) fn m2_pulse_fires(&self, en: u8) -> bool {
        let (evp, lycp) = if self.model.is_cgb() {
            (self.stat_ev_fresh(), self.lyc_ev_m_fresh())
        } else {
            (self.stat_ev, self.lyc_ev_m)
        };
        en & STAT_SRC_OAM != 0
            && en & STAT_SRC_HBLANK == 0
            && !(evp & STAT_SRC_LYC != 0 && lycp == self.line - 1)
    }

    /// Synchronise every delayed event copy with the live registers
    /// (LCD transitions: gambatte lcdReset / LycIrq::lcdReset).
    pub(super) fn flush_stat_copies(&mut self) {
        self.stat_ev = self.stat_en;
        self.stat_ev_staged = None;
        self.stat_lyc_ev = self.stat_en;
        self.stat_lyc_ev_staged = None;
        self.lyc_ev_m = self.lyc;
        self.lyc_ev_m_staged = None;
    }

    /// S2b interrupt-facing mode ([`Ppu::mode_for_interrupt`]) for the current
    /// dot — the decoupled view the S5 STAT engine will read. Exposed for the
    /// S2b divergence test; not yet consulted in production.
    #[cfg(test)]
    pub(crate) fn mode_for_interrupt(&self) -> u8 {
        self.mode_for_interrupt
    }

    /// S2b: recompute the interrupt-facing mode ([`Ppu::mode_for_interrupt`])
    /// for the current dot, applying the mode-2 lead / mode-0 lag anchor swing
    /// against the CPU-visible [`Self::vis_mode`]. Inert today; the substrate
    /// for the S5 STAT engine and the S2d kernel-pair flip.
    pub(super) fn update_mode_for_interrupt(&mut self) {
        // `mfi_m0_prev` lags `line_render_done` by one dot: read the previous
        // dot's value for this dot's mode-0 decision, then latch this dot's.
        let prev_done = self.mfi_m0_prev;
        self.mfi_m0_prev = self.enabled && self.line <= 143 && self.line_render_done;
        self.mode_for_interrupt = if !self.enabled {
            0
        } else if self.line >= 144 || self.glitch_line {
            // VBlank / glitch line: no anchor swing modelled here yet (the
            // visible mode is the IRQ mode); refined with the STAT swap (S5).
            self.vis_mode()
        } else if self.dot < 84 {
            // Mode-2 region. SameBoy raises the OAM STAT source as a 1-dot
            // *pulse* at line start, then sets `mode_for_interrupt = -1`
            // (NONE) for the rest of the OAM search (`display.c:1781` →
            // `:1799`) — so the source level falls and a later LYC rise can
            // re-fire (STAT blocking), rather than the source staying high
            // across all of mode 2. Lines 1-143 pulse one dot early at dot 3
            // (the "OAM int 1 T-cycle before STAT" lead; `display.c:1778`),
            // one dot before the visible byte flips to 2 at dot 4. Line 0 has
            // no early fire ("except on line 0"), but SameBoy's `GB_SLEEP 7,1`
            // step still sets `mode_for_interrupt = 2` unconditionally at the
            // visible mode→2 edge (`display.c:1792`), so line 0 pulses *at*
            // dot 4 — matching `ModeTimeline::mode2_irq_offset(0) == 0`. Before
            // the pulse (dots 0-2, and line 0 dot 3) the mode-0/1 carryover
            // holds; after it the source is NONE.
            // (Still deferred to the S5 wiring: the VBlank-entry mode-2 source
            // — `display.c:2138`, the vblank display loop.)
            if (self.line != 0 && self.dot == 3) || (self.line == 0 && self.dot == 4) {
                // OAM (mode 2) IRQ pulse: leads the visible byte by one dot on
                // lines 1-143 (dot 3), but fires AT the visible mode→2 edge on
                // line 0 (dot 4) — no early lead there (`display.c:1778`
                // "except on line 0" / the unconditional `:1792` set).
                2
            } else if self.dot < 4 {
                self.vis_mode() // line-start mode-0/1 carryover
            } else {
                crate::stat_update::MODE_FOR_INTERRUPT_NONE // OAM-search body: no source
            }
        } else if !prev_done {
            // Mode 3 holds for the IRQ side one dot past the visible 3→0 flip
            // (`display.c:2091` visible vs `:2108` IRQ — the mode-0 lag).
            3
        } else {
            0
        };
    }
}
