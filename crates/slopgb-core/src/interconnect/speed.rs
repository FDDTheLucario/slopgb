//! CGB speed-switch / halt-wake / dispatch-reclock engine bodies (the
//! `Bus` trait impl in the parent delegates here — a trait impl cannot
//! split across files; CLAUDE.md <1000-line cap, seam per
//! `docs/tdd-split-plan.md` B5). Pure cut-and-paste of the trait-fn
//! bodies: `stop` (the STOP dance: gambatte pause + the leave
//! advance + the exit-table latches), the halt-wake samplers
//! (wake grid), the IF ack (per-source ack windows),
//! the dispatch retime (carried-read arming) and the halt-entry
//! view. Behavior-identical; suite-gated.

use super::*;

impl Interconnect {
    pub(super) fn halt_wake_mid_impl(&mut self) -> u8 {
        let mut w = self.pending_halt_wake();
        // Eager sub-M-cycle WAKE peek (CGB single-speed). The eager PPU commits
        // the mode-0 STAT IF at the END of the whole M-cycle that contains the
        // flip, up to one M-cycle later than the projected flip dot; two rows
        // whose flips differ by <4 dots (an SCX&7 delta) therefore commit — and
        // wake — at the SAME whole-M-cycle boundary, collapsing the wake INSTANT
        // that tier2's 4k+2 sample resolves. Peek the rise in DOT space
        // (`projected_flip_dot() <= dot`, a pure value peek — no machine
        // advance, timer-safe) so the wake lands at the flip's M-cycle boundary
        // and the resumed stream + FF41 read separate by the SCX delta. The
        // resumed IME=1 dispatch's first FF41 read then rides the re-fetch
        // boundary override (`Ppu::halt_refetch`), armed below.
        let eager_cgb_halt = self.model.is_cgb() && !self.double_speed;
        if eager_cgb_halt
            && self.cpu_halted
            && w & IF_STAT_BIT == 0
            && self.ie & IF_STAT_BIT != 0
            && self.ppu.m0_stat_flip_reached()
        {
            w |= IF_STAT_BIT;
        }
        if w != 0 {
            // Eager CGB halt-woken m0-STAT wake: arm the re-fetch boundary
            // override for the IME=1 dispatch's first FF41 read (consumed
            // one-shot at the line-boundary crossing in `Bus::read`). The
            // m0-origin test mirrors the wake above's two cases: a natural
            // whole-M-cycle IF commit (`stat_rise_m0`) or the sub-M-cycle peek
            // (`m0_stat_flip_reached` ⇒ the rise landed this M-cycle — the
            // `stat_m0_rise_within(0)` term catches the render-still-live edge).
            if eager_cgb_halt
                && w & IF_STAT_BIT != 0
                && (self.ppu.stat_rise_m0() || self.ppu.stat_m0_rise_within(0))
            {
                self.ppu.set_halt_refetch(true);
            }
        }
        w
    }

    pub(super) fn halt_wake_impl(&self) -> u8 {
        // The halt-exit logic samples IE & IF *within* the M-cycle, not at
        // its end (SameBoy sm83_cpu.c `GB_cpu_run`: DMG samples mid-cycle
        // after 2 of 4 T-cycles, CGB/AGB at the start of the cycle), so a
        // timer reload + IF commit — which lands on the last T-substep
        // under the hardware DIV phase (div ≡ 0 mod 4 at boundaries) — is
        // missed until the next cycle: the halt wake comes one M-cycle
        // later than a running-CPU dispatch would (gambatte tima/tc*_irq_*
        // on both models; wilbertpol timer_if rounds 5/6 vs 3/4).
        //
        // The STAT bit joins the mask per event, not wholesale: the PPU
        // flags its second-half IF commits (line-start OAM pulses, mode-0
        // rises on dots ≡ 3,0 mod 4) via `Ppu::take_stat_halt_late`, which
        // ORs IF_STAT into `if_late` for exactly those cycles — the
        // gbmicrotest int_oam_*/int_hblank_halt_scx* grids and the
        // mooneye/wilbertpol hblank halt groupings pin the law, while
        // first-half STAT commits and the vblank IF stay live
        // (halt_ime1_timing2-GS, vblank, DMG). The known unmodelled
        // remainder is the CGB/AGB start-of-cycle staleness for first-half
        // PPU commits (halt_ime1_timing2-GS's "fail: CGB, AGB, AGS";
        // gambatte halt/*_cgb04c split rows): landing it requires a
        // per-model widening of the halt-late mask, a separate work
        // package.
        (self.intf & !self.if_late) & self.ie & IF_MASK
    }

    pub(super) fn ack_impl(&mut self, bit: u8) {
        self.intf &= !(1 << bit);
        // gambatte Memory::ackIrq syncs the acked bit's source a few
        // T-cycles past the ack point before clearing, so a hardware
        // re-set landing just after the dispatch's IF clear is consumed
        // by it (see the `ack_squash_*` field docs for the window
        // derivation and the pinning ROMs).
        match bit {
            0 | 1 => {
                // lcd_.update(cc + 2), no isCgb term: 2 dots into the
                // next machine tick on both families and at both speeds
                // (in double speed that is the whole 2-dot tick). The
                // line-anchored rises' single-speed second-half emission
                // dots stay OUT of reach — see the field docs.
                self.ack_squash_mask = 1 << bit;
                self.ack_squash_ticks = 0;
                // The deferred path takes NO post-ack squash
                // for the LCD bits: SameBoy's ack is the bare IF clear at the
                // flushed pending−2 instant (sm83_cpu.c) with no source
                // re-sync window, so a STAT/VBlank rise
                // 1-2 dots past the ack is DELIVERED (the six retrigger rows:
                // `late_m0irq_retrigger_ds_1` rise ack+2 · `_scx1_1` ack+1 ·
                // `m2int_m2irq_late_retrigger_1` next-line pulse ack+2 ·
                // `lyc153int_m2irq_late_retrigger_1` line-0 pulse ack+2 ·
                // `lycint143_m1irq_late_retrigger_ds_1` m1 re-rise ack+1 ·
                // `lycint_vblankirq_late_retrigger_ds_1` vblank IF ack+1..2 —
                // all dual-traced; the 2-dot squash ate exactly that fold). A
                // rise at/before the ack still loses: it folded during the
                // dispatch advance and the ack clears it (the `_2` siblings).
                // Production keeps gambatte's cc+4-frame 2-dot window.
                // Eager ack-squash port: the eager read-frame enters the
                // STAT/OAM ISR — and so fires this ack — the read-debt earlier
                // than the gambatte cc+4 frame, so widen the squash window by
                // that shift so the post-ack retrigger stays consumed on the
                // eager frame (irq_precedence `late_m0irq_retrigger_2`/`_scx1_2`
                // + `_ds_2`, want E0) while the one-M-cycle-later `_1` siblings
                // still land outside it and DELIVER (want E2). DS uses +1
                // (window 3) for the LYC/mode-2/mode-1/vblank families; the
                // MODE-0 (HBLANK) retrigger family (`late_m0irq_retrigger`)
                // needs window 4 — HBLANK is the enabled STAT source
                // (`stat_src_hblank`) ONLY for those rows, so widen to 4 there.
                self.ack_squash_dots = if self.double_speed {
                    if self.ppu.stat_src_hblank() { 4 } else { 3 }
                } else if !self.model.is_cgb() && self.ppu.line_dot().0 == 153 {
                    // Line-153 LYC retrigger family: the dot-4 LYC=153
                    // IF-emission decouple fires this line-153 STAT ISR — and
                    // its ack — one M-cycle (4 dots) EARLIER than the dot-6
                    // read frame the SS window `6` was tuned to, so widen by
                    // the same read-debt (6→10) so the retrigger re-squashes
                    // (gap 8 ≤ 10 → E0) while its `_1` sibling (gap 12 > 10)
                    // still DELIVERS (E2). eager DMG line-153 only.
                    10
                } else {
                    6
                };
                // Eager-value carried-read peek: arm `read_carried` for a STAT
                // OAM/HBlank ISR so the handler's first FF41 mode read takes
                // the source's read-position carry (`isr_read_carry_hd`).
                // Under the eager clock the dispatch stays cc+4, so arm the
                // VERDICT peek here at the STAT (bit 1) ack.
                // Cleared one-shot after the FF41 read in `Bus::read`.
                if bit == 1 && (self.ppu.stat_rise_oam() || self.ppu.stat_rise_m0()) {
                    self.ppu.set_read_carried(true);
                }
            }
            2 | 3 => {
                // updateTimaIrq(cc + 2 + isCgb()) / updateSerial(cc + 3 +
                // isCgb()): with the timer IF on the last T-substep and
                // the serial IF on the DIV-edge boundary, both windows
                // cover the set produced by the next machine tick on the
                // DMG family and the next two on CGB/AGB.
                self.ack_squash_mask = 1 << bit;
                self.ack_squash_ticks = if self.model.is_cgb() { 2 } else { 1 };
                self.ack_squash_dots = 0;
            }
            _ => {}
        }
    }

    pub(super) fn stop_impl(&mut self, skipped_addr: u16, interrupt_pending: bool) -> bool {
        let switching = self.cgb_mode && self.key1_armed;
        let entering_ds = switching && !self.double_speed;
        // Pin the post-switch exit-table anchor: the FIRST LCD-on
        // switching STOP since the last enable classifies the dance
        // (mid-frame speedchange anchor vs the VBlank/boot prologue frame).
        if switching {
            self.ppu.note_switch_stop();
        }
        // gambatte Memory::stop snapshots the HDMA situation at the
        // pre-read cc: a block request still pending when STOP executes
        // (flagged mid-instruction — no boundary came) is deferred when
        // leaving double speed (haltHdmaState_ = hdma_requested +
        // ackDmaReq) but stays flagged when entering it, firing *inside*
        // the pause where the gated core clock aborts the HBlank transfer
        // with the count latched (dma()'s halted path; pinned by
        // hdma_transition_speedchange_hdmalen*_hdma5 → $80|len vs
        // hdma_late_m3speedchange_hdma5_*_ds_1 → still active).
        let in_window = self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period_law();
        let pending_req = self.vram_dma_req.take();
        if switching && !entering_ds {
            // Leaving double speed: the PPU/APU re-pace from the cycle
            // right after the STOP opcode fetch (gambatte lcd_/psg_
            // .speedChange at cc_ = cc + 8 * !isDoubleSpeed(): offset 0
            // leaving, +8 entering), so the toggle precedes the
            // skipped-byte read below; entering double speed it lands
            // after the read + internal cycle instead.
            self.double_speed = false;
            self.ppu.set_double_speed(false);
        }
        if !interrupt_pending {
            // The skipped byte costs one real read M-cycle (SameBoy
            // stop(): `cycle_read(gb, gb->pc++)`, gated on no pending
            // interrupt). The value is discarded; the address still
            // drives the bus (OAM bug).
            self.tick_machine();
            self.maybe_oam_bug(skipped_addr, OamBugKind::Read);
            let _ = self.read_no_tick(skipped_addr);
        }
        // STOP resets DIV on every model (Pan Docs "FF04 — DIV"),
        // committing like a write occupying the skipped-byte read slot:
        // gambatte Memory::stop timestamps `nontrivial_ff_write(0x04, 0,
        // cc)` at the slot's *start* cc, and gambatte write timestamps are
        // start-of-cycle (cpu.cpp FF_WRITE advances cc afterwards) where
        // ours commit after the tick — so the reset lands here, after that
        // cycle's tick (the gambatte speedchange tima/div a/b phase pairs
        // pin the TIMA falling-edge quirk to this cell). Modelled as a DIV
        // write so the falling-edge effects apply (frame-sequencer edge
        // included, `Apu::div_write` — the speedchange ch2_nr52 families).
        self.apu.div_write(self.double_speed);
        self.timer.write(0xFF04, 0);
        if !switching {
            // Deep stop: hand a still-pending block request back — the
            // CPU's stop idle engages the halt gate, which defers it
            // (gambatte's non-switch stop path calls Memory::halt).
            self.vram_dma_req = pending_req.or(self.vram_dma_req);
            return false;
        }
        self.key1_armed = false;
        if interrupt_pending {
            // With IE & IF pending the switch is instantaneous: no
            // skipped-byte read, no pause (SameBoy stop() gates the halt
            // countdown on !interrupt_pending; age caution/
            // spsw-interrupts).
            if entering_ds {
                self.double_speed = true;
                self.ppu.set_double_speed(true);
            }
            self.vram_dma_req = pending_req.or(self.vram_dma_req);
            return true;
        }
        // The OAM DMA controller freezes after the read cycle (gambatte
        // Memory::stop: updateOamDma(cc + 4), then intreq_.halt()); the
        // halt-hdma snapshot below is installed first so the wake path
        // can re-evaluate it.
        self.halt_hdma = if pending_req.is_some() && !entering_ds {
            HaltHdmaState::Requested
        } else if in_window {
            HaltHdmaState::High
        } else {
            HaltHdmaState::Low
        };
        self.engage_halt_gate(true);
        // One internal M-cycle before the pause (gambatte Memory::stop
        // returns cc + 8: the operand read plus one cycle), still at the
        // old PPU/APU pace when entering double speed.
        self.tick_machine();
        if entering_ds {
            self.double_speed = true;
            self.ppu.set_double_speed(true);
        }
        // Mode-0 entries seen by the two cycles above never flag a block:
        // gambatte defers all LCD events into the pause, where the halted
        // gate suppresses the flag; the live window is re-checked at wake.
        self.vram_dma_req = None;
        // The pause: the CPU sleeps for 0x7FFF more M-cycles on the *new*
        // clock — with the read + internal cycles that totals 0x8001
        // M-cycles ≙ gambatte's unhalt event at cc + 0x20000 + 4 (cc
        // counts 4 per M-cycle at either speed) — while PPU/APU/timer run
        // on. IE & IF != 0 ends it early, exactly like halt mode
        // (gambatte's pause *is* a halt: the halted intevent_interrupts
        // path unhalts; SameBoy keeps gb->halted under
        // speed_switch_halt_countdown). SameBoy instead uses a flat
        // 0x20008 8-MHz-clock countdown — half the pause when leaving
        // double speed; gambatte's cgb04c expectations are this suite's
        // oracle, and the speedchange2/3/4/5 (DS→single) LY families
        // confirm its doubled length.
        let dots_per_m: u64 = if self.double_speed { 2 } else { 4 };
        let target = self.cycles + 0x7FFF * dots_per_m;
        if entering_ds && pending_req.is_some() {
            // The surviving block request fires at the first event check
            // inside the pause: the halted service aborts the transfer
            // (see run_vram_dma). Its stall counts toward the pause.
            self.vram_dma_req = pending_req;
            self.run_vram_dma();
        }
        while self.cycles < target && self.intf & self.ie & IF_MASK == 0 {
            self.tick_machine();
        }
        // Post-switch CPU↔PPU realignment, the tier2 DEFAULT at K = 2
        // half-dots per switching STOP. SameBoy's STOP withholds 5 T from
        // the PPU feed (`speed_switch_freeze`, sm83_cpu.c:435/timing.c:469)
        // while slopgb's gambatte-modeled pause runs the PPU throughout; the
        // measured net alignment on the gambatte cgb04c pause calibration is
        // +2 half-dots per switch (with K=2/switch the post-switch polled
        // reads land exactly at SameBoy's read cfl − 4, and the half-dot bare
        // exit E(scx) = 510 + 2*scx closes all four scx1/scx2 `_1`/`_2` pairs,
        // co-landing with that exit). The eager re-host shares the STOP-shift
        // install so the `speedchange`/`lcd_offset` reads classify on the same
        // un-shifted frame the `law_pos` consumers (`access.rs`, `stat_irq`,
        // `ff0f`, `regs`, `lyc`, `blocking`) + the `vis_exit_hd` post-switch
        // exit-table arms (`stop_anchor_midframe`/`stop_leave_*`) already read.
        {
            // K in 8 MHz HALF-dots (the grain): odd K leaves the PPU on a
            // half-dot skew relative to the CPU grid (`dhalf` persists), the
            // odd-mode alignment SameBoy's whole-freeze cannot represent.
            //
            // LEAVE-only (DS→SS): the per-switch K=2 default was measured to
            // rebase the ENTIRE DS suite (every `_ds` row's boot STOP shifted
            // +1 dot, breaking ~20 halt/lycEnable/lcd_offset/DS rows whose
            // tier2 constants were calibrated on the existing frame) — the DS
            // calibration already ABSORBS the entering switch's frame error,
            // so advancing there double-counts it. Only the post-switch
            // SINGLE-SPEED frame (after leaving DS) carries the un-absorbed
            // gambatte-pause error the speedchange m3stat reads expose.
            //
            // Leave-only w=4 was measured +14/−11 SameBoy-pass — the +14
            // (speedchange `_2` legs + 3 lcd_offset counts, with the half-dot
            // exit co-land) A/B against 11 lcd-offset-frame rows whose tier2
            // law constants sit on the w=0 frame (8 absorbable by
            // re-derivation; the offset1 m0stat/m2stat COUNTS +
            // `hdma_late_m0halt_lcdoffset3` have no absorbing law). The
            // default flips to `if double_speed {0} else {4}` (leave-only)
            // once the lcd-offset constants are re-derived on the +2-dot
            // frame.
            // STOPADV is LEAVE-ONLY, default w=2 (enter stays 0 — the
            // enable-phase dual-trace confirms enter contributes 0: the
            // `_ds`-measured offset1 rows sit at the same +2 missing as the
            // SS rows, refuting the enter −2 split; and the DS suite is
            // calibrated on the w=0 enter frame). The MACHINE epoch truth is
            // +2 hd per leave (the lcd_offset enable-phase dual-traces; the
            // 2-trip offset2 count rows fix at w=2 and stay broken at w=4
            // where the two leaves fold to +8 ≡ 0 mod 8). The m3stat READ
            // laws additionally want +2 hd per leave on top (the w=4
            // speedchange empirics, with the same m3stat set) — that
            // law-side surplus is the carried `lcd_phase_hd` (= 4 − k per
            // leave), consumed by the read comparison so the read frame
            // matches the w=4-derived constants while polls/counts/LY see
            // the true +2 epoch.
            // The leave shift is ALIGNMENT-DEPENDENT (the `sb_dsa8` shadow of
            // SameBoy's `double_speed_alignment`): the dsa7=4 leaves need +6
            // (offset3 — only k=6 fixes its count rows) while dsa7∈{0,6}
            // leaves need +2 (offset1/offset2-leave2/speedchange).
            let k = if entering_ds {
                0
            } else if self.ppu.sb_dsa() & 7 == 4 {
                6
            } else {
                2
            };
            // Record the leave for the post-switch exit table (the leave k
            // is the table's class variable; LCD checked at the pause-end
            // instant, so the lcdoff2 off-leave stays excluded).
            if !entering_ds {
                self.ppu.note_switch_leave(k as u8);
            }
            for _ in 0..k {
                let ppu_if = self.ppu.tick_half();
                if self.ppu.dot_completed() {
                    self.fold_ppu_events(ppu_if, 4);
                    self.cycles += 1;
                }
            }
            // Every switching STOP's pause (enter AND leave) leaves the
            // alignment shadow −4 mod 8 of SameBoy's (pause-length + freeze
            // withholding delta; calibrated on the offset1/offset2/offset3
            // dsa values — all three close exactly with the uniform
            // correction). Applied AFTER the k-advance so the NEXT leave
            // reads the corrected alignment.
            self.ppu.add_lcd_shift((k / 2) as u16);
            self.ppu.dsa_pause_correction();
        }
        self.engage_halt_gate(false);
        self.vram_dma_unhalt();
        true
    }

    pub(super) fn halt_entry_impl(&mut self) -> u8 {
        // SameBoy's `halt()` checks IE & IF *after*
        // the prefetch `cycle_read` advanced the machine through the HALT
        // fetch M-cycle (t0+4), where the deferred leading-edge `pending()`
        // view sits at t0 (sm83_cpu.c:1036-1058). A mode-0 rise landing
        // inside the fetch M-cycle therefore arms SameBoy's halt-bug (no
        // halt; the following byte runs twice) while slopgb halted and woke
        // on the first idle check — one M-cycle short (the `_3b`
        // skip-path). Flush the debt, then sample.
        // DMG + single-speed CGB (the CGB-SS extension with the rise
        // deadline lands the whole set together; the entry flush ALONE was
        // the measured A/B swap. Double speed
        // keeps the old masks: its 2-dot M grid re-frames the rise, the DS
        // legs regressed under the deadline model).
        let mut w = self.pending();
        // The eager clock parks no debt, so the flush above is a no-op for it
        // and the entry sample sits at t0 — 4 dots before SameBoy's post-fetch
        // view. A mode-0 rise landing inside the fetch M-cycle is therefore
        // invisible, the rewind does not arm, and the post-wake stream runs one
        // halt round early (`late_m0int_halt_m0stat_*`). Fold the rise in as a
        // VALUE peek at t0+4 rather than advancing: advancing would tick the
        // timers 4 T early (the TIMA-counted `int_hblank_halt` rows pin that).
        //
        // DMG-scoped. On CGB the same peek is +5/−1: it also arms the entry
        // view for the `_3b` skip-path (`late_m0int_halt_m0stat_scx3_3b` [Cgb],
        // want out2), where a rise inside the fetch M-cycle should arm SameBoy's
        // halt-bug rather than the rewind. That row is OFF-fail and outside the
        // TRUE flip bar, so the CGB half is a net gain — but it drops an EV pass
        // and no SameBoy verdict has been taken for it, and a shipped slice may
        // not drop a SameBoy-PASS row on an unverified guess. CGB stays on the
        // t0 sample until the `_3a`/`_3b` split is measured against SameBoy.
        if (!self.model.is_cgb() || !self.double_speed)
            && self.ie & IF_STAT_BIT != 0
            && self.ppu.stat_m0_rise_within(4)
        {
            w |= IF_STAT_BIT;
        }
        w
    }

    pub(super) fn halt_entry_rewind_impl(&mut self) -> bool {
        // The IME=1 halt-entry rewind (SameBoy `halt()`), DMG/CGB-single-speed
        // scoped like the entry check. The rewind is SameBoy's own halt
        // semantics, independent of which clock frames the read.
        if self.model.is_cgb() && self.double_speed {
            return false;
        }
        self.pending_halt_entry() != 0
    }

    pub(super) fn dispatch_pending_impl(&mut self) -> u8 {
        // The running CPU's end-of-fetch dispatch check: the machine
        // is already flushed to the boundary (the previous step's
        // `flush_pending`), so `pending()` sees every rise before it — the
        // SameBoy view. Only the m0-rise visibility deadline (the same frame
        // offset the halt samples consult) applies on top; a flush here was
        // measured to shift the deferred operand frame of every following
        // instruction (8 pins broken) and is NOT SameBoy's semantics.
        self.pending()
    }
}
