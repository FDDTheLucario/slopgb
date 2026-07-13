//! `impl Bus for Interconnect`: the CPU-facing bus contract. Each
//! read/write/tick advances every peripheral one M-cycle then performs
//! the access; plus interrupt dispatch/ack, halt-wake, STOP, the
//! dispatch reclock, and the instruction-boundary clock flush. Oracle:
//! full mooneye + gbtr matrix.

use super::*;

impl Bus for Interconnect {
    fn read(&mut self, addr: u16) -> u8 {
        // Deferred-commit clock: pay the previous M-cycle's parked debt
        // and park this read's 4 T-cycles.
        let _leading_edge = self.clock.read();
        // Latch the leading-edge (cc+0) value for PPU-positional reads
        // *before* the PPU advances (`None` for non-positional addresses).
        let leading = self.leading_edge_sample(addr);
        // The carried-read peek (armed at the STAT ack in `ack_impl`) is
        // one-shot: `leading_edge_sample`'s FF41 read has now consumed
        // `read_carried` inside `vis_mode_read`, so clear it.
        if addr == 0xFF41 {
            self.ppu.set_read_carried(false);
            // The halt-woken re-fetch override is one-shot at the line
            // boundary: it survives the sub-boundary polls (`read_pos_hd` short
            // of the line) and clears once this read has crossed it (the same
            // read `vis_mode_read` resolved to mode 2).
            if self.ppu.halt_refetch_crossed() {
                self.ppu.set_halt_refetch(false);
            }
        }
        self.service_vram_dma();
        self.tick_machine();
        // A trigger inside this very cycle still steals the bus before
        // the read samples (see `service_vram_dma`: reads yield, writes
        // in flight commit first).
        self.service_vram_dma();
        self.maybe_oam_bug(addr, OamBugKind::Read);
        self.check_access(addr, false);
        let trailing = self.read_no_tick(addr);
        // FF0F read-frame peek: the CGB LYC/STAT engine rise lands beyond the
        // cc+0 read, so the raw `intf` misses the deterministically-imminent bit
        // SameBoy's events-first read frame has already folded. Fold it in as a
        // verdict-only value peek (`Ppu::ff0f_stat_peek`, less the LY0 pulse the
        // whole-dot frame set a dot early) — the same VALUE-at-cc+4 shape as the
        // halt-entry peek. `intf` is untouched; the rise still folds at its own dot.
        let trailing = if addr == 0xFF0F {
            (trailing | self.ppu.ff0f_stat_peek())
                & !self.ppu.ff0f_ly0_pulse_mask()
                & !self.ppu.ff0f_cgb_ds_glitch_m0_mask()
        } else {
            trailing
        };
        leading.unwrap_or(trailing)
    }

    fn write(&mut self, addr: u16, value: u8) {
        // Deferred-commit clock: a write commits per its per-model conflict
        // class (`write_conflict`, the SameBoy `cycle_write` map). The commit
        // position is currently discarded (nothing samples it).
        let conflict = self.write_conflict(addr);
        let _commit = self.clock.write(conflict);
        self.service_vram_dma();
        // The CPU drives the data bus during the second half of the write
        // M-cycle (gbctr "Memory access timing"), which the dot-clocked
        // pixel pipeline can observe mid-cycle: stage rendering-register
        // writes with the PPU before ticking. The architectural commit
        // below is unchanged — `Ppu::stage_write` affects only the
        // pipeline's register view (mealybug m3_* mid-mode-3 writes).
        if let 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B = addr {
            // Per-register render-frame stage offsets: mid-mode-3
            // SCX/SCY/palette/WX/LCDC land at the render position
            // `stage_write_dots` picks.
            let dots = self.stage_write_dots(addr);
            self.ppu.stage_write(addr, value, dots);
        }
        self.tick_machine();
        // Write-conflict commit: a CGB single-speed WriteCpu-conflict engine
        // write (FF41 STAT / FF0F IF / FF45 LYC) commits its engine-visible
        // effect (`eng_stat`/`intf`/LYC compare) ONE T past the M-cycle boundary
        // in SameBoy (`GB_CONFLICT_WRITE_CPU`), not at the boundary where the
        // whole-M-cycle tick lands `write_no_tick`. At single speed 1 T = 1 dot,
        // so borrow the next M-cycle's first PPU dot here — running this dot's
        // engine tick (folding any co-instant STAT rise into `intf` FIRST)
        // before the write commits — then the next `tick_machine` skips cc 1 to
        // restore phase, landing `write_no_tick` at the WriteCpu dot (D+1).
        // Borrow only on the aligned whole-dot grid: an LCD-enable sub-dot
        // offset (`lcd_shift_dots != 0`) shifts the CPU/PPU grid, where a
        // whole-dot borrow mis-maps a co-instant STAT rise onto the wrong side
        // of the write (`lycwirq_trigger_*_lcdoffset1_1`).
        // DMG is single-speed with the same 4-dot M-cycle and 1-T WriteCpu
        // commit as CGB SS, so the identical whole-dot borrow re-hosts the DMG
        // FF0F-clear straddle (`m2int_m0irq_scx3_ifw_2`/`_4`,
        // `ly0/lycint152_lyc153irq_ifw_2`). Scoped to FF0F on DMG: the FF41/FF45
        // WriteCpu borrow recovers on CGB but is a net-negative A/B swap on DMG
        // (`m0enable/lycdisable_ff41_2`/`ff45_3` invert), so only the FF0F
        // IF-clear borrow crosses to DMG.
        let borrow_addr = if self.model.is_cgb() {
            matches!(addr, 0xFF0F | 0xFF41 | 0xFF45)
        } else {
            addr == 0xFF0F
        };
        let borrow = !self.double_speed && !self.ppu.lcd_shift_active() && borrow_addr;
        if borrow {
            let a = self.ppu.tick_half();
            let b = self.ppu.tick_half();
            self.fold_ppu_events(a | b, 1);
            self.eager_wr_borrow = true;
        }
        // Corruption first, then the (mode-blocked) write attempt — during
        // the scan the CPU byte never lands (oam_write_blocked).
        self.maybe_oam_bug(addr, OamBugKind::Write);
        self.check_access(addr, true);
        // Exception break: disabling the LCD outside vblank — sample the *old*
        // LCDC (`write_no_tick` commits the new one below).
        self.check_exc_lcd(addr, value);
        // A bit1-clearing FF0F write consumes a STAT engine rise landing within
        // the next 2 dots; with the borrow above the commit now sits at the
        // WriteCpu dot, so the same squash arm applies
        // (`lycint152_lyc153irq_ifw_2`). Shares the borrow's scope.
        //
        // Double-speed extension: at DS SameBoy's WriteCpu commits 1 T = half a
        // dot into the M-cycle, but the whole-M-cycle tick already lands
        // `write_no_tick` at the SAME dot (measured: `m2int_m0irq_scx{3,4}_ifw_ds`
        // commit dots match), so NO commit-dot borrow is needed — the DS mode-0
        // rise sits 1 to 2 dots past the write (`w=2` in `stat_update_tick`), not
        // co-instant, and the existing squash countdown consumes it (`_ds_2`
        // Δ1-2) while the `_ds_1` siblings' writes sit Δ3-4 and survive. Arm only
        // (no `tick_half`, no `eager_wr_borrow` repay). Same `!lcd_shift_active`
        // grid guard as SS.
        let ff0f_ds_squash =
            self.model.is_cgb() && self.double_speed && !self.ppu.lcd_shift_active();
        if (borrow || ff0f_ds_squash) && addr == 0xFF0F && value & 0x02 == 0 {
            self.ppu.arm_ff0f_if_squash();
        }
        self.write_no_tick(addr, value);
    }

    fn tick(&mut self) {
        // Deferred-commit clock: an internal M-cycle parks +4 without
        // committing (SameBoy `cycle_no_access`); the next access pays it.
        self.clock.internal();
        self.service_vram_dma();
        self.tick_machine();
    }

    fn tick_addr(&mut self, value: u16) {
        // Deferred-commit clock: the OAM-bug-carrying internal M-cycle (a
        // 16-bit register driven on the address bus) is SameBoy's
        // `cycle_oam_bug` (`sm83_cpu.c:326`), which — unlike `cycle_no_access`
        // — commits the previous debt at the leading edge and reparks 4, just
        // like a read. (Conserves the same 4 T as `internal`; the difference
        // is the commit *phase*, which matters once a later stage samples on
        // this cycle.)
        let _leading_edge = self.clock.read();
        self.service_vram_dma();
        self.tick_machine();
        self.maybe_oam_bug(value, OamBugKind::Write);
    }

    fn read_inc(&mut self, addr: u16) -> u8 {
        // Deferred-commit clock: same leading-edge read as `read`.
        let _leading_edge = self.clock.read();
        // Leading-edge sample (cc+0).
        let leading = self.leading_edge_sample(addr);
        // Mirror `Bus::read`: clear the one-shot carried-read peek once
        // the FF41 read has consumed it.
        if addr == 0xFF41 {
            self.ppu.set_read_carried(false);
            // The halt-woken re-fetch override is one-shot at the line
            // boundary: it survives the sub-boundary polls (`read_pos_hd` short
            // of the line) and clears once this read has crossed it (the same
            // read `vis_mode_read` resolved to mode 2).
            if self.ppu.halt_refetch_crossed() {
                self.ppu.set_halt_refetch(false);
            }
        }
        self.service_vram_dma();
        self.tick_machine();
        self.service_vram_dma(); // reads yield to a same-cycle trigger
        self.maybe_oam_bug(addr, OamBugKind::ReadIncrease);
        self.check_access(addr, false);
        let trailing = self.read_no_tick(addr);
        leading.unwrap_or(trailing)
    }

    /// Inert unless the live debugger enabled profiling — `prof` is `None` on
    /// every golden/test path, so this records nothing and the emulated state
    /// (and the fingerprint) is byte-identical.
    fn profile_pc(&mut self, pc: u16) {
        // CDL: mark the executed instruction's opcode byte as code (X=4). Operand
        // bytes are marked R by the fetch read path (acceptable over-approx).
        // `None` when the log is off → no-op, so golden is byte-identical.
        self.cdl_mark(pc, 4);
        if let Some(m) = &mut self.prof {
            let count = m.entry(pc).or_insert(0);
            let first_seen = *count == 0;
            *count += 1;
            // Break mode: remember an address's first execution so the free run
            // can halt there (consumed by `take_prof_break_hit`).
            if first_seen && self.prof_break {
                self.prof_break_hit = Some(pc);
            }
        }
    }

    /// Inert unless an opcode exception was armed (`exc_mask == 0` on every
    /// golden/test path), so this is byte-identical there.
    fn check_exec(&mut self, pc: u16, opcode: u8) {
        self.exec_exception(pc, opcode);
    }

    fn pending(&self) -> u8 {
        self.intf & self.ie & IF_MASK & !self.if_stat_late
    }

    fn pending_halt_wake_mid(&mut self) -> u8 {
        self.halt_wake_mid_impl()
    }

    fn pending_halt_wake(&self) -> u8 {
        self.halt_wake_impl()
    }

    fn ack(&mut self, bit: u8) {
        self.ack_impl(bit)
    }

    fn stop(&mut self, skipped_addr: u16, interrupt_pending: bool) -> bool {
        self.stop_impl(skipped_addr, interrupt_pending)
    }

    fn set_halted(&mut self, halted: bool) {
        self.set_cpu_halted(halted);
    }

    fn pending_halt_entry(&mut self) -> u8 {
        self.halt_entry_impl()
    }

    fn halt_entry_rewind(&mut self) -> bool {
        self.halt_entry_rewind_impl()
    }

    fn flush_pending(&mut self) {
        // Instruction boundary: drain the deferred-commit clock's parked debt
        // (SameBoy `flush_pending_cycles`), keeping `clock.now()` exact at
        // boundaries for the leading-edge sampling.
        self.clock.flush();
    }
}
