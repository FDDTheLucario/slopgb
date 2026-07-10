//! `impl Bus for Interconnect`: the CPU-facing bus contract. Each
//! read/write/tick advances every peripheral one M-cycle then performs
//! the access; plus interrupt dispatch/ack, halt-wake, STOP, the
//! dispatch reclock, and the instruction-boundary clock flush. Oracle:
//! full mooneye + gbtr matrix.

use super::*;

impl Bus for Interconnect {
    fn read(&mut self, addr: u16) -> u8 {
        if self.tier2_reclock {
            // The deferred-commit reclock advances the machine to
            // this M-cycle's leading edge before sampling.
            return self.read_deferred(addr, OamBugKind::Read);
        }
        // Deferred-commit clock: pay the previous M-cycle's parked debt
        // and park this read's 4 T-cycles.
        let _leading_edge = self.clock.read();
        // Latch the leading-edge (cc+0) value for PPU-positional reads
        // *before* the PPU advances. Inert while the flag is off (`None`).
        let leading = self.leading_edge_sample(addr);
        // The eager-value carried-read peek (armed at the STAT ack in
        // `ack_impl`) is one-shot: `leading_edge_sample`'s FF41 read has now
        // consumed `read_carried` inside `vis_mode_read`, so clear it (the
        // tier2 twin clears in `read_deferred`). Never set flag-off → no-op.
        // Under `read_true_t` the FF41 read trails at cc+4 (below), so the clear
        // moves after `read_no_tick` — the trailing read must still see the arm.
        if self.eager_value && addr == 0xFF41 && !self.read_true_t {
            self.ppu.set_read_carried(false);
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
        if self.read_true_t && addr == 0xFF41 {
            self.ppu.set_read_carried(false);
        }
        leading.unwrap_or(trailing)
    }

    fn write(&mut self, addr: u16, value: u8) {
        if self.tier2_reclock {
            return self.write_deferred(addr, value);
        }
        // Deferred-commit clock: a write commits per its per-model
        // conflict class (`write_conflict`, the SameBoy `cycle_write` map).
        // The commit position is still discarded — write-only scaffold —
        // so swapping `ReadOld` for the real class is byte-identical; the
        // architectural-commit move that consumes it lands later.
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
            // Under `eager_value` the tier2 per-register render-frame stage
            // offsets apply on the eager clock (mid-mode-3 SCX/SCY/palette/WX/
            // LCDC land at the tier2 render position); off (production +
            // tier2-off) this stays byte-identical to the gambatte {2 SS, 1 DS}
            // mid-cycle staging.
            let dots = if self.eager_value {
                self.stage_write_dots(addr)
            } else if self.double_speed {
                1
            } else {
                2
            };
            self.ppu.stage_write(addr, value, dots);
        }
        self.tick_machine();
        // Corruption first, then the (mode-blocked) write attempt — during
        // the scan the CPU byte never lands (oam_write_blocked).
        self.maybe_oam_bug(addr, OamBugKind::Write);
        self.check_access(addr, true);
        // Exception break: disabling the LCD outside vblank — sample the *old*
        // LCDC (`write_no_tick` commits the new one below).
        self.check_exc_lcd(addr, value);
        self.write_no_tick(addr, value);
    }

    fn tick(&mut self) {
        if self.tier2_reclock {
            return self.tick_deferred();
        }
        // Deferred-commit clock: an internal M-cycle parks +4 without
        // committing (SameBoy `cycle_no_access`); the next access pays it.
        self.clock.internal();
        self.service_vram_dma();
        self.tick_machine();
    }

    fn tick_addr(&mut self, value: u16) {
        if self.tier2_reclock {
            return self.tick_addr_deferred(value);
        }
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
        if self.tier2_reclock {
            return self.read_deferred(addr, OamBugKind::ReadIncrease);
        }
        // Deferred-commit clock: same leading-edge read as `read`.
        let _leading_edge = self.clock.read();
        // Leading-edge sample (cc+0), inert while the flag is off.
        let leading = self.leading_edge_sample(addr);
        // Mirror `Bus::read`: clear the one-shot eager carried-read peek once
        // the FF41 read has consumed it (the tier2 twin clears both paths in
        // `read_deferred`). Never set flag-off → no-op. Under `read_true_t` the
        // clear moves after the trailing cc+4 read (as in `Bus::read`).
        if self.eager_value && addr == 0xFF41 && !self.read_true_t {
            self.ppu.set_read_carried(false);
        }
        self.service_vram_dma();
        self.tick_machine();
        self.service_vram_dma(); // reads yield to a same-cycle trigger
        self.maybe_oam_bug(addr, OamBugKind::ReadIncrease);
        self.check_access(addr, false);
        let trailing = self.read_no_tick(addr);
        if self.read_true_t && addr == 0xFF41 {
            self.ppu.set_read_carried(false);
        }
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

    fn dispatch_reclock(&self) -> bool {
        // EXPERIMENT: the eager-native CGB −2 dispatch retime (the machine
        // advance is skipped in `dispatch_retime_impl` under `eager_value` — see
        // there). CGB-scoped so DMG dispatch stays cc+4 (intr_2 count-safe).
        self.tier2_reclock || (self.coherent_dispatch && self.model.is_cgb())
    }

    fn dispatch_retime(&mut self) {
        self.dispatch_retime_impl()
    }

    fn pending_halt_entry(&mut self) -> u8 {
        self.halt_entry_impl()
    }

    fn halt_entry_rewind(&mut self) -> bool {
        self.halt_entry_rewind_impl()
    }

    fn pending_dispatch(&mut self) -> u8 {
        self.dispatch_pending_impl()
    }

    fn flush_pending(&mut self) {
        if self.tier2_reclock {
            // Drain the parked debt AND advance the machine to
            // catch up, so the deferred −2 read shift is reabsorbed at the
            // instruction boundary (SameBoy `flush_pending_cycles`).
            let before = self.clock.now();
            self.clock.flush();
            self.advance_machine_t(before, self.clock.now());
            return;
        }
        // Instruction boundary: drain the deferred-commit clock's parked
        // debt (SameBoy `flush_pending_cycles`). Net-zero — the clock is
        // write-only scaffold; this only keeps `clock.now()` exact at
        // boundaries for the leading-edge port.
        self.clock.flush();
    }
}
