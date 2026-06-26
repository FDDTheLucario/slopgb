//! S5/tier2 StatUpdate-driver: the leading-edge / `tier2_reclock` flag-on STAT
//! IRQ engine (SameBoy `GB_STAT_update` port) — the `stat_update_tick`
//! rising-edge dispatch + vblank/OAM direct pokes, the halt-commit masks, the
//! decoupled `mode_for_interrupt` derivation, and the delayed
//! `ly_for_comparison` LYC input. Second `impl Ppu` block split out of
//! `stat_irq.rs` for the CLAUDE.md <1000-line cap; flag-OFF production never
//! runs this path — see the parent `stat_irq.rs` for the legacy gambatte event
//! engine.

use super::*;

impl Ppu {
    /// Port Stage S5 — SameBoy `GB_STAT_update` (`display.c:523`), the flag-on
    /// replacement for [`Self::stat_events_tick`]. There is a single STAT
    /// interrupt *line* — the OR of the one mode source selected by
    /// `mode_for_interrupt` and the LYC source — and `IF |= STAT` fires only on
    /// its 0→1 rising edge (the classic STAT-blocking model: a second source
    /// joining an already-high line raises nothing). Driven by
    /// [`StatUpdate`](crate::stat_update); the LYC input is the
    /// [`Self::lyc_interrupt_line`] latch, re-evaluated from
    /// [`Self::ly_for_comparison`] whenever that is a real line and held across
    /// the `-1` gaps (`display.c:534-544`).
    ///
    /// The OAM-DMA mode-2 guard (`display.c:526`) clears only the *visible*
    /// STAT mode bits, not the IRQ line (which reads `mode_for_interrupt`), so
    /// it does not affect this rising edge; the visible-mode side is handled by
    /// the interconnect's OAM-DMA freeze. The LCD-off guard is the tick's
    /// `!enabled` early-return, which holds the line low.
    ///
    /// Still uses [`Self::refresh_cmp`] for the *readable* FF41 mode/LYC bits so
    /// register reads stay identical to the flag-off path; only the IRQ event
    /// source changes. The per-source emission masks (`stat_late` /
    /// `stat_halt_late` / `m0_rise`) that the gambatte engine sets for the
    /// halt-wake interaction have no `GB_STAT_update` equivalent — they are part
    /// of the remaining atomic-flip work and are left unset here (so the flag-on
    /// path does not yet reproduce the halt-late commit timing).
    pub(super) fn stat_update_tick(&mut self) {
        // Keep the readable comparison/mode flags + the legacy level current
        // (FF41 reads, the write-edge baseline) exactly as the flag-off path.
        self.refresh_cmp(true);
        // Drain the one-shot mode-0 event flag the gambatte engine would have
        // consumed this dot, so it does not leak into a later flag-off tick.
        let _ = std::mem::take(&mut self.m0_rise_dot);
        // `lyc_interrupt_line` latch: re-evaluate only when `ly_for_comparison`
        // names a real line; hold across the `-1` gaps (`display.c:534`).
        let ly = self.ly_for_comparison();
        // Mech 3 root 2 (S5) — the line-start LYC-carryover hold. SameBoy
        // re-evaluates `lyc_interrupt_line` only at the `GB_SLEEP` steps that set
        // `ly_for_comparison` (`display.c:1811` state-6 `= -1` holds; `:1830`
        // state-7 `= N` re-latch) — NOT during the held carryover before state-6,
        // where it still names the previous line (lines 1-143, dots 0-2 = `line
        // - 1`). A late FF45 write whose new LYC equals that carryover raises no
        // fresh edge (writes land at state-7, `lyfc=-1`/`0`); slopgb's per-dot
        // engine re-latched it → a spurious `ly1 dot0` (`got=E2`, want E0). Hold
        // like the `-1` gap (a legit LYC=N-1 tail is already latched true at line
        // N-1). DMG-family only (CGB lcd-offset banked); LE/Tier-2 only. Detail:
        // `m1lyc-ifdelivery-groundtruth-2026-06-25.md` "#11l".
        let line_start_carryover =
            !self.model.is_cgb() && (1..=143).contains(&self.line) && self.dot <= 2;
        if ly != -1 && !line_start_carryover {
            self.lyc_interrupt_line = ly == i16::from(self.lyc);
        }
        // Mech 3 root 1 (S5 engine-driver) — the vblank-entry LYC-latch drop.
        // A held visible-line LYC match (e.g. LYC=143 carried high from line 143)
        // stays latched across line 144's `ly_for_comparison == -1` line-start
        // gap, so the STAT line never dips at vblank entry — and when
        // `mode_for_interrupt` flips to the VBlank (mode-1) source at dot 4 the
        // fall of LYC fuses into the rise of mode-1, producing no fresh 0→1 edge
        // (the missing m1 re-arm: gambatte `m1/lycint143_m1irq_*` read if=01,
        // want if=03 — the serviced ly143 LYC-STAT bit is never restored).
        // SameBoy releases the latch at vblank entry (measured `SBLEVEL ly=144
        // cfl=0 lyc_line 1->0` then `0->1 mfi=1`, IF|=2): the line dips, then the
        // mode-1 source re-arms a fresh edge. Drop ONLY a held-true match that no
        // longer applies at line 144 (the pure carry-release); never force-set a
        // fresh match here — for LYC=144 the latch is set by the natural dot-4
        // `ly_for_comparison` re-evaluation, and front-running it to dot 0 would
        // suppress the LYC-source edge those rows need (`m1/m1irq_enable_after_
        // lyc144_*`). Gate on the VBlank (mode-1) source being ENABLED: SameBoy
        // drops the latch unconditionally, but the line only re-rises into a
        // fresh edge when mode 1 is armed to take over at dot 4 — the dip-and-
        // rise this whole-dot model reproduces. With mode 1 disabled SameBoy's
        // line dips and stays low (no IF), which a whole-dot drop would mis-frame
        // against the deferred read (`m1/lyc143_late_m0enable_lycdisable_*`,
        // VBlank off). LE/Tier-2 only — `stat_update_tick` never runs flag-off,
        // so production is byte-identical.
        if self.line == 144
            && self.dot == 0
            && self.lyc_interrupt_line
            && i16::from(self.lyc) != 144
            && self.stat_en & STAT_SRC_VBLANK != 0
        {
            self.lyc_interrupt_line = false;
        }
        let mfi = self.mode_for_interrupt;
        if self
            .stat_update
            .update(mfi, self.stat_en, self.lyc_interrupt_line)
        {
            self.pending_if |= IF_STAT;
            if super::s5dbg_on() && self.line < 144 {
                eprintln!(
                    "SLOPGB dispatch ly={} dot={} mfi={}",
                    self.line, self.dot, mfi
                );
            }
            self.stat_update_halt_masks(mfi);
        }
        self.stat_update_vblank_oam_pulses();
    }

    /// Port Stage A10 — the vblank-entry OAM (mode-2) STAT pulse the flag-on
    /// rising-edge [`Self::stat_update_tick`] engine does not emit.
    ///
    /// In vblank [`Self::update_mode_for_interrupt`] mirrors [`Self::vis_mode`]
    /// (mode 0 across 144:0-3, mode 1 from 144:4), so `mode_for_interrupt` never
    /// selects the OAM (mode-2) source there and the `GB_STAT_update` line never
    /// rises for it. SameBoy raises the 144-entry pulse as a **direct `IF |= 2`
    /// poke** (`display.c:2160`), independent of `stat_interrupt_line`, NOT a
    /// line rise. This reproduces it on the flag-on path with the *same* guard
    /// and commit masks the flag-off [`Self::stat_events_tick`] engine uses (the
    /// `vblank_stat_intr-GS` DMG / `-C` CGB lift; flag-on it recovers 5 mooneye
    /// combos and 8 gambatte rows with zero SameBoy-passing rows lost — see
    /// `ppu-subdot-ladder.md` "A10").
    ///
    /// The visible-line m2 pulses (lines 1-143 dot 0) are already covered by the
    /// rising-edge engine — its level-OR naturally reproduces `m2_pulse_fires`'
    /// `¬HBlank` / `¬held-LYC` blocking (a held source keeps the line high → no
    /// edge) — so only the 144:0 slot `mode_for_interrupt` skips is added here,
    /// and it cannot double-fire with the engine (at 144:0 `mfi==0`, and
    /// `m2_pulse_fires` requires HBlank disabled, so a held HBlank that would
    /// raise the engine line is exactly the case the pulse is suppressed). The
    /// DMG 145-153 dot-12 pulses are deferred (see below).
    fn stat_update_vblank_oam_pulses(&mut self) {
        // 144-entry OAM pulse (`display.c:2160`), one M-cycle before the vblank
        // IF, on both families. The DMG commit is halt- *and* dispatch-late so
        // `vblank_stat_intr-GS` observes it together with the vblank IF; the CGB
        // 144 entry is exempt and is visible in its own cycle
        // (`vblank_stat_intr-C`). Same `!glitch_line` + `m2_pulse_fires` guards
        // as the flag-off line-start pulse (the previous line's held LYC compare
        // blocks it; a glitched LCD-enable line runs no OAM scan, no pulse).
        if !self.glitch_line
            && self.line == 144
            && self.dot == 0
            && self.m2_pulse_fires(self.stat_en)
        {
            self.pending_if |= IF_STAT;
            if !self.model.is_cgb() {
                self.stat_late = true;
                self.stat_halt_late = true;
            }
        }
        // The DMG per-line vblank OAM pulses at dot 12 (`display.c:2185`;
        // `stat_events_tick`'s 145-153 block; `intr_1_2_timing-GS`) are
        // DEFERRED with the rest of the atomic read-frame work. Adding them on
        // the flag-on path was MEASURED net-negative (`ppu-subdot-ladder.md`
        // "A10"): the extra dot-12 IF regresses 6 SameBoy-PASSING rows
        // (gambatte ly0/lycint152_m2irq, lycm2int/lyc0m2int_m2irq,
        // window/late_enable_afterVblank ×4 — all in the SameBoy gap list).
        // SameBoy fires these pulses too, so they are faithful, but flag-on's
        // cc+4 read/halt frame mis-places the resulting read until the global
        // reclock lands — exactly the atomic-convergence trap. The 144:0
        // entry pulse above does NOT have this problem (zero lift lost,
        // +8 gambatte / +5 mooneye), so it banks standalone.
    }

    /// Port Stage A6 — the halt/interrupt-sample commit masks for the flag-on
    /// [`Self::stat_update_tick`] rising edge, the leading-edge-frame analogue of
    /// the per-source `stat_late` / `stat_halt_late` / `m0_rise` masks the
    /// gambatte [`Self::stat_events_tick`] engine sets (see its truth table).
    /// `mfi` is the [`Ppu::mode_for_interrupt`] that drove this 0→1 rise, so it
    /// names the source.
    ///
    /// **Calibration (measured, `ppu-subdot-ladder.md` "A6"):** the gambatte
    /// engine reads FF41/IF at the M-cycle trailing edge (cc+4) and masks the
    /// mode-2 line-start pulse from BOTH the running CPU's interrupt sample
    /// (`stat_late`) and the halt-exit sampler (`stat_halt_late`). On the
    /// leading-edge (cc+0) flag-on path the regular interrupt dispatch is already
    /// aligned to SameBoy's frame, so the mode-2 pulse needs only the **halt**
    /// mask (SameBoy `GB_cpu_run` samples the halt exit mid-cycle — `sm83_cpu.c`;
    /// gbmicrotest `int_oam_*`); applying `stat_late` too would re-delay the
    /// non-halt `ldh a,(FF41)` dispatch and collapse the separated kernel pair
    /// (`m2int_m3stat_1` reverts 3→0). With only `stat_halt_late` the canonical
    /// mooneye `intr_2_mode0_timing` passes flag-on (DMG+CGB) **and** the kernel
    /// pair stays separated (m2int=3 ∧ m0int=0) — the first config in the port to
    /// hold both. The mode-0 `m0_rise` mask carries the half-cycle halt law as
    /// before; it is neutral on the flag-on suite until the mode-0 IRQ dispatch
    /// is reclocked (its rise still lands at our cc+4 dot, the remaining atomic
    /// work — see the field docs).
    fn stat_update_halt_masks(&mut self, mfi: u8) {
        // The rise's source is unambiguous from `mfi` alone: this runs only on a
        // 0→1 edge, so the line was LOW the previous dot — meaning neither source
        // held it high. If the mode source is enabled with `mfi` selecting it
        // (`mfi == 2 && OAM`, or `mfi == 0 && HBlank`), that source is high NOW
        // yet was low before, so the mode source IS what just rose — it cannot be
        // a "LYC-only" rise (a held-high mode source would have made the previous
        // dot high → not an edge). A pure-LYC rise lands where `mfi` is NONE/1/3
        // (no branch). `stat_lyc_onoff` exercises this flag-on.
        if mfi == 2 && self.stat_en & STAT_SRC_OAM != 0 {
            // Mode-2 (OAM) line-start pulse. Lines 1-143 carry it across the
            // line-start window (the halt-exit sampler misses the rise for one
            // M-cycle); line 0's pulse (dot 4) takes no halt mask (SameBoy
            // "except on line 0"). No `stat_late` in the leading-edge frame.
            if self.line != 0 {
                self.stat_halt_late = true;
            }
        } else if mfi == 0 && self.stat_en & STAT_SRC_HBLANK != 0 {
            // Mode-0 (HBlank) source rise carries the half-cycle halt law
            // (`if_late` via the interconnect's second-half check).
            self.m0_rise = true;
        }
    }

    /// S2b interrupt-facing mode ([`Ppu::mode_for_interrupt`]) for the current
    /// dot — the decoupled view the S5 STAT engine will read. Exposed for the
    /// S2b divergence test; not yet consulted in production.
    #[cfg(test)]
    pub(crate) fn mode_for_interrupt(&self) -> u8 {
        self.mode_for_interrupt
    }

    /// Test view of the S5 [`StatUpdate`](crate::stat_update) interrupt-line
    /// level (the flag-on engine's `stat_interrupt_line`).
    #[cfg(test)]
    pub(crate) fn stat_update_line(&self) -> bool {
        self.stat_update.line()
    }

    /// S2b: recompute the interrupt-facing mode ([`Ppu::mode_for_interrupt`])
    /// for the current dot, applying the mode-2 lead / mode-0 lag anchor swing
    /// against the CPU-visible [`Self::vis_mode`]. Inert today; the substrate
    /// for the S5 STAT engine and the S2d kernel-pair flip.
    pub(super) fn update_mode_for_interrupt(&mut self) {
        // `mfi_m0_prev` lags `line_render_done` by one dot: read the previous
        // dot's value for this dot's mode-0 decision, then latch this dot's.
        let prev_done = self.mfi_m0_prev;
        self.mfi_m0_prev = self.enabled && self.line <= 143 && self.line_render_done;
        // Port Stage A8 — on the flag-on path the mode-0 IRQ fires at
        // `line_render_done` (our dot 254 = the gambatte-calibrated `m0_rise_dot`
        // frame the mode-0 halt grids pin: gbmicrotest int_hblank_halt, mooneye
        // hblank_ly_scx_timing), NOT the +1-dot `mfi_m0_prev` lag (255). The lag
        // models SameBoy's mode-0 IRQ 1 dot after the visible flip
        // (`display.c:2108` vs `:2091`), but it over-applies in our frame —
        // `line_render_done` is ALREADY the gambatte IRQ dot here, so the lag put
        // the `StatUpdate` mode-0 STAT IF one dot late vs `stat_events_tick` and
        // broke `hblank_ly_scx_timing` flag-on (kernel `m0int` and the canonical
        // both hold at 254; only the 252 full-SameBoy-frame move regresses them —
        // see `ppu-subdot-ladder.md` "DISPATCH-RECLOCK"). Flag-OFF keeps the
        // lagged `prev_done`; `stat_events_tick` never reads `mode_for_interrupt`,
        // so production is byte-identical.
        let prev_done = if self.leading_edge_reads {
            self.enabled && self.line <= 143 && self.line_render_done
        } else {
            prev_done
        };
        self.mode_for_interrupt = if !self.enabled {
            0
        } else if self.glitch_line {
            // Port Stage A15 — the LCD-enable glitch line. `vis_mode` yields
            // mode 0 in TWO regions: the line-start PREFIX (`dot < GLITCH_MODE3_START`,
            // before the glitch mode-3 window) and the post-render tail
            // (`line_render_done`/`vis_early`). Only the tail is a real hblank;
            // the prefix is the LCD-enable glitch, which raises NO mode-0 STAT
            // IRQ — `stat_line_level` and `stat_write_trigger_dmg` both suppress
            // the HBlank source there with `!(glitch_line && dot < GLITCH_MODE3_START)`.
            // The rising-edge engine had no such guard: with HBlank enabled it
            // saw mode 0 in the prefix and fired a spurious m0 IRQ at the first
            // glitch dot (SameBoy + gambatte render outE0; the bare engine gave
            // E2 — `enable_display/ly0_m0irq`, `irq_precedence/late_m0irq_retrigger`).
            // Select NONE in the prefix so no mode source contributes (LYC still
            // can — `level` ORs them); keep `vis_mode` (the real post-render m0,
            // or mode 3) elsewhere. `mode_for_interrupt` is inert flag-OFF
            // (`stat_events_tick` never reads it), so production is byte-identical.
            //
            // SINGLE SPEED only (`!ds`): the recovered slice is the single-speed
            // `enable_display/ly0_m0irq_trigger` (+2 flag-on, SameBoy-confirmed
            // out0). The double-speed `ly0_m0irq_scxN_ds_{1,2}` reads BRACKET the
            // glitch m0 IRQ dot (`_1` wants outE0 / read before, `_2` wants outE2
            // / read after), which our whole-dot model misframes (fires at the
            // prefix AND the post-render dot, never the DS mid-line dot SameBoy
            // hits) — so suppressing the DS prefix is a read-frame A/B swap that
            // drops the SameBoy-passing `ly0_m0irq_scx0_ds_2` (outE2). That DS
            // slice is part of the atomic Phase-B reclock, deferred. Measured
            // (`ppu-subdot-ladder.md` "A15"): SS-gated = +2 / 0 regress / 0 lift
            // lost; universal = +6 / 0 regress / −1 SameBoy-passing drop.
            let vm = self.vis_mode();
            if vm == 0 && !self.ds && !(self.line_render_done || self.vis_early) {
                crate::stat_update::MODE_FOR_INTERRUPT_NONE
            } else {
                vm
            }
        } else if self.line >= 144 {
            // VBlank. The visible mode IS the IRQ mode here: `vis_mode` already
            // yields line 144's HBlank carryover (mode 0, dots 0-3) flipping to
            // the VBlank source (mode 1) at the vblank-entry step
            // (`display.c:2178`, ~dot 4), and mode 1 for every later vblank line
            // (145-153) — there is no mode-2 carryover into vblank
            // (`display.c:2138` skips `LINES-1`) and no `-1` gap. The per-line
            // DMG OAM vblank pulses + the line-144 OAM IF pokes are direct
            // `IF |= 2` writes (`display.c:2160`, `:2185`), handled in the STAT
            // engine, not `mode_for_interrupt` transitions.
            self.vis_mode()
        } else if self.dot < 84 {
            // Mode-2 region. SameBoy holds the OAM STAT source high across the
            // line-start window, then sets `mode_for_interrupt = -1` (NONE) for
            // the rest of the OAM search (`display.c:1781` → `:1799`) — so the
            // source level falls and a later LYC rise can re-fire (STAT
            // blocking), rather than staying high across all of mode 2. On lines
            // 1-143 the source is carried high across dots 0-3 (set at the prior
            // line's end `display.c:2138`, re-set at the line top `:1781`) — the
            // "OAM int 1 T-cycle before STAT" lead (`display.c:1778`) as a
            // sustained window, leading the visible mode→2 edge at dot 4. Line 0
            // has no prior-line carryover and no early lead ("except on line
            // 0"), but SameBoy's `GB_SLEEP 7,1` step (`display.c:1789`) still
            // sets `mode_for_interrupt = 2` unconditionally (`:1781`) at the
            // step the visible byte flips to 2 (`:1792`), so line 0 pulses *at*
            // dot 4 — matching `ModeTimeline::mode2_irq_offset(0) == 0`. (Whole-
            // dot caveat for the S5 wiring: SameBoy drops the source back to -1
            // at the *same* cycle as the line-0 rise, so its NONE/re-fire window
            // opens a dot earlier than this pulse — revisit if a line-0 dot-4
            // LYC=0 re-fire ever needs it.)
            if self.line == 0 {
                // Line 0: no prior-line OAM carryover (line 153 runs no
                // `display.c:2138` set) and no early lead (`display.c:1778`
                // "except on line 0"). Its OWN OAM pulse fires AT the visible
                // mode→2 edge (dot 4, the unconditional `:1792`/`:1781` set),
                // then falls to NONE.
                //
                // Mech 3 root 2 (S5 engine-driver) — the line-0 VBlank carry.
                // Dots 0-3 carry the **VBlank (mode-1) source**, not `vis_mode`.
                // SameBoy never re-sets `mode_for_interrupt` between the line-144
                // entry (`display.c:2215`, `= 1`) and line 0's `GB_SLEEP 7,1` OAM
                // step (`:1828`, `= 2`): it holds 1 across all of vblank AND line
                // 0's first dots. So when VBlank is enabled the STAT line stays
                // continuously HIGH from line 144 through the line-0 OAM rise —
                // the dot-4 OAM pulse joins an already-high line and raises NO
                // fresh 0→1 edge (`m1/m2m1irq_ifw_2`: SameBoy fires ly1-143, NOT
                // ly0; slopgb's `vis_mode`=0 here dropped the line at dot 0 and
                // re-raised it at dot 4 → spurious ly0 OAM IRQ → `got=3` for
                // `want=1`). With VBlank disabled the carried mode-1 source
                // contributes nothing, so the line is low into dot 4 and the OAM
                // pulse fires its real edge (matching SameBoy's vblank-off rows).
                // `vis_mode` differs only for DMG (CGB line-0 dots 0-3 already
                // read mode 1); the IRQ side is decoupled from the FF41 read, so
                // the visible DMG line-0 mode-0 window is untouched. LE/Tier-2
                // only — `mode_for_interrupt` is inert flag-off, production
                // byte-identical.
                if self.dot == 4 {
                    2
                } else if self.dot < 4 {
                    1
                } else {
                    crate::stat_update::MODE_FOR_INTERRUPT_NONE
                }
            } else if self.dot < 4 {
                // Lines 1-143: the OAM (mode-2) IRQ source is carried high
                // across the whole line-start window (dots 0-3). SameBoy sets
                // `mode_for_interrupt = 2` at the prior line's end
                // (`display.c:2138`, skipped only for `LINES-1`) and re-sets it
                // at the line top (`display.c:1781`), so the source leads the
                // visible mode→2 edge (dot 4) by the entire window — the "OAM
                // int 1 T-cycle before STAT" glitch (`display.c:1778`) seen as a
                // sustained carryover, not only the dot-3 lead.
                2
            } else {
                crate::stat_update::MODE_FOR_INTERRUPT_NONE // OAM-search body: no source
            }
        } else if !prev_done {
            // Mode 3 holds for the IRQ side one dot past the visible 3→0 flip
            // (`display.c:2091` visible vs `:2108` IRQ — the mode-0 lag).
            3
        } else {
            0
        };
    }

    /// SameBoy `ly_for_comparison` (`display.c`) — the *delayed* LY value the
    /// LYC==LY STAT source compares against, distinct from the live FF44. It is
    /// `-1` ("no line", SameBoy's `0xFFFF`/`-1` sentinel: nothing matches) at the
    /// top of each line, latches to the line number a few dots in, and holds the
    /// previous line's value across the next line's first dots (the LYC-match
    /// tail). This is the LYC input the S5 [`StatUpdate`](crate::stat_update)
    /// engine consumes on the flag-on path; inert (unread) flag-off.
    ///
    /// Single speed is pinned exactly (DMG / CGB-C / AGB). Double speed doubles
    /// the line-153 GB_SLEEP offsets — deferred to the S7 DS unification; the DS
    /// branch below uses the single-speed dot grid as a documented placeholder
    /// (inert, so it changes no observable behaviour until the flip recalibrates
    /// it). The LCD-enable glitch line returns `-1` (its LY/LYC view is the live
    /// flag-off path's concern, `lcdon_*` tables).
    pub(super) fn ly_for_comparison(&self) -> i16 {
        if !self.enabled || self.glitch_line {
            return -1;
        }
        let line = i16::from(self.line);
        if self.line <= 143 {
            // Visible line: prev-line carryover (dots 0-2) → -1 at the dot-3
            // reset (`display.c:1776`, `current_line ? -1 : 0`) → N at dot 4
            // (`display.c:1786`). Line 0's predecessor (line 153) ends holding 0.
            if self.dot >= 4 {
                line
            } else if self.dot == 3 {
                if self.line == 0 { 0 } else { -1 }
            } else if self.line == 0 {
                0
            } else {
                line - 1
            }
        } else if self.line <= 152 {
            // VBlank 144-152: `-1` set at line entry, `= current_line` after
            // GB_SLEEP 26+12 (≈dot 4) (`display.c` 144-152 loop).
            if self.dot >= 4 { line } else { -1 }
        } else {
            self.ly_for_comparison_line_153()
        }
    }

    /// Line 153's model-specific `ly_for_comparison` micro-sequence (the
    /// `display.c` line-153 tail). See [`Self::ly_for_comparison`].
    fn ly_for_comparison_line_153(&self) -> i16 {
        if self.model == Model::Agb {
            // `model > CGB_C`: GB_SLEEP(14,2) lands the first set at dot 4, and
            // `model>CGB_C||ds` keeps it 153 through the LY=0 step; no -1 gap.
            match self.dot {
                0..=3 => -1,
                4..=11 => 153,
                _ => 0,
            }
        } else {
            // DMG / MGB / CGB-C single speed (`model <= CGB_C`, `!ds`):
            // GB_SLEEP(14,4) delays the first set to dot 6, then the LY=0 step
            // drops `ly_for_comparison` back to -1 (the `model>CGB_C||ds` arm is
            // false) before the final = 0 at dot 12. (DS placeholder, see above.)
            match self.dot {
                0..=5 => -1,
                6..=7 => 153,
                8..=11 => -1,
                _ => 0,
            }
        }
    }
}
