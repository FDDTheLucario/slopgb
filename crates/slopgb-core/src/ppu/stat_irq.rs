//! STAT IRQ event engine: per-source predicates (m0/m1/m2/LYC) with delayed FF41/FF45 copies, mode readout, FF41-write trigger tables, edge/IF takers. Port of gambatte mstat_irq.h. Oracle: gbtr m2int/m0irq/lycm2int, gbmicrotest hblank_int/oam_int, mooneye intr_2_*/stat_irq_blocking.

use super::*;

impl Ppu {
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
            // Leading-edge (cc+0) reads sample the PPU 4 dots before the
            // trailing cc+4 view, so the glitch line's mode-3 window must be
            // back-dated the same 4 dots (single speed) to stay
            // observationally neutral: the ENTRY boundary (mode 0→3) moves
            // 78→74 and the EXIT (`line_render_done`, the visible 3→0 flip)
            // is anticipated by `vis_early` rising 4 dots early (the glitch
            // line takes the +4 `early_lead` in `m0_flip_events`). Both are
            // exactly the read-offset back-date, restricted to `vis_mode`
            // — the OAM/VRAM/palette accessibility reads keep the raw
            // `GLITCH_MODE3_START` (they are byte-identical flag-on,
            // `lcdon_timing-GS` OAM/VRAM legs). Always 78 / never-`vis_early`
            // flag-OFF, so production is byte-identical.
            // The −4 glitch back-date is LEADING-EDGE-ONLY (like
            // `mode3_entry_dot`). The Tier-2 deferred read samples the glitch
            // 0→3 entry at the trailing frame, so it takes no back-date — dot 74
            // made the deferred `lcdon_timing-GS` STAT read see mode 3 a full
            // M-cycle early (round-1 STAT $87 vs $84). 78 restores it.
            let start = if self.leading_edge_reads && !self.tier2_reclock && !self.ds {
                GLITCH_MODE3_START - 4
            } else {
                GLITCH_MODE3_START
            };
            if self.dot < start || self.line_render_done || self.vis_early {
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
        } else if self.dot < self.mode3_entry_dot() {
            2
        } else if !(self.line_render_done || self.vis_early) {
            // `vis_early` back-dates the CPU-visible mode→0 boundary to
            // SameBoy's frame on the flag-on path (3 dots before the dispatch
            // flip, bare single-speed lines); always false in production, so
            // this reads `line_render_done` exactly. See the field docs.
            3
        } else if self.dot < self.vis_hold_until {
            // Window vis-HOLD: a triggering window extends the CPU-visible
            // mode-3 PAST the dispatch flip to SameBoy's `263 + SCX&7` exit
            // (`vis_hold_until`, set in `m0_flip_events`). 0 (no hold) in
            // production, so byte-identical OFF. See the `vis_hold_until` docs.
            3
        } else {
            0
        }
    }

    /// The dot the CPU-visible STAT mode flips 2→3 (the mode-2 OAM scan end).
    ///
    /// On the **leading-edge-only** (cc+0 read, eager machine)
    /// flag-on path the boundary is back-dated by the read offset (4 dots,
    /// single speed) to dot 80, so the cc+0 FF41 read reproduces the flag-off
    /// cc+4 mode-3 detection timing: the leading-edge read latches the PPU 4
    /// dots before the trailing view, and moving the boundary the same 4 dots
    /// makes that read **observationally neutral** for the mode-2→3 entry
    /// (mooneye `intr_2_mode3_timing` passes LE-only).
    ///
    /// The **Tier-2 deferred-commit** frame does NOT take
    /// that back-date (84, the flag-off value). The deferred read pays the
    /// previous M-cycle's parked debt — advancing the PPU to this cycle's
    /// leading edge — then samples; for the 2→3 ENTRY that lands the read at
    /// the trailing (cc+4) frame, not LE-only's 4-dots-early peek, so dot 80
    /// makes the deferred read see mode 3 a full M-cycle early (`test_iter 2`
    /// counts one poll, want two). 84 restores it (`intr_2_mode3_timing` passes
    /// flag-on, both models). The mode-0 *exit* differs (`early_lead`, gated on
    /// `tier2_reclock` in `m0_flip_events`): it keeps a back-date because the
    /// kernel separation needs the −1 net shift. Single speed only (the DS read
    /// offset is deferred with the rest of the DS back-dating); always 84
    /// flag-OFF, so production is byte-identical.
    pub(super) fn mode3_entry_dot(&self) -> u16 {
        if self.leading_edge_reads && !self.tier2_reclock && !self.ds {
            80
        } else if self.eager_value && self.ds {
            // EAGER double speed: the eager cc+0 FF41 value peek
            // (`leading_edge_sample`) samples the PPU pre-tick, a DS M-cycle (2
            // dots) before the trailing cc+4 view, so the mode-2→3 entry
            // back-dates to 80 (as single speed) to land that peek on mode 3
            // where SameBoy's cc+4 view reads mode 3 (`m2int_m2stat_ds_2`,
            // `enable_display/frame*_m3stat_count_ds_2`). Tier2's deferred DS
            // frame keeps 84 (the DS back-date is folded into `read_deferred`'s
            // advance); `eager_value` off → byte-identical.
            80
        } else {
            84
        }
    }

    /// EAGER off-screen-window (WX=166) mode-3 exit arming — the eager twin of
    /// the `render.win_active` guard on the [`Ppu::vis_exit_hd`] window-length
    /// arms (arm 1 CGB / arm D1 DMG), for the pre-activation read. The WX=166
    /// window activates during HBlank (SameBoy `wx_166_interrupt_glitch`), so
    /// slopgb's render only sets `win_active` at the HBlank match (~dot 256/264);
    /// the eager cc+0 FF41 read lands ONE M-cycle (4 dots) BEFORE that, with
    /// `win_active` still false, so the window arm misses and the bare arm-8
    /// (`2*flip + 2`) fires against the render's ALREADY-window-elevated
    /// projection (`projected_flip_dot` reflects the impending extension) — an
    /// exit 4 hd too high, so the read stays mode 3 (`m2int_wxA6_scx5_m3stat`
    /// [Cgb] want 0, `m2int_wxA6_firstline_m3stat` [Dmg] want 0). Firing the
    /// window-length arm for this armed-but-not-yet-active read uses the closed
    /// form (CGB `259+SCX&7`, DMG `253+SCX&7`) the deferred read already lands
    /// on 4 dots later. WX=166 only (the glitch value; on-screen windows caught
    /// by the render's own `win_active`), window enabled + WY-triggered by this
    /// line, not aborted. `eager_value`-gated → tier2 + production
    /// byte-identical (never fires there — the deferred read lands post-activation).
    pub(super) fn eager_offscreen_win_arming(&self) -> bool {
        self.eager_value
            && self.eff.wx == 0xA6
            && self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && self.line >= 1
            && self.line < 144
            && self.wy2 <= self.line
            && self.wy2 <= 143
            && !self.render.win_aborted
            && !self.render.win_active
    }

    /// STAT mode bits (FF41 bits 0-1) as currently visible to the CPU, for
    /// the interconnect (FEA0-FEFF prohibited-area reads key on OAM locking).
    pub(crate) fn mode_bits(&self) -> u8 {
        self.vis_mode()
    }

    /// Current (line, dot) — the rendering-FSM position, for the interconnect's
    /// post-halt-wake LY read-phase carry (`Interconnect::halt_ly_phase`).
    pub(crate) fn line_dot(&self) -> (u8, u16) {
        (self.line, self.dot)
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

    /// Whether the currently-pending STAT IRQ
    /// was raised by the mode-2 OAM line-start rise (vs mode-0/LYC). A sticky
    /// level read (not drained) by the interconnect's `dispatch_retime` to key
    /// the per-ISR deferred-read carry (see the [`Ppu::stat_rise_oam`] field +
    /// [`crate::ppu::m2carry_on`]).
    pub(crate) fn stat_rise_oam(&self) -> bool {
        self.stat_rise_oam
    }

    /// Whether the currently-pending STAT IRQ was the mode-0 HBlank rise
    /// (the +2-dot ISR read carry). See [`Ppu::stat_rise_m0`].
    pub(crate) fn stat_rise_m0(&self) -> bool {
        self.stat_rise_m0
    }

    /// Whether the mode-0 HBlank STAT rise lands within the next `dots` dots —
    /// a pure VALUE peek of the emergent mode-3 exit, advancing nothing.
    ///
    /// SameBoy's `halt()` samples `IE & IF` *after* the prefetch `cycle_read`
    /// walked the machine through the HALT fetch M-cycle (t0+4). The deferred
    /// clock reaches that view by flushing its parked debt; the eager clock has
    /// no debt to flush, so its entry sample sits at t0 and misses a rise that
    /// lands inside the fetch. Reconstructing the rise's VALUE at t0+4 — the
    /// same decomposition `read_pos_hd` and [`Ppu::boot_read`] use — restores
    /// the SameBoy view without fabricating machine time (advancing the clock
    /// here would tick the timers 4 T early and break the TIMA-counted rows).
    ///
    /// Mirrors the DS FF0F read-view peek in `stat_irq/ff0f.rs` (`rise <= dot + 1`).
    pub(crate) fn stat_m0_rise_within(&self, dots: u16) -> bool {
        self.eng_stat & STAT_SRC_HBLANK != 0
            && self.line <= 143
            && !self.line_render_done
            && self.render.active
            && self.projected_flip_dot() <= self.dot + dots
    }

    /// Whether `self.dot` sits in the M-cycle that contains the current line's
    /// mode-0 (HBlank) STAT flip — a pure dot-space peek robust after the render
    /// completes (unlike [`Self::stat_m0_rise_within`], which needs
    /// `!line_render_done`). The eager whole-M-cycle wake commits the mode-0 IF
    /// at the END of the M-cycle containing the flip, so two lines whose flips
    /// differ by <4 dots (an SCX&7 delta) wake at the same boundary; this
    /// recovers the flip's own dot (`flip_dot` once recorded, else the
    /// projection) and confines the wake peek to the flip's own M-cycle
    /// `[flip, flip+4)` (single-speed) so the eager wake lands at tier2's
    /// sub-M-cycle wake instant instead of the whole-M-cycle IF commit — and
    /// does NOT re-fire on the stale, already-passed flip after a halt rewind.
    pub(crate) fn m0_stat_flip_reached(&self) -> bool {
        if self.eng_stat & STAT_SRC_HBLANK == 0 || self.line > 143 {
            return false;
        }
        let flip = if self.line_render_done {
            if self.flip_dot == 0 {
                return false;
            }
            self.flip_dot
        } else if self.render.active {
            self.projected_flip_dot()
        } else {
            return false;
        };
        flip <= self.dot && self.dot < flip + 4
    }

    /// Whether the current line is the LCD-enable
    /// glitch line — its mode-0 engine rise is emitted at a different offset
    /// from the true (SameBoy) commit than normal lines' (rise == visexit vs
    /// visexit − 3, dual-trace measured), so the halt-wake visibility
    /// deadline carries a per-shape correction.
    pub(crate) fn glitch_line_now(&self) -> bool {
        self.glitch_line
    }

    /// Arm/disarm the SCOPED carried-read exit override (see the
    /// [`Ppu::read_carried`] field). `dispatch_retime` sets it after a STAT-ISR
    /// read carry; the interconnect clears it once the handler's FF41 read has
    /// resolved (one-shot).
    pub(crate) fn set_read_carried(&mut self, v: bool) {
        self.read_carried = v;
    }

    /// Arm the eager halt-woken re-fetch boundary override (see the
    /// [`Ppu::halt_refetch`] field). Set by the eager CGB halt wake; the
    /// interconnect clears it on the boundary-crossing FF41 read (one-shot).
    pub(crate) fn set_halt_refetch(&mut self, v: bool) {
        self.halt_refetch = v;
    }

    /// Whether an armed [`Ppu::halt_refetch`] read has now crossed the line
    /// boundary (`read_pos_hd >= LINE_DOTS*2`) — the interconnect's one-shot
    /// clear signal for the halt-woken re-fetch FF41 read.
    pub(crate) fn halt_refetch_crossed(&self) -> bool {
        self.halt_refetch && self.read_pos_hd() >= i32::from(LINE_DOTS) * 2
    }

    /// The eager halt-woken re-fetch boundary override for `vis_mode_read`
    /// (CGB single-speed): an IME=1 halt wake on the mode-0 STAT rise dispatches
    /// the STAT ISR, whose first FF41 read the sub-M-cycle wake peek
    /// (`halt_wake_mid_impl`) lands at the line's last dot — `read_pos_hd ==
    /// LINE_DOTS*2`, the +8hd cc+4 debt having crossed the line boundary while
    /// `self.dot` (452) has not — where SameBoy's cc+4 re-fetch view already
    /// sits in the next line's OAM scan (mode 2). Without the sub-M-cycle wake
    /// this fired on the want-0 `_a` siblings too (#11cz, −9 SameBoy-pass), but
    /// the wake peek wakes them one M-cycle earlier so their read lands one dot
    /// short of the boundary (`read_pos_hd` 904 < 912, stays mode 0) — the
    /// coupling that makes the arm collateral-free. `halt_refetch` is armed only
    /// on the eager clock → byte-identical OFF. `None` = no override.
    pub(in crate::ppu) fn halt_refetch_read_override(&self, m: u8) -> Option<u8> {
        if self.eager_value
            && self.halt_refetch
            && self.model.is_cgb()
            && !self.ds
            && self.line >= 1
            && self.line < 144
            && !self.glitch_line
            && m == 0
            && self.read_pos_hd() >= i32::from(LINE_DOTS) * 2
        {
            Some(2)
        } else {
            None
        }
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
        // Classify the write on the un-shifted calibrated frame (the
        // machine STOPADV advance moved post-leave writes deeper into the
        // frame; identity for never-switched ROMs).
        let (ll, ld) = self.law_pos();
        let cmp_cgb = if self.glitch_line {
            0
        } else {
            match (ll, ld) {
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
        // Unshifted CGB single-speed Tier-2: the engine's two-phase
        // FF41 view (`eng_stat_pending`) owns the LYC-source write fires (the
        // bit6-late continuity fire at commit+4 + external edges against the
        // armed old bit6), replacing the write-instant lyc arms below. The
        // shifted (lcd-offset) frames keep the calibrated arms — their write
        // law positions are one poll quantum ambiguous.
        let eng_lyc = self.leading_edge_reads && !self.ds && self.lcd_shift_dots == 0;
        // The engine owns the LINE-BOUNDARY region (where the staged view +
        // the lyfc schedule decide); a MID-LINE enable keeps gambatte's
        // write-instant fire — the `lyc_ff41_trigger_delay` pair collapses to
        // one deferred commit dot (both legs dot 77, measured), so only the
        // calibrated write-instant arm can satisfy it.
        let eng_boundary = eng_lyc && !(16..448).contains(&ld);
        let lyc_fire = !eng_boundary && lyc_high && data & STAT_SRC_LYC != 0;
        // The dispatch-class write-trigger, LYC sub-family. The lcd-offset
        // shifts `late_ff41_enable_lcdoffset1_1`'s
        // LYC-source enable into the line-start carryover (dots 0-3 of lines
        // 1-143, which on the gambatte grid still belong to the previous line):
        // it sets LYC = ly-1 and enables LYC at `ly7 dot3`, where SameBoy fires
        // via the carryover compare (LYC matched the previous line) while
        // `cmp_cgb` has already switched to the new line (so `lyc_high` is false).
        // Under Tier-2 a fresh LYC enable matching the PREVIOUS line fires in the
        // carryover; `cmp_cgb` (pinning the new-line compare for the calibrated
        // m1statwirq/lyc_ff41_enable_3 rows) is untouched. Byte-identical OFF.
        let lyc_carryover = self.leading_edge_reads
            && !eng_lyc
            && (1..=143).contains(&ll)
            && ld < 4
            && old & STAT_SRC_LYC == 0
            && data & STAT_SRC_LYC != 0
            && self.lyc == ll - 1
            // A DS line-start (dots 0-1) fresh bit6 enable
            // whose OLD value armed HBlank joins a line still latched HIGH
            // from the previous line's mode-0 (SameBoy holds the level until
            // the ~dot-2 mfi re-eval; SBLEVEL: the natural 1→0 lands at dot 2
            // and only THEN does a fresh enable edge —
            // `lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_ds_2` commit
            // dot 0 silent E0 / `_ds_3` commit dot 2 fires E2). The engine
            // level is seeded high by the write path (`regs.rs`) so the
            // next tick raises no spurious edge either.
            && !(self.ds && old & STAT_SRC_HBLANK != 0 && ld < 2);
        // The dispatch-class write-trigger, ly153 LYC-WRAP sub-family. The
        // lcd-offset shifts
        // `lyc153_late_ff41_enable_lcdoffset1_1`'s LYC enable into the ly153 LY=0
        // wrap window (dots 8-11), where `cmp_cgb`'s `(153, _) => 0` arm has
        // already wrapped to 0 (≠ lyc=153) so `lyc_high` is false — yet the held
        // `lyc_interrupt_line` latch is still TRUE there (SameBoy holds it across
        // the line-153 `ly_for_comparison == -1` gaps, `display.c:534`; the latch
        // matched 153 at the dot-6 step and only drops at the dot-12 LY=0 step).
        // SameBoy fires the fresh enable at `ly153 cfl0 lyc_line=1` (measured
        // `SBLEVEL 0->1 stat=c5`); slopgb's `cmp_cgb`-snapshot `lyc_fire` missed
        // it. Under Tier-2 a fresh LYC enable at line 153 with the held latch high
        // (the real-state discriminator, not the offset) fires; `cmp_cgb` (pinning
        // the dot-6 base `lyc153_late_ff41_enable_1` compare) is untouched.
        // Byte-identical OFF.
        // On a shifted ROM the held latch is engine state at the REAL
        // instant — the write that law-lands inside the dots-6..11 latch window
        // arrives after the real latch dropped, so re-derive the latch at the
        // law position (LYC=153 matched at the dot-6 step, drops at the dot-12
        // LY=0 step).
        let latch_at_law = if self.lcd_shift_dots == 0 {
            self.lyc_interrupt_line
        } else {
            self.lyc_interrupt_line || (self.lyc == 153 && (6..12).contains(&ld))
        };
        let lyc_wrap_153 = self.leading_edge_reads
            && !eng_lyc
            && ll == 153
            && latch_at_law
            && !lyc_high
            && old & STAT_SRC_LYC == 0
            && data & STAT_SRC_LYC != 0;
        // m2 sub-trigger window (kept from the pre-port calibration;
        // gambatte's ly==143 and ly==153 branches are empty at single
        // speed, so the (144,0) and (0,0) cells never fire it).
        let m2 = old & STAT_SRC_OAM == 0
            && data & (STAT_SRC_OAM | STAT_SRC_HBLANK) == STAT_SRC_OAM
            && (1..=143).contains(&ll)
            && ld < 2 + 2 * u16::from(self.ds);
        // gambatte's ly-region split on our grid: dots 0-3 still belong
        // to the previous line, so (0, 0-3) is mode 1's tail and
        // (144, 0-3) line 143's hblank (an m0 enable written there still
        // fires: m1/ly143_late_m0enable_ds_1 cgb04c_out3).
        let vis = (ll <= 143 && !(ll == 0 && ld < 4)) || (ll == 144 && ld < 4);
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
            let tail = ld < 4;
            // The dispatch-class write-trigger, HBlank sub-family. At the
            // dots-0-3 line-start tail the gambatte
            // logic defers a fresh m0 enable to the scheduled m0irq event; but
            // when the PREVIOUS line's mode-0 has already passed (`vis_mode==0`
            // hblank carryover, not the pre-mode-0 tail), that scheduled event
            // points at the NEXT line's mode-0 — beyond the LY increment — so
            // the deferral loses the IRQ before the cc+0 read samples it. The
            // lcd-offset shifts these late enables into exactly this carryover
            // tail (`late_enable_lcdoffset1_1` writes FF41 at `ly dot3`), where
            // SameBoy raises IF at the write. Under Tier-2 (`leading_edge_reads`)
            // a fresh m0 enable in the carryover tail raises IF immediately;
            // glitch lines excluded (the LCD-enable prefix is not a real
            // hblank). Never set in production / LE-only → byte-identical OFF.
            //
            // Double speed HALVES the carryover window: the deferred cc+0 write
            // lands 2 dots earlier in the dot grid (the CPU runs at 2×), so the
            // `_ds_lcdoffset1_1` enable that fires lands `dot0` while its `_2`
            // sibling lands `dot2` — and at `dot2` SameBoy's fire is *early*
            // (cleared by the test's IF-clear) so it must NOT be delivered. A
            // `dot < 4` window would over-fire the `_2` enable; halve to `< 2`
            // (mirrors the `m2`/`stage_stat_copies` DS halving). LE-only.
            let carryover_tail = ld < if self.ds { 2 } else { 4 };
            // At a shifted position the vis check evaluates on the law frame
            // (ld < 4 at line start is always the mode-0 hold on non-glitch
            // lines); un-shifted keeps the live view.
            let vis0 = if self.lcd_shift_dots == 0 {
                self.vis_mode() == 0
            } else {
                true
            };
            // The held-LYC pre-write-high suppression: a
            // carryover-tail m0 enable whose OLD value armed LYC with the
            // latch still held (the lines-1-143 / line-144 lyfc-gap hold)
            // rewrites a line that is already HIGH — no 0→1 edge on hardware
            // (`m1/lyc143_late_m0enable_lycdisable_2` want 1: old=0x40, the
            // LYC=143 hold spans line-144 dots 0-3; `ly143_late_m0enable_ds_1`
            // old=0x00 keeps firing). The top-of-fn `lyc_high` check misses
            // this: `cmp_cgb` has switched to the new line while the ENGINE
            // latch still names the old match.
            // UNSHIFTED CGB SS (`eng_lyc`) hands the carryover-tail
            // m0-enable fire to the two-phase ENGINE view, whose phase-1
            // lands at commit+2 where `mode_for_interrupt` already reads the
            // line-start OAM carry (mfi=2) — hardware's dead-tail: an enable
            // committing in the last M-cycle / next line's dots 0-1 catches
            // nothing on CGB (`m0enable/late_enable_2` want 0, commit ly+1
            // dot 1; `_1` commits mid-line 449 and fires via phase-1; the
            // asm ROW 3 `ttnl > 4` cutoff). The write-instant fire stays for
            // the SHIFTED frames it was built on (`late_enable_lcdoffset1_1`,
            // eng_lyc false) and DS.
            if self.leading_edge_reads
                && !eng_lyc
                && carryover_tail
                && !self.glitch_line
                && vis0
                && old & STAT_SRC_HBLANK == 0
                && data & STAT_SRC_HBLANK != 0
                && !(old & STAT_SRC_LYC != 0 && self.lyc_interrupt_line)
            {
                return true;
            }
            if m0_pending || tail {
                lyc_fire
            } else if old & STAT_SRC_HBLANK != 0 {
                false
            } else {
                // Under the two-phase engine view (unshifted CGB
                // SS Tier-2) the fresh-m0-enable fire moves to the ENGINE
                // (the phase-1 rise at its effective instant, where the
                // line-end OAM carryover in `update_mode_for_interrupt`
                // provides the hardware cutoff); the write-instant fire
                // would double up or fire enables the carryover blocks.
                (data & STAT_SRC_HBLANK != 0 && !eng_lyc) || lyc_fire
            }
        } else {
            // Vblank region. Mode 1's last M-cycle (line 0 dots 0-3)
            // doesn't fire a written m1 enable, and an old m1 enable
            // still suppresses a written lyc condition there (gambatte's
            // `old & m1irqen` arm; miscmstatirq
            // lycstatwirq_trigger_ly00_10_50_1 reads E0).
            // The dispatch-class write-trigger, VBlank sub-family. The gambatte
            // `m1_tail` (line 0 dots 0-3 = mode
            // 1's last M-cycle) suppresses a freshly-written m1 enable; but the
            // lcd-offset shifts `m1irq_late_enable_lcdoffset1_1`'s FF41 enable
            // into exactly that tail (`ly0 dot3`), where SameBoy fires the fresh
            // VBlank enable (out2) — slopgb delivered `if=00`. Under Tier-2
            // (`leading_edge_reads`) drop the `m1_tail` suppression so the fresh
            // VBlank enable raises IF; the `old & STAT_SRC_VBLANK` suppression and
            // the lyc arm (the `lycstatwirq_trigger_ly00` E0 rows) are
            // untouched. Never set in production / LE-only → byte-identical OFF.
            let m1_tail = ll == 0 && ld < 4 && !self.leading_edge_reads;
            if old & STAT_SRC_VBLANK != 0 {
                false
            } else {
                (data & STAT_SRC_VBLANK != 0 && !m1_tail) || lyc_fire
            }
        };
        main || m2 || lyc_carryover || lyc_wrap_153
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
}
