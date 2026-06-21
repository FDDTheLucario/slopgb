//! Deferred-commit cycle-clock + leading-edge read helpers (port Stage S1/S2a).
//! A behaviour-preserving submodule of [`Interconnect`] (a second `impl` block
//! via `use super::*`); the `clock` / `leading_edge_reads` fields live in the
//! parent struct. See `docs/sameboy-port/PORT-PLAN.md`.

use super::*;

impl Interconnect {
    /// S2a leading-edge (cc+0) read value for a PPU-positional address, or
    /// `None` when the read should use the trailing cc+4 view (the flag is
    /// off, or the address is not PPU-positional). Pure (`&self`): called
    /// *before* `tick_machine`, so it samples the PPU at the M-cycle's
    /// leading edge. Today only FF41 (the kernel-pair mode read) is routed;
    /// OAM/VRAM/palette accessibility back-dating lands at S4. `Ppu::read`
    /// is side-effect-free (`ppu/regs.rs`).
    pub(super) fn leading_edge_sample(&self, addr: u16) -> Option<u8> {
        if !self.leading_edge_reads {
            return None;
        }
        match addr {
            0xFF41 => Some(self.ppu.read(0xFF41)),
            _ => None,
        }
    }

    /// Test/probe hook: enable leading-edge (cc+0) PPU-positional reads. Held
    /// off in production until the S2d atomic flip; flipped here only by the
    /// S2 unit tests and the S2d gap-count measurement.
    #[cfg(test)]
    pub(crate) fn set_leading_edge_reads(&mut self, on: bool) {
        self.leading_edge_reads = on;
        // Forward to the PPU so its S5 StatUpdate engine takes over from
        // `stat_events_tick` on the same flag.
        self.ppu.set_leading_edge_reads(on);
    }

    /// Test-only view of the deferred-commit CPU clock's committed position
    /// (CPU T-cycles). Used to assert the S1 net-zero conservation invariant
    /// (`clock.now()` after a boundary flush == 4 × M-cycles executed).
    #[cfg(test)]
    pub(crate) fn cpu_clock_t(&self) -> u64 {
        self.clock.now()
    }

    /// Test-only view of the clock's outstanding (un-flushed) parked debt.
    #[cfg(test)]
    pub(crate) fn cpu_clock_pending(&self) -> u32 {
        self.clock.pending()
    }
}
