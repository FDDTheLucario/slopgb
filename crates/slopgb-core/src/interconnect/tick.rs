//! Per-M-cycle machine advance (the clock-master step) + the HALT/STOP core-clock gate. docs/ARCHITECTURE.md §Timing.

use super::*;

impl Interconnect {
    /// Consume this M-cycle's dispatch-ack timer/serial squash mask: the
    /// `ack_squash_mask & 0x0C` while the sync-ahead window is open (stepping
    /// its countdown), else 0. Shared by the eager `tick_machine` (stores it
    /// in a local) and the deferred `advance_machine_t` (stores it in
    /// `deferred_squash`).
    fn take_ack_squash_tick_mask(&mut self) -> u8 {
        if self.ack_squash_ticks > 0 {
            self.ack_squash_ticks -= 1;
            self.ack_squash_mask & 0x0C
        } else {
            0
        }
    }

    /// Clear the per-M-cycle late-mask + accessibility/STAT edge stamps at the
    /// M-cycle head. Shared by the eager `tick_machine` and the deferred
    /// `advance_machine_t` phase-0 head.
    fn reset_mcycle_edges(&mut self) {
        self.if_stat_late = 0;
        self.m0_access_edge = None;
        self.pal_access_edge = None;
        self.stat_mode_edge = None;
    }

    /// Advance the whole machine by one CPU M-cycle (docs/ARCHITECTURE.md
    /// §Timing: timer, OAM DMA engine, PPU dots, VRAM DMA, APU, serial,
    /// joypad; IF bits OR-ed in as produced).
    pub(super) fn tick_machine(&mut self) {
        let dots: u64 = if self.double_speed { 2 } else { 4 };
        self.cycles += dots;
        // Dispatch-ack sync-ahead window for this tick (see `ack`):
        // timer/serial sets produced by an in-window tick are consumed
        // by the preceding ack instead of re-raising IF.
        let tick_squash = self.take_ack_squash_tick_mask();
        let t = self.timer.tick();
        // IF reads must see a second-half commit within its own cycle
        // (mooneye tima_reload access sequences) — only the halt-exit
        // sampling misses it, via the `if_late` mask.
        let t_iff = t.iff & IF_MASK & !tick_squash;
        self.intf |= t_iff;
        self.if_late = if t.late { t_iff } else { 0 };
        self.oam_dma_tick();
        self.reset_mcycle_edges();
        // cc-granular reclock: advance the M-cycle one CPU cc at a time
        // (cc=1..=4), advancing the PPU per 8-MHz half-dot (2 per dot SS, 1 DS)
        // and folding on the dot-completing half. Each event edge is stamped
        // with its dot's [`cc_eighth`]. `cycles` is bumped once above (`dots`),
        // so no per-dot bump here.
        for cc in 1..=4u8 {
            // Repay a WriteCpu-conflict engine write's borrowed dot: the
            // previous `Bus::write` ticked cc-1's PPU dot ahead of its
            // `write_no_tick` (SameBoy WriteCpu commits 1 T into the
            // M-cycle), so skip it here to restore CPU/PPU phase.
            if cc == 1 && self.eager_wr_borrow {
                self.eager_wr_borrow = false;
                continue;
            }
            let half_dots = if self.double_speed { 1 } else { 2 };
            for _ in 0..half_dots {
                let ppu_if = self.ppu.tick_half();
                if self.ppu.dot_completed() {
                    self.fold_ppu_events(ppu_if, cc);
                }
            }
        }
        let div = self.timer.div_counter();
        self.apu.tick(div, self.double_speed);
        self.intf |= self.serial.tick(div) & IF_MASK & !tick_squash;
        self.intf |= self.joypad.take_irq() & IF_MASK;
        // RTC wall time is dot time (2 dots per M-cycle in double speed).
        self.cart.tick_rtc(dots as u32);
    }

    /// Fold a completed PPU dot's IF bits, halt-late masks, accessibility/STAT
    /// edge stamps and the HBlank-DMA level detector into the machine state for
    /// cc `cc` (1..=4). `ppu_if` is the raw IF the PPU tick returned. Called by
    /// the eager half-dot loop in `tick_machine` (once per completed PPU dot).
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
                if second_half {
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
                // [`ACCESS_PHASE`] ([`stamp_blocks`]).
                self.m0_access_edge = Some(event_phase(EdgeKind::M0Access, cc, lead));
            }
            if let Some(lead) = self.ppu.take_pal_access_flip() {
                // The CGB palette-RAM unblock commits at the M-cycle end
                // ([`event_phase`] gives `PalAccess` the whole-M-cycle block):
                // the FF69/FF6B read stays $FF for the entire straddle M-cycle,
                // not just its second half (gambatte cgbpal_m3end).
                self.pal_access_edge = Some(event_phase(EdgeKind::PalAccess, cc, lead));
            }
            if let Some(lead) = self.ppu.take_m0_stat_flip() {
                // A sprite-line m3→m0 flip holds the double-speed FF41 mode bits
                // at the pre-flip mode 3 for the WHOLE straddle M-cycle
                // (`event_phase(StatMode)=END_PHASE`): the earlier dot-END
                // half-split caught only the +43 rows whose flip lands in
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
            // Backstop-clear an unconsumed eager halt-woken re-fetch override
            // (the previous round's wake never reached the line-boundary read)
            // so it cannot leak into this round. One-shot; byte-identical OFF.
            self.ppu.set_halt_refetch(false);
            // gambatte Memory::halt: a flagged-but-unserviced block
            // request is deferred (hdma_requested) and re-flagged at
            // wake — HBlank DMA never proceeds while the core clock is
            // gated; otherwise remember whether the hblank window was
            // already active so the same hblank cannot retrigger at wake.
            self.halt_hdma = if self.vram_dma_req.take().is_some() {
                HaltHdmaState::Requested
            } else if self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period_law() {
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
