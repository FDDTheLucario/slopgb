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
    /// the intermediate masked write are memory effects deferred to Stage S6),
    /// and the two-stage `STAT_*`/`PALETTE_*` classes collapse to their final
    /// value-write phase (`WriteCpu`/`ReadNew`/`EarlyTwo`). The result is
    /// discarded by `Bus::write` today, so this is byte-identical in both flag
    /// states (the commit position is consumed only at Stage S6).
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

    /// Port validation hook: enable leading-edge (cc+0) PPU-positional reads +
    /// the S5 `StatUpdate` engine + the `vis_early` back-date (the whole flag-on
    /// path). Held off in production (`new` defaults it false) until the S2d
    /// atomic flip; exposed through [`crate::GameBoy::set_leading_edge_reads`]
    /// for the S0 kernel-pair acceptance spec + the S2d gap-count measurement.
    pub(crate) fn set_leading_edge_reads(&mut self, on: bool) {
        self.leading_edge_reads = on;
        // Forward to the PPU so its S5 StatUpdate engine takes over from
        // `stat_events_tick` on the same flag.
        self.ppu.set_leading_edge_reads(on);
    }

    /// Port Stage B (Tier 2) hook: enable the deferred-commit machine advance
    /// (B1) + the −2 interrupt-dispatch reclock (B2/B3). Implies
    /// [`Self::set_leading_edge_reads`] — the cc+0 reads are the frame the
    /// reclock recalibrates against. Held off in production and in the S0
    /// kernel-pair specs (which set only `leading_edge`); the make-or-break
    /// thesis measurement sets it through [`crate::GameBoy::set_tier2_reclock`].
    pub(crate) fn set_tier2_reclock(&mut self, on: bool) {
        self.tier2_reclock = on;
        if on {
            self.set_leading_edge_reads(true);
        }
        self.ppu.set_tier2_reclock(on);
    }

    /// Port Stage B (Tier 2) deferred-commit read: pay the previous M-cycle's
    /// parked debt — advancing the whole machine to this M-cycle's leading edge
    /// (cc+0) — then sample. Unlike the eager [`Bus::read`] (which advances a
    /// full M-cycle *after* a single FF41 leading-edge peek and otherwise
    /// trails at cc+4), every read here samples at the leading edge, and the
    /// dispatch reclock's `pending=2` lands the vector/handler reads 2 dots
    /// early.
    pub(super) fn read_deferred(&mut self, addr: u16, kind: OamBugKind) -> u8 {
        let before = self.clock.now();
        let pend_dbg = self.clock.pending(); // C2 cc-exact: the debt paid before this read
        let _ = self.clock.read(); // clock += old pending; park 4
        self.advance_machine_t(before, self.clock.now());
        self.service_vram_dma();
        self.maybe_oam_bug(addr, kind);
        let v = self.read_no_tick(addr);
        // S5 read-dot tracer: line slopgb's deferred FF41 read dot up against
        // SameBoy's `read_high_memory` cfl (`SLOPGB_S5DBG`; byte-identical when
        // unset). FF41 reads are infrequent, so the gate check is cheap here.
        if addr == 0xFF41 && crate::ppu::s5dbg_on() {
            let (line, dot) = self.ppu.scan_pos();
            if line < 144 {
                let (wa, ve, lrd, vh, vm, ns) = self.ppu.dbg_read_state();
                eprintln!(
                    "SLOPGB ff41 ly={line} dot={dot} mode={} pend={pend_dbg} wa={wa} ve={ve} lrd={lrd} vh={vh} vm={vm} ns={ns}",
                    v & 3
                );
            }
        }
        // S5 IF-delivery tracer: the m1/lycEnable family observes the STAT-vs-
        // vblank IRQ delivery by reading FF0F (IF), not FF41 — the FF41 tracer
        // is blind to them. NOT gated to `ly < 144`: the vblank-entry reads
        // that matter land at ly 143–153 (`SLOPGB_S5DBG`, byte-identical unset).
        if addr == 0xFF0F && crate::ppu::s5dbg_on() {
            let (line, dot) = self.ppu.scan_pos();
            eprintln!("SLOPGB ff0f ly={line} dot={dot} if={:02x}", v & 0x1f);
        }
        // S5 accessibility read-dot tracer (mech-1 read-observer, OAM/VRAM
        // postread families): logs the deferred OAM (FE00-FE9F) / VRAM
        // (8000-9FFF) read's dot + value (0xFF = blocked) on visible lines, the
        // counterpart to SameBoy's OAM/VRAM-read instrumentation. `SLOPGB_S5DBG`,
        // byte-identical when unset.
        if matches!(addr, 0xFE00..=0xFE9F | 0x8000..=0x9FFF) && crate::ppu::s5dbg_on() {
            let (line, dot) = self.ppu.scan_pos();
            if line < 144 {
                let kind = if addr < 0xA000 { "vram" } else { "oam" };
                eprintln!("SLOPGB {kind} ly={line} dot={dot} v={v:02x}");
            }
        }
        v
    }

    /// Port Stage B deferred-commit write: the value commits at the leading edge
    /// per its conflict class ([`Self::write_conflict`]), advancing the machine
    /// by the class's pre-commit split.
    pub(super) fn write_deferred(&mut self, addr: u16, value: u8) {
        let conflict = self.write_conflict(addr);
        let before = self.clock.now();
        // S5/C2 write-frame tracer: the FF41/FF45 register-write's leading-edge
        // (cc+0) dot vs its commit (cc+4) dot — the write-side analogue of the
        // `read_deferred` FF41 read tracer. `SLOPGB_S5DBG`, byte-identical unset.
        let le_pos = if matches!(addr, 0xFF41 | 0xFF45) && crate::ppu::s5dbg_on() {
            Some(self.ppu.scan_pos())
        } else {
            None
        };
        let _ = self.clock.write(conflict);
        self.advance_machine_t(before, self.clock.now());
        if let Some((lly, ldot)) = le_pos {
            let (cly, cdot) = self.ppu.scan_pos();
            eprintln!(
                "SLOPGB w{addr:04x} val={value:02x} lead ly={lly} dot={ldot} commit ly={cly} dot={cdot}"
            );
        }
        self.service_vram_dma();
        if let 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B = addr {
            let dots = if self.double_speed { 1 } else { 2 };
            // C2 #11ab window-trigger tracer: the LCDC (FF40) window-enable and
            // WY (FF4A) writes drive SameBoy's `wy_check` (window mode-3
            // extension). Lines slopgb's write (line, dot) up against SameBoy's
            // `SBWLCDC`/`SBWWY` to diagnose the late_wy window-trigger / LCDC
            // frame-phase residual (`measurements/s7-readclock-refuted-2026-06-28.md`).
            // NOT `ly < 144`-gated: the VBlank LCDC-enable/disable writes are the
            // frame-phase evidence. `SLOPGB_S5DBG`, byte-identical when unset.
            if matches!(addr, 0xFF40 | 0xFF4A) && crate::ppu::s5dbg_on() {
                let (l, d) = self.ppu.scan_pos();
                eprintln!("SLOPGB w{addr:04x} val={value:02x} ly={l} dot={d}");
            }
            self.ppu.stage_write(addr, value, dots);
        }
        self.maybe_oam_bug(addr, OamBugKind::Write);
        self.write_no_tick(addr, value);
    }

    /// Port Stage B deferred-commit internal M-cycle (`cycle_no_access`): park
    /// +4 and advance nothing now — the debt is paid by the next real access.
    pub(super) fn tick_deferred(&mut self) {
        let before = self.clock.now();
        self.clock.internal();
        self.advance_machine_t(before, self.clock.now()); // delta 0 (deferred)
        self.service_vram_dma();
    }

    /// Port Stage B deferred-commit `cycle_oam_bug` (`tick_addr`): commits the
    /// previous debt at the leading edge and reparks 4, like a read.
    pub(super) fn tick_addr_deferred(&mut self, value: u16) {
        let before = self.clock.now();
        let _ = self.clock.read();
        self.advance_machine_t(before, self.clock.now());
        self.service_vram_dma();
        self.maybe_oam_bug(value, OamBugKind::Write);
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
