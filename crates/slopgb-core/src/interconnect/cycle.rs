//! Deferred-commit cycle-clock + leading-edge read helpers.
//! A behaviour-preserving submodule of [`Interconnect`] (a second `impl` block
//! via `use super::*`); the `clock` / `leading-edge` fields live in the
//! parent struct.

use super::*;

impl Interconnect {
    /// Leading-edge (cc+0) read value for a PPU-positional address, or
    /// `None` when the address is not PPU-positional. Pure (`&self`): called
    /// *before* `tick_machine`, so it samples the PPU at the M-cycle's
    /// leading edge. Today only FF41 (the kernel-pair mode read) is routed;
    /// OAM/VRAM/palette accessibility back-dating is not routed here. `Ppu::read`
    /// is side-effect-free (`ppu/regs.rs`).
    pub(super) fn leading_edge_sample(&self, addr: u16) -> Option<u8> {
        match addr {
            0xFF41 => Some(self.ppu.read(0xFF41)),
            _ => None,
        }
    }

    /// SameBoy's per-model write-conflict class for an IO write (`cycle_write`,
    /// `sm83_cpu.c:113`, + the four conflict maps `:31-82`). Selects the
    /// clock phase the deferred-commit [`crate::cycle_clock::CycleClock::write`]
    /// re-parks with — keyed on the hardware **model** (CGB-family incl. AGB,
    /// SGB-family, else DMG) and double speed, exactly as SameBoy's map
    /// selection (`sm83_cpu.c:120-127`), **not** `cgb_mode`.
    ///
    /// The value-/PPU-state-dependent sub-cases keep their *default* clock class
    /// here: the `LCDC_CGB`/`DMG_LCDC` tile-sel & object-fetch glitches resolve
    /// to `ReadOld`/`ReadNew` (their value-dependent `WxHold` glitch branch and
    /// the intermediate masked write are memory effects handled elsewhere),
    /// and the two-stage `STAT_*`/`PALETTE_*` classes collapse to their final
    /// value-write phase (`WriteCpu`/`ReadNew`/`EarlyTwo`). The result is
    /// discarded by `Bus::write` today, so this is byte-identical in both flag
    /// states (the commit position is not yet consumed).
    pub(super) fn write_conflict(&self, addr: u16) -> Conflict {
        // Only the IO page FF00-FF7F conflicts; everything else reads old.
        if addr & 0xFF80 != 0xFF00 {
            return Conflict::ReadOld;
        }
        let reg = addr & 0x7F;
        if self.model.is_cgb() {
            if self.double_speed {
                // cgb_double_conflict_map (sm83_cpu.c:44). LCDC
                // (LCDC_CGB_DOUBLE), LYC, WY, NR10, WX all share the ReadOld
                // clock phase; SCX commits 2 T early.
                match reg {
                    0x0F | 0x41 => Conflict::WriteCpu, // IF, STAT_CGB_DOUBLE
                    0x43 => Conflict::EarlyTwo,        // SCX
                    _ => Conflict::ReadOld,
                }
            } else {
                // cgb_conflict_map (sm83_cpu.c:31). LCDC (LCDC_CGB), WY, SCX
                // share the ReadOld clock phase.
                match reg {
                    0x0F | 0x45 | 0x41 | 0x4B => Conflict::WriteCpu, // IF, LYC, STAT_CGB, WX
                    // PALETTE_CGB: ≥ CGB-D commits 2 T early, < CGB-D 1 T early.
                    // Model::Cgb is CGB-C (< D); Model::Agb is ≥ D.
                    0x47..=0x49 => {
                        if self.model == Model::Agb {
                            Conflict::EarlyTwo
                        } else {
                            Conflict::ReadNew
                        }
                    }
                    _ => Conflict::ReadOld,
                }
            }
        } else if matches!(self.model, Model::Sgb | Model::Sgb2) {
            // sgb_conflict_map (sm83_cpu.c:71). LYC, WY are ReadOld.
            match reg {
                0x0F | 0x41 => Conflict::WriteCpu, // IF, STAT_DMG
                0x40 | 0x42 | 0x47 | 0x48 | 0x49 => Conflict::ReadNew, // SGB_LCDC, SCY, BGP/OBP
                0x4B => Conflict::WxHold,          // WX_DMG
                0x43 => Conflict::EarlyTwo,        // SCX
                _ => Conflict::ReadOld,
            }
        } else {
            // dmg_conflict_map (sm83_cpu.c:56) — Dmg0/Dmg/Mgb. LYC, WY are
            // ReadOld.
            match reg {
                0x0F | 0x41 => Conflict::WriteCpu, // IF, STAT_DMG
                0x40 | 0x42 | 0x47 | 0x48 | 0x49 => Conflict::ReadNew, // DMG_LCDC, SCY, PALETTE_DMG
                0x4B => Conflict::WxHold,          // WX_DMG
                0x43 => Conflict::EarlyTwo,        // SCX
                _ => Conflict::ReadOld,
            }
        }
    }

    /// The per-register mid-mode-3 write-commit stage offset (in dots) for the
    /// eager-value write path ([`crate::interconnect::Bus`]`::write`). Delegates
    /// to [`crate::ppu::Ppu::stage_write_dots`] with the live double-speed flag —
    /// the offset is a pure function of `addr` / `scan_pos` / speed, so the PPU
    /// render-test harness shares the exact same computation.
    pub(super) fn stage_write_dots(&self, addr: u16) -> u8 {
        self.ppu.stage_write_dots(addr, self.double_speed)
    }

    /// Test-only view of the deferred-commit CPU clock's committed position
    /// (CPU T-cycles). Used to assert the net-zero conservation invariant
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
