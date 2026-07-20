//! Per-M-cycle machine advance (the clock-master step) + the HALT/STOP core-clock gate. docs/ARCHITECTURE.md §Timing.

use super::*;

impl Interconnect {
    /// This M-cycle's dispatch-ack timer/serial squash mask: `ack_squash_mask
    /// & 0x0C` while the sync-ahead window is open (stepping its countdown),
    /// else 0. Consumed once per `tick_machine` (see the `ack_squash_*` fields).
    fn take_ack_squash_tick_mask(&mut self) -> u8 {
        if self.ack_squash_ticks > 0 {
            self.ack_squash_ticks -= 1;
            self.ack_squash_mask & 0x0C
        } else {
            0
        }
    }

    /// Clear the per-M-cycle late-mask + accessibility/STAT edge stamps at the
    /// M-cycle head (called by `tick_machine`).
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
        // Read-only diagnostic (GB-CPU-usage meter): the fraction of elapsed
        // cycles spent HALT-gated. Bumped in lockstep with `cycles`; never read
        // by emulation, so it can't perturb the golden output.
        if self.cpu_halted {
            self.halt_cycles += dots;
        }
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
        self.cart.tick_time(dots as u32);
    }

    /// Fold a completed PPU dot's IF bits, halt-late masks, accessibility/STAT
    /// edge stamps and the HBlank-DMA level detector into the machine state for
    /// cc `cc` (1..=4). `ppu_if` is the raw IF the PPU tick returned. Called by
    /// the half-dot loop in `tick_machine` (once per completed PPU dot).
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
                // cycle's interrupt sample must not see it (the SameBoy
                // timer-`if_late` shape; mealybug "line 0 timing is different
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
                // The OAM/VRAM accessibility unblock trails the IRQ rise by one
                // half-dot (gambatte m0Time = xpos lcd_hres+7 vs the IRQ at +6),
                // so a second-half unblock is not yet visible to the cc+2 MID
                // OAM read (gambatte oam_access/postread_*; see the field doc).
                // Stamped with its dot-END commit eighth ([`event_phase`]).
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
                // (`event_phase(StatMode)=END_PHASE`): both first- and
                // second-half flips block, pinning gambatte sprites `m3stat_ds_1`.
                // Costs 3 in-cluster swaps (`late_sizechange_sp00/01/39_ds_1`,
                // whose `_ds_2` siblings want mode 3 too — no `event_phase`
                // offset separates two reads in one M-cycle). The sprite-line
                // gate stays: dropping it floors dma gdma/hdma_cycles_scx5_ds_2
                // and lcd_offset m0stat_count. The edge stamps the whole-M-cycle
                // END phase; the FF41 read blocks against [`ACCESS_PHASE`]
                // ([`stamp_blocks`]).
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
    /// HALT/STOP switch off the CPU core clock that drives the OAM DMA
    /// controller while the PPU keeps running, so a transfer in progress freezes
    /// mid-byte: copied bytes stay, the in-flight byte never commits, the rest
    /// of OAM keeps its old contents, and the PPU renders that mixture for as
    /// long as the CPU sleeps. The frozen access (the OAM index about to be
    /// replaced plus the in-flight source byte) is handed to the PPU, whose MGB
    /// OAM scan sees glitched data derived from exactly those bytes; a freeze
    /// during the setup
    /// delay has none and hands over nothing. Hardware-verified by mooneye
    /// madness/mgb_oam_dma_halt_sprites.s ("OAM DMA is in the middle of OAM
    /// access (but not proceeding with it!)"); see `Ppu::set_oam_dma_freeze`.
    /// The halted CPU performs no bus accesses, so no bus conflict is modelled.
    pub fn set_cpu_halted(&mut self, halted: bool) {
        if self.cpu_halted == halted {
            return;
        }
        if halted {
            // Backstop-clear an unconsumed halt-woken re-fetch override (the
            // previous round's wake never reached the line-boundary read) so it
            // cannot leak into this round.
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
            // Reset the (now-inert) deferred mode-0 halt-wake delay scratch.
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
