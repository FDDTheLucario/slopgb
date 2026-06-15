//! LYC compare machinery: held-compare windows, LYC period, FF45 write trigger (DMG/CGB). Port of gambatte lyc_irq.cpp. Oracle: gbtr age ly/ly-ncm, wilbertpol -C LY, mooneye LYC.

use super::*;

impl Ppu {
    /// LY value the LYC comparator's *readable flag* sees, or None while
    /// the delayed-LY value is invalid (flag forced to 0). See module
    /// docs. The IRQ-side comparison uses [`Self::compare_ly_irq`].
    pub(super) fn compare_ly(&self) -> Option<u8> {
        if self.glitch_line {
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
        let upcoming = if self.line == 152 { 153 } else { self.line + 1 };
        let protected = !self.glitch_line
            && (self.dot < 4
                || (self.line == 153 && (8..12).contains(&self.dot))
                || (self.line <= 152 && self.dot >= 452 && value == upcoming));
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
        } else if self.line == 153 {
            match self.dot {
                // Line-152 tail: the upcoming line is 153.
                0..=3 => Some(153),
                // incLy(153) = 0, with the exception while ret > 2.
                4..=7 if old == 153 => None,
                _ => Some(0),
            }
        } else {
            match self.dot {
                // Tail of the previous line / last M-cycle of this one:
                // the upcoming line's number.
                0..=3 => Some(self.line),
                452..=455 if old == self.line => None,
                452..=455 => Some(if self.line == 152 { 153 } else { self.line + 1 }),
                _ => Some(self.line),
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
        let blocked = if self.line <= 143 && !self.glitch_line {
            // Blocked only once this line's mode-0 IRQ has passed (the
            // write sits in the hblank): gambatte checks the next m0irq
            // event lying beyond the line end. Writes earlier in the
            // line fire (lycwirq_trigger_m0_early_ly44 rows).
            self.stat_en & STAT_SRC_HBLANK != 0 && self.m0_src && value == self.line
        } else {
            self.stat_en & STAT_SRC_VBLANK != 0 && !(self.line == 153 && self.dot >= 452)
        };
        let lyc_level_high = self.stat_line && self.stat_line_level(STAT_SRC_LYC & self.stat_en);
        let fire = self.stat_en & STAT_SRC_LYC != 0
            && target == Some(value)
            && !blocked
            && !lyc_level_high;
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
                || (self.line == 153 && self.dot == 8 && value == 0));
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
        let their_line = if self.dot < 8 { prev } else { self.line };
        let blocked = if self.glitch_line {
            false
        } else if their_line <= 143 {
            self.stat_en & STAT_SRC_HBLANK != 0
                && (self.m0_src || self.dot < 8)
                && value == their_line
        } else {
            self.stat_en & STAT_SRC_VBLANK != 0 && !(self.line == 0 && self.dot == 4)
        };
        if self.stat_en & STAT_SRC_LYC != 0 && target == Some(value) && !blocked {
            self.pending_if |= IF_STAT;
        }
        self.refresh_cmp(false);
    }
}
