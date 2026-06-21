//! Deferred-commit ("lazy-advance") CPU clock — the validated foundation for
//! the SameBoy cycle-exact timing port (Stage S1 of `docs/sameboy-port/PORT-PLAN.md`).
//!
//! **This module is committed but not yet wired into the live `Bus`.** It is
//! the executable, unit-tested encoding of SameBoy 1.0.2's `pending_cycles`
//! deferred-commit clock (`sm83_cpu.c`), the load-bearing primitive the floor
//! lift depends on. Today the live core uses tick-then-access (a read samples
//! peripheral state at the M-cycle's *trailing* edge, cc+4); this clock samples
//! at the *leading* edge (cc+0) and defers the M-cycle's own 4 T-cycles, which
//! is what lands a STAT/OAM/VRAM read on the correct side of a mode-3→mode-0
//! boundary (`docs/sameboy-port/cpu-timing-map.md` §2, §7). Wiring it is the
//! atomic Stage S2+S3 of the port (NOT net-zero — the PPU boundary dots shift to
//! SameBoy's frame together), so it stays inert here, validated against the
//! spec's worked numbers, until that stage lands.
//!
//! Model (CPU T-cycles, 4 = one M-cycle, in both speeds — the double-speed
//! factor is applied once, centrally, only to the PPU/APU domain, never here;
//! `cpu-timing-map.md` §5): a bus op (1) advances the *previous* M-cycle's
//! `pending` debt, (2) samples/commits at the now-current clock (the leading
//! edge of *this* M-cycle), (3) parks a fresh debt for this M-cycle's own
//! cycles. `flush` drains the debt at the instruction boundary.
#![allow(dead_code)] // Inert staged-port foundation; see the module doc above.

/// SameBoy's per-IO-write conflict classes (`sm83_cpu.c:131-318`). Each splits
/// the M-cycle's 4 T-cycles into a pre-commit advance and a re-parked debt so
/// the *sub-M-cycle* commit point varies while the per-M-cycle total is
/// conserved. Only the cases needed to validate the conservation invariant are
/// modelled here; the full per-model maps land with Stage S6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Conflict {
    /// `GB_CONFLICT_READ_OLD` (`sm83_cpu.c:131`): plain write, commits at the
    /// leading edge like a read, re-parks 4. The component reads the OLD value.
    ReadOld,
    /// `GB_CONFLICT_READ_NEW` (`:137`): advance `pending-1`, re-park 5 — the
    /// write lands 1 T early, the component reads the NEW value.
    ReadNew,
    /// `GB_CONFLICT_WRITE_CPU` (`:143`): advance `pending+1`, re-park 3 — the
    /// CPU wins a same-cycle write (e.g. IF), landing 1 T late.
    WriteCpu,
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
    /// reparks its class total, which still conserves the per-M-cycle 4 for the
    /// `ReadOld` class S1 uses (the `ReadNew`/`WriteCpu` ±1 splits assume the
    /// real-flow `pending >= 1` and are not wired until S6).
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

#[cfg(test)]
#[path = "cycle_clock_tests.rs"]
mod tests;
