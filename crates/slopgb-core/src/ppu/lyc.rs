//! LYC compare machinery: held-compare windows, LYC period, FF45 write trigger (DMG/CGB). Port of gambatte lyc_irq.cpp. Oracle: gbtr age ly/ly-ncm, wilbertpol -C LY, mooneye LYC.

use super::*;

impl Ppu {
    /// LY value the LYC comparator's *readable flag* sees, or None while
    /// the delayed-LY value is invalid (flag forced to 0). See module
    /// docs. The IRQ-side comparison uses [`Self::compare_ly_irq`].
    pub(super) fn compare_ly(&self) -> Option<u8> {
        if self.glitch_line {
            // On the leading-edge (cc+0) read frame, a glitch-line FF41
            // read in the last 4 dots is the cc+0 view of what its cc+4
            // trailing edge sees 4 dots later — the *next* line (line 1)
            // at dots 0-3 (the −4 read offset, exactly A7/A13's mode
            // back-date). Back-date the readable compare to that line-1
            // dots-0-3 value: DMG forces the coincidence flag invalid
            // there (None), CGB holds 0 (its readable flag = LY−1 = 0
            // across dots 0-3, so no change). Single-speed only (the DS
            // read offset differs); gated `leading_edge_reads`, so flag-off
            // production stays Some(0) the whole glitch line (byte-identical).
            // The −4 glitch readable-compare back-date is
            // LEADING-EDGE-ONLY. The Tier-2 deferred read samples at the
            // trailing frame, so the glitch line holds Some(0) the whole line
            // (no line-1-dots-0-3 forced-invalid view) — the −4 made
            // `lcdon_timing-GS`'s round-3 STAT read drop the LYC=0 coincidence
            // bit ($80 vs $84). Single speed only.
            if self.leading_edge_reads
                && !self.tier2_reclock
                && !self.ds
                && self.dot >= GLITCH_LINE_DOTS - 4
            {
                return if self.model.is_cgb() { Some(0) } else { None };
            }
            // LCD enable: the comparison runs immediately with LY=0
            // (`stat_lyc_onoff` rounds 1-4).
            return Some(0);
        }
        if self.model.is_cgb() {
            // CGB-C: no forced-invalid gaps in the *readable* flag — it
            // holds the previous line's value through dots 0-3 and
            // switches at dot 4; line 153 holds 153 through dot 11
            // (twice the DMG window) before switching to 0 at dot 12
            // (wilbertpol ly_lyc-C/ly_lyc_144-C/ly_lyc_153-C rounds 7-8
            // pin the holds; ly_lyc_0-C's expectations equal the -GS
            // build's, so the 0-compare start stays at 153:12).
            return Some(match (self.line, self.dot) {
                (0, _) => 0,
                (153, 0..=3) => 152,
                (153, 4..=11) => 153,
                (153, _) => 0,
                (line, 0..=3) => line - 1,
                (line, _) => line,
            });
        }
        self.compare_ly_irq()
    }

    /// The LYC-coincidence bit (FF41 bit 2) as an FF41 read sees it. Under
    /// the eager clock the read samples cc+0, but the CPU-visible value is the
    /// cc+4 one — the SAME +4 (SS) / +2 (DS) read-debt the mode bits take via
    /// [`Self::read_pos_hd`]. The CGB readable compare switches from `L-1` to
    /// `L` at line-start dot 4 ([`Self::compare_ly`]), so a cc+0 line-start
    /// read must back-date the coincidence to the debt-shifted dot to match
    /// SameBoy's real cc+4 read (`lycint_lycflag`, `lycEnable` STAT bytes).
    /// Byte-identical OFF (`eager_value` false → the live latched `self.cmp`);
    /// tier2's deferred read already advances the PPU to cc+4 so it keeps
    /// `self.cmp`. CGB-only (the DMG readable flag drops at line starts, a
    /// different frame); glitch lines keep `self.cmp` (their own leading-edge
    /// back-date lives in [`Self::compare_ly`]).
    pub(super) fn read_cmp(&self) -> bool {
        if self.eager_value && self.enabled && !self.glitch_line {
            if self.model.is_cgb() {
                let debt = if self.ds { 2 } else { 4 };
                return self.compare_ly_shift(debt) == Some(self.lyc);
            }
            // DMG: the readable flag drops at line starts (`compare_ly_irq`
            // forced-invalid gaps), a different frame than CGB's held compare,
            // so shift that table by the +4-dot debt (DMG is single-speed). The
            // line wrap folds a 153→0 boundary read to the cc+4 line-0 Some(0)
            // (`ly0/lycint152_lyc0flag`/`lyc153flag`, `lycint_lycflag`).
            // Byte-identical OFF (`eager_value` false → the latched `self.cmp`).
            return self.compare_ly_irq_shift(4) == Some(self.lyc);
        }
        self.cmp
    }

    /// The DMG readable-flag LY ([`Self::compare_ly_irq`] table) evaluated at
    /// the eager read's cc+4 position — the DMG twin of
    /// [`Self::compare_ly_shift`], for [`Self::read_cmp`].
    fn compare_ly_irq_shift(&self, debt: u16) -> Option<u8> {
        let mut d = self.dot + debt;
        let mut l = u16::from(self.line);
        while d >= LINE_DOTS {
            d -= LINE_DOTS;
            l += 1;
        }
        if l >= 154 {
            l -= 154;
        }
        match l {
            0 => Some(0),
            153 => match d {
                0..=3 => None,
                4..=7 => Some(153),
                8..=11 => None,
                _ => Some(0),
            },
            _ if d < 4 => None,
            _ => Some(l as u8),
        }
    }

    /// The CGB readable-compare LY ([`Self::compare_ly`] CGB arm) evaluated at
    /// the eager read's cc+4 position — the current dot advanced by `debt` on
    /// the 154×456 grid — for [`Self::read_cmp`].
    fn compare_ly_shift(&self, debt: u16) -> Option<u8> {
        let mut d = self.dot + debt;
        let mut l = u16::from(self.line);
        while d >= LINE_DOTS {
            d -= LINE_DOTS;
            l += 1;
        }
        if l >= 154 {
            l -= 154;
        }
        Some(match (l, d) {
            (0, _) => 0,
            (153, 0..=3) => 152,
            (153, 4..=11) => 153,
            (153, _) => 0,
            (line, 0..=3) => (line - 1) as u8,
            (line, _) => line as u8,
        })
    }

    /// LY value the IRQ-side comparator sees (and the DMG readable
    /// flag). Unlike the CGB readable flag it drops at line starts —
    /// gambatte's lyc and m1 IRQs are separate events, and the m1 event
    /// at 144:4 fires even when LYC matched line 143 (lycint143_m1irq
    /// expects both IRQs; a held level would swallow the m1 edge).
    pub(super) fn compare_ly_irq(&self) -> Option<u8> {
        if self.glitch_line {
            return Some(0);
        }
        match self.line {
            0 => Some(0),
            153 => match self.dot {
                0..=3 => None,
                4..=7 => Some(153),
                8..=11 => None,
                _ => Some(0),
            },
            _ => {
                if self.dot < 4 {
                    None
                } else {
                    Some(self.line)
                }
            }
        }
    }

    /// The LY value gambatte's `getLycCmpLy` compares STAT-write and
    /// FF45-write triggers against: the *held* compare — the previous
    /// line's value persists through the line-start dots (their compare
    /// switches 2 cc before the LY increment, which sits near our dot 4
    /// — see the FF45 trigger tables), and line 153 holds 153 through
    /// dot 11. Identical on both models (it is the CGB readable-flag
    /// table; the DMG readable flag differs only by its forced-invalid
    /// gaps, which the *trigger* comparison does not have).
    fn lyc_cmp_held(&self) -> u8 {
        if self.glitch_line {
            return 0;
        }
        match (self.line, self.dot) {
            (0, _) => 0,
            (153, 0..=3) => 152,
            (153, 4..=11) => 153,
            (153, _) => 0,
            (line, 0..=3) => line - 1,
            (line, _) => line,
        }
    }

    /// The trigger-side LYC level: the held compare matches the live
    /// FF45 value and the source is enabled (gambatte `lycperiod`).
    pub(super) fn lyc_period(&self) -> bool {
        self.lyc == self.lyc_cmp_held() && self.lyc < 154
    }

    /// CGB FF45 write path (LCD on): the IRQ decision follows gambatte's
    /// `lycRegChangeTriggersStatIrq` — writes committing near a line
    /// boundary compare against the *upcoming* line's value, with the
    /// simultaneous-increment exception — and a raised IF lands one
    /// M-cycle after the write at single speed (`lyc_if_delay`). The
    /// line-start event comparator keeps a delayed copy (`lyc_event`)
    /// that writes inside the event's 4-dot lead-in cannot reach.
    /// Pinned by wilbertpol ly_lyc_write-C / ly_lyc_0_write-C /
    /// ly_lyc_153_write-C and the gambatte lycEnable family.
    pub(super) fn write_lyc_cgb(&mut self, old: u8, value: u8) {
        // Event-comparator copy (gambatte LycIrq::regChange windows):
        // protected at the event's lead-in M-cycle, and — CGB only — for
        // a boundary write in the previous line's last M-cycle whose new
        // value targets the imminent upcoming-line event (`time_ - cc >
        // 6 + 4*ds` reaches one M-cycle further back than the DMG `> 4`;
        // lycEnable/lyc153_late_ff45_enable_2 cgb04c_outE0 pins the
        // cell — the matching write at (152,452) misses the (153,4)
        // event while its DMG sibling fires).
        // Classify the write on the un-shifted calibrated frame
        // (identity for never-switched ROMs; see `Ppu::law_pos`).
        let (ll, ld) = self.law_pos();
        let upcoming = if ll == 152 { 153 } else { ll + 1 };
        let protected = !self.glitch_line
            && (ld < 4
                || (ll == 153 && (8..12).contains(&ld))
                || (ll <= 152 && ld >= 452 && value == upcoming));
        if !protected {
            self.lyc_event = value;
        }
        // The m0/m2 events' delayed FF45 copy (mstat_irq.h lycRegChange
        // `cc + 5*cgb + 1 - ds < nextM0/M2IrqTime`): wider than the FF41
        // window by one M-cycle — staged 8 dots (m0enable/
        // lycdisable_ff45_2/_3 keep the old value at their line's m0
        // event through the fresh view's `d <= 1`, while
        // lyc1_m2irq_late_lyc255_1's write 8 dots before the pulse
        // lands).
        self.lyc_ev_m_staged = Some((value, if self.ds { 2 } else { 8 }));
        // Trigger target: the compare value gambatte's predicate uses,
        // translated to commit-dot coordinates (gambatte cc = commit
        // state minus 4; tail window = returned timeToNextLy <= 6).
        // `None` = the simultaneous-increment exception (old value
        // matched the held compare inside the tail: "lyc flag never
        // goes low -> no trigger").
        let target = if self.glitch_line {
            Some(0)
        } else if ll == 153 {
            match ld {
                // Line-152 tail: the upcoming line is 153.
                0..=3 => Some(153),
                // incLy(153) = 0, with the exception while ret > 2.
                4..=7 if old == 153 => None,
                _ => Some(0),
            }
        } else {
            match ld {
                // Tail of the previous line / last M-cycle of this one:
                // the upcoming line's number.
                0..=3 => Some(ll),
                452..=455 if old == ll => None,
                452..=455 => Some(if ll == 152 { 153 } else { ll + 1 }),
                _ => Some(ll),
            }
        };
        // The trigger is an event, not a line edge: it fires even while
        // another source holds the line high, blocked only by gambatte's
        // lycRegChangeStatTriggerBlockedByM0OrM1Irq — a pending mode-0
        // IRQ for a now-matching value on visible lines, the m1 enable
        // on vblank lines (except the very end of line 153) — and by an
        // already-matching lyc level (the old value's match means the
        // target comparison fails, handled by `target` above; an
        // unchanged-source rise needs `stat_line` low).
        let blocked = if ll <= 143 && !self.glitch_line {
            // Blocked only once this line's mode-0 IRQ has passed (the
            // write sits in the hblank): gambatte checks the next m0irq
            // event lying beyond the line end. Writes earlier in the
            // line fire (lycwirq_trigger_m0_early_ly44 rows).
            self.stat_en & STAT_SRC_HBLANK != 0 && self.m0_src && value == ll
        } else {
            self.stat_en & STAT_SRC_VBLANK != 0 && !(ll == 153 && ld >= 452)
        };
        let lyc_level_high = self.stat_line && self.stat_line_level(STAT_SRC_LYC & self.stat_en);
        // The ly153 LYC-WRITE wrap, the FF45 sibling
        // of the FF41 `lyc_wrap_153` write-trigger. The lcd-offset shifts a late
        // FF45 write (new LYC = 153) out of the line-153 dots-0-3 carryover (where
        // `target` = Some(153) and the gambatte path fires) into the dots-6-7
        // window, where the `target` table wraps to Some(0) (the LY=0 increment)
        // so `target == Some(value)` fails. But SameBoy's `ly_for_comparison` is
        // still 153 there (`ly_for_comparison_line_153`: 153 at dots 6-7) — the
        // write makes a fresh LYC=153 match and `GB_STAT_update` fires. Under Tier-2,
        // a late FF45 write whose value matches the held `ly_for_comparison`
        // (the real-state discriminator, not the offset) fires; the gambatte
        // `target` table (pinning the base `lyc153_late_ff45_enable_{1,2}` cells)
        // is untouched. Byte-identical OFF.
        let lyc_write_wrap_153 = self.leading_edge_reads
            && ll == 153
            && self.ly_for_comparison_at(ll, ld) == i16::from(value)
            && !blocked
            && !lyc_level_high;
        // The FF45 "weirdpoint" (`ff45_enable_weirdpoint_lcdoffset1_2`).
        // The lcd-offset shifts a late FF45 write into
        // the `ly_for_comparison == -1` line-start gap (dot 3 on lines 1-143). At
        // `lyfc == -1` SameBoy's `GB_STAT_update` leaves `lyc_interrupt_line`
        // unchanged (`display.c:534` only clears the visible bit, never re-latches
        // a match) — the write raises NO fresh LYC edge. slopgb's gambatte `target`
        // table treats line-start dots 0-3 as the upcoming-line match (Some(line))
        // and would fire (`got=2`, want 0). Under Tier-2 (the SameBoy/reclock
        // grid), suppress the FF45 fire in the `-1` gap on visible lines 1-143.
        // Line 153 is EXCLUDED: its `-1` gaps (dots 0-5, 8-11) carry the held
        // LYC=153 latch + the `lyc_write_wrap_153` wrap fire that SameBoy delivers
        // (`lyc153_late_ff45_enable_3` outE2) — suppressing there drops a real
        // edge. Byte-identical OFF.
        let tier2_minus1_gap = self.leading_edge_reads
            && (1..=143).contains(&ll)
            && self.ly_for_comparison_at(ll, ld) == -1;
        // The FF45-write fire is suppressed under the tier2
        // engine when a NON-LYC source holds the line: SameBoy's
        // `GB_STAT_update` raises IF only on the line's 0→1 edge, and a line
        // held by an enabled MODE source (the VBlank source carried across
        // the ly153→ly0 wrap: `lycwirq_trigger_ly00_stat50_1`, LYC:=0 commits
        // ly0 dot1 with STAT=$50, SameBoy line continuously high mode-1 → no
        // edge, want E0; slopgb fired E2) never dips for a LYC
        // rewrite. A line held only by the LYC source does NOT suppress:
        // SameBoy's dot-4 re-latch against the OLD LYC dips it before the
        // (+4-later) write lands, so its write re-rise IS an edge — slopgb's
        // early write frame never sees the dip, and the event-like fire is
        // its stand-in (`ff45_enable_weirdpoint_3` / `lyc153_late_ff45_
        // enable_3`, both SameBoy-pass, dropped by the blanket engine-line
        // guard). LE/Tier-2 only → byte-identical OFF.
        let mode_src_en = match self.mode_for_interrupt {
            0 => self.stat_en & STAT_SRC_HBLANK,
            1 => self.stat_en & STAT_SRC_VBLANK,
            2 => self.stat_en & STAT_SRC_OAM,
            _ => 0,
        };
        let engine_line_high =
            self.leading_edge_reads && self.stat_update.line() && mode_src_en != 0;
        let fire = !tier2_minus1_gap
            && !engine_line_high
            && self.stat_en & STAT_SRC_LYC != 0
            && ((target == Some(value) && !blocked && !lyc_level_high) || lyc_write_wrap_153);
        // A SHIFTED write whose law position is the line-start tail
        // (ld < 4) commits after the engine's dot-4 re-latch already ran with
        // the OLD value, dropping a held match SameBoy never drops (its write
        // lands before that step) — the next engine tick would then re-latch
        // the NEW match as a spurious rising edge. Re-latch silently at the
        // commit so the OLD-match → NEW-match transition has no intermediate
        // drop. Identity for unshifted ROMs (lcd_shift_dots == 0).
        if self.leading_edge_reads && self.lcd_shift_dots > 0 && ld < 4 && self.dot >= 4 {
            let lyfc = self.ly_for_comparison();
            if lyfc != -1 {
                self.lyc_interrupt_line = lyfc == i16::from(value);
                // The engine's same-dot step already pushed the dropped level
                // into the edge detector; restore the corrected level QUIETLY
                // so the next tick sees no rise (SameBoy's line never fell).
                let lvl = crate::stat_update::StatUpdate::level(
                    self.mode_for_interrupt,
                    self.stat_en,
                    self.lyc_interrupt_line,
                );
                self.stat_update.force_level(lvl);
            }
        }
        // Converge the readable flag and the line level (no edge — the
        // trigger decision above is the only write-path IF source).
        self.refresh_cmp(false);
        if fire {
            if self.ds {
                self.pending_if |= IF_STAT;
            } else {
                self.lyc_if_delay = 4;
            }
        }
    }

    /// DMG FF45 write path (LCD on): gambatte `lycRegChangeTriggersStatIrq`
    /// plus `LycIrq::regChange`'s DMG copy rule. The dot tables translate
    /// gambatte's `getLycCmpLy` to our grid (the gambatte-side LY
    /// increment sits near our dot 6, so writes committing at dots 0-3
    /// still see the previous line, and a dot-4 commit sees the compare
    /// already switched to the new line). Calibrated against the
    /// lycEnable lyc153_late_ff45_enable / lycwirq_trigger_ly00_stat50 /
    /// lycwirq_trigger_m0_late ladders.
    pub(super) fn write_lyc_dmg(&mut self, old: u8, value: u8) {
        // Delayed event copy (`time_ - cc > 4 || timeSrc != time_`): only
        // a write committing at the line-start M-cycle of its own (new)
        // target event misses that event; everything else lands.
        let protected = !self.glitch_line
            && ((self.dot == 0 && self.line >= 1 && value == self.line)
                || (self.line == 153 && self.dot == 8 && value == 0)
                // Eager: a DISABLE of a held LYC=153 (`old == 153`, new value
                // away from 153) landing in the line-153 dots-4-7 coincidence
                // window holds the delayed copy at 153, so the held LY=153
                // coincidence still fires at the dots-6-7 window (wilbertpol
                // `ly_lyc_153_write-GS` C015) — the DMG twin of the CGB
                // `write_lyc_cgb`/`step_dot` protection. Scoped to the held-153
                // disable (not any window write, which spuriously fires
                // `lycEnable/lyc0_ff45_disable`). The window starts at dot 4
                // (where this ROM's disable write commits on the DMG grid).
                || (self.eager_value
                    && self.line == 153
                    && old == 153
                    && value != 153
                    && (4..=7).contains(&self.dot)));
        if !protected {
            self.lyc_event = value;
        }
        // The m0/m2 events' copy updates immediately on DMG
        // (mstat_irq.h lycRegChange `cc + 1 < nextEventTime`).
        self.lyc_ev_m = value;
        // Write trigger: compare target per getLycCmpLy. `None` = the
        // simultaneous-increment exception ("lyc flag never goes low ->
        // no trigger": the old value still matches the held compare in
        // the tail cell, so the flag never drops).
        let prev = if self.line == 0 { 153 } else { self.line - 1 };
        let target = if self.glitch_line {
            Some(0)
        } else {
            match self.dot {
                0..=3 if prev == 153 => Some(0),
                0..=3 if old == prev => None,
                0..=3 => Some(self.line),
                4..=7 if prev == 153 => Some(0),
                4..=7 => Some(self.line),
                8..=11 if self.line == 153 && old == 153 => None,
                _ if self.line == 153 => Some(0),
                _ => Some(self.line),
            }
        };
        // lycRegChangeStatTriggerBlockedByM0OrM1Irq on the same grid:
        // visible lines block a now-matching value once the line's m0
        // event has passed; vblank lines block under the m1 enable,
        // except the compare-wrap cell at (0,4) (`ly == 153 &&
        // timeToNextLy <= 2`; lycwirq_trigger_ly00_stat50_3 fires there
        // while _1/_2 stay blocked).
        //
        // The (0,4) un-block cell is the LE/tier2 (deferred) read
        // frame's compare-wrap position — there `_3` (the fire leg) lands at
        // dot 4. Under the EAGER clock the FF45 writes are recorded a full
        // M-cycle earlier: `_3` moves to dot 8 (`their_line == line == 0`, the
        // VISIBLE branch, which fires it naturally on the LYC=0 match), while
        // `_2` (want no-fire) moves to dot 4 and would be spuriously un-blocked
        // here. So under eager the vblank branch fully blocks — no (0,4)
        // exception (`lycwirq_trigger_ly00_stat50_2` want E0). `eager_value`-
        // gated → byte-identical flag-off.
        let their_line = if self.dot < 8 { prev } else { self.line };
        let blocked = if self.glitch_line {
            false
        } else if their_line <= 143 {
            self.stat_en & STAT_SRC_HBLANK != 0
                && (self.m0_src || self.dot < 8)
                && value == their_line
        } else {
            // The (0,4) compare-wrap un-block cell. It was disabled under the
            // earlier eager frame because there `_3` fired at dot 8 (VISIBLE
            // branch) and `_2` (want-block) had moved to dot 4. The dot-4
            // LYC=153 IF-emission decouple shifts the whole ISR-timed ly0 LYC
            // write ANOTHER M-cycle earlier: `_3` moves dot 8→4 (back onto this
            // compare-wrap cell, VBLANK branch, must FIRE) and `_2` moves dot
            // 4→0 (still fully blocked). So re-enable the exception for the eager
            // frame too — `lycwirq_trigger_ly00_stat50_3` fires at (0,4) while
            // `_1`/`_2` (dot 0) stay blocked. Deferred keeps the original cell.
            self.stat_en & STAT_SRC_VBLANK != 0 && !(self.line == 0 && self.dot == 4)
        };
        if self.stat_en & STAT_SRC_LYC != 0 && target == Some(value) && !blocked {
            self.pending_if |= IF_STAT;
        }
        // Seal the eager line-0 vblank-carry → LYC seamless handoff.
        // On line 0 the STAT line is held HIGH by the mode-1 (VBlank) carry
        // across dots 0-3, then the carry ends at dot 4 (`mode_for_interrupt`
        // flips to 2 with OAM disabled → the line DIPS). A LYC-match write that
        // the m1 block suppresses here (`_1` at dot 0, `_2` at dot 4) must NOT
        // then let the dot-engine re-raise a FRESH 0→1 edge one dot later: on
        // SameBoy the just-matched LYC=0 seamlessly continues the already-high
        // vblank line (`lycwirq_trigger_ly00_stat50_2` want E0 — the eager cc+0
        // write lands the match at the dot-4 dip). Seed the engine line HIGH so
        // the match joins (STAT blocking), no fresh IF. `_3` (dot 8) lands in
        // the VISIBLE branch (`their_line == 0`), is not blocked, and fires its
        // real edge. `eager_value`+DMG-scoped → byte-identical flag-off.
        if self.eager_value
            && !self.model.is_cgb()
            && blocked
            && their_line > 143
            && self.stat_en & STAT_SRC_LYC != 0
            && target == Some(value)
        {
            self.stat_update.force_level(true);
        }
        self.refresh_cmp(false);
    }
}
