//! FF0F (IF register) read-view + squash family: the deferred cc+0 FF0F read's
//! verdict laws — the group-A STAT engine-rise PEEK, the DMG mode-0 two-latch
//! DELIVER anchor + SERVICE-CLEAR, the glitch-line co-instant read-view mask,
//! and the group-B/C/E write-race / dispatch-ack / OAM-pulse squash arms. A
//! third `impl Ppu` block split out of `reclock.rs` for the <1000-line cap
//! (`use super::*` == the `ppu` module, like `reclock.rs`/`read_laws.rs`);
//! every arm is tier2-gated so production (`tier2_reclock` off) is
//! byte-identical. Consumed by the deferred FF0F read/ack path in
//! `interconnect/cycle.rs`.

use super::*;

impl Ppu {
    /// FF0F group-A read PEEK (the FF41-peek shape on the IF
    /// register): the deferred cc+0 FF0F read's VERDICT includes a STAT engine
    /// rise whose emission dot is deterministically known at read time and
    /// lands within the read's SameBoy-frame sample window — verdict-only, no
    /// machine advance, no IF commit (the refuted sub-M FF0F sampling
    /// moved the machine and broke the want-early `_1` legs; this never moves
    /// anything). SameBoy's `read_high_memory` samples IF after
    /// `GB_display_sync` runs the PPU to the exact read half-dot with
    /// PPU-events-first ordering; slopgb's deferred machine advance stops a
    /// hair short of that frame, so a rise at the very next dot(s) that
    /// SameBoy has already folded reads as clear. Dual-traced (SBIF/SBREAD
    /// ff0f fp vs SLOPGB ff0f/dispatch):
    ///
    /// - mode-0 (HBlank) arm, window +1 dot: `m2int_m0irq_ds_2` reads dot 254
    ///   with the rise at 255 (SameBoy reads 2 dots AFTER its rise, if=03);
    ///   the `_ds_1`/`scx5_ds_1` guards read 3/2 dots shy (rise 255/260 vs
    ///   252+1/258+1 — stay clear). On the SS 4-dot read grid the arm is
    ///   provably inert (reads land ≡0 mod 4, the rise ≡2 mod 4, window 1).
    /// - LYC arm, window = half an M-cycle (2 dots SS / 1 dot DS):
    ///   `lycint152_lyc153irq_2` reads line-153 dot 4 with the LYC=153 latch
    ///   at dot 6 (SameBoy reads 4 dots after its rise, if=02); the `_1`
    ///   (dot 0 → lyfc −1 at 1..2) and `_ds_1` (dot 2 → lyfc −1 at 3) guards
    ///   stay clear, `_ds_2` (read dot 4 = the DS latch dot) is already
    ///   folded. Skips the lines-1-143 dots ≤ 2 carryover window (the engine's
    ///   `line_start_carryover` does not re-latch there) and never crosses the
    ///   line boundary.
    ///
    /// The mode-2 pulse is deliberately NOT peeked: the `m2int_m2irq_1/_2`
    /// legs read 1 M apart around the next line's pulse and pass on the
    /// current frame — a +window peek would flip the `_1` legs.
    /// Tier2-only caller (`read_deferred`) → production byte-identical OFF.
    pub(crate) fn ff0f_stat_peek(&self) -> u8 {
        if !self.enabled || self.glitch_line || self.stat_update.line() {
            return 0;
        }
        // (a) the mode-0 flip rise, one dot ahead. DOUBLE-SPEED only: the DS
        // read grid (≡0 mod 2) straddles the odd rise dot one dot shy where
        // SameBoy's frame has folded it (`m2int_m0irq_ds_2`); on the SS 4-dot
        // grid the scx3 rows (rise 257, `_1` read 256, want 0 — SameBoy reads
        // clear there) sit at the same +1 geometry with the OPPOSITE verdict,
        // so the SS window is 0 (measured: +1-dot SS peek flips
        // `m2int_m0irq_scx3_{,ei_,reti_}1`).
        // Anchored to the mode-2-rise-dispatched ISR read frame
        // (`stat_rise_oam`, the per-ISR source tag — sticky since
        // the dispatching edge): `lyc0int_m0irq_ds_1` reads the IDENTICAL
        // dot-254/rise-255 geometry from an LYC-anchored ISR with the OPPOSITE
        // want (SameBoy's per-ISR read position separates them; slopgb
        // collapses both to one dot — measured).
        // Unshifted frames only (the lcd_offset STOP dances re-phase the poll
        // grid: `offset1_lyc99int_m0irq_count_scx1_ds_1` polls rise−1 and
        // must stay clear).
        if self.ds
            && self.stat_rise_oam
            && self.lcd_shift_dots == 0
            && self.eng_stat & STAT_SRC_HBLANK != 0
            && self.line <= 143
            && !self.line_render_done
            && self.render.active
        {
            let (proj, lead) = self.flip_projection();
            let rise = self.dot + proj.saturating_sub(lead);
            if rise <= self.dot + 1 {
                return IF_STAT;
            }
        }
        // (a-dmg) the DMG single-speed mode-0 STAT-IF DELIVER window.
        // The tier2 deferred FF0F read samples the leading edge (cc+0), 4 dots
        // before production's cc+4 read of the SAME `ldh a,(FF0F)`, so a read
        // whose TRUE (cc+4) position `dot + 4` has crossed the counter-pinned
        // mode-0 rise R observes the STAT bit SameBoy's events-first frame has
        // already folded, while slopgb's whole-dot cc+0 read still sees it
        // clear (`hblank_int_scx*_if_c`: read R-2, want E2 — the delivered bit —
        // slopgb reads E0). Deliver iff `dot` in [R-4, R): the read's cc+4
        // position has reached R but the raw read dot has not. `intf` and the
        // R dispatch are UNTOUCHED (verdict-only; the bit was going to rise at
        // R regardless — this restores the production/SameBoy read value the
        // cc+0 frame lost, NOT a new edge). `!is_cgb`/SS-scoped via
        // [`Self::dmg_m0_if_rise`] (CGB uses the DS (a) arm above / its native
        // frame; production byte-identical — the arm is tier2-gated).
        if let Some(r) = self.dmg_m0_if_rise() {
            if self.dot + 4 >= r && self.dot < r {
                return IF_STAT;
            }
        }
        // (b) the LYC latch rise, half an M-cycle ahead.
        if self.eng_stat & STAT_SRC_LYC != 0 && !self.lyc_interrupt_line {
            let kmax = if self.ds { 1 } else { 2 };
            for k in 1..=kmax {
                let d = self.dot + k;
                if d >= 456 || ((1..=143).contains(&self.line) && d <= 2) {
                    continue;
                }
                let ly = self.ly_for_comparison_at(self.line, d);
                if ly != -1 && ly == i16::from(self.lyc) {
                    return IF_STAT;
                }
            }
        }
        0
    }

    /// The DMG single-speed mode-0 (HBlank) STAT-IF two-latch window
    /// anchor: the mode-0 rise dot R for the current bare DMG line, or `None`
    /// when out of scope. R is the render's own recorded flip (`flip_dot` once
    /// the visible mode-3→0 flip has fired) or its projection
    /// ([`Self::flip_projection`]) — the same anchor `vis_exit_hd` /
    /// `ff0f_stat_peek` arm (a) use. Scoped to the tier2 DMG single-speed
    /// path with the mode-0 STAT source armed on a visible non-glitch line;
    /// production and CGB take neither the DELIVER ([`Self::ff0f_stat_peek`]
    /// arm a-dmg) nor the SERVICE-CLEAR ([`Self::ff0f_dmg_service_clear`])
    /// override → byte-identical.
    fn dmg_m0_if_rise(&self) -> Option<u16> {
        if !self.tier2_reclock
            || self.model.is_cgb()
            || self.ds
            || !self.enabled
            || self.glitch_line
            || self.eng_stat & STAT_SRC_HBLANK == 0
            || !(1..=143).contains(&self.line)
        {
            return None;
        }
        if self.line_render_done && self.flip_dot != 0 {
            Some(self.flip_dot)
        } else if self.render.active {
            let (proj, lead) = self.flip_projection();
            Some(self.dot + proj.saturating_sub(lead))
        } else {
            None
        }
    }

    /// The DMG mode-0 STAT-IF SERVICE-CLEAR window: a tier2 deferred
    /// FF0F read whose raw dot has crossed the counter-pinned mode-0 rise R
    /// returns 0 (the whole byte), the read-frame proxy for SameBoy's dispatch
    /// PREEMPTING the instruction's own `ldh a,(FF0F)` — on hardware the
    /// interrupt is serviced before the load commits, so the handler observes
    /// the pre-read accumulator (0), never the set bit. slopgb's deferred read
    /// DOES commit (its machine advance let it sneak in before the R dispatch),
    /// loading the set bit; returning 0 restores the serviced-frame value
    /// (`hblank_int_scx*_if_d`: read R+2, ISR compares A==0, slopgb reads E2 →
    /// want 0). Window [R, R+4) — if_d reads land R+1..R+3 across scx0-7; the
    /// deliver arm owns [R-4, R). Verdict-only (the R dispatch/`intf`
    /// untouched); consumed by the FF0F read path in `interconnect/cycle.rs`.
    pub(crate) fn ff0f_dmg_service_clear(&self) -> bool {
        let Some(r) = self.dmg_m0_if_rise() else {
            return false;
        };
        self.dot >= r && self.dot < r + 4
    }

    /// The DMG glitch-line mode-0 co-instant read-view mask. On the
    /// first line after an LCD enable (`glitch_line`) an `enable_display` ROM
    /// polls FF0F (DI, `IE=0`) with the mode-0 STAT source armed; a deferred
    /// read landing EXACTLY on the recorded mode-0 flip dot reads the PRE-rise
    /// value on hardware/SameBoy — the CPU read precedes the STAT rise at the
    /// shared instant (read-before-rise event order, SameBoy reads at cfl257
    /// with the rise there and returns E0) — while slopgb's whole-dot frame
    /// folds the rise first and commits the set bit (E2). `ly0_m0irq_scx1_1`
    /// reads dot253 == flip_dot253 and wants E0; its `_2` sibling reads
    /// dot257 > flip (poll after the rise, E2) and `scx0_2` reads flip+1 (E2),
    /// so the mask is EXACT at `dot == flip_dot`, never a window. Returns
    /// `IF_STAT` to clear from the read verdict (the pure poll is ungated —
    /// this is the read-before-rise complement of the `intf & ie`-gated
    /// [`Self::ff0f_dmg_service_clear`], which fires only for a SERVICED read).
    /// Verdict-only (`intf`/dispatch untouched); tier2 + DMG single-speed +
    /// glitch-line scoped → production and CGB byte-identical.
    pub(crate) fn ff0f_dmg_m0_coincident_mask(&self) -> u8 {
        if self.tier2_reclock
            && !self.model.is_cgb()
            && !self.ds
            && self.enabled
            && self.glitch_line
            && self.line < 144
            && self.eng_stat & STAT_SRC_HBLANK != 0
            && self.line_render_done
            && self.flip_dot != 0
            && self.dot == self.flip_dot
        {
            IF_STAT
        } else {
            0
        }
    }

    /// Arm the FF0F write-race squash window (see the
    /// `stat_if_squash` field doc + the consumption site in
    /// [`Self::stat_update_tick`]). Called by the interconnect at the deferred
    /// FF0F write's commit instant, only when the written value clears bit 1.
    /// Tier-2 caller only → production byte-identical OFF.
    pub(crate) fn arm_ff0f_if_squash(&mut self) {
        self.stat_if_squash = 2;
    }

    /// The co-instant line-0 dot-4 OAM-pulse read-view mask
    /// (see the `ly0_pulse_age` field doc): a deferred FF0F read landing on
    /// the pulse's own dot returns the bit CLEAR (CPU-read-first at the
    /// shared instant, SameBoy-measured). Verdict-only.
    pub(crate) fn ff0f_ly0_pulse_mask(&self) -> u8 {
        // LYC==153 names the anchor: only the LYC-153 ISR's read lands
        // BEFORE the pulse in SameBoy's frame (line-0 dot 3, rise −1;
        // `lyc153int_m2irq_1` want 0). The LYC-152 ISR's `_2` read — the
        // same slopgb dot-4 collapse — lands 4 dots AFTER the rise on
        // SameBoy (SBREAD fp = rise fp + 8) and must SEE it
        // (`lycint152_m2irq_2`/`_ds_2` want E2, measured A/B without this
        // guard).
        // Second arm: the shifted-frame co-instant mode-0 rise
        // (see `m0sh_age`).
        if (self.ly0_pulse_age > 0 && self.line == 0 && self.dot == 4 && self.lyc == 153)
            || (self.m0sh_age > 0 && self.dot == self.m0sh_dot && self.lcd_shift_dots != 0)
        {
            IF_STAT
        } else {
            0
        }
    }

    /// Arm the dispatch-ack squash window for the acked IF
    /// bit (see the `ack_squash_ppu` field doc + the consumption sites in
    /// [`Self::stat_update_tick`] and the vblank raise). Called by the
    /// interconnect's `ack` on the tier2 path only.
    pub(crate) fn arm_ack_squash(&mut self, bit: u8) {
        self.ack_squash_ppu_mask = 1 << bit;
        self.ack_squash_ppu = 2;
    }
}
