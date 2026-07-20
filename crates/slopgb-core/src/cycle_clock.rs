//! Deferred-commit ("lazy-advance") CPU clock: the executable encoding of
//! SameBoy 1.0.2's `pending_cycles` clock (`sm83_cpu.c`).
//!
//! A bus access samples at the *leading* edge (cc+0) and defers the M-cycle's
//! own 4 T-cycles — what lands a STAT/OAM/VRAM read on the correct side of a
//! mode-3→mode-0 boundary. `interconnect::Bus` drives this park/flush
//! bookkeeping (`read`/`write`/`internal`/`flush`) while sampling PPU state
//! directly at the access.
//!
//! Model (CPU T-cycles, 4 = one M-cycle, in both speeds — the double-speed
//! factor is applied once, centrally, only to the PPU/APU domain, never
//! here): a bus op (1) advances the *previous* M-cycle's
//! `pending` debt, (2) samples/commits at the now-current clock (the leading
//! edge of *this* M-cycle), (3) parks a fresh debt for this M-cycle's own
//! cycles. `flush` drains the debt at the instruction boundary.

/// SameBoy's per-IO-write conflict classes (`sm83_cpu.c:131-318`). Each splits
/// the M-cycle's 4 T-cycles into a pre-commit advance and a re-parked debt so
/// the *sub-M-cycle* commit point varies while the per-M-cycle total is
/// conserved. `Interconnect::write_conflict` holds the per-model address→class
/// maps; SameBoy's two-stage classes collapse to their final value-write phase.
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
    /// `Interconnect::write_conflict` routes only WX_DMG here: the LCDC
    /// tile-sel glitch is value-dependent (`(~value & old) & TILE_SEL`) and
    /// can't be decided from the address alone, so CGB LCDC takes `ReadOld`.
    WxHold,
}

/// The deferred-commit clock. `clock` is the running CPU T-cycle position;
/// `pending` is the debt of the current M-cycle not yet advanced.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CycleClock {
    clock: u64,
    /// Debt of the current M-cycle not yet advanced. `flush` drains it every
    /// instruction (SameBoy holds it in a byte), but the `Bus` is also driven
    /// *standalone* by memory/blocking unit tests that run hundreds of M-cycles
    /// unflushed, so this is a `u32` to stay overflow-safe there
    /// ([`Self::internal`] still traps a genuine runaway loudly).
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

    /// The current committed clock position (CPU T-cycles). Test-only
    /// introspection (the conservation-invariant unit tests).
    #[cfg(test)]
    pub(crate) fn now(&self) -> u64 {
        self.clock
    }

    /// Outstanding debt (this M-cycle's un-advanced cycles). Test-only
    /// introspection (the conservation-invariant unit tests).
    #[cfg(test)]
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
    /// pre-commit split is exact (`sm83_cpu.c:115` asserts this). The `Bus` is
    /// also driven *standalone* by memory/blocking unit tests (no preceding
    /// fetch, `pending == 0`); the `saturating_sub` below keeps that case
    /// underflow-safe — it commits at the current clock and still conserves the
    /// per-M-cycle 4. `Bus::write` discards the returned commit position; the
    /// advance/re-park split is the live effect.
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

    /// `flush_pending_cycles` (`sm83_cpu.c:336`): drain the debt and park 0;
    /// called at every instruction boundary.
    pub(crate) fn flush(&mut self) {
        self.clock += u64::from(self.pending);
        self.pending = 0;
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
