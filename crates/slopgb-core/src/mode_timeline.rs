//! Decoupled visible-mode / interrupt-mode timeline — the PPU-side half of the
//! finer-resolution event-phase model (the half-dot grid re-clocked toward
//! SameBoy's cycle-exact frame, the class-A floor's documented lift mechanism:
//! "re-clock observable event commits to a [finer] grid").
//!
//! Companion to [`crate::cycle_clock`] (the CPU-side deferred-commit clock).
//! **Test-only cross-check oracle** (`#[cfg(test)]`): the live PPU never
//! consults it — it keeps its whole-dot `vis_mode` + joint
//! `m0_src`/`m0_rise_dot` anchor — so this encodes the finer-resolution
//! timeline purely as an independent check the unit tests compare against
//! (see `ppu/mod_tests/stat.rs`).
//!
//! Scope: the **mode-2 / mode-3 / mode-0 spine of a visible line** (the kernel
//! pair lives entirely in it). The line-start mode-0/1 window (dots 0-3), the
//! VBlank lines, and SameBoy's `mode_for_interrupt == -1` "no source" gaps
//! between transitions (`display.c:1799`) are out of scope here — they are
//! handled by the live STAT engine and are not needed to separate the kernel.
//!
//! It captures the two structural degrees of freedom the whole-dot model folds
//! together (`docs/sameboy-port/ppu-timing-map.md` §2, §6):
//!
//! 1. **Two separate fields** — the CPU-visible STAT mode (`io[STAT]&3`) and
//!    the interrupt-facing `mode_for_interrupt` — updated on *different* dots
//!    (SameBoy `display.c:545-559`, `gb.h:612`).
//! 2. **Opposite-sign anchor offsets** — the mode-2 STAT IRQ fires **1 dot
//!    before** the visible mode→2 edge on lines 1-143 (`display.c:1787` vs
//!    `1792`, the "OAM int 1 T-cycle early" glitch — *except on line 0*,
//!    `display.c:1778`), while the mode-0 STAT IRQ fires **1 dot after** the
//!    visible mode→0 edge (`display.c:2108` vs `2091`).
//!
//! Those two facts resolve the `m2int_m3stat` / `m0int_m3stat` kernel pair with
//! **no CPU-call-stack discriminator**: the 2-dot relative swing between the
//! anchors places the two equal-latency `ldh a,(FF41)` reads on opposite sides
//! of a mode-3→0 boundary. This is the executable proof the whole-dot
//! "irreducible" verdict was a model-coarseness artifact, not a hardware fact.
#![allow(dead_code)] // Inert staged-port foundation; see the module doc above.

/// PPU mode as read through FF41 bits 0-1.
pub(crate) const MODE_HBLANK: u8 = 0;
pub(crate) const MODE_OAM: u8 = 2;
pub(crate) const MODE_XFER: u8 = 3;

/// Mode-2 (OAM scan) length in dots (`display.c:158`); also the dot the visible
/// STAT mode reads 2 from (the spine start).
const MODE2_LENGTH: u16 = 80;
/// Minimum mode-3 length, no objects / no window (`display.c:1493`,
/// `167 + (SCX & 7)`).
const MODE3_BASE: u16 = 167;

/// One visible line's mode-2/3/0 spine on the finer (sub-dot-faithful) grid,
/// parameterised by the line number (for the line-0 mode-2-IRQ exception) and
/// the SCX fine scroll + any sprite/window penalty the line renders with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModeTimeline {
    line: u16,
    /// Mode-3 extension: `SCX & 7` plus sprite/window penalty dots.
    mode3_extra: u16,
}

impl ModeTimeline {
    /// A bare line `line` (no sprites, no window): mode 3 = `167 + (SCX & 7)`.
    pub(crate) fn bare(line: u16, scx: u8) -> Self {
        Self {
            line,
            mode3_extra: u16::from(scx & 7),
        }
    }

    /// A line whose mode 3 is extended by `penalty` further dots (sprite fetch
    /// stalls, a window start) on top of the SCX fine scroll.
    pub(crate) fn with_penalty(line: u16, scx: u8, penalty: u16) -> Self {
        Self {
            line,
            mode3_extra: u16::from(scx & 7) + penalty,
        }
    }

    /// The dot the **visible** STAT mode flips 3→0 (SameBoy `display.c:2091`
    /// step A) — the same dot a CPU FF41 read first sees mode 0. Equal to
    /// `247 + (SCX & 7)` on a bare line (`80 + 167`).
    pub(crate) fn visible_mode0_dot(&self) -> u16 {
        MODE2_LENGTH + MODE3_BASE + self.mode3_extra
    }

    /// The dot the **mode-0 STAT IRQ** fires (`display.c:2108` step C) — one
    /// dot *after* the visible edge.
    pub(crate) fn mode0_irq_dot(&self) -> u16 {
        self.visible_mode0_dot() + 1
    }

    /// The mode-2 STAT IRQ's dot offset from this line's visible mode→2 edge
    /// (dot `MODE2_LENGTH`-relative; the spine starts at dot 0 reading mode 2).
    /// Lines 1-143 fire it **1 dot early** (`display.c:1787`, the "OAM int 1
    /// T-cycle before STAT" glitch); **line 0 does not** (`display.c:1778`
    /// "except on line 0") so the offset is 0 there.
    pub(crate) fn mode2_irq_offset(&self) -> i16 {
        if self.line == 0 { 0 } else { -1 }
    }

    /// The CPU-visible STAT mode at `dot` (what an FF41 read returns) over the
    /// spine.
    pub(crate) fn visible_mode(&self, dot: u16) -> u8 {
        if dot < MODE2_LENGTH {
            MODE_OAM
        } else if dot < self.visible_mode0_dot() {
            MODE_XFER
        } else {
            MODE_HBLANK
        }
    }

    /// The interrupt-facing mode at `dot` (`mode_for_interrupt`). It diverges
    /// from [`Self::visible_mode`] in the one-dot window the mode-0 anchor
    /// straddles: the visible mode is already 0 there, but the mode-0 IRQ has
    /// not yet fired, so the interrupt side still holds mode 3. (The mode-2
    /// early-fire is expressed through [`Self::mode2_irq_offset`], not here.)
    pub(crate) fn mode_for_interrupt(&self, dot: u16) -> u8 {
        if dot < MODE2_LENGTH {
            MODE_OAM
        } else if dot < self.mode0_irq_dot() {
            // Mode 3 holds for the interrupt side until the IRQ dot — one dot
            // past where the visible byte already reads 0.
            MODE_XFER
        } else {
            MODE_HBLANK
        }
    }
}

#[cfg(test)]
#[path = "mode_timeline_tests.rs"]
mod tests;
