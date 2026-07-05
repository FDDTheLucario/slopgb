//! Deferred-commit ("lazy-advance") CPU clock — the validated foundation for
//! the SameBoy cycle-exact timing port.
//!
//! **This module is committed but not yet wired into the live `Bus`.** It is
//! the executable, unit-tested encoding of SameBoy 1.0.2's `pending_cycles`
//! deferred-commit clock (`sm83_cpu.c`), the load-bearing primitive the floor
//! lift depends on. Today the live core uses tick-then-access (a read samples
//! peripheral state at the M-cycle's *trailing* edge, cc+4); this clock samples
//! at the *leading* edge (cc+0) and defers the M-cycle's own 4 T-cycles, which
//! is what lands a STAT/OAM/VRAM read on the correct side of a mode-3→mode-0
//! boundary. Wiring it is the atomic reclock stage of the port (NOT net-zero —
//! the PPU boundary dots shift to SameBoy's frame together), so it stays inert
//! here, validated against the spec's worked numbers, until that stage lands.
//!
//! Model (CPU T-cycles, 4 = one M-cycle, in both speeds — the double-speed
//! factor is applied once, centrally, only to the PPU/APU domain, never
//! here): a bus op (1) advances the *previous* M-cycle's
//! `pending` debt, (2) samples/commits at the now-current clock (the leading
//! edge of *this* M-cycle), (3) parks a fresh debt for this M-cycle's own
//! cycles. `flush` drains the debt at the instruction boundary.
#![allow(dead_code)] // Inert port foundation; see the module doc above.

/// SameBoy's per-IO-write conflict classes (`sm83_cpu.c:131-318`). Each splits
/// the M-cycle's 4 T-cycles into a pre-commit advance and a re-parked debt so
/// the *sub-M-cycle* commit point varies while the per-M-cycle total is
/// conserved. Only the cases needed to validate the conservation invariant are
/// modelled here; the remaining per-model maps are not yet modelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Conflict {
    /// `GB_CONFLICT_READ_OLD` (`sm83_cpu.c:131`): plain write, commits at the
    /// leading edge like a read, re-parks 4. The component reads the OLD value.
    ReadOld,
    /// `GB_CONFLICT_READ_NEW` (`:137`): advance `pending-1`, re-park 5 — the
    /// write lands 1 T early, the component reads the NEW value.
    ReadNew,
    /// `GB_CONFLICT_WRITE_CPU` (`:143`): advance `pending+1`, re-park 3 — the
    /// CPU wins a same-cycle write (e.g. IF), landing 1 T late. Also the clock
    /// phase of the two-stage `GB_CONFLICT_STAT_*` classes (`:150,170,180`),
    /// whose final value write lands at the same `+1` point (the intermediate
    /// `0xFF`/masked write is a memory effect deferred to a later stage).
    WriteCpu,
    /// Advance `pending-2`, re-park 6 — the write commits 2 T early.
    /// `GB_CONFLICT_PALETTE_CGB` on model ≥ CGB-D (`:205`) and
    /// `GB_CONFLICT_SCX_DMG_AND_CGB_DOUBLE` (`:297`). (Model < CGB-D palette is
    /// the 1-T-early `ReadNew` phase instead.)
    EarlyTwo,
    /// `GB_CONFLICT_WX_DMG` (`:262`) and the `GB_CONFLICT_LCDC_CGB` tile-sel
    /// glitch (`:271`): the value commits at the leading edge (like `ReadOld`),
    /// then one extra T elapses — the `wx_just_changed` / `tile_sel_glitch`
    /// one-T window — before re-parking 3. So the running clock advances past
    /// the commit while only 3 T stay parked, conserving the per-M-cycle 4.
    /// Today [`Interconnect::write_conflict`] routes only WX_DMG here; the
    /// value-dependent LCDC tile-sel glitch (`((~value & old) & TILE_SEL)`)
    /// can't be decided from the address alone, so CGB LCDC stays `ReadOld`
    /// until its memory effect lands in a later stage.
    WxHold,
}

/// The deferred-commit clock. `clock` is the running CPU T-cycle position;
/// `pending` is the debt of the current M-cycle not yet advanced.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CycleClock {
    clock: u64,
    /// Debt of the current M-cycle not yet advanced. In real CPU flow this is
    /// tiny — `flush` drains it every instruction (≤ ~24 T), so SameBoy holds
    /// it in a byte. The clock is embedded in an `Interconnect` whose `Bus`
    /// is also driven *standalone* by memory/blocking unit tests that advance
    /// hundreds of M-cycles without an instruction-boundary flush; a `u32`
    /// keeps the parked debt from overflowing across those long unflushed
    /// runs (the overflow ceiling is then unreachable, not a `u8`'s 63 ticks)
    /// while [`Self::internal`] still traps a genuine runaway loudly.
    pending: u32,
}

impl CycleClock {
    /// Fresh clock at T-cycle 0 with no debt (the state at instruction entry,
    /// after `flush_pending_cycles`; `sm83_cpu.c:1718`).
    pub(crate) fn new() -> Self {
        Self {
            clock: 0,
            pending: 0,
        }
    }

    /// The current committed clock position (CPU T-cycles).
    pub(crate) fn now(&self) -> u64 {
        self.clock
    }

    /// Outstanding debt (this M-cycle's un-advanced cycles).
    pub(crate) fn pending(&self) -> u32 {
        self.pending
    }

    /// `cycle_read` / baseline `cycle_write` (`sm83_cpu.c:85`, `:131`): pay the
    /// previous M-cycle's debt, return the clock position the byte is sampled
    /// at (the LEADING edge of this M-cycle, cc+0), then park this M-cycle's 4.
    pub(crate) fn read(&mut self) -> u64 {
        self.clock += u64::from(self.pending);
        let sample = self.clock;
        self.pending = 4;
        sample
    }

    /// A conflict-staged IO write (`sm83_cpu.c:113`). Returns the clock position
    /// the value commits at; advances the pre-commit split and re-parks per the
    /// class, conserving the per-M-cycle total of 4.
    ///
    /// In real CPU flow a write is never an instruction's first access — a
    /// fetch always parks debt first — so `pending >= 1` and every class's
    /// pre-commit split is exact (`sm83_cpu.c:115` asserts this). The clock is
    /// nonetheless embedded in a [`crate::interconnect::Interconnect`] whose
    /// `Bus::write` is also driven *standalone* by memory/blocking unit tests
    /// (no preceding fetch, `pending == 0`); the `saturating_sub` below keeps
    /// that case underflow-safe in release rather than panicking. A standalone
    /// write commits at the current clock (`pending == 0` → no advance) and
    /// reparks its class total, which still conserves the per-M-cycle 4. Every
    /// class's commit position is nonetheless still **discarded** by the live
    /// `Bus::write` (`interconnect.rs`); the per-model class *lookup*
    /// ([`Interconnect::write_conflict`]) is already wired (byte-identical,
    /// the clock is write-only scaffold), and the architectural-commit *move*
    /// that consumes the position lands in a later stage.
    pub(crate) fn write(&mut self, conflict: Conflict) -> u64 {
        let repark = match conflict {
            Conflict::ReadOld => {
                self.clock += u64::from(self.pending);
                4
            }
            Conflict::ReadNew => {
                // -1 T: the write lands 1 T early (component reads NEW value).
                self.clock += u64::from(self.pending.saturating_sub(1));
                5
            }
            Conflict::WriteCpu => {
                // +1 T: the CPU wins a same-cycle write, landing 1 T late.
                self.clock += u64::from(self.pending) + 1;
                3
            }
            Conflict::EarlyTwo => {
                // -2 T: the write commits 2 T early (PALETTE_CGB≥D / SCX).
                self.clock += u64::from(self.pending.saturating_sub(2));
                6
            }
            Conflict::WxHold => {
                // The value commits at the leading edge, then one extra T
                // elapses before re-parking 3 (the wx_just_changed /
                // tile_sel_glitch window). Return the leading-edge commit, not
                // the post-window clock — the value lands at `clock`, the +1
                // is dead time the next M-cycle inherits via the short repark.
                self.clock += u64::from(self.pending);
                let commit = self.clock;
                self.clock += 1;
                self.pending = 3;
                return commit;
            }
        };
        let commit = self.clock;
        self.pending = repark;
        commit
    }

    /// `cycle_no_access` (`sm83_cpu.c:321`): an internal execution M-cycle that
    /// touches no bus — park +4, advance nothing now (the debt is paid by the
    /// next real access).
    pub(crate) fn internal(&mut self) {
        // `flush` drains the debt every instruction, so a real CPU never
        // approaches the u32 ceiling — but trap a genuine runaway (a missing
        // flush that lets debt accumulate without bound) loudly rather than
        // silently wrapping.
        self.pending = self
            .pending
            .checked_add(4)
            .expect("pending debt overflow — flush missing");
    }

    /// Per-ISR read-position carry: add `t` CPU T-cycles of extra
    /// parked debt to be paid (advanced) by the next bus op *before* it samples,
    /// shifting that read — and every subsequent handler read — `t` T later
    /// WITHOUT moving the current clock position (the IF-ack latch already
    /// committed). Used by `dispatch_retime` to carry the OAM-IRQ source's
    /// sub-M-cycle phase into the ISR handler reads, decoupled from the dispatch
    /// dot. Inert unless `SLOPGB_M2CARRY`.
    pub(crate) fn carry_read(&mut self, t: u32) {
        self.pending = self
            .pending
            .checked_add(t)
            .expect("pending debt overflow — flush missing");
    }

    /// The sub-M-cycle WAKE clock: commit `t` of the parked
    /// debt NOW — advance the clock by `t` and reduce the debt by the same,
    /// conserving the per-M-cycle total. The DMG halt loop samples the wake
    /// condition mid-M-cycle (SameBoy `GB_cpu_run`, `sm83_cpu.c:1621-1628`:
    /// advance-2 → sample → advance-2); committing the first 2 T before the
    /// sample lands the sample — and, on a wake, the whole subsequent
    /// dispatch + handler read stream — at the rise's sub-M-cycle T until
    /// the machine re-aligns. The remaining debt is paid by the next bus op
    /// as usual.
    pub(crate) fn advance_pending(&mut self, t: u32) {
        debug_assert!(t <= self.pending, "advance_pending exceeds parked debt");
        let t = t.min(self.pending);
        self.clock += u64::from(t);
        self.pending -= t;
    }

    /// Drop `t` of the parked debt WITHOUT advancing — the un-run
    /// tail of a halt idle iteration that woke at its mid-M-cycle sample.
    /// SameBoy's DMG halt loop iterations are genuinely 2 T long
    /// (`GB_advance_cycles(gb, 2)`), so a wake at the mid sample means the
    /// remaining 2 T of the idle "M-cycle" never happen: the resumed CPU's
    /// next op starts at the wake T and every subsequent M-cycle boundary
    /// carries the 2-T offset — the sub-M-cycle resume the wake clock
    /// needs. Only the CPU↔machine alignment shifts; machine time itself
    /// stays continuous.
    pub(crate) fn forgive(&mut self, t: u32) {
        debug_assert!(t <= self.pending, "forgive exceeds parked debt");
        self.pending -= t.min(self.pending);
    }

    /// `flush_pending_cycles` (`sm83_cpu.c:336`): drain the debt and park 0;
    /// called at every instruction boundary.
    pub(crate) fn flush(&mut self) {
        self.clock += u64::from(self.pending);
        self.pending = 0;
    }

    /// The interrupt-dispatch vector retime (`sm83_cpu.c:1690-1692`):
    /// `pending -= 2; flush; pending = 2` — the IF-ack / vector latch lands 2 T
    /// before the final push M-cycle completes. Returns the latch clock.
    pub(crate) fn dispatch_vector_retime(&mut self) -> u64 {
        assert!(
            self.pending > 2,
            "dispatch retime needs pending > 2 (SameBoy sm83_cpu.c:1689)"
        );
        self.pending -= 2;
        self.flush();
        let latch = self.clock;
        self.pending = 2;
        latch
    }
}

// --- Save state (manual serialization; see `crate::state`) ---
impl CycleClock {
    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        w.u64(self.clock);
        w.u32(self.pending);
    }
    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.clock = r.u64()?;
        self.pending = r.u32()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "cycle_clock_tests.rs"]
mod tests;
