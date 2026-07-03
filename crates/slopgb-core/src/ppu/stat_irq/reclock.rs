//! S5/tier2 StatUpdate-driver: the leading-edge / `tier2_reclock` flag-on STAT
//! IRQ engine (SameBoy `GB_STAT_update` port) — the `stat_update_tick`
//! rising-edge dispatch + vblank/OAM direct pokes, the halt-commit masks, the
//! decoupled `mode_for_interrupt` derivation, and the delayed
//! `ly_for_comparison` LYC input. Second `impl Ppu` block split out of
//! `stat_irq.rs` for the CLAUDE.md <1000-line cap; flag-OFF production never
//! runs this path — see the parent `stat_irq.rs` for the legacy gambatte event
//! engine.

use super::*;

impl Ppu {
    /// Port Stage S5 — SameBoy `GB_STAT_update` (`display.c:523`), the flag-on
    /// replacement for [`Self::stat_events_tick`]. There is a single STAT
    /// interrupt *line* — the OR of the one mode source selected by
    /// `mode_for_interrupt` and the LYC source — and `IF |= STAT` fires only on
    /// its 0→1 rising edge (the classic STAT-blocking model: a second source
    /// joining an already-high line raises nothing). Driven by
    /// [`StatUpdate`](crate::stat_update); the LYC input is the
    /// [`Self::lyc_interrupt_line`] latch, re-evaluated from
    /// [`Self::ly_for_comparison`] whenever that is a real line and held across
    /// the `-1` gaps (`display.c:534-544`).
    ///
    /// The OAM-DMA mode-2 guard (`display.c:526`) clears only the *visible*
    /// STAT mode bits, not the IRQ line (which reads `mode_for_interrupt`), so
    /// it does not affect this rising edge; the visible-mode side is handled by
    /// the interconnect's OAM-DMA freeze. The LCD-off guard is the tick's
    /// `!enabled` early-return, which holds the line low.
    ///
    /// Still uses [`Self::refresh_cmp`] for the *readable* FF41 mode/LYC bits so
    /// register reads stay identical to the flag-off path; only the IRQ event
    /// source changes. The per-source emission masks (`stat_late` /
    /// `stat_halt_late` / `m0_rise`) that the gambatte engine sets for the
    /// halt-wake interaction have no `GB_STAT_update` equivalent — they are part
    /// of the remaining atomic-flip work and are left unset here (so the flag-on
    /// path does not yet reproduce the halt-late commit timing).
    pub(super) fn stat_update_tick(&mut self) {
        // #11bg — resolve the staged CGB SS FF41 two-phase engine view (see
        // the `eng_stat_pending` field doc): phase-1 rises fire (a mode
        // enable at its effective instant), falls are silent; the final
        // value fires only a genuine enable (pre-write line LOW), through
        // the CGB delivery delay.
        if let Some((phase1, fin, pre_high, mfi_t0, k)) = self.eng_stat_pending {
            let mfi_now = self.mode_for_interrupt;
            // m0-flip fast-forward (stored k >= 1, i.e. the flip tick is >= D+2 = T0): slopgb's mode-3→0
            // flip sits LATER than T0+1T in SameBoy's frame, so a stage past
            // T0 at the flip has fully committed on hardware — resolve it.
            // When the final value cannot hold the line (the LYC hold dies)
            // the sub-dot dip is forced so the mode-0 rise re-edges
            // (`m0enable/lycdisable_ff41_scx*` want 2). A stage with k < 1
            // (write committed within a dot of the flip) keeps the OLD view
            // through the flip (`m0enable/disable_2` scx0 want 2: the dying
            // enable still catches the rise).
            if mfi_now == 0 && self.eng_mfi_prev == 3 && k >= 1 {
                self.eng_stat = fin;
                self.eng_stat_pending = None;
                if self.stat_update.line()
                    && !(fin & STAT_SRC_LYC != 0 && self.lyc_interrupt_line)
                {
                    self.stat_update.force_level(false);
                }
                // The main update() below evaluates the flip against `fin`.
            } else {
                let k = k + 1;
                if k == 2 {
                    self.eng_stat = phase1;
                    let lvl = crate::stat_update::StatUpdate::level(
                        mfi_now,
                        self.eng_stat,
                        self.lyc_interrupt_line,
                    );
                    // A rise at T0 is a mode-source enable reaching its
                    // effective instant (bit6 is OLD in phase-1, it cannot
                    // rise here) — fire immediately. A fall is silent.
                    if lvl && !self.stat_update.line() {
                        self.pending_if |= IF_STAT;
                        self.stat_update_halt_masks(mfi_now);
                        if super::s5dbg_on() && self.line < 154 {
                            eprintln!(
                                "SLOPGB dispatch ly={} dot={} mfi={} lycln={} (ff41 t0)",
                                self.line, self.dot, mfi_now, self.lyc_interrupt_line as u8
                            );
                        }
                    }
                    self.stat_update.force_level(lvl);
                    self.eng_stat_pending = Some((phase1, fin, pre_high, mfi_now, k));
                } else if k >= 4 {
                    self.eng_stat = fin;
                    self.eng_stat_pending = None;
                    // Evaluate the final value at the T0+1T-instant mode
                    // (`mfi_t0`) — the sub-dot dip: a fall forces the line
                    // low silently and any later natural rise re-fires
                    // per-dot (`lyc1_m2irq_late_lycdisable_1`). A rise (the
                    // bit6-late enable) fires iff the line was LOW at the
                    // write (continuity — the m1→LYC handoff
                    // `lyc153_late_enable_m1disable_3` stays silent), through
                    // the CGB delivery delay (`lyc_if_delay`, the FF41 twin
                    // of the FF45-write delay — `lyc_ff41_trigger_delay`).
                    let lvl = crate::stat_update::StatUpdate::level(
                        mfi_t0,
                        self.eng_stat,
                        self.lyc_interrupt_line,
                    );
                    if !lvl {
                        self.stat_update.force_level(false);
                    } else if !self.stat_update.line() {
                        if !pre_high {
                            self.lyc_if_delay = self.lyc_if_delay.max(3);
                            if super::s5dbg_on() && self.line < 154 {
                                eprintln!(
                                    "SLOPGB dispatch ly={} dot={} mfi={} lycln={} (ff41 fin delayed)",
                                    self.line, self.dot, mfi_t0, self.lyc_interrupt_line as u8
                                );
                            }
                        }
                        self.stat_update.force_level(true);
                    }
                } else {
                    self.eng_stat_pending = Some((phase1, fin, pre_high, mfi_t0, k));
                }
            }
        }
        // #11bg — the DS m0-flip dip (the immediate-view analogue of the
        // fast-forward above): a bit6-DROPPING FF41 commit within one M of
        // the mode-3→0 flip means hardware's LYC-hold death precedes the
        // mode-0 IF rise sub-dot; slopgb's whole-dot view collapsed them
        // into a seamless LYC→m0 handoff. Force the dip so the flip tick's
        // main update() re-edges (`m0enable/lycdisable_ff41_ds_1` want 2;
        // the `_2` sibling's drop commits after the flip and stays seamless).
        if self.ds && self.mode_for_interrupt == 0 && self.eng_mfi_prev == 3 {
            if let Some((l, d)) = self.ff41_ds_drop.take() {
                if l == self.line
                    && self.dot.wrapping_sub(d) <= 2
                    && self.lyc_interrupt_line
                    && self.eng_stat & STAT_SRC_LYC == 0
                    && self.stat_update.line()
                {
                    self.stat_update.force_level(false);
                }
            }
        }
        self.eng_mfi_prev = self.mode_for_interrupt;
        // Keep the readable comparison/mode flags + the legacy level current
        // (FF41 reads, the write-edge baseline) exactly as the flag-off path.
        self.refresh_cmp(true);
        // Drain the one-shot mode-0 event flag the gambatte engine would have
        // consumed this dot, so it does not leak into a later flag-off tick.
        let _ = std::mem::take(&mut self.m0_rise_dot);
        // `lyc_interrupt_line` latch: re-evaluate only when `ly_for_comparison`
        // names a real line; hold across the `-1` gaps (`display.c:534`).
        let ly = self.ly_for_comparison();
        // Mech 3 root 2 (S5) — the line-start LYC-carryover hold. SameBoy
        // re-evaluates `lyc_interrupt_line` only at the `GB_SLEEP` steps that set
        // `ly_for_comparison` (`display.c:1811` state-6 `= -1` holds; `:1830`
        // state-7 `= N` re-latch) — NOT during the held carryover before state-6,
        // where it still names the previous line (lines 1-143, dots 0-2 = `line
        // - 1`). A late FF45 write whose new LYC equals that carryover raises no
        // fresh edge (writes land at state-7, `lyfc=-1`/`0`); slopgb's per-dot
        // engine re-latched it → a spurious `ly1 dot0` (`got=E2`, want E0). Hold
        // like the `-1` gap (a legit LYC=N-1 tail is already latched true at line
        // N-1). DMG-family for the general lines-1-143 hold; LE/Tier-2 only.
        // Detail: `m1lyc-ifdelivery-groundtruth-2026-06-25.md` "#11l".
        //
        // S5 mech 3 (#11r) — the CGB ly0→ly1 LYC=0 wrap. The CGB lcd-offset shifts
        // `lycwirq_trigger_ly00_stat50_lcdoffset1_1`'s FF45=0 write to land at the
        // ly0→ly1 boundary (not ly0 cfl0 like SameBoy), so slopgb's line never
        // matches LYC=0 across ly0 (stays low) and then RE-RISES at the ly1 dot-0
        // carryover (`ly_for_comparison = line-1 = 0` matches the fresh LYC=0) — a
        // spurious `ly1 dot0` STAT edge (`got=E2`, want E0). SameBoy holds the line
        // HIGH across ly153→ly0 (LYC=0 matched at ly0 cfl0) and only FALLS at ly1
        // cfl0 (measured `SBLEVEL ly1 cfl0 1->0 lyc_line=0`), raising no edge. A
        // REAL LYC=0 always matches at ly0 first on SameBoy (ly_for_comparison=0
        // there), so no genuine fresh LYC=0 edge can exist at ly1 — holding the
        // line-1 carryover (the ly0→ly1 wrap only, NOT the lines-2-143 carryover
        // that #11l's ungated CGB hold broke at ly6/ly7) drops nothing SameBoy
        // delivers. CGB line 1 only; LE/Tier-2 only (`stat_update_tick` never runs
        // flag-off → byte-identical OFF).
        let line_start_carryover = if self.model.is_cgb() {
            // S5 mech 3 root 2 (#11al) — the CGB last-M-cycle LYC-write hold (the
            // line-END half of the #11l/#11r line-START carryover hold). slopgb's
            // leading-edge write frame commits a last-M-cycle (dot >= 452) FF45
            // write 1 M-cycle EARLIER than SameBoy, on the CURRENT line's tail,
            // where it maps to SameBoy's NEXT line's cfl0 (the held carryover, no
            // re-latch). A freshly-matching just-written LYC there re-latched
            // `lyc_interrupt_line` → a spurious last-dot STAT edge SameBoy never
            // fires. Measured (SBWRITE/SBLEVEL): `lyc0_late_ff45_enable_2`'s write
            // lands `ly1 cfl0` (no edge) where its `_1` sibling lands `ly0 cfl0`
            // (fires) — slopgb fired `_2` at ly0 dot453; `late_ff45_enable_2`'s
            // write lands `ly7 cfl0` (no edge) — slopgb fired it at ly6 dot453;
            // both spurious. Hold the latch across the last M-cycle so the
            // just-written LYC carries into the next line unchanged; a write
            // before dot 452 (e.g. `_1`'s dot 449) still re-latches and fires.
            // `dot >= 452` is the same last-M-cycle boundary `write_lyc_cgb`'s
            // lyc_event protection uses. SINGLE-SPEED only: at double speed the
            // last M-cycle is 2 dots (the leading-edge write offset is +1, not
            // +4), so `dot >= 452` over-covers the DS grid and inverts the
            // SameBoy-passing `_ds_1` siblings (`lyc153_late_ff45_enable_ds_1`,
            // `lyc1_m2irq_late_lyc255_ds_1`) — the DS last-M-cycle hold is the S6
            // DS-grid stage, parked. This is the line-END complement of the
            // line-START carryover hold (#11l's lines-2-143 START hold stays
            // REFUTED on CGB: the lcd-offset shifts a REAL edge onto the START
            // carryover dot — but NOT onto the line-END last M-cycle, where lyfc
            // is fixed and only a fresh write moves the latch). CGB only; LE/Tier-2
            // only (production byte-identical).
            // Part C stage 4 (ENGINE-IF re-measure) — the CGB line-START
            // carryover hold GENERALIZED to lines 1-143 (was line 1 only):
            // SameBoy re-latches `lyc_interrupt_line` ONLY at the state-6/-7
            // GB_SLEEP steps (dot 3 → -1/hold, dot 4 → line), never during
            // the dots-0-2 carryover where `ly_for_comparison` still names
            // line-1 — so a late FF45 write whose new LYC equals the PREVIOUS
            // line raises no fresh edge there (`late_ff45_enable_2/_3`:
            // slopgb's per-dot re-latch caught LYC=6 against the ly7 dots-0-2
            // carryover value 6 → spurious edge, if=03 where SameBoy reads
            // 01 — dual-traced). The #11l refutation of this hold predates
            // `law_pos`/#11bd; re-measured with the full family set this
            // session (lycEnable/m1/m2enable/miscmstatirq/ly0/lcd_offset).
            // UNSHIFTED frames only for lines 2-143: on STOP-shifted ROMs
            // the write's law position in this window is one poll quantum
            // ambiguous (the #11bd under-correction), and the shifted
            // `late_ff45_enable_lcdoffset1_1`/`ff45_enable_weirdpoint_
            // lcdoffset1_1` SameBoy-passes need their carryover re-latch
            // (measured drop without the gate — the #11l refutation shape,
            // now confined to the shifted frame). Line 1 keeps the
            // unconditional #11r wrap hold.
            ((1..=143).contains(&self.line) && self.dot <= 2 && self.lcd_shift_dots == 0)
                || (self.line == 1 && self.dot <= 2)
                || (self.dot >= 452 && !self.ds)
        } else {
            (1..=143).contains(&self.line) && self.dot <= 2
        };
        if ly != -1 && !line_start_carryover {
            // Part C stage 4 — the engine's LYC compare takes the DELAYED
            // FF45 view for the DISABLE direction: the deferred write commits
            // ~4 dots (SS) EARLY of SameBoy's instant, so a LYC rewrite
            // landing in dots 0-3 kills the dot-4 match slopgb-side while
            // SameBoy's edge fires first (`ff45_disable_2` want out3: LYC 6→FF
            // commits ly6 dot1, SameBoy fires the ly6 edge with LYC still 6,
            // then the disable lands). `lyc_event` (the production engine's
            // delayed FF45 copy, protected through dots 1-4) IS that view —
            // OR-ing it in delays disables while a fresh MATCH (enable
            // direction) stays live via `self.lyc` (the A12 write-trigger
            // discipline is untouched). DMG's `lyc_event` mirrors `lyc`
            // immediately → DMG unchanged.
            // SS + unshifted only: the DS write frame is +1 dot (the
            // `lyc_event` protected window over-covers it — the OR broke the
            // `*_ff45_disable_ds_1` legs, measured) and shifted frames
            // mis-map the window (`ff45_enable_weirdpoint_lcdoffset1_1`).
            self.lyc_interrupt_line = ly == i16::from(self.lyc)
                || (!self.ds
                    && self.lcd_shift_dots == 0
                    && ly == i16::from(self.lyc_event));
        }
        // Mech 3 root 1 (S5 engine-driver) — the vblank-entry LYC-latch drop.
        // A held visible-line LYC match (e.g. LYC=143 carried high from line 143)
        // stays latched across line 144's `ly_for_comparison == -1` line-start
        // gap, so the STAT line never dips at vblank entry — and when
        // `mode_for_interrupt` flips to the VBlank (mode-1) source at dot 4 the
        // fall of LYC fuses into the rise of mode-1, producing no fresh 0→1 edge
        // (the missing m1 re-arm: gambatte `m1/lycint143_m1irq_*` read if=01,
        // want if=03 — the serviced ly143 LYC-STAT bit is never restored).
        // SameBoy releases the latch at vblank entry (measured `SBLEVEL ly=144
        // cfl=0 lyc_line 1->0` then `0->1 mfi=1`, IF|=2): the line dips, then the
        // mode-1 source re-arms a fresh edge. Drop ONLY a held-true match that no
        // longer applies at line 144 (the pure carry-release); never force-set a
        // fresh match here — for LYC=144 the latch is set by the natural dot-4
        // `ly_for_comparison` re-evaluation, and front-running it to dot 0 would
        // suppress the LYC-source edge those rows need (`m1/m1irq_enable_after_
        // lyc144_*`). Gate on the VBlank (mode-1) source being ENABLED: SameBoy
        // drops the latch unconditionally, but the line only re-rises into a
        // fresh edge when mode 1 is armed to take over at dot 4 — the dip-and-
        // rise this whole-dot model reproduces. With mode 1 disabled SameBoy's
        // line dips and stays low (no IF), which a whole-dot drop would mis-frame
        // against the deferred read (`m1/lyc143_late_m0enable_lycdisable_*`,
        // VBlank off). LE/Tier-2 only — `stat_update_tick` never runs flag-off,
        // so production is byte-identical.
        if self.line == 144
            && self.dot == 0
            && self.lyc_interrupt_line
            && i16::from(self.lyc) != 144
            && self.eng_stat & STAT_SRC_VBLANK != 0
        {
            self.lyc_interrupt_line = false;
        }
        let mfi = self.mode_for_interrupt;
        // (A blanket delayed-enable view `stat_en | stat_ev` was BUILT +
        // MEASURED here: it fixes `ff41_disable_2` but over-delays the
        // m2enable/m1 disable families — +5/+1 fails — the mode-source
        // disables are pinned LIVE while only the LYC-source disable rides
        // the delayed copy. The LYC side lands via `lyc_event` above. #11bg
        // supersedes the write side: the engine reads `eng_stat`, the CGB
        // two-phase FF41 write view — per-bit exact where the blanket OR
        // over-delayed.)
        if self
            .stat_update
            .update(mfi, self.eng_stat, self.lyc_interrupt_line)
        {
            // #11bh group B — the FF0F write-race: a bit1-clearing FF0F write
            // committed within the last 2 dots CONSUMES this rise (SameBoy
            // `GB_CONFLICT_WRITE_CPU`: the CPU's IF write lands +1 T after its
            // leading edge and beats a co/prior-instant PPU rise in SameBoy's
            // frame — slopgb's deferred write commits ~2 dots early of that
            // frame, so the raced rise lands 1-2 slopgb-dots AFTER the
            // commit). The line still went high (`update` latched it), so the
            // edge is spent: no level-re-raise (`m2int_m0irq_scx{3,4}_ifw_ds_2`
            // want 0, `lycint152_lyc153irq_ifw_2` want E0; the `_1` siblings'
            // writes sit 3-5 dots clear and stay live). One-shot.
            // Per-source consume window (dots since the write commit,
            // Δ = 3 − counter): DS mode-0 rise w=2 (`scx3`/`scx4_ifw_ds_2`
            // consume at Δ 1-2, `_ds_1` survive at 3-4) · SS LYC rise w=1
            // (`lyc153irq_ifw_2` consumes at Δ=1, `_ifw_1` survives at 5) ·
            // everything else w=0 — the SS mode-0 (`scx4_ifw_1` survives
            // Δ=1), DS LYC (`lyc153irq_ifw_ds_1` survives Δ=2), mode-2
            // (`m2int_m2irq_ifw_ds_1`) and mode-1 rises sit on the other
            // side of the write in SameBoy's frame (all measured, this
            // session's family probe).
            let m0_rise = mfi == 0 && self.eng_stat & STAT_SRC_HBLANK != 0;
            let m2_rise = mfi == 2 && self.eng_stat & STAT_SRC_OAM != 0;
            let lyc_rise = !m0_rise
                && !m2_rise
                && self.eng_stat & STAT_SRC_LYC != 0
                && self.lyc_interrupt_line;
            let w = if self.ds && m0_rise {
                2
            } else if !self.ds && lyc_rise {
                1
            } else {
                0
            };
            // #11bh group C — the dispatch-ack squash (per-source windows, see
            // the `ack_squash_ppu` field doc): a rise of the just-acked STAT
            // bit inside the window merges into the dispatch; past it, it
            // survives and re-sets IF (the retrigger `_1` legs).
            let w_ack = if self.ack_squash_ppu_mask & IF_STAT != 0 {
                if m0_rise {
                    u8::from(self.ds)
                } else if m2_rise {
                    0
                } else if self.ds {
                    0
                } else {
                    2
                }
            } else {
                0
            };
            let ack_consumed = w_ack > 0 && self.ack_squash_ppu >= 3 - w_ack;
            if (w > 0 && self.stat_if_squash >= 3 - w) || ack_consumed {
                self.stat_if_squash = 0;
                if ack_consumed {
                    self.ack_squash_ppu = 0;
                }
                if super::s5dbg_on() && self.line < 154 {
                    eprintln!(
                        "SLOPGB dispatch ly={} dot={} mfi={} lycln={} (ifw squashed)",
                        self.line, self.dot, mfi, self.lyc_interrupt_line as u8
                    );
                }
            } else {
                self.pending_if |= IF_STAT;
                // #11bh group E — tag the line-0 dot-4 OAM pulse for the
                // co-instant FF0F read-view mask (`ly0_pulse_age`).
                if self.line == 0 && self.dot == 4 && mfi == 2 {
                    // 2: survives this tick's own end-of-tick decrement so
                    // the post-advance deferred read on the same dot sees it
                    // (the dot==4 gate keeps later dots out regardless).
                    self.ly0_pulse_age = 2;
                }
                if super::s5dbg_on() && self.line < 154 {
                    eprintln!(
                        "SLOPGB dispatch ly={} dot={} mfi={} lycln={}",
                        self.line, self.dot, mfi, self.lyc_interrupt_line as u8
                    );
                }
                self.stat_update_halt_masks(mfi);
            }
        }
        self.stat_if_squash = self.stat_if_squash.saturating_sub(1);
        self.stat_update_vblank_oam_pulses();
    }

    /// Port Stage A10 — the vblank-entry OAM (mode-2) STAT pulse the flag-on
    /// rising-edge [`Self::stat_update_tick`] engine does not emit.
    ///
    /// In vblank [`Self::update_mode_for_interrupt`] mirrors [`Self::vis_mode`]
    /// (mode 0 across 144:0-3, mode 1 from 144:4), so `mode_for_interrupt` never
    /// selects the OAM (mode-2) source there and the `GB_STAT_update` line never
    /// rises for it. SameBoy raises the 144-entry pulse as a **direct `IF |= 2`
    /// poke** (`display.c:2160`), independent of `stat_interrupt_line`, NOT a
    /// line rise. This reproduces it on the flag-on path with the *same* guard
    /// and commit masks the flag-off [`Self::stat_events_tick`] engine uses (the
    /// `vblank_stat_intr-GS` DMG / `-C` CGB lift; flag-on it recovers 5 mooneye
    /// combos and 8 gambatte rows with zero SameBoy-passing rows lost — see
    /// `ppu-subdot-ladder.md` "A10").
    ///
    /// The visible-line m2 pulses (lines 1-143 dot 0) are already covered by the
    /// rising-edge engine — its level-OR naturally reproduces `m2_pulse_fires`'
    /// `¬HBlank` / `¬held-LYC` blocking (a held source keeps the line high → no
    /// edge) — so only the 144:0 slot `mode_for_interrupt` skips is added here,
    /// and it cannot double-fire with the engine (at 144:0 `mfi==0`, and
    /// `m2_pulse_fires` requires HBlank disabled, so a held HBlank that would
    /// raise the engine line is exactly the case the pulse is suppressed). The
    /// DMG 145-153 dot-12 pulses are deferred (see below).
    fn stat_update_vblank_oam_pulses(&mut self) {
        // 144-entry OAM pulse (`display.c:2160`), one M-cycle before the vblank
        // IF, on both families. The DMG commit is halt- *and* dispatch-late so
        // `vblank_stat_intr-GS` observes it together with the vblank IF; the CGB
        // 144 entry is exempt and is visible in its own cycle
        // (`vblank_stat_intr-C`). Same `!glitch_line` + `m2_pulse_fires` guards
        // as the flag-off line-start pulse (the previous line's held LYC compare
        // blocks it; a glitched LCD-enable line runs no OAM scan, no pulse).
        if !self.glitch_line
            && self.line == 144
            && self.dot == 0
            && self.m2_pulse_fires(self.eng_stat)
        {
            self.pending_if |= IF_STAT;
            if !self.model.is_cgb() {
                self.stat_late = true;
                self.stat_halt_late = true;
            }
        }
        // The DMG per-line vblank OAM pulses at dot 12 (`display.c:2185`;
        // `stat_events_tick`'s 145-153 block; `intr_1_2_timing-GS`) are
        // DEFERRED with the rest of the atomic read-frame work. Adding them on
        // the flag-on path was MEASURED net-negative (`ppu-subdot-ladder.md`
        // "A10"): the extra dot-12 IF regresses 6 SameBoy-PASSING rows
        // (gambatte ly0/lycint152_m2irq, lycm2int/lyc0m2int_m2irq,
        // window/late_enable_afterVblank ×4 — all in the SameBoy gap list).
        // SameBoy fires these pulses too, so they are faithful, but flag-on's
        // cc+4 read/halt frame mis-places the resulting read until the global
        // reclock lands — exactly the atomic-convergence trap. The 144:0
        // entry pulse above does NOT have this problem (zero lift lost,
        // +8 gambatte / +5 mooneye), so it banks standalone.
    }

    /// Port Stage A6 — the halt/interrupt-sample commit masks for the flag-on
    /// [`Self::stat_update_tick`] rising edge, the leading-edge-frame analogue of
    /// the per-source `stat_late` / `stat_halt_late` / `m0_rise` masks the
    /// gambatte [`Self::stat_events_tick`] engine sets (see its truth table).
    /// `mfi` is the [`Ppu::mode_for_interrupt`] that drove this 0→1 rise, so it
    /// names the source.
    ///
    /// **Calibration (measured, `ppu-subdot-ladder.md` "A6"):** the gambatte
    /// engine reads FF41/IF at the M-cycle trailing edge (cc+4) and masks the
    /// mode-2 line-start pulse from BOTH the running CPU's interrupt sample
    /// (`stat_late`) and the halt-exit sampler (`stat_halt_late`). On the
    /// leading-edge (cc+0) flag-on path the regular interrupt dispatch is already
    /// aligned to SameBoy's frame, so the mode-2 pulse needs only the **halt**
    /// mask (SameBoy `GB_cpu_run` samples the halt exit mid-cycle — `sm83_cpu.c`;
    /// gbmicrotest `int_oam_*`); applying `stat_late` too would re-delay the
    /// non-halt `ldh a,(FF41)` dispatch and collapse the separated kernel pair
    /// (`m2int_m3stat_1` reverts 3→0). With only `stat_halt_late` the canonical
    /// mooneye `intr_2_mode0_timing` passes flag-on (DMG+CGB) **and** the kernel
    /// pair stays separated (m2int=3 ∧ m0int=0) — the first config in the port to
    /// hold both. The mode-0 `m0_rise` mask carries the half-cycle halt law as
    /// before; it is neutral on the flag-on suite until the mode-0 IRQ dispatch
    /// is reclocked (its rise still lands at our cc+4 dot, the remaining atomic
    /// work — see the field docs).
    fn stat_update_halt_masks(&mut self, mfi: u8) {
        // #11aq (C2 read-position carry): record whether THIS STAT 0→1 edge — the
        // one setting the currently-pending STAT bit — is the mode-2 OAM
        // line-start rise. Sticky until the next STAT edge (a held STAT bit
        // raises no new edge, so the flag keeps naming the source of the pending
        // bit). The interconnect's `dispatch_retime` keys the per-ISR deferred
        // read carry on it (`SLOPGB_M2CARRY`). Line 0's OAM pulse takes no carry
        // (its read frame already matches — same exemption as the halt mask).
        self.stat_rise_oam = mfi == 2 && self.eng_stat & STAT_SRC_OAM != 0 && self.line != 0;
        // #11aq: the mode-0 HBlank ISR read is +2 dots early (half the mode-2 +4);
        // tagged so `dispatch_retime` carries it the matching +2.
        self.stat_rise_m0 = mfi == 0 && self.eng_stat & STAT_SRC_HBLANK != 0;
        // The rise's source is unambiguous from `mfi` alone: this runs only on a
        // 0→1 edge, so the line was LOW the previous dot — meaning neither source
        // held it high. If the mode source is enabled with `mfi` selecting it
        // (`mfi == 2 && OAM`, or `mfi == 0 && HBlank`), that source is high NOW
        // yet was low before, so the mode source IS what just rose — it cannot be
        // a "LYC-only" rise (a held-high mode source would have made the previous
        // dot high → not an edge). A pure-LYC rise lands where `mfi` is NONE/1/3
        // (no branch). `stat_lyc_onoff` exercises this flag-on.
        if mfi == 2 && self.eng_stat & STAT_SRC_OAM != 0 {
            // Mode-2 (OAM) line-start pulse. Lines 1-143 carry it across the
            // line-start window (the halt-exit sampler misses the rise for one
            // M-cycle); line 0's pulse (dot 4) takes no halt mask (SameBoy
            // "except on line 0"). No `stat_late` in the leading-edge frame.
            if self.line != 0 {
                self.stat_halt_late = true;
            }
        } else if mfi == 0 && self.eng_stat & STAT_SRC_HBLANK != 0 {
            // Mode-0 (HBlank) source rise carries the half-cycle halt law
            // (`if_late` via the interconnect's second-half check).
            self.m0_rise = true;
        }
    }

    /// S2b interrupt-facing mode ([`Ppu::mode_for_interrupt`]) for the current
    /// dot — the decoupled view the S5 STAT engine will read. Exposed for the
    /// S2b divergence test; not yet consulted in production.
    #[cfg(test)]
    pub(crate) fn mode_for_interrupt(&self) -> u8 {
        self.mode_for_interrupt
    }

    /// Test view of the S5 [`StatUpdate`](crate::stat_update) interrupt-line
    /// level (the flag-on engine's `stat_interrupt_line`).
    #[cfg(test)]
    pub(crate) fn stat_update_line(&self) -> bool {
        self.stat_update.line()
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
        // Port Stage A8 — on the flag-on path the mode-0 IRQ fires at
        // `line_render_done` (our dot 254 = the gambatte-calibrated `m0_rise_dot`
        // frame the mode-0 halt grids pin: gbmicrotest int_hblank_halt, mooneye
        // hblank_ly_scx_timing), NOT the +1-dot `mfi_m0_prev` lag (255). The lag
        // models SameBoy's mode-0 IRQ 1 dot after the visible flip
        // (`display.c:2108` vs `:2091`), but it over-applies in our frame —
        // `line_render_done` is ALREADY the gambatte IRQ dot here, so the lag put
        // the `StatUpdate` mode-0 STAT IF one dot late vs `stat_events_tick` and
        // broke `hblank_ly_scx_timing` flag-on (kernel `m0int` and the canonical
        // both hold at 254; only the 252 full-SameBoy-frame move regresses them —
        // see `ppu-subdot-ladder.md` "DISPATCH-RECLOCK"). Flag-OFF keeps the
        // lagged `prev_done`; `stat_events_tick` never reads `mode_for_interrupt`,
        // so production is byte-identical.
        let prev_done = if self.leading_edge_reads {
            self.enabled
                && self.line <= 143
                && (self.line_render_done
                    // #11bh — the WINDOW-line mode-0 engine rise leads the
                    // render flip by 2 dots (SS CGB): the win-line render
                    // clock sits uniformly +2 late in slopgb's frame (the
                    // FF41 `vis_mode_read` window laws already compensate;
                    // the ENGINE still read the raw flip). SameBoy raises
                    // the win-line mode-0 IF at the same instant as its
                    // visible flip (dual-traced `m2int_wxA5_m0irq_2`: SB
                    // rise = flip fp = slopgb-frame 259, slopgb rise 261 →
                    // the `_2` dot-260 FF0F read missed it; the `_1`
                    // dot-256 read stays clear). Projection-based so
                    // mid-line SCX/WX writes track.
                    || (self.tier2_reclock
                        && self.model.is_cgb()
                        && !self.ds
                        && self.render.win_active
                        && !self.glitch_line
                        && self.render.active
                        && {
                            let (proj, lead) = self.flip_projection();
                            proj.saturating_sub(lead) <= 2
                        }))
        } else {
            prev_done
        };
        self.mode_for_interrupt = if !self.enabled {
            0
        } else if self.glitch_line {
            // Port Stage A15 — the LCD-enable glitch line. `vis_mode` yields
            // mode 0 in TWO regions: the line-start PREFIX (`dot < GLITCH_MODE3_START`,
            // before the glitch mode-3 window) and the post-render tail
            // (`line_render_done`/`vis_early`). Only the tail is a real hblank;
            // the prefix is the LCD-enable glitch, which raises NO mode-0 STAT
            // IRQ — `stat_line_level` and `stat_write_trigger_dmg` both suppress
            // the HBlank source there with `!(glitch_line && dot < GLITCH_MODE3_START)`.
            // The rising-edge engine had no such guard: with HBlank enabled it
            // saw mode 0 in the prefix and fired a spurious m0 IRQ at the first
            // glitch dot (SameBoy + gambatte render outE0; the bare engine gave
            // E2 — `enable_display/ly0_m0irq`, `irq_precedence/late_m0irq_retrigger`).
            // Select NONE in the prefix so no mode source contributes (LYC still
            // can — `level` ORs them); keep `vis_mode` (the real post-render m0,
            // or mode 3) elsewhere. `mode_for_interrupt` is inert flag-OFF
            // (`stat_events_tick` never reads it), so production is byte-identical.
            //
            // SINGLE SPEED only (`!ds`): the recovered slice is the single-speed
            // `enable_display/ly0_m0irq_trigger` (+2 flag-on, SameBoy-confirmed
            // out0). The double-speed `ly0_m0irq_scxN_ds_{1,2}` reads BRACKET the
            // glitch m0 IRQ dot (`_1` wants outE0 / read before, `_2` wants outE2
            // / read after), which our whole-dot model misframes (fires at the
            // prefix AND the post-render dot, never the DS mid-line dot SameBoy
            // hits) — so suppressing the DS prefix is a read-frame A/B swap that
            // drops the SameBoy-passing `ly0_m0irq_scx0_ds_2` (outE2). That DS
            // slice is part of the atomic Phase-B reclock, deferred. Measured
            // (`ppu-subdot-ladder.md` "A15"): SS-gated = +2 / 0 regress / 0 lift
            // lost; universal = +6 / 0 regress / −1 SameBoy-passing drop.
            if self.tier2_reclock && self.ds && self.model.is_cgb() {
                // #11bd item 5b — the DS glitch-line arm the A15 note deferred:
                // suppress the enable-glitch PREFIX (the spurious dot-19 rise
                // the `_1` legs' pre-rise reads caught -> got E2 want E0) AND
                // hold mode 3 for the IRQ until line_render_done + 2 - SameBoy
                // raises the glitch-line mode-0 STAT at cfl259, TWO dots past
                // the bare-line 257 (fresh SBTRACE, frame-0 ly0). The prior
                // "suppress-prefix drops ly0_m0irq_scx0_ds_2" A/B resolved by
                // the +2: the `_2` read lands between the old post-render dot
                // and the true rise.
                if self.dot < GLITCH_MODE3_START {
                    crate::stat_update::MODE_FOR_INTERRUPT_NONE
                } else if self.line_render_done {
                    0
                } else {
                    3
                }
            } else if self.tier2_reclock && !self.ds && self.model.is_cgb() {
                // Port Stage C/S5 — the Tier-2 SS glitch-line mode-0 IRQ
                // dispatch reclock. The IRQ side keys on `line_render_done`
                // (the dispatch dot, our dot 254 = SameBoy `cfl=257`), NOT on
                // `vis_early` (dot 252) the way `vis_mode` does — exactly the
                // bare-line law (`m0_flip_events`: "The IRQ side keys on
                // `line_render_done`, not `vis_early`"). `vis_early` back-dates
                // the CPU-visible FF41 mode→0 for the `lcdon_timing-GS` reads,
                // but the mode-0 STAT IRQ must fire at the standard dispatch
                // dot: SameBoy raises the glitch-line mode-0 STAT at the same
                // cfl=257 as every bare line. Keying the engine on `vis_early`
                // dispatched it 2 dots early, so the deferred dot-252 IF poll in
                // `enable_display/frame0_m0irq_count` observed the bit a poll
                // early and the ROM mis-measured (read LY=0, want LY=144). The
                // FF41 *read* side (`vis_mode`/`vis_early`) is untouched — only
                // the STAT-IRQ source moves. The prefix (`dot < GLITCH_MODE3_START`)
                // still raises NO mode-0 (the LCD-enable glitch, A15); mode 3
                // holds for the IRQ until `line_render_done`, then mode 0.
                //
                // CGB ONLY — a genuine multi-mechanism atomic on DMG (the
                // measured negative, this session). On CGB the glitch
                // `line_render_done` already lands at dot 254 (= SameBoy
                // cfl=257), so keying the IRQ on it is exactly right and there
                // is no DMG-style mode-0 halt pin on the glitch line to
                // conflict. On DMG the glitch `line_render_done` lands at dot
                // 252, and the SAME glitch-line mode-0 rise drives both the
                // poll path (`enable_display/frame0_m0irq_count`, SameBoy
                // renders 90 → wants the rise PAST the dot-252 poll) AND the
                // halt-wake path (`gbmicrotest int_hblank_halt_scx*`, the
                // 62,62,62,63,63,63,63,64 grid, calibrated at the dot-252
                // frame): moving the DMG glitch dispatch +2 lands the poll but
                // breaks int_hblank +1 (and vice-versa). SameBoy resolves both
                // at its sub-T-cycle IF-raise phase (cfl=257 = a specific T
                // within the M-cycle, seen by the mid-cycle halt sampler but not
                // the leading-edge poll read); slopgb's whole-dot raise cannot
                // place a single dot that satisfies both — so the DMG
                // `frame0_m0irq_count` (a baselined floor; production renders
                // 00) stays unfixed and DMG is byte-identical here. Gating to
                // CGB keeps int_hblank green while landing the CGB poll rows.
                // Detail: `ppu-subdot-ladder.md` "#11ad".
                if self.dot < GLITCH_MODE3_START {
                    crate::stat_update::MODE_FOR_INTERRUPT_NONE
                } else if self.line_render_done {
                    0
                } else {
                    3
                }
            } else {
                // LE-only / DS: the original A15 vis_mode (vis_early) path.
                let vm = self.vis_mode();
                if vm == 0 && !self.ds && !(self.line_render_done || self.vis_early) {
                    crate::stat_update::MODE_FOR_INTERRUPT_NONE
                } else {
                    vm
                }
            }
        } else if self.line >= 144 {
            // VBlank. The visible mode IS the IRQ mode here: `vis_mode` already
            // yields line 144's HBlank carryover (mode 0, dots 0-3) flipping to
            // the VBlank source (mode 1) at the vblank-entry step
            // (`display.c:2178`, ~dot 4), and mode 1 for every later vblank line
            // (145-153) — there is no mode-2 carryover into vblank
            // (`display.c:2138` skips `LINES-1`) and no `-1` gap. The per-line
            // DMG OAM vblank pulses + the line-144 OAM IF pokes are direct
            // `IF |= 2` writes (`display.c:2160`, `:2185`), handled in the STAT
            // engine, not `mode_for_interrupt` transitions.
            self.vis_mode()
        } else if self.dot < 84 {
            // Mode-2 region. SameBoy holds the OAM STAT source high across the
            // line-start window, then sets `mode_for_interrupt = -1` (NONE) for
            // the rest of the OAM search (`display.c:1781` → `:1799`) — so the
            // source level falls and a later LYC rise can re-fire (STAT
            // blocking), rather than staying high across all of mode 2. On lines
            // 1-143 the source is carried high across dots 0-3 (set at the prior
            // line's end `display.c:2138`, re-set at the line top `:1781`) — the
            // "OAM int 1 T-cycle before STAT" lead (`display.c:1778`) as a
            // sustained window, leading the visible mode→2 edge at dot 4. Line 0
            // has no prior-line carryover and no early lead ("except on line
            // 0"), but SameBoy's `GB_SLEEP 7,1` step (`display.c:1789`) still
            // sets `mode_for_interrupt = 2` unconditionally (`:1781`) at the
            // step the visible byte flips to 2 (`:1792`), so line 0 pulses *at*
            // dot 4 — matching `ModeTimeline::mode2_irq_offset(0) == 0`. (Whole-
            // dot caveat for the S5 wiring: SameBoy drops the source back to -1
            // at the *same* cycle as the line-0 rise, so its NONE/re-fire window
            // opens a dot earlier than this pulse — revisit if a line-0 dot-4
            // LYC=0 re-fire ever needs it.)
            if self.line == 0 {
                // Line 0: no prior-line OAM carryover (line 153 runs no
                // `display.c:2138` set) and no early lead (`display.c:1778`
                // "except on line 0"). Its OWN OAM pulse fires AT the visible
                // mode→2 edge (dot 4, the unconditional `:1792`/`:1781` set),
                // then falls to NONE.
                //
                // Mech 3 root 2 (S5 engine-driver) — the line-0 VBlank carry.
                // Dots 0-3 carry the **VBlank (mode-1) source**, not `vis_mode`.
                // SameBoy never re-sets `mode_for_interrupt` between the line-144
                // entry (`display.c:2215`, `= 1`) and line 0's `GB_SLEEP 7,1` OAM
                // step (`:1828`, `= 2`): it holds 1 across all of vblank AND line
                // 0's first dots. So when VBlank is enabled the STAT line stays
                // continuously HIGH from line 144 through the line-0 OAM rise —
                // the dot-4 OAM pulse joins an already-high line and raises NO
                // fresh 0→1 edge (`m1/m2m1irq_ifw_2`: SameBoy fires ly1-143, NOT
                // ly0; slopgb's `vis_mode`=0 here dropped the line at dot 0 and
                // re-raised it at dot 4 → spurious ly0 OAM IRQ → `got=3` for
                // `want=1`). With VBlank disabled the carried mode-1 source
                // contributes nothing, so the line is low into dot 4 and the OAM
                // pulse fires its real edge (matching SameBoy's vblank-off rows).
                // `vis_mode` differs only for DMG (CGB line-0 dots 0-3 already
                // read mode 1); the IRQ side is decoupled from the FF41 read, so
                // the visible DMG line-0 mode-0 window is untouched. LE/Tier-2
                // only — `mode_for_interrupt` is inert flag-off, production
                // byte-identical.
                if self.dot == 4 {
                    2
                } else if self.dot < 4 {
                    1
                } else {
                    crate::stat_update::MODE_FOR_INTERRUPT_NONE
                }
            } else if self.dot < 4 {
                // Lines 1-143: the OAM (mode-2) IRQ source is carried high
                // across the whole line-start window (dots 0-3). SameBoy sets
                // `mode_for_interrupt = 2` at the prior line's end
                // (`display.c:2138`, skipped only for `LINES-1`) and re-sets it
                // at the line top (`display.c:1781`), so the source leads the
                // visible mode→2 edge (dot 4) by the entire window — the "OAM
                // int 1 T-cycle before STAT" glitch (`display.c:1778`) seen as a
                // sustained carryover, not only the dot-3 lead.
                2
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

    /// #11bh — FF0F group-A read PEEK (the #11ar FF41-peek shape on the IF
    /// register): the deferred cc+0 FF0F read's VERDICT includes a STAT engine
    /// rise whose emission dot is deterministically known at read time and
    /// lands within the read's SameBoy-frame sample window — verdict-only, no
    /// machine advance, no IF commit (the refuted #11bd sub-M FF0F sampling
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
        // (`stat_rise_oam`, the #11aq/#11ar per-ISR source tag — sticky since
        // the dispatching edge): `lyc0int_m0irq_ds_1` reads the IDENTICAL
        // dot-254/rise-255 geometry from an LYC-anchored ISR with the OPPOSITE
        // want (SameBoy's per-ISR read position separates them; slopgb
        // collapses both to one dot — measured, this session's two-bin).
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

    /// #11bh group B — arm the FF0F write-race squash window (see the
    /// `stat_if_squash` field doc + the consumption site in
    /// [`Self::stat_update_tick`]). Called by the interconnect at the deferred
    /// FF0F write's commit instant, only when the written value clears bit 1.
    /// Tier-2 caller only → production byte-identical OFF.
    pub(crate) fn arm_ff0f_if_squash(&mut self) {
        self.stat_if_squash = 2;
    }

    /// #11bh group E — the co-instant line-0 dot-4 OAM-pulse read-view mask
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
        if self.ly0_pulse_age > 0 && self.line == 0 && self.dot == 4 && self.lyc == 153 {
            IF_STAT
        } else {
            0
        }
    }

    /// #11bh group C — arm the dispatch-ack squash window for the acked IF
    /// bit (see the `ack_squash_ppu` field doc + the consumption sites in
    /// [`Self::stat_update_tick`] and the vblank raise). Called by the
    /// interconnect's `ack` on the tier2 path only.
    pub(crate) fn arm_ack_squash(&mut self, bit: u8) {
        self.ack_squash_ppu_mask = 1 << bit;
        self.ack_squash_ppu = 2;
    }

    /// SameBoy `ly_for_comparison` (`display.c`) — the *delayed* LY value the
    /// LYC==LY STAT source compares against, distinct from the live FF44. It is
    /// `-1` ("no line", SameBoy's `0xFFFF`/`-1` sentinel: nothing matches) at the
    /// top of each line, latches to the line number a few dots in, and holds the
    /// previous line's value across the next line's first dots (the LYC-match
    /// tail). This is the LYC input the S5 [`StatUpdate`](crate::stat_update)
    /// engine consumes on the flag-on path; inert (unread) flag-off.
    ///
    /// Single speed is pinned exactly (DMG / CGB-C / AGB). Double speed doubles
    /// the line-153 GB_SLEEP offsets — deferred to the S7 DS unification; the DS
    /// branch below uses the single-speed dot grid as a documented placeholder
    /// (inert, so it changes no observable behaviour until the flip recalibrates
    /// it). The LCD-enable glitch line returns `-1` (its LY/LYC view is the live
    /// flag-off path's concern, `lcdon_*` tables).
    pub(super) fn ly_for_comparison(&self) -> i16 {
        self.ly_for_comparison_at(self.line, self.dot)
    }

    /// [`Self::ly_for_comparison`] evaluated at an explicit (line, dot) — the
    /// #11bd law-frame variant (write-instant classifications on shifted ROMs
    /// pass [`Ppu::law_pos`]).
    pub(super) fn ly_for_comparison_at(&self, at_line: u8, at_dot: u16) -> i16 {
        if !self.enabled || self.glitch_line {
            return -1;
        }
        let line = i16::from(at_line);
        if at_line <= 143 {
            // Visible line: prev-line carryover (dots 0-2) → -1 at the dot-3
            // reset (`display.c:1776`, `current_line ? -1 : 0`) → N at dot 4
            // (`display.c:1786`). Line 0's predecessor (line 153) ends holding 0.
            if at_dot >= 4 {
                line
            } else if at_dot == 3 {
                if at_line == 0 { 0 } else { -1 }
            } else if at_line == 0 {
                0
            } else {
                line - 1
            }
        } else if at_line <= 152 {
            // VBlank 144-152: `-1` set at line entry, `= current_line` after
            // GB_SLEEP 26+12 (≈dot 4) (`display.c` 144-152 loop).
            if at_dot >= 4 { line } else { -1 }
        } else {
            self.ly_for_comparison_line_153_at(at_dot)
        }
    }

    /// Line 153's model-specific `ly_for_comparison` micro-sequence (the
    /// `display.c` line-153 tail). See [`Self::ly_for_comparison`].
    fn ly_for_comparison_line_153_at(&self, at_dot: u16) -> i16 {
        if self.ds && self.model.is_cgb() {
            // #11bg — the CGB double-speed line-153 schedule (replacing the
            // documented SS placeholder): `ly_for_comparison` latches 153
            // EARLY (dot 4) and holds through the LY=0 step with NO -1 gap
            // (`display.c:2246` keeps 153 when `cgb_double_speed`), dropping
            // to 0 at dot 12. The dot-4 rise + the immediate DS engine view
            // is the unique whole-dot solution to the four
            // `lyc153_m1disable_ds` / `lyc0_m1disable_ds` leg constraints
            // (dip-vs-seamless m1→LYC handoffs bracketing dots 4 and 12 —
            // asm-derived, dual-traced); the SS dot-6 table stays pinned by
            // wilbertpol ly_lyc_153-C.
            // The [8,12) window is the `-1` GAP, not live 153: a fresh LYC
            // write landing there must NOT re-latch (`lyc153_late_ff45_
            // enable_ds_6` E0 — SameBoy's "writing to LYC during this period
            // has side effects" zone) while a HELD true match carries through
            // it (the m1disable seamless handoffs).
            return match at_dot {
                0..=3 => -1,
                4..=7 => 153,
                8..=11 => -1,
                _ => 0,
            };
        }
        if self.model == Model::Agb {
            // `model > CGB_C`: GB_SLEEP(14,2) lands the first set at dot 4, and
            // `model>CGB_C||ds` keeps it 153 through the LY=0 step; no -1 gap.
            match at_dot {
                0..=3 => -1,
                4..=11 => 153,
                _ => 0,
            }
        } else {
            // DMG / MGB / CGB-C single speed (`model <= CGB_C`, `!ds`):
            // GB_SLEEP(14,4) delays the first set to dot 6, then the LY=0 step
            // drops `ly_for_comparison` back to -1 (the `model>CGB_C||ds` arm is
            // false) before the final = 0 at dot 12. (DS placeholder, see above.)
            match at_dot {
                0..=5 => -1,
                6..=7 => 153,
                8..=11 => -1,
                _ => 0,
            }
        }
    }
}
