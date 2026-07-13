//! Deferred-commit cycle-clock + leading-edge read helpers.
//! A behaviour-preserving submodule of [`Interconnect`] (a second `impl` block
//! via `use super::*`); the `clock` / `leading_edge_reads` fields live in the
//! parent struct.

use super::*;

impl Interconnect {
    /// Leading-edge (cc+0) read value for a PPU-positional address, or
    /// `None` when the read should use the trailing cc+4 view (the flag is
    /// off, or the address is not PPU-positional). Pure (`&self`): called
    /// *before* `tick_machine`, so it samples the PPU at the M-cycle's
    /// leading edge. Today only FF41 (the kernel-pair mode read) is routed;
    /// OAM/VRAM/palette accessibility back-dating is not routed here. `Ppu::read`
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

    /// Enable leading-edge (cc+0) PPU-positional reads + the `StatUpdate`
    /// engine + the `vis_early` back-date (the whole flag-on path). Held off in
    /// production (`new` defaults it false) until the atomic flip; exposed
    /// through [`crate::GameBoy::set_leading_edge_reads`] for the kernel-pair
    /// acceptance spec + the gap-count measurement.
    pub(crate) fn set_leading_edge_reads(&mut self, on: bool) {
        self.leading_edge_reads = on;
        // Forward to the PPU so its StatUpdate engine takes over from
        // `stat_events_tick` on the same flag.
        self.ppu.set_leading_edge_reads(on);
    }

    /// Enable the deferred-commit machine advance + the −2 interrupt-dispatch
    /// reclock. Implies [`Self::set_leading_edge_reads`] — the cc+0 reads are
    /// the frame the reclock recalibrates against. Held off in production and in
    /// the kernel-pair specs (which set only `leading_edge`); set through
    /// [`crate::GameBoy::set_tier2_reclock`].
    pub(crate) fn set_tier2_reclock(&mut self, on: bool) {
        self.tier2_reclock = on;
        if on {
            self.set_leading_edge_reads(true);
            // The tier2 deferred clock and the eager-value clock are mutually
            // exclusive frames; enabling tier2 clears any armed eager so a
            // `set_tier2_reclock`-built machine runs PURE tier2 even when the
            // eager construction default is temp-flipped on (else the tier2
            // pins would run an incoherent tier2∧eager hybrid). Inert in
            // production (eager already off → no-op) — the shipped defaults are
            // both false, so the steady-state tier2 two-bin is unperturbed.
            self.eager_value = false;
            self.ppu.set_eager_value(false);
        }
        self.ppu.set_tier2_reclock(on);
    }

    /// Enable the **eager-value** reclock: the eager clock + counter-pinned
    /// dispatch (cc+4) + the tier2 read laws as cc+0 value peeks. Implies
    /// [`Self::set_leading_edge_reads`] but does NOT set `tier2_reclock` — no
    /// deferred clock, no −2 dispatch move → the DMG-count-safe foundation (see
    /// `docs/sameboy-port/tools/measurements/eager-clock-foundation-2026-07-07.md`).
    pub(crate) fn set_eager_value(&mut self, on: bool) {
        self.eager_value = on;
        self.set_leading_edge_reads(on);
        self.ppu.set_eager_value(on);
    }

    /// `(leading_edge_reads, tier2_reclock)` — read-only, for the golden-safe
    /// "production defaults OFF" guard test.
    #[cfg(test)]
    pub(crate) fn reclock_flags(&self) -> (bool, bool) {
        (self.leading_edge_reads, self.tier2_reclock)
    }

    /// Reproduce the C3-flip **construction default** exactly as flipping the
    /// `interconnect.rs` struct-literal `eager_value`/`leading_edge_reads` to
    /// `true` would: set only this struct's own fields, WITHOUT the
    /// PPU-propagation that [`Self::set_eager_value`] runs. That leaves the
    /// machine incoherent (eager reads, non-eager PPU) — the exact bug
    /// `GameBoy::post_boot_inner`'s deferred re-arm exists to repair. Only ever
    /// armed via the `cfg(test)`/`flip_hooks`-gated `GameBoy::new_with_eager`
    /// (production `post_boot_inner` always passes `eager_default = false`), so
    /// no shipped/golden path can reach it.
    pub(crate) fn arm_eager_construction_default(&mut self) {
        self.eager_value = true;
        self.leading_edge_reads = true;
    }

    /// Repay the outstanding sub-M-cycle wake skew — the next access pays the
    /// extra T and lands back on the aligned 4-T grid.
    pub(super) fn repay_wake_skew(&mut self) {
        self.clock.carry_read(std::mem::take(&mut self.wake_skew));
    }

    /// Deferred-commit read: pay the previous M-cycle's parked debt —
    /// advancing the whole machine to this M-cycle's leading edge (cc+0) — then
    /// sample. Unlike the eager [`Bus::read`] (which advances a full M-cycle
    /// *after* a single FF41 leading-edge peek and otherwise trails at cc+4),
    /// every read here samples at the leading edge, and the dispatch reclock's
    /// `pending=2` lands the vector/handler reads 2 dots early.
    ///
    /// The `GB_display_sync` analogue: `advance_machine_t` is T-granular and
    /// drives the PPU per 8 MHz half-dot (`Ppu::tick_half`), so by the time the
    /// sample below runs the PPU is resolved to the read's EXACT half-dot: a DS
    /// read landing on an odd CPU-T sits mid-dot (half-dot phase 1),
    /// exactly like SameBoy's zero-cycle force-run
    /// (`sync_ppu_if_needed → GB_display_run(gb, 0, true)`,
    /// memory.c:540 / display.h:51) draining the display coroutine to the
    /// read's T before returning `STAT&3`. The FF41/FF44/accessibility
    /// verdicts sampled here are therefore "as of that true half-dot"; the
    /// half-dot read-position laws compare [`Ppu::read_pos_hd`] (+ the per-ISR
    /// carry [`Ppu::isr_read_carry_hd`]) against half-dot boundaries. The
    /// frame mapping to SameBoy is `slopgb dot D ↔ SameBoy cfl D+4` (single
    /// speed) / `D+3` (double speed — the odd offset is why the mid-dot
    /// position is load-bearing there).
    pub(super) fn read_deferred(&mut self, addr: u16, kind: OamBugKind) -> u8 {
        // An outstanding sub-M-cycle wake skew is consumed by the handler's
        // FIRST FF41 read — that read samples the STAT mode at the wake's true
        // sub-M-cycle T (2 T early) — and REPAID before any other IO/PPU-
        // positional read, so the TIMA-counted (`int_hblank_halt`) and
        // LY-straddle (`hblank_ly_scx`) wake grids keep their aligned
        // calibration. One-shot; also repaid at the next halt entry
        // (`set_cpu_halted`) as the backstop.
        if self.tier2_reclock && self.wake_skew != 0 && addr & 0xFF80 == 0xFF00 && addr != 0xFF41 {
            // IO-page reads other than FF41 re-align first (ROM/RAM fetches
            // ride the skew — the handler's code path must not consume it).
            self.repay_wake_skew();
        }
        let before = self.clock.now();
        let _ = self.clock.read(); // clock += old pending; park 4
        self.advance_machine_t(before, self.clock.now());
        self.service_vram_dma();
        self.maybe_oam_bug(addr, kind);
        let v = self.read_no_tick(addr);
        // DMG power-on boot-frame read law: the tier2 deferred FF41/
        // FF44/OAM/VRAM read samples cc+0, 4 dots before production's cc+4 read
        // of the same `LD A,(nn)`, so a boot read straddling a mode transition
        // returns the pre-transition value; restore the read's true (cc+4)
        // verdict (`Ppu::boot_read`). Verdict-only, `!is_cgb`/first-frame scoped
        // → `None` (byte-identical) off the boot frame and in production.
        let v = self.ppu.boot_read(addr).unwrap_or(v);
        // FF0F read peek: the deferred IF read's verdict includes the
        // deterministically-imminent STAT engine rise SameBoy's events-first
        // read frame has already folded (see `Ppu::ff0f_stat_peek`).
        // Verdict-only: `intf` is untouched, the rise still folds at its own dot.
        let v = if addr == 0xFF0F {
            // The DMG mode-0 STAT-IF SERVICE-CLEAR: a deferred read that
            // crossed the counter-pinned mode-0 rise returns 0 when the STAT
            // interrupt is pending AND enabled (`intf & ie & STAT`) — the CPU is
            // servicing it, and on hardware the dispatch clears IF at the read's
            // cycle so the `ldh a,(FF0F)` observes 0 (`hblank_int_scx*_if_d`,
            // ISR CP A==0). The `intf & ie` gate is the discriminator vs a pure
            // poll of the same dot (`hblank_scx2_if_a`: DI + IE=0, no dispatch →
            // the bit stays set, want E2). See `Ppu::ff0f_dmg_service_clear`.
            if self.intf & self.ie & IF_STAT_BIT != 0 && self.ppu.ff0f_dmg_service_clear() {
                0
            } else {
                // The glitch-line mode-0 co-instant read-view mask (a
                // read landing on the flip dot precedes the rise on hardware;
                // `Ppu::ff0f_dmg_m0_coincident_mask`) joins the OAM-pulse mask;
                // both clear a bit slopgb's whole-dot frame folded a dot too
                // early.
                (v | self.ppu.ff0f_stat_peek())
                    & !self.ppu.ff0f_ly0_pulse_mask()
                    & !self.ppu.ff0f_dmg_m0_coincident_mask()
            }
        } else {
            v
        };
        // The SCOPED carried-read exit override is one-shot — clear the
        // arm after the STAT-ISR handler's first FF41 mode read has resolved (its
        // `vis_mode_read` already consumed `read_carried` inside `read_no_tick`).
        if addr == 0xFF41 {
            self.ppu.set_read_carried(false);
            // The wake skew is consumed by this read — repay it so everything
            // after runs on the aligned grid.
            if self.tier2_reclock && self.wake_skew != 0 {
                self.repay_wake_skew();
            }
        }
        v
    }

    /// Deferred-commit write: the value commits at the leading edge
    /// per its conflict class ([`Self::write_conflict`]), advancing the machine
    /// by the class's pre-commit split.
    pub(super) fn write_deferred(&mut self, addr: u16, value: u8) {
        // A CPU write to any LCD register (FF40-FF4B) ends the pristine
        // boot hand-off frame, so the DMG boot-frame read law (`Ppu::boot_read`)
        // no longer applies. The `poweron_*` ROMs read the untouched boot frame
        // (pure NOP sled, no PPU write); every other early reader configures the
        // PPU first — `lcdon_to_*`/`oam_read`/`sprite`/`win` turn the LCD off/on
        // (FF40), the gambatte kernel/halt STAT-ISR tests arm a mode interrupt
        // (FF41) — and reads its own frame at cc+0. Boot's own register install
        // goes through the direct `ppu.write`/`write_no_tick` paths, not this
        // CPU write path, so it does not trip the flag.
        if matches!(addr, 0xFF40..=0xFF4B) {
            self.ppu.mark_lcd_reg_written();
        }
        let conflict = self.write_conflict(addr);
        self.vram_dma_req_pre = self.vram_dma_req.is_some();
        let before = self.clock.now();
        let _ = self.clock.write(conflict);
        self.advance_machine_t(before, self.clock.now());
        // A racing DMA-register write beats a same-advance
        // HBlank-DMA steal: SameBoy runs `GB_hdma_run` only after the
        // current instruction completes (sm83_cpu.c:1718), so the write's
        // store is visible to the block (`hdma_late_destl_1`:
        // SameBoy order w54 → run dst=8010; the deferred head-service ran
        // the block with the stale dst=8000; likewise `_length`/`_wrambank`/
        // `_disable`). SCOPED to the registers the block itself consumes
        // (FF51-FF55 counters/arm + FF70 WRAM bank + FF4F VRAM bank): the
        // steal defers past their store. A GENERAL post-store service was
        // measured to break `irq_precedence/hdma_vs_m0_scx2_halt` (a
        // base-passing row) and 60+ hdma rows in the first cut; a request
        // already pending at the op's entry still steals first even for
        // the scoped registers. Production (eager) untouched: its head
        // service runs before the write's own tick flags the request.
        let defer_steal = self.cgb_mode && matches!(addr, 0xFF51..=0xFF55 | 0xFF70 | 0xFF4F);
        if !defer_steal || self.vram_dma_req_pre {
            self.service_vram_dma();
        }
        if let 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B = addr {
            // Stage the mid-mode-3 pipeline-view commit at SameBoy's render
            // frame: the deferred clock lands the eager `commit_eff` 4 dots
            // EARLY of the render's cc+4-calibrated fetch grid, so each register
            // re-commits after its per-register offset (the `staged_pending`
            // survive skip in `Ppu::write` keeps the stage alive). The offsets
            // and their per-register rationale live in [`Self::stage_write_dots`]
            // (SCX +4, SCX-DS +2, SCY/DMG-palette even-parity, WX +0). Production
            // keeps the gambatte mid-cycle staging ({2 SS, 1 DS}) — byte-identical
            // OFF; glitch lines commit immediately (no deferred fetch grid).
            let dots = self.stage_write_dots(addr);
            self.ppu.stage_write(addr, value, dots);
        }
        self.maybe_oam_bug(addr, OamBugKind::Write);
        // A bit1-clearing FF0F write consumes a STAT engine rise landing within
        // the next 2 dots (see `Ppu::arm_ff0f_if_squash` + the consumption site
        // in `stat_update_tick`).
        if addr == 0xFF0F && value & 0x02 == 0 {
            self.ppu.arm_ff0f_if_squash();
        }
        self.write_no_tick(addr, value);
        if defer_steal {
            self.service_vram_dma();
        }
    }

    /// The per-register mid-mode-3 write-commit stage offset (in dots),
    /// shared by the tier2 deferred write path ([`Self::write_deferred`]) and
    /// the eager-value write path ([`crate::interconnect::Bus`]`::write`). A
    /// pure function of `addr`/`scan_pos`/`speed`; the tier2 render-frame
    /// offsets also apply under `eager_value` (for the tier2 path
    /// `tier2_reclock` is already true, so `|| eager_value` is a no-op →
    /// byte-identical there).
    pub(super) fn stage_write_dots(&self, addr: u16) -> u8 {
        if let (0xFF43, true, true) = (
            addr,
            (self.tier2_reclock || self.eager_value) && !self.ppu.glitch_active(),
            self.double_speed,
        ) {
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
        } else if (self.tier2_reclock || self.eager_value)
            && !self.model.is_cgb()
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
        } else if (self.tier2_reclock || self.eager_value)
            && addr == 0xFF42
            && !self.double_speed
            && !self.ppu.glitch_active()
        {
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
        } else if (self.tier2_reclock || self.eager_value)
            && matches!(addr, 0xFF42 | 0xFF43 | 0xFF47..=0xFF49)
            && !self.ppu.glitch_active()
        {
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
        } else if (self.tier2_reclock || self.eager_value)
            && addr == 0xFF4B
            && !self.ppu.glitch_active()
        {
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

    /// Deferred-commit internal M-cycle (`cycle_no_access`): park
    /// +4 and advance nothing now — the debt is paid by the next real access.
    pub(super) fn tick_deferred(&mut self) {
        let before = self.clock.now();
        self.clock.internal();
        self.advance_machine_t(before, self.clock.now()); // delta 0 (deferred)
        self.service_vram_dma();
    }

    /// Deferred-commit `cycle_oam_bug` (`tick_addr`): commits the
    /// previous debt at the leading edge and reparks 4, like a read.
    pub(super) fn tick_addr_deferred(&mut self, value: u16) {
        let before = self.clock.now();
        let _ = self.clock.read();
        self.advance_machine_t(before, self.clock.now());
        self.service_vram_dma();
        self.maybe_oam_bug(value, OamBugKind::Write);
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
