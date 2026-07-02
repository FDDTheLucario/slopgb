//! Per-M-cycle machine advance (the clock-master step) + the HALT/STOP core-clock gate. docs/ARCHITECTURE.md §Timing.

use super::*;

impl Interconnect {
    /// Advance the whole machine by one CPU M-cycle (docs/ARCHITECTURE.md
    /// §Timing: timer, OAM DMA engine, PPU dots, VRAM DMA, APU, serial,
    /// joypad; IF bits OR-ed in as produced).
    pub(super) fn tick_machine(&mut self) {
        let dots: u64 = if self.double_speed { 2 } else { 4 };
        self.cycles += dots;
        // Dispatch-ack sync-ahead window for this tick (see `ack`):
        // timer/serial sets produced by an in-window tick are consumed
        // by the preceding ack instead of re-raising IF.
        let tick_squash = if self.ack_squash_ticks > 0 {
            self.ack_squash_ticks -= 1;
            self.ack_squash_mask & 0x0C
        } else {
            0
        };
        let t = self.timer.tick();
        // IF reads must see a second-half commit within its own cycle
        // (mooneye tima_reload access sequences) — only the halt-exit
        // sampling misses it, via the `if_late` mask.
        let t_iff = t.iff & IF_MASK & !tick_squash;
        self.intf |= t_iff;
        self.if_late = if t.late { t_iff } else { 0 };
        self.oam_dma_tick();
        self.if_stat_late = 0;
        self.m0_access_edge = None;
        self.pal_access_edge = None;
        self.stat_mode_edge = None;
        // cc-granular reclock: advance the M-cycle one CPU cc at a time
        // (cc=1..=4), ticking a whole PPU dot only on the cc's
        // [`dot_ticks_on_cc`] selects for this speed + `dot_phase`. At phase 0
        // this is bit-identical to the old `for i in 0..dots` loop — single
        // speed ticks every cc (4 dots), double speed the even cc {2,4} (2
        // dots) — but a phase-1 double-speed M-cycle ticks the odd cc {1,3}
        // instead, the half-dot offset the LCD dot clock keeps across a STOP
        // speed switch (`cc_grid_matches_dot_loop`). Each event edge is stamped
        // with its dot's [`cc_eighth`] (carrying that sub-dot offset) instead
        // of the loop index. `dot_phase` stays 0 (the fixed even-cc alignment =
        // bit-identical to the dot loop): a speed-switch phase set was measured
        // to lift nothing — only a full pixel-pipe reclock uses it (see the
        // `dot_phase` field docs).
        for cc in 1..=4u8 {
            if !dot_ticks_on_cc(cc, self.double_speed, self.dot_phase) {
                continue;
            }
            self.tick_machine_dot(cc);
        }
        let div = self.timer.div_counter();
        self.apu.tick(div, self.double_speed);
        self.intf |= self.serial.tick(div) & IF_MASK & !tick_squash;
        self.intf |= self.joypad.take_irq() & IF_MASK;
        // RTC wall time is dot time (2 dots per M-cycle in double speed).
        self.cart.tick_rtc(dots as u32);
    }

    /// One PPU dot of `tick_machine`'s per-dot work (the body of its `for cc`
    /// loop): tick the PPU, fold its IF / second-half halt-late masks
    /// (`stat_late`/`stat_halt_late`/`m0_rise`) and the accessibility/STAT edge
    /// stamps for cc `cc` (1..=4), and pump the dot-exact HBlank-DMA level
    /// detector. Shared by the eager whole-M-cycle `tick_machine` and the port
    /// Stage-B deferred per-T advance ([`Self::advance_machine_t`]).
    pub(super) fn tick_machine_dot(&mut self, cc: u8) {
        let ppu_if = self.ppu.tick();
        self.fold_ppu_events(ppu_if, cc);
    }

    /// Fold a completed PPU dot's IF bits, halt-late masks, accessibility/STAT
    /// edge stamps and the HBlank-DMA level detector into the machine state for
    /// cc `cc` (1..=4). `ppu_if` is the raw IF the PPU tick returned. Shared by
    /// the whole-dot [`Self::tick_machine_dot`] and the half-dot deferred
    /// [`Self::advance_machine_t`] (which calls it only on a dot-completing
    /// half-dot, so the fold still runs exactly once per PPU dot).
    pub(super) fn fold_ppu_events(&mut self, ppu_if: u8, cc: u8) {
        {
            // STAT/VBlank rises in the first 2 dots after the ack are
            // consumed too (gambatte ackIrq lcd_.update(cc + 2); in
            // double speed the window spans the whole tick — see `ack`).
            let dot_squash = if self.ack_squash_dots > 0 {
                self.ack_squash_dots -= 1;
                self.ack_squash_mask & 0x03
            } else {
                0
            };
            self.intf |= ppu_if & IF_MASK & !dot_squash;
            if self.ppu.take_stat_late() {
                // The line-0 OAM STAT rise sits in the second half of the
                // M-cycle: the IF bit is readable at once, but this
                // cycle's interrupt sample must not see it (see
                // Ppu::stat_events_tick; mealybug "line 0 timing is different
                // by 4 cycles").
                self.if_stat_late |= IF_STAT_BIT;
            }
            if self.ppu.take_stat_halt_late() {
                // Second-half STAT IF commit (line-start OAM pulses):
                // readable at once, but the halt-exit sampler misses it
                // for one cycle — the same shape as the timer's `if_late`
                // mask (SameBoy GB_cpu_run halt path; gbmicrotest
                // int_oam_* grids pin the law).
                self.if_late |= IF_STAT_BIT;
            }
            if self.ppu.take_m0_rise() {
                let second_half = obs_pre_edge(MID_PHASE, event_phase(EdgeKind::M0Rise, cc, 0));
                if self.tier2_reclock && self.cpu_halted {
                    // Port Stage B — re-derive the mode-0 halt-wake mask for the
                    // deferred cc+0 frame (the `int_hblank_halt_scx0-7` DMG grid;
                    // `ppu-subdot-ladder.md` THESIS RESULT #8/#9). The deferred
                    // halt loop samples `pending_halt_wake` at cc+0, ~2 M-cycles
                    // before SameBoy's `GB_cpu_run` DMG mid-cycle sample
                    // (`sm83_cpu.c:1621-1628`, advance-2 → sample → advance-2)
                    // plus the dispatch-retime's const −1 TIMA phase. A forward
                    // advance before the sample is MEASURED WORSE (the IRQ
                    // becomes visible earlier → wake earlier → lower count), so
                    // the delay is supplied as extra `if_late` masking, not an
                    // advance. The 2 uniform M-cycles = this cycle (masked here) +
                    // one countdown cycle (`m0_halt_hold = 1`); the per-SCX
                    // `mask{rise cc==4}` second-half term adds the 8th-scx +1, at
                    // cc==4 because the deferred frame rotates the rise cc to
                    // `eager_cc + 1` (the eager cc==3 second half becomes the
                    // M-cycle-END cc==4). This recovers `int_hblank` to the baked
                    // 62,62,62,63,63,63,63,64 target on DMG while the kernel pair
                    // (FF41 reads) and `intr_2` (mode-2 OAM halt-wake) — which do
                    // not halt-wake on mode 0 — stay byte-identical; CGB is
                    // empirically unchanged (its `GB_cpu_run` samples cc+0, no
                    // mid-cycle). Reached only on the reclock path (`tier2_reclock`),
                    // so production is byte-identical.
                    self.if_late |= IF_STAT_BIT;
                    // C1.3 (S7): carry the rise's within-M-cycle phase to the
                    // first post-wake LY read (see `Interconnect::halt_ly_phase`).
                    // The back-date dots are indexed by the rise's M-cycle phase
                    // `cc` (1..=4). Geometrically pinned: a rise at cc=2 lands the
                    // straddling read 2 dots past the LY-increment (back-date 2),
                    // cc=3 lands it on the correct side already (0); cc∈{1,4} put
                    // the read clear of the wrap so any ≥1 carry is inert
                    // (`hblank_ly_scx_timing-GS` resolves only the straddlers).
                    // Calibrated to the ROM's hardware values (the SameBoy/HW
                    // ground truth); broader generalisation is golden + all-oracle
                    // checked at C4. Gated on `tier2_reclock` + `cpu_halted`.
                    const HALT_LY_PHASE_BY_CC: [u8; 4] = [1, 2, 0, 1];
                    // PORT 2 sweep knob (`SLOPGB_P2TBL`, temporary): the carry
                    // table re-derived for the mid-sample wake frame.
                    self.halt_ly_phase = match std::env::var("SLOPGB_P2TBL") {
                        Ok(t) if t.len() == 4 => {
                            t.as_bytes()[(cc as usize - 1) & 3] - b'0'
                        }
                        _ => HALT_LY_PHASE_BY_CC[(cc as usize - 1) & 3],
                    };
                    // C1.2: base 0 (was 1). The C0 boot-DIV +4 (the deferred
                    // hand-off frame the real flip installs at construction)
                    // advances the timer phase one M-cycle, which shifts this
                    // TIMA-counted halt-wake +1; dropping the uniform mask from
                    // 2 M-cycles to 1 (this cycle + `m0_halt_hold = cc==4`)
                    // restores the `int_hblank_halt_scx0-7` DMG grid
                    // (62,62,62,63,63,63,63,64) under the construction-time
                    // reclock. The prior `1 +` was calibrated against the
                    // set-after-boot path that does NOT apply the +4 — see the
                    // `tier2_int_hblank_halt_passes_dmg` pin (now boots with the
                    // reclock).
                    // PORT 2 (#11bc): +1 uniform mask cycle re-derived under
                    // the mid-cycle (w2) wake sample — the w2 advance runs the
                    // following M-cycle's phase-0 mask bookkeeping 2 T early,
                    // consuming one hold a half-cycle sooner; the extra cycle
                    // restores the C1.2-calibrated visibility (measured: the
                    // full 33-pin gate incl. `int_hblank_halt_scx0-7`'s
                    // 62,62,62,63,63,63,63,64 grid AND `hblank_ly_scx` with
                    // the unchanged C1.3 carry table passes ONLY at +1).
                    // `SLOPGB_P2HH` overrides for measurement.
                    // DMG-only: the +1 compensates the mid-cycle (w2)
                    // sampler's early phase-0 mask consumption, which only
                    // exists on the DMG path — on CGB it is a pure +1 wake
                    // delay (broke the CGB `halt *_m0stat` want-0 legs,
                    // measured −15 on the two-bin).
                    let p2hh: u8 = std::env::var("SLOPGB_P2HH")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(u8::from(!self.model.is_cgb()));
                    self.m0_halt_hold = p2hh + u8::from(cc == 4);
                } else if second_half {
                    // The mode-0 STAT rise carries the second-half halt law
                    // — the same shape as the line-start OAM pulses — but
                    // its dot moves with SCX/sprites/window, so the half is
                    // decided here against the CPU's M-cycle: a rise in the
                    // second half (PPU dots 3-4 within the cycle; the last
                    // dot in double speed) is readable at once and fully
                    // visible to the running CPU's interrupt sample, yet
                    // missed by the halt-exit sampler for one M-cycle
                    // (SameBoy GB_cpu_run samples the halt exit mid-cycle).
                    // mooneye hblank_ly_scx_timing-GS and the gbmicrotest
                    // int_hblank_halt_scx0-7 grid pin all eight SCX phases
                    // between them.
                    self.if_late |= IF_STAT_BIT;
                }
            }
            if let Some(lead) = self.ppu.take_m0_access_flip() {
                // The OAM/VRAM accessibility unblock trails the IRQ rise by
                // one half-dot (gambatte m0Time = xpos lcd_hres+7 vs the IRQ
                // at +6). A CPU OAM read samples at the cc+2 MID phase — two
                // dots before this M-cycle's end-sampled view — so when the
                // unblock lands in the cycle's second half it still reads
                // mode 3 ($FF). The IRQ, mode-bit flip and every other
                // access keep the end view; only the OAM read consults this
                // (gambatte oam_access/postread_*). The edge is stamped with
                // its dot-END commit eighth ([`event_phase`]); the read decides
                // blocking against the single CPU-access observer phase
                // [`ACCESS_PHASE`] ([`stamp_blocks`]). Sub-dot event-phase
                // model, increment 1.
                self.m0_access_edge = Some(event_phase(EdgeKind::M0Access, cc, lead));
            }
            if let Some(lead) = self.ppu.take_pal_access_flip() {
                // The CGB palette-RAM unblock commits at the M-cycle end
                // ([`event_phase`] gives `PalAccess` the whole-M-cycle block):
                // the FF69/FF6B read stays $FF for the entire straddle M-cycle,
                // not just its second half (gambatte cgbpal_m3end). INC-G3 task 5.
                self.pal_access_edge = Some(event_phase(EdgeKind::PalAccess, cc, lead));
            }
            if let Some(lead) = self.ppu.take_m0_stat_flip() {
                // A sprite-line m3→m0 flip holds the double-speed FF41 mode bits
                // at the pre-flip mode 3 for the WHOLE straddle M-cycle
                // (`event_phase(StatMode)=END_PHASE`, INC-G3 task 6): INC-DS-1's
                // dot-END half-split caught only the +43 rows whose flip lands in
                // the M-cycle's second half; the whole-M-cycle block adds the +84
                // residual `m3stat_ds_1` rows whose flip lands in the first half
                // (gambatte sprites). Net-positive A/B trade (full-gbtr +84/−3,
                // net floor −84): the only regressions are the 3
                // `late_sizechange_sp00/01/39_ds_1` (a net-neutral in-cluster
                // swap — their `_ds_2` siblings are in the lift; whole-M-cycle
                // forces both same-line size-change reads to mode 3, the `_2`
                // want it and the `_1` do not, and no `event_phase` offset
                // separates two reads in one M-cycle). The sprite-line gate stays
                // (dropping it floors 5
                // bare-line reads at a different chain offset:
                // dma gdma/hdma_cycles_scx5_ds_2, lcd_offset m0stat_count). The
                // edge stamps the whole-M-cycle END phase ([`event_phase`]); the
                // FF41 read blocks against the single CPU-access observer phase
                // [`ACCESS_PHASE`] ([`stamp_blocks`]).
                self.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, cc, lead));
            }
            // Dot-exact mode-0 entry: each visible line's hblank start
            // requests one HBlank DMA block, serviced at the head of the
            // CPU's next bus operation (gambatte video.cpp: memevent_hdma
            // fires at predictedNextM0Time). The flag is suppressed while
            // the core clock is gated (video.h EventTimes::flagHdmaReq:
            // `if (!intreq_.halted())`); the level detector keeps
            // tracking so a wake never sees a stale edge.
            let hb = self.ppu.hdma_trigger_level();
            if hb
                && !self.hdma_prev_hblank
                && self.hdma_mode == HdmaMode::ArmedLcdOn
                && !self.cpu_halted
            {
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
            self.hdma_prev_hblank = hb;
        }
    }

    /// Port Stage B (Tier 2) — the deferred-commit machine advance. Advances the
    /// PPU/timer/APU/serial across the half-open CPU-T-cycle span `[from_t,
    /// to_t)` (the debt the deferred-commit clock just paid), instead of a flat
    /// whole-M-cycle quantum. With the dispatch reclock re-parking `pending=2`
    /// (B2), the vector fetch + first handler reads advance only 2 T before
    /// sampling, so they land 2 dots early ("re-frames every read";
    /// `docs/sameboy-port/PORT-PLAN.md` Tier 2).
    ///
    /// Mirrors `tick_machine`'s per-M-cycle structure but T-by-T: each M-cycle's
    /// first T (`phase==0`) does the timer reset / OAM-DMA / edge-reset /
    /// squash-latch pre-work; every T runs one timer substep + (on a
    /// dot-ticking cc) one PPU dot via `tick_machine_dot`; the last T
    /// (`phase==3`) runs APU/serial/joypad/RTC. M-cycle boundaries are at
    /// multiples of 4 T from the clock origin, so the phase is `t % 4` — a
    /// fractional advance (the retime's 2 T) simply suspends mid-M-cycle and the
    /// next advance completes it, the `deferred_squash` latch carrying the
    /// per-M-cycle squash across the split. Conserves the per-M-cycle 4-T total.
    pub(super) fn advance_machine_t(&mut self, from_t: u64, to_t: u64) {
        for pos in from_t..to_t {
            let phase = (pos % 4) as u8; // 0..=3 within the M-cycle
            if phase == 0 {
                // M-cycle pre-work (the head of `tick_machine`): latch this
                // M-cycle's timer/serial squash window, run the OAM-DMA engine,
                // reset the per-M-cycle late masks + accessibility edge stamps.
                self.deferred_squash = if self.ack_squash_ticks > 0 {
                    self.ack_squash_ticks -= 1;
                    self.ack_squash_mask & 0x0C
                } else {
                    0
                };
                self.timer.begin_mcycle();
                self.oam_dma_tick();
                self.if_late = 0;
                // Carry the deferred mode-0 halt-wake delay across the following
                // M-cycles (see `m0_halt_hold`): one extra masked cycle each.
                // `advance_machine_t` is itself only reached on the reclock path,
                // so the `tier2_reclock` guard is redundant here — kept explicit
                // to pin the byte-identical-OFF invariant at the use site.
                if self.tier2_reclock && self.m0_halt_hold > 0 {
                    self.m0_halt_hold -= 1;
                    self.if_late |= IF_STAT_BIT;
                }
                self.if_stat_late = 0;
                self.m0_access_edge = None;
                self.pal_access_edge = None;
                self.stat_mode_edge = None;
            }
            // PORT 3 (#11bc, the S6 completion frame): the deferred path's
            // timer/serial ack-squash window is the EXACT SameBoy
            // T-threshold (`updateTimaIrq(cc + 2 + isCgb())` /
            // `updateSerial(cc + 3 + isCgb())` before `ackIrq`), not the
            // whole-M-cycle `deferred_squash` latch — a re-set committing
            // past the threshold is DELIVERED (`tima/tc00_irq_late_
            // retrigger_1` wants E4) while one inside it is consumed (the
            // `_2` sibling wants E0). `pos` is the commit T.
            let squash_t = if pos < self.ack_squash_deadline_t {
                self.ack_squash_mask & 0x0C
            } else {
                0
            };
            // Timer substep (T-granular): the second-half commit feeds the
            // `if_late` halt-wake mask exactly as `tick_machine`'s assignment.
            let (tiff, tlate) = self.timer.tick_substep(phase);
            let t_iff = tiff & IF_MASK & !squash_t;
            self.intf |= t_iff;
            if tlate {
                self.if_late |= t_iff;
            }
            // PORT 3: the serial completion commits at its true T — the
            // DIV-edge falls are detected per T-substep on the deferred path
            // (the eager `tick_machine` keeps the M-cycle-tail sample; the
            // detector is a falling-edge compare, so the finer cadence finds
            // the same edges at their exact T).
            self.intf |= self.serial.tick(self.timer.div_counter()) & IF_MASK & !squash_t;
            // Half-dot PPU advance (HALFDOT-BUILD-PLAN.md Part A): each CPU-T is
            // 2 8-MHz half-dots (single speed) or 1 (double speed); the PPU runs
            // per half-dot via [`Ppu::tick_half`]. A whole dot completes every
            // 2nd half-dot (single speed → 1 dot per T; double speed → 1 dot per
            // 2 T), and the fold + the `cycles` dot-clock bump run only on that
            // completing half — reproducing the old whole-dot `dot_ticks_on_cc`
            // grid exactly (single speed every cc; double speed the even cc), so
            // Stage 1 is byte-identical while the grain is now half-dot. `cc` of
            // the completing dot = `phase + 1`, matching the whole-dot fold.
            let cc = phase + 1;
            let half_dots = if self.double_speed { 1 } else { 2 };
            for _ in 0..half_dots {
                let ppu_if = self.ppu.tick_half();
                if self.ppu.dot_completed() {
                    self.fold_ppu_events(ppu_if, cc);
                    self.cycles += 1;
                }
            }
            if phase == 3 {
                // M-cycle tail (the foot of `tick_machine`): APU, joypad,
                // RTC (the serial moved to the per-T substep above).
                let div = self.timer.div_counter();
                self.apu.tick(div, self.double_speed);
                self.intf |= self.joypad.take_irq() & IF_MASK;
                self.cart.tick_rtc(if self.double_speed { 2 } else { 4 });
            }
        }
    }

    /// Gate (true) or ungate (false) the OAM DMA controller's clock.
    ///
    /// The OAM DMA controller is clocked by the CPU core clock, which HALT
    /// (and STOP) switches off while the PPU keeps running on its own clock.
    /// A transfer in progress therefore does not proceed while the CPU is
    /// halted: bytes already copied stay, the byte in flight never commits,
    /// and the rest of OAM keeps its old contents — the PPU renders from
    /// that mixture for as long as the CPU sleeps. Hardware-verified by
    /// mooneye madness/mgb_oam_dma_halt_sprites.s ("OAM DMA is in the middle
    /// of OAM access (but not proceeding with it!)"); its observed sprite
    /// data pins the freeze mid-byte, with the overwritten OAM byte intact.
    ///
    /// Called by the CPU wiring on halt/stop entry and exit (via
    /// [`Bus::set_halted`]); the halted CPU performs no bus accesses on
    /// hardware, so the CPU-visible bus state during the freeze is
    /// unobservable and no bus conflict is modelled.
    ///
    /// While a transfer sits frozen mid-byte, the PPU is handed the frozen
    /// access (OAM index about to be replaced + in-flight source byte): the
    /// DMA controller is "in the middle of OAM access (but not proceeding
    /// with it!)" and the MGB PPU's OAM scan sees glitched data derived
    /// from exactly these bytes (madness/mgb_oam_dma_halt_sprites.s; see
    /// `Ppu::set_oam_dma_freeze`). A freeze during the setup delay has no
    /// OAM access in flight and hands over nothing.
    pub fn set_cpu_halted(&mut self, halted: bool) {
        if self.cpu_halted == halted {
            return;
        }
        if halted {
            // PORT 2 (#11bc): repay an outstanding sub-M-cycle wake skew
            // before the next halt round begins, re-aligning the CPU to the
            // machine's 4-T grid — the skew lives from the mid-cycle wake
            // through the WHOLE handler (its measurement reads sample at the
            // wake's true sub-M-cycle T) and dies at the next halt entry, so
            // each round starts from the calibrated aligned frame (an
            // unbounded skew was measured to hang the multi-round mooneye
            // `hblank_ly_scx_timing-GS`, B=42).
            if self.tier2_reclock && self.wake_skew != 0 {
                let before = self.clock.now();
                self.clock.carry_read(std::mem::take(&mut self.wake_skew));
                self.clock.flush();
                self.advance_machine_t(before, self.clock.now());
            }
            // gambatte Memory::halt: a flagged-but-unserviced block
            // request is deferred (hdma_requested) and re-flagged at
            // wake — HBlank DMA never proceeds while the core clock is
            // gated; otherwise remember whether the hblank window was
            // already active so the same hblank cannot retrigger at wake.
            self.halt_hdma = if self.vram_dma_req.take().is_some() {
                HaltHdmaState::Requested
            } else if self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period() {
                HaltHdmaState::High
            } else {
                HaltHdmaState::Low
            };
        }
        self.engage_halt_gate(halted);
        if !halted {
            // Clear any unspent deferred mode-0 halt-wake delay so it cannot
            // leak into a later halt (the wake consumed the IRQ this masked).
            self.m0_halt_hold = 0;
            // The halt-mode wake restarts the OAM DMA controller's clock
            // one M-cycle ahead of the CPU pipeline: a single catch-up
            // cycle runs at the wake itself, before the CPU's first
            // post-wake M-cycle (SameBoy sm83_cpu.c `GB_cpu_run` halt
            // exit: `gb->dma_cycles = 4; GB_dma_run(gb)` on both the
            // IME=0 resume and the dispatch path, while `GB_dma_run`
            // returns early whenever `gb->halted`; hardware-pinned by
            // gambatte oamdma/oamdmasrc80_halt_*_read8000 out81 and
            // dma/hdma_transition_oamdma_2 out67, which read the
            // in-flight source index after a wake). The speed-switch
            // pause's gate release deliberately does NOT take this path:
            // oamdma/oamdmasrcC0_speedchange_readC000 out11 pins the
            // un-caught-up resume there (SameBoy's
            // speed_switch_halt_countdown expiry likewise skips the
            // catch-up). The conflict state left behind is unobservable —
            // every CPU bus access ticks the machine, refreshing it,
            // before the access.
            self.oam_dma_tick();
            // The catch-up byte commits at the wake itself (SameBoy's
            // GB_dma_run writes within the call); no PPU dots run before
            // the next machine cycle's head, so this is indistinguishable
            // from the regular deferred commit to the scan.
            self.oam_dma_commit_pending();
            self.vram_dma_unhalt();
        }
    }

    /// The raw core-clock gate: freezes the OAM DMA controller and hands
    /// the frozen access to the PPU (see [`Self::set_cpu_halted`] for the
    /// HBlank-DMA bookkeeping layered on top; `Interconnect::stop` drives
    /// this directly because the speed-switch pause sequences the HDMA
    /// state itself).
    pub(super) fn engage_halt_gate(&mut self, halted: bool) {
        self.cpu_halted = halted;
        let freeze = if halted {
            self.dma_run
                .as_ref()
                .map(|run| (run.idx, self.oam_dma_source_read(run.src, run.idx)))
        } else {
            None
        };
        self.ppu.set_oam_dma_freeze(freeze);
    }
}
