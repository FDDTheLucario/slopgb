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
        // The sub-M-cycle WAKE clock. SameBoy's DMG halt
        // loop advances 2 T, samples `interrupt_queue`, then advances the
        // remaining 2 (`GB_cpu_run`, `sm83_cpu.c:1621-1628` — CGB samples at
        // the M-cycle head, no mid-step), so the halt-exit check runs on a
        // HALF-M-cycle grid: an IRQ rising in the first half of the idle
        // M-cycle wakes at its own half, and the resumed CPU's dispatch +
        // handler reads carry that 2-T offset (the deferred clock's reduced
        // park) until the stream re-aligns — the WAKE-CLOCK class's
        // discriminator (`halt *_m0stat` `Na`/`Nb` sub-legs share the read
        // POSITION; the wake INSTANT separates them). The
        // machine's own 4-T grid (timer/OAM-DMA/APU in `advance_machine_t`,
        // keyed on ABSOLUTE `pos % 4`) is untouched, so the TIMA-counted
        // wake grids (`int_hblank_halt`) keep their frame. Gated to the
        // tier2 deferred path + DMG family + an already-engaged halt gate
        // (`cpu_halted` — the FIRST idle prefetch samples at the M-cycle
        // head like SameBoy's `just_halted` 4-T step). Production and the
        // LE-only path take the default (plain `pending_halt_wake`) —
        // byte-identical OFF.
        if self.tier2_reclock
            && !self.model.is_cgb()
            && self.cpu_halted
            && self.clock.pending() >= 2
        {
            // The SameBoy-exact wake grid, SCOPED to the
            // mode-0-origin STAT rise (`GB_cpu_run` sm83_cpu.c:1629-1642:
            // one iq sample per iteration at the mid point 4k+2; the
            // post-sample advance(2) COMPLETES the M-cycle so the wake
            // resumes aligned at 4k+4). Every other source keeps the
            // calibrated w0/w2 semantics unchanged. An m0-origin STAT
            // never wakes at the head sample — its visibility is the
            // T-deadline (`stat_vis_from_t`) consulted at the +2 grid
            // (and by the plain first-check sample at halt entry).
            let m0_head = self.ppu.stat_rise_m0();
            let w0 = self.pending_halt_wake();
            let w0m = if m0_head { w0 & !IF_STAT_BIT } else { w0 };
            probe!(self.dbg_wake("w0", w0m));
            if w0m != 0 {
                return w0m;
            }
            let before = self.clock.now();
            self.clock.advance_pending(2);
            self.advance_machine_t(before, self.clock.now());
            let w2 = self.pending_halt_wake();
            // Re-sample the rise origin AFTER the advance — the rise may
            // have fired during these 2 T (the head value is stale).
            if w2 & IF_STAT_BIT != 0
                && self.ppu.stat_rise_m0()
                && self.clock.now() >= self.stat_vis_from_t
            {
                // The SameBoy-exact m0 wake: complete the idle M-cycle,
                // resume aligned — no forgiven tail, no skew. Then the
                // RE-FETCH M-cycle: SameBoy's halt loop performs no
                // prefetch (pure `GB_advance_cycles`), so the woken
                // instruction (IME=0 run path) or the dispatch's aborted
                // prefetch (IME=1) is a FRESH `cycle_read` after the
                // post-sample advance — one M-cycle later than slopgb's
                // reused idle prefetch. Carried as read debt so the
                // resumed stream shifts +4 T without moving the wake
                // sample or machine time (`GB_cpu_run` sm83_cpu.c:1706+).
                let before = self.clock.now();
                self.clock.advance_pending(2);
                self.advance_machine_t(before, self.clock.now());
                self.clock.carry_read(4);
                probe!(self.dbg_wake("g2", w2));
                return w2;
            }
            // Non-STAT sources keep the calibrated mid-sample semantics:
            // forgiven tail + 2-T wake skew (mooneye intr_2_* / the tima
            // halt rows pin them; the mode-2 line-start pulse stays on the
            // w0 grid via its `if_late` mask).
            let w2n = w2 & !IF_STAT_BIT;
            if w2n != 0 {
                self.clock.forgive(2);
                self.wake_skew = 2;
                probe!(self.dbg_wake("w2", w2n));
                return w2n;
            }
            return 0;
        }
        let w = self.pending_halt_wake();
        if w != 0 {
            // The first idle check (SameBoy's `just_halted` head
            // sample) waking on the m0-origin STAT also re-fetches — the
            // woken instruction is a fresh `cycle_read` after the jh
            // advance(4), one M-cycle later than the reused idle prefetch.
            let dmg_first = !self.model.is_cgb() && !self.cpu_halted;
            let cgb_any = self.model.is_cgb() && !self.double_speed;
            if self.tier2_reclock
                && (dmg_first || cgb_any)
                && w & IF_STAT_BIT != 0
                && self.ppu.stat_rise_m0()
            {
                self.clock.carry_read(4);
            }
            probe!(self.dbg_wake(if self.cpu_halted { "plain" } else { "first" }, w));
        }
        w
    }

    /// Mask the mode-0-origin STAT rise bit out of an interrupt word `w`
    /// while it is not yet visible on the T-deadline (the same frame offset
    /// the halt/dispatch samples consult). Tier2 DMG (+CGB single-speed);
    /// production leaves `w` untouched (flag-gated OFF → byte-identical).
    fn mask_hidden_m0_stat(&self, w: u8) -> u8 {
        if w & IF_STAT_BIT != 0
            && self.tier2_reclock
            && (!self.model.is_cgb() || !self.double_speed)
            && self.ppu.stat_rise_m0()
            && self.clock.now() < self.stat_vis_from_t
        {
            return w & !IF_STAT_BIT;
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
        let w = (self.intf & !self.if_late) & self.ie & IF_MASK;
        // The m0-origin STAT rise's halt visibility is the T-deadline
        // (covers the halt-entry first check too). Tier2 DMG (+CGB sweep).
        self.mask_hidden_m0_stat(w)
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
                self.ack_squash_dots = if self.tier2_reclock { 0 } else { 2 };
                if self.tier2_reclock {
                    self.ppu.arm_ack_squash(bit);
                }
                // Eager-value carried-read peek: the tier2 dispatch retime
                // (`dispatch_retime_impl`) arms `read_carried` for a STAT
                // OAM/HBlank ISR so the handler's first FF41 mode read takes
                // the source's read-position carry (`isr_read_carry_hd`);
                // under the eager clock the dispatch stays cc+4 (no retime),
                // so arm the same VERDICT peek here at the STAT (bit 1) ack.
                // Cleared one-shot after the FF41 read in `Bus::read`. Never
                // fires flag-off (`eager_value` false) → byte-identical.
                if self.eager_value
                    && bit == 1
                    && (self.ppu.stat_rise_oam() || self.ppu.stat_rise_m0())
                {
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
                // On the deferred path the window is the
                // EXACT SameBoy T-threshold instead (see
                // `ack_squash_deadline_t`); the tick counters above are
                // still set but unused there.
                if self.tier2_reclock {
                    let cgb = u64::from(self.model.is_cgb());
                    self.ack_squash_deadline_t =
                        self.clock.now() + if bit == 2 { 2 } else { 3 } + cgb;
                }
            }
            _ => {}
        }
    }

    pub(super) fn stop_impl(&mut self, skipped_addr: u16, interrupt_pending: bool) -> bool {
        let switching = self.cgb_mode && self.key1_armed;
        let entering_ds = switching && !self.double_speed;
        // Pin the post-switch exit-table anchor: the FIRST LCD-on
        // switching STOP since the last enable classifies the dance
        // (mid-frame speedchange anchor vs the VBlank/boot prologue frame
        // the tier2 suite constants absorb). Tier2 + eager, byte-identical OFF.
        if (self.tier2_reclock || self.eager_value) && switching {
            self.ppu.note_switch_stop();
        }
        probe!(if crate::probe::s5dbg_on() {
            let (l, d) = self.ppu.scan_pos();
            eprintln!(
                "SLOPGB stop ly={l} dot={d} clk={} sw={} from_ds={} ip={}",
                self.cycles,
                u8::from(switching),
                u8::from(self.double_speed),
                u8::from(interrupt_pending)
            );
        });
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
        // co-landing with that exit). Tier2 + eager; production byte-identical
        // (both off). The eager re-host shares the STOP-shift install so the
        // `speedchange`/`lcd_offset` reads classify on the same un-shifted frame
        // the tier2 `law_pos` consumers (`access.rs`, `stat_irq`, `ff0f`,
        // `regs`, `lyc`, `blocking`) + the `vis_exit_hd` post-switch exit-table
        // arms (`stop_anchor_midframe`/`stop_leave_*`) already read.
        if self.tier2_reclock || self.eager_value {
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
            } else {
                crate::probe::tune_stopadv(if self.ppu.sb_dsa() & 7 == 4 { 6 } else { 2 })
            };
            probe!(if crate::probe::s5dbg_on() && !entering_ds {
                let (l, d) = self.ppu.scan_pos();
                eprintln!(
                    "SLOPGB leave ly={l} dot={d} clk={} dsa={} dsa7={} k={k}",
                    self.cycles,
                    self.ppu.sb_dsa(),
                    self.ppu.sb_dsa() & 7
                );
            });
            // Record the leave for the post-switch exit table (the leave k
            // is the table's class variable; LCD checked at the pause-end
            // instant, so the lcdoff2 off-leave stays excluded).
            if !entering_ds {
                self.ppu.note_switch_leave(k as u8);
            }
            probe!(if !entering_ds {
                let ph = std::env::var("SLOPGB_LCDPH")
                    .ok()
                    .and_then(|v| v.parse::<i16>().ok())
                    .unwrap_or(0);
                self.ppu.add_lcd_phase(ph);
            });
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

    pub(super) fn dispatch_retime_impl(&mut self) {
        // Re-park the clock 2 T early (SameBoy
        // sm83_cpu.c:1690) and advance the deferred machine by the 2 T it
        // commits, so the vector fetch + first handler reads sample 2 dots
        // early. Only reached on the reclock path (`dispatch_reclock`), after
        // the low push parked 4 (`pending == 4 > 2`).
        // EAGER EXPERIMENT (`coherent_dispatch`): under the eager clock the PPU
        // is advanced by `tick_machine` (whole-M-cycle, inline), NOT by
        // `advance_machine_t`; running the deferred machine drive here would
        // DOUBLE-advance the PPU +2 dots per STAT dispatch (measured: EV CGB
        // 365→611, intr_2-CGB B=42). Skip it on eager — the eager reads peek the
        // PPU directly (not via `clock.now()`), so the −2 read-reframe the
        // deferred retime buys is inert; only the post-push ack reorder +
        // `read_carried` arm (below) remain.
        if !self.eager_value || self.disp_advance {
            let before = self.clock.now();
            let _ = self.clock.dispatch_vector_retime();
            self.advance_machine_t(before, self.clock.now());
        }
        // If this dispatch is a DS OAM/HBlank STAT IRQ, arm the SCOPED
        // carried-read override for the handler's first FF41 read (cleared in
        // `read_deferred`). The override (`vis_mode_read`) shifts ONLY that
        // read's mode VERDICT by the IRQ source's read-position offset
        // (mode-2 OAM +4 dots / mode-0 HBlank +2) — a transient PEEK, NOT a
        // machine advance — so it decouples the read from the counter-pinned
        // dispatch dot
        // + IF delivery without disturbing the non-m3stat STAT-ISR reads
        // (m0stat/m2stat/enable) a real clock carry would mis-position. Tier2-
        // unconditional (the reclock frame the flip turns on). The dispatched bit
        // is STAT iff it is the lowest pending bit (VBlank, bit 0, out-prioritizes
        // STAT); guarding on that keeps a coincident vblank/timer dispatch clear.
        // Arm the read-position PEEK (both speeds) when this is an OAM/HBlank STAT
        // ISR (the dispatched bit is STAT iff it is the lowest pending bit). The
        // DS m3stat peek + the SS/DS WAKE-CLOCK peek both consume `read_carried`;
        // each branch re-gates on its own speed/mode in `vis_mode_read`.
        if self.pending().trailing_zeros() == 1
            && (self.ppu.stat_rise_oam() || self.ppu.stat_rise_m0())
        {
            self.ppu.set_read_carried(true);
        }
        probe!(if crate::probe::s5dbg_on() {
            let (line, dot) = self.ppu.scan_pos();
            eprintln!(
                "SLOPGB vec ly={line} dot={dot} clk={} pend={}",
                self.clock.now(),
                self.clock.pending()
            );
        });
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
        if self.tier2_reclock && (!self.model.is_cgb() || !self.double_speed) {
            let before = self.clock.now();
            self.clock.flush();
            self.advance_machine_t(before, self.clock.now());
        }
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
        if self.eager_value
            && !self.tier2_reclock
            && !self.model.is_cgb()
            && self.ie & IF_STAT_BIT != 0
            && self.ppu.stat_m0_rise_within(4)
        {
            w |= IF_STAT_BIT;
        }
        probe!(if crate::probe::s5dbg_on() {
            let (l, d) = self.ppu.scan_pos();
            eprintln!(
                "SLOPGB hentry ly={l} dot={d} clk={} w={w:02x} m0={} vis_from={}",
                self.clock.now(),
                u8::from(self.ppu.stat_rise_m0()),
                self.stat_vis_from_t
            );
        });
        // The entry decision observes the machine genuinely — the
        // m0-origin STAT rise's frame offset (the same T-deadline the wake
        // sampler consults) applies here too, else a rise landing in the
        // fetch M-cycle falsely arms the halt-bug.
        self.mask_hidden_m0_stat(w)
    }

    pub(super) fn halt_entry_rewind_impl(&mut self) -> bool {
        // The IME=1 halt-entry rewind (SameBoy `halt()`), on the
        // same tier2 DMG/CGB-single-speed scope as the entry check; the
        // t0+4 flushed + deadline-masked view decides.
        //
        // Hosted on the eager clock too: the rewind is SameBoy's own halt
        // semantics, independent of which clock frames the read. Without it the
        // eager CPU takes the halted+first-check-wake path and the post-wake
        // stream runs one halt round early — `pending_halt_entry`'s tier2 gate
        // skips the entry flush, so eager samples `pending()` unadvanced.
        if !(self.tier2_reclock || self.eager_value) || (self.model.is_cgb() && self.double_speed) {
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
        let w = self.pending();
        self.mask_hidden_m0_stat(w)
    }
}
