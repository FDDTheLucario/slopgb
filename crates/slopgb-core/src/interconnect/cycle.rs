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
    /// eager-value write path ([`crate::interconnect::Bus`]`::write`). A pure
    /// function of `addr`/`scan_pos`/`speed`.
    pub(super) fn stage_write_dots(&self, addr: u16) -> u8 {
        if let (0xFF43, true, true) = (addr, !self.ppu.glitch_active(), self.double_speed) {
            // SCX in DOUBLE SPEED takes a +2 render-frame defer, not single
            // speed's +4 (dots=3): the DS M-cycle is 2 PPU dots (vs 4), so
            // the write-commit-to-fetch-grid offset halves. dots=2 fixes the
            // 5 `scx_during_m3_ds` fine-scroll pixel legs AND holds
            // `late_scx4`'s DS read law (the fine-scroll comparator
            // straddle) — a swept dots=1 broke the read law
            // (`tier2_late_scx_writestrobe`), dots=3 broke the render.
            // SCY/palette keep dots=3 in DS (no DS pixel legs, and their
            // timing never reaches an OCR verdict).
            2
        } else if !self.model.is_cgb()
            && matches!(addr, 0xFF47..=0xFF49)
            && !self.ppu.glitch_active()
        {
            // The DMG palette (BGP/OBP FF47-49) commit anchors to the EVEN
            // (CPU-M-cycle) dot grid, resolving the sub-dot render POP grid
            // that the whole-dot defer=3 could not. SameBoy commits the
            // palette at the write M-cycle's exact half-dot and the pixel
            // pops at a half-dot; single speed is whole-dot aligned so the
            // write commit lands at a whole (EVEN) dot, from which the pop is
            // visible +2 dots. The tier2 deferred write's leading edge
            // (`scan_pos().1` — the machine already advanced there above) is
            // whole-dot but loses which side of the even grid it sits on: an
            // ODD leading edge means the M-cycle boundary rounds up one dot
            // so the commit is visible +3 (round_up_even(LE)+2), an EVEN one
            // +2. The mealybug BGP/OBP legs land EVEN leading edges (want +2
            // — a flat +3 renders the change one column late), the gambatte
            // dmgpalette legs ODD (want +3). DMG only — CGB has no FF47-49
            // render path (its palettes are FF68-6B) and no BGP OR-quirk, so
            // it keeps the plain +3. Render-only (pure colour selection, no
            // mode-3-length or FF41-read-law coupling): production
            // byte-identical OFF, CGB two-bin unaffected.
            2 + (self.ppu.scan_pos().1 & 1) as u8
        } else if addr == 0xFF42 && !self.double_speed && !self.ppu.glitch_active() {
            // SCY (FF42) commit takes the same EVEN-dot parity anchor as the
            // DMG palette, resolving the sub-dot render-fetch grid the
            // whole-dot defer=3 could not. On a sprite-stalled line the
            // ~11-dot OBJ fetch shifts the BG fetch grid so a tile's Lo/Hi
            // data read (`bg_tile_addr`, fine row = LY+SCY & 7) lands EXACTLY
            // on the deferred SCY-commit dot: SameBoy/production commits the
            // write at the M-cycle's mid-point (its true half-dot, visible +2
            // from an EVEN leading edge / +3 from an ODD one — the same
            // round_up_even(LE)+2 the palette derives), so a per-tile data
            // read straddling it re-samples the NEW scroll while the
            // already-latched tile NUMBER keeps the old (the mealybug
            // m3_scy_change mixed-fetch behaviour). The flat defer=3 rendered
            // the SCY change one column late on `scy_during_m3_spx08_2` (the
            // sprite-stalled scy leg). The objectless scy_during_m3_{1,4,5,6}
            // writes land ODD leading edges (want +3 — a flat +2 broke all
            // 8); the sprite leg lands an EVEN one (want +2). SCY is pure row
            // selection — no mode-3-length or FF41-read-law coupling (those
            // sample ARCH `self.scy`) — so this is render-only, production
            // byte-identical OFF. SS only (the DS M-cycle is 2 dots; SCY has
            // no DS pixel legs and DS keeps defer=3, the `else` below).
            2 + (self.ppu.scan_pos().1 & 1) as u8
        } else if matches!(addr, 0xFF42 | 0xFF43 | 0xFF47..=0xFF49) && !self.ppu.glitch_active() {
            // SCX takes the full +4 render-frame deferral (visible from
            // `step(L+4)`): PROVEN by late_scx4 SS+DS + scx_m3_extend —
            // the fine-scroll comparator hunt (dots 89-96) is calibrated
            // to the production cc+4 frame, 4 dots late of the deferred
            // write's true instant, so the pipeline-view SCX must lag the
            // same 4 dots for the straddle pairs to separate. LCDC +4 was
            // BUILT + MEASURED NET-NEGATIVE (A/B-inverts the
            // sprites/late_sizechange pairs and pushes the late_disable
            // pre-draw aborts into post-draw); WX/WY likewise keep the
            // production frame (late_wx/late_wy `_1` legs) — write-vs-
            // render-event races already compare in a consistent frame,
            // only the hunt's absolute-dot anchor needs the lag. The
            // per-register split mirrors SameBoy's per-register conflict
            // maps (each register carries its own commit class).
            3
        } else if addr == 0xFF4B && !self.ppu.glitch_active() {
            // WX (FF4B) render-VIEW defer: `eff.wx` (the window
            // activation/reactivation comparator) now survives the arch
            // write (see `regs.rs` `staged_pending`) and strobe-commits at
            // the production frame — leading+2 at BOTH speeds — instead of
            // the leading edge (cc+0), where the eager commit landed the WX
            // change 2-4 dots early of the render's per-dot WX comparator
            // (`late_wx_ds` DS: the eager cc+0 WX=255 pre-empted the wx=7
            // window activation → bare; m3_wx_6 SS: the un-catch straddle
            // needs the change to split the two `pos_dot==wx+6` compares).
            // The un-catch READ law (`wx_write_dot`, FF41 mode-3 length) keeps
            // its cc+0 input in `regs.rs` (the split). stage_write adds the
            // FF4B +1 (WX one dot later than the palette class) → final 2:
            // leading+2 == production. Render-only, byte-identical OFF.
            0
        } else if self.double_speed {
            1
        } else {
            2
        }
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
