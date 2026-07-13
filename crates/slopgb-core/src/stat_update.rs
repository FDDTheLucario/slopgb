//! SameBoy `GB_STAT_update` rising-edge STAT-interrupt line — wired into the
//! PPU as `Ppu.stat_update`, the production STAT-line tracker.
//!
//! SameBoy keeps a single **level** — `stat_interrupt_line` = the OR of the one
//! mode source selected by [`mode_for_interrupt`](crate::ppu) and the LYC
//! source — and fires `IF |= STAT` only on its **0→1 rising edge**
//! (`display.c:523-560`). That is the classic STAT-blocking model: while the
//! line is already high from one source, a second source going high produces
//! no new interrupt. (This replaced an earlier gambatte-derived per-source
//! event engine; the fork collapse removed that alternate path, so this is now
//! the sole STAT IRQ source.)
//!
//! This module is the executable, unit-tested encoding of that rising-edge
//! core. It is a pure function of the *decoupled* interrupt mode (the
//! [`Ppu::mode_for_interrupt`](crate::ppu) field) plus the STAT register's
//! enable bits and the LYC match — exactly the inputs SameBoy reads. It drives
//! the STAT line every dot (`stat_update_tick`).

/// `mode_for_interrupt == -1`: the deliberate "no mode source" state SameBoy
/// uses to force the STAT line low between transitions (`display.c:1799`,
/// stored as `0xFF` in the `uint8_t` field). Selecting it makes the mode
/// source contribute nothing; only LYC can hold the line high.
pub(crate) const MODE_FOR_INTERRUPT_NONE: u8 = 0xFF;

/// STAT register source-enable bits (FF41 bits 3-6), the only bits
/// [`StatUpdate::update`] consults (`gb.h` / `display.c:545-556`).
const STAT_EN_HBLANK: u8 = 0x08; // bit 3 — mode 0 source
const STAT_EN_VBLANK: u8 = 0x10; // bit 4 — mode 1 source
const STAT_EN_OAM: u8 = 0x20; // bit 5 — mode 2 source
const STAT_EN_LYC: u8 = 0x40; // bit 6 — LY==LYC source

/// SameBoy's edge-detected STAT interrupt line (`stat_interrupt_line`,
/// `gb.h:569`). Holds the current level; [`Self::update`] recomputes it and
/// reports the 0→1 rising edge that raises `IF` bit 1.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StatUpdate {
    /// The last computed line level — the edge is detected against this.
    line: bool,
}

impl StatUpdate {
    /// A fresh line, low (LCD-off / power-on state).
    pub(crate) fn new() -> Self {
        Self { line: false }
    }

    /// The current STAT interrupt line level.
    pub(crate) fn line(&self) -> bool {
        self.line
    }

    /// Silently force the line level (no edge detection). Used by the
    /// shifted-ROM FF45-commit re-latch: the engine registered a latch drop one
    /// step before the write landed (SameBoy's line never fell there), so the
    /// corrected level must not edge-fire on the next tick.
    pub(crate) fn force_level(&mut self, level: bool) {
        self.line = level;
    }

    /// The level the STAT line *would* hold for these inputs, without mutating
    /// or edge-detecting — the pure OR of the selected mode source and the LYC
    /// source (`display.c:545-556`).
    ///
    /// * `mode_for_interrupt` — the decoupled interrupt-facing mode (0/1/2, or
    ///   [`MODE_FOR_INTERRUPT_NONE`] for "no source"); any other value selects
    ///   no mode source, matching SameBoy's `default:` arm.
    /// * `stat` — the FF41 register byte (only the enable bits 3-6 are read).
    /// * `lyc_match` — whether the *delayed* `ly_for_comparison` equals LYC
    ///   (SameBoy keeps this as `lyc_interrupt_line`; the caller supplies it so
    ///   this stays a pure function of the STAT-line inputs).
    pub(crate) fn level(mode_for_interrupt: u8, stat: u8, lyc_match: bool) -> bool {
        let mode_source = match mode_for_interrupt {
            0 => stat & STAT_EN_HBLANK != 0,
            1 => stat & STAT_EN_VBLANK != 0,
            2 => stat & STAT_EN_OAM != 0,
            _ => false, // mode 3 / NONE: no mode source (display.c default:)
        };
        let lyc_source = stat & STAT_EN_LYC != 0 && lyc_match;
        mode_source || lyc_source
    }

    /// Recompute the line and report whether this is a 0→1 rising edge — i.e.
    /// whether `GB_STAT_update` would raise `IF` bit 1 (`display.c:557-559`).
    /// Returns `true` exactly on the transition from low to high; a line that
    /// is already high (a second source joining) returns `false` (STAT
    /// blocking), and a line going high again after a fall re-fires.
    pub(crate) fn update(&mut self, mode_for_interrupt: u8, stat: u8, lyc_match: bool) -> bool {
        let previous = self.line;
        self.line = Self::level(mode_for_interrupt, stat, lyc_match);
        self.line && !previous
    }
}

// --- Save state (manual serialization; see `crate::state`) ---
impl StatUpdate {
    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        w.bool(self.line);
    }
    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.line = r.bool()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "stat_update_tests.rs"]
mod tests;
