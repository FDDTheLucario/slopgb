//! PPU register read/write dispatch (FF40-FF4B) + LCDC.7 enable/disable. The
//! mode-3 write strobe staging (stage_write/commit_eff/strobe_tick) lives in
//! regs/stage.rs. docs/ARCHITECTURE.md §Mode-3 write strobe. Oracle: mealybug m3_*, gambatte scx/scy/dmgpalette during_m3.

use super::*;

impl Ppu {
    /// Read VRAM (0x8000-0x9FFF, current bank), OAM (0xFE00-0xFE9F), or a
    /// PPU register (FF40-FF4B, FF4F, FF68-FF6B). Mode-based access blocking
    /// applies to VRAM/OAM.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                if self.vram_read_blocked() {
                    0xFF
                } else {
                    self.vram[self.vram_index(addr)]
                }
            }
            0xFE00..=0xFE9F => {
                if self.oam_read_blocked() {
                    0xFF
                } else {
                    self.oam[usize::from(addr - 0xFE00)]
                }
            }
            0xFF40 => self.lcdc,
            0xFF41 => {
                // The eager halt-woken re-fetch boundary override rides on top
                // of the read-law mode; the sole non-probe consumer.
                let vm = self.vis_mode_read();
                let vm = self.halt_refetch_read_override(vm).unwrap_or(vm);
                0x80 | self.stat_en | (u8::from(self.read_cmp()) << 2) | vm
            }
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F if self.model.is_cgb() => 0xFE | self.vbk,
            0xFF68 if self.model.is_cgb() => 0x40 | self.bcps,
            0xFF69 if self.model.is_cgb() => {
                if self.pal_ram_blocked() {
                    0xFF
                } else {
                    self.bg_pal_ram[usize::from(self.bcps & 0x3F)]
                }
            }
            0xFF6A if self.model.is_cgb() => 0x40 | self.ocps,
            0xFF6B if self.model.is_cgb() => {
                if self.pal_ram_blocked() {
                    0xFF
                } else {
                    self.obj_pal_ram[usize::from(self.ocps & 0x3F)]
                }
            }
            0xFF6C if self.model.is_cgb() => 0xFE | self.opri,
            _ => 0xFF,
        }
    }

    /// Write counterpart of [`Self::read`]. Returns IF bits raised by the
    /// write itself (same encoding as [`Self::tick`]): STAT/LYC/LCDC writes
    /// can raise the STAT line in the very M-cycle of the write —
    /// `stat_lyc_onoff` round 4 needs that interrupt to dispatch before the
    /// next instruction — so the caller must OR the returned bits into IF
    /// immediately, like a `tick` result.
    pub fn write(&mut self, addr: u16, value: u8) -> u8 {
        // Architectural commit point: converge the pipeline view with the
        // registers (the staged copy of this same write may already have
        // expired into it — see `stage_write`; writes that never went
        // through the staging path land in both views here).
        //
        // On the tier2 deferred path the stage SURVIVES the architectural
        // write and the
        // pipeline view commits via `strobe_tick` at SameBoy's frame instead
        // (the io write lands at the write M-cycle's END — `GB_advance_cycles`
        // runs the display coroutine BEFORE the write commits, memory.c /
        // sm83_cpu.c — so the pixel pipe sees the new value only from the
        // next dot after the M-cycle). The eager production path ticks the
        // machine BEFORE this call, so its stage has already expired and this
        // immediate convergence is what commits it; the deferred path calls
        // this at the leading edge with zero dots ticked, so the immediate
        // convergence was collapsing every mid-mode-3 register write onto the
        // write's leading edge — the measured "deferred WRITE collapse"
        // behind the late_scx/late_disable/late_wx render-length pairs
        // (`late_scx4`: SameBoy separates the legs by whether the SCX commit
        // lands before/after the fine-scroll comparator's first sample;
        // slopgb collapsed both legs onto the leading edge). Production
        // (`!eager`) is byte-identical.
        // The mode-3 render regs (SCY/SCX/BGP/OBP) survive the arch
        // write so they strobe-commit at the render frame instead of the
        // leading edge (see the dots calc in `cycle.rs::write_deferred`). LCDC
        // lands via the split `render_lcdc` view.
        // The eager render-frame debt ([`Ppu::stage_write`]) keeps the staged
        // commit alive past the write's own `tick_machine` (its `dots_left`
        // exceeds the M-cycle's half-dots), so — like tier2 at the leading edge
        // — the stage is still present here and this survive-check holds, letting
        // the debt-delayed strobe commit at the read frame instead of the
        // redundant M-cycle-END (D+4) re-commit clobbering it. On the un-shifted
        // DMG eager path (debt 0) the stage drains inside `tick_machine`, so this
        // is false and the M-cycle-END commit still runs — byte-identical to the
        // pre-slice DMG eager behaviour.
        let staged_pending = matches!(addr, 0xFF42 | 0xFF43 | 0xFF47..=0xFF49 | 0xFF4B)
            && !self.glitch_line
            && self
                .staged
                .as_ref()
                .is_some_and(|s| s.addr == addr && s.value == value);
        if !staged_pending {
            if self.staged.as_ref().is_some_and(|s| s.addr == addr) {
                self.staged = None;
            }
            self.commit_eff(addr, value);
        }
        match addr {
            0x8000..=0x9FFF => {
                // Record the attempt for the DS line-end VRAM read release
                // (blocked attempts too — the M-cycle cost is what SameBoy
                // spreads).
                self.vram_wr_line = self.line;
                self.vram_wr_dot = self.dot;
                if !self.vram_write_blocked() {
                    self.vram[self.vram_index(addr)] = value;
                }
            }
            0xFE00..=0xFE9F => {
                if !self.oam_write_blocked() {
                    self.oam[usize::from(addr - 0xFE00)] = value;
                }
            }
            0xFF40 => self.write_lcdc(value),
            0xFF41 => {
                let old = self.stat_en;
                let data = value & STAT_SRC_ALL;
                if self.enabled {
                    let fire = if self.model.is_cgb() {
                        // Retroactive pulse reach: the CGB line-start m2
                        // pulse sits a sub-cycle after our dot-0 tick, so
                        // a write committing in that same M-cycle still
                        // decides it (the un-fire direction is
                        // unrepresentable — m2enable disable_1 stays a
                        // documented swap).
                        let retro = self.dot == 0
                            && !self.glitch_line
                            && (1..=143).contains(&self.line)
                            && old & STAT_SRC_HBLANK == 0
                            && !self.m2_pulse_fires(old)
                            && self.m2_pulse_fires(data);
                        // (The FF45-write edge-only engine-line guard does
                        // NOT port here: the FF41 retro/m2 pulse reach is
                        // event-like in the pinned m2enable cells — the guard
                        // was built + measured +3 fails there.)
                        retro || self.stat_write_trigger_cgb(old, data)
                    } else {
                        // The glitch trigger, plus the DMG pulse reach:
                        // an m2 enable committing at the pulse's M-cycle
                        // or the one after re-decides a pulse that did
                        // not exist under the old enables (old m2en off),
                        // blocked by the held LYC match — through the
                        // *new* lyc enable at dot 0, either enable at
                        // dot 4 (the m2enable late_enable /
                        // late_enable_after_lycint(_disable) dmg08 cell
                        // grids pin all eleven cells).
                        // The eager cc+0 write records the STAT-enable a
                        // full M-cycle (4 dots) EARLIER than the tier2 cc+4 frame
                        // the {0,4} window + the data-only dot-0 lycen were
                        // calibrated against (an inserted NOP in the `_1`/`_2`
                        // pair shifts the LDH($41) write +4 dots; slopgb records
                        // it at eager dot 0 vs tier2 dot 4). Add the +4 read-debt
                        // so the eager retro resolves in the tier2 frame:
                        // late_enable_2 (eager dot4→rd8, out of window, want no-
                        // fire) + late_enable_after_lycint_disable_2 (eager dot0→
                        // rd4, held-LYC suppressed via the data|old lycen, want
                        // no-fire). `eager`+DMG-scoped → byte-identical.
                        let rd = self.dot + 4;
                        let retro = (rd == 0 || rd == 4)
                            && !self.glitch_line
                            && (1..=144).contains(&self.line)
                            && old & (STAT_SRC_OAM | STAT_SRC_HBLANK) == 0
                            && data & STAT_SRC_OAM != 0
                            && data & STAT_SRC_HBLANK == 0
                            && {
                                let lycen = if rd == 0 { data } else { data | old };
                                !(lycen & STAT_SRC_LYC != 0 && self.lyc_ev_m == self.line - 1)
                            };
                        retro || self.stat_write_trigger_dmg(old)
                    };
                    if fire {
                        self.pending_if |= IF_STAT;
                    }
                    // Seed the held-high engine level for
                    // the suppressed DS line-start carryover enable (the
                    // `lyc_carryover` DS dots-0-1 suppression in
                    // `stat_write_trigger_cgb`): hardware's line is still
                    // HIGH from the previous line's mode-0 there, so the
                    // post-write level (fresh bit6 + the held carryover
                    // match) continues it — without the seed the next dot's
                    // engine tick re-fires the same edge as a spurious 0→1.
                    if self.model.is_cgb()
                        && self.ds
                        && self.dot < 2
                        && old & STAT_SRC_HBLANK != 0
                        && old & STAT_SRC_LYC == 0
                        && data & STAT_SRC_LYC != 0
                        && (1..=143).contains(&self.line)
                        && self.lyc == self.line - 1
                    {
                        self.stat_update.force_level(true);
                    }
                    self.stat_en = data;
                    // The CGB FF41 two-phase write for the engine
                    // view (see `eng_stat_pending` for the full schedule):
                    // phase-1 (mode bits new, bit6 old) applies at engine
                    // tick commit+2 (= SameBoy T0), the final value at
                    // commit+4, externals in between edging against the
                    // armed phase-1.
                    if self.model.is_cgb() && !self.ds && self.lcd_shift_dots == 0 {
                        // Single speed: the two-phase engine view (see the
                        // `eng_stat_pending` field doc for the schedule).
                        // phase-1 = mode bits NEW, LYC enable (bit 6) OLD —
                        // SameBoy `GB_CONFLICT_STAT_CGB` holds bit 6 one T
                        // past the mode bits.
                        //
                        // Double speed: 1 T = half a dot — the whole window is
                        // sub-dot, and the deferred DS write commit (+1 dot)
                        // already lands at the hardware instant: the engine
                        // sees the new value from the next tick (immediate),
                        // with the write-after-tick order giving the
                        // display-step-first collision semantics the
                        // `lyc0_m1disable_ds` / `lyc153_m1disable_ds` pairs
                        // pin (together with the DS line-153 lyfc table).
                        let phase1 = (old & STAT_SRC_LYC) | (data & !STAT_SRC_LYC);
                        self.eng_stat_pending = Some(EngStatPending {
                            phase1,
                            fin: data,
                            pre_high: self.stat_update.line(),
                            mfi_t0: 0,
                            k: 0,
                        });
                    } else if !self.model.is_cgb() && self.line == 153 && !self.glitch_line {
                        // HALFDOT piece 4: on LINE 153 the DMG FF41
                        // engine-view (`eng_stat`) write commits its
                        // disable/enable ~2 dots LATER than the eager cc+4 whole-
                        // dot landing — the line-153 write quirk. SameBoy's
                        // VBLANK-disable on line 153 lands COINCIDENT with the
                        // LYC=153 re-latch (dot 6), so the held LYC match keeps
                        // the STAT line high across the disable → no fresh edge
                        // (`lycEnable/lyc153_late_m1disable_3` want E0; slopgb's
                        // whole-dot cc+4 commit at dot 4 dropped the line 2 dots
                        // before the LYC re-latch → spurious edge → E2). Deferring
                        // ONLY the engine `eng_stat` view via the odd-half
                        // `stat_update_half` (piece 1) resolves it at the
                        // coincident sub-dot without moving the whole-dot cc+4
                        // FF41-read frame. Line-153-scoped (the write side of the
                        // documented line-153 LYC side-effect zone), NOT ROM-
                        // specific. The sibling `m0enable/lycdisable_ff41_2` (line
                        // 1) is untouched. `eager`-gated → byte-identical.
                        self.eng_stat_half = Some((data, 2));
                        self.eng_stat_pending = None;
                    } else {
                        self.eng_stat = data;
                        self.eng_stat_pending = None;
                        // Record a DS LYC-enable DROP for the
                        // m0-flip dip (see `ff41_ds_drop`).
                        if self.model.is_cgb()
                            && self.ds
                            && old & STAT_SRC_LYC != 0
                            && data & STAT_SRC_LYC == 0
                        {
                            self.ff41_ds_drop = Some((self.line, self.dot));
                        }
                    }
                    self.stage_stat_copies();
                    self.refresh_cmp(false);
                    if fire {
                        // When the gambatte write-trigger fired (`fire`),
                        // re-sync the flag-on [`StatUpdate`]
                        // line to the post-write level so the next dot-clocked
                        // `stat_update_tick` does NOT re-fire the SAME edge.
                        // Without this, enabling a source whose condition is
                        // already met fires IF twice flag-on: once here, again
                        // when the dot engine re-sees the new enable as a 0→1
                        // rise (`ff41_enable_lyc_fires_once_flag_on`). The edge
                        // is discarded — the write-trigger keeps gambatte's
                        // position-exact fire (replacing it wholesale with the
                        // rising edge is net-negative in our cc+4 frame);
                        // this only seeds the line level. Gated on `fire`: a
                        // write that does NOT
                        // trigger here must leave the line untouched so a
                        // legitimate dot-engine rise next tick still fires (the
                        // un-gated sync suppressed 15 such lifts — measured).
                        // Read-frame-independent, flag-gated → byte-identical
                        // flag-OFF.
                        let _ = self.stat_update.update(
                            self.mode_for_interrupt,
                            data,
                            self.lyc_interrupt_line,
                        );
                    }
                    // Suppress the spurious mid-mode-2 OAM rise on the
                    // eager clock. Lines 1-143 carry the OAM (mode-2) STAT source
                    // high only across the line-start window (dots 0-3), then drop
                    // to NONE (`update_mode_for_interrupt`). A FRESH OAM enable
                    // landing in that window (the eager cc+0 write records it 4
                    // dots before its true cc+4 commit at dot 4, where mfi is
                    // already NONE) makes the dot-engine see a 0→1 edge one dot
                    // later and fire IF — but the line-start m2 pulse already
                    // passed, so gambatte/SameBoy raise nothing
                    // (`m2enable/late_enable_m0disable_2` want 0: enable at ly2
                    // dot 0, old=HBLANK excludes retro → no legit catch). When
                    // neither retro nor the write-trigger fired (`!fire` — the
                    // write did NOT catch the pulse), seed the engine line HIGH
                    // (STAT blocking, no edge) so the post-window rise is spent;
                    // the line falls silently at dot 4. A carried-from-prev-line
                    // OAM (`old & OAM != 0`) fires its real dot-0 pulse and is
                    // excluded. `eager`+DMG-scoped → byte-identical.
                    if !self.model.is_cgb()
                        && !fire
                        && self.dot < 4
                        && (1..=143).contains(&self.line)
                        && !self.glitch_line
                        && old & STAT_SRC_OAM == 0
                        && data & STAT_SRC_OAM != 0
                    {
                        self.stat_update.force_level(true);
                    }
                } else {
                    self.stat_en = data;
                    self.eng_stat = data;
                    self.eng_stat_pending = None;
                    self.flush_stat_copies();
                    self.legacy_level_edge();
                }
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only.
            0xFF4A => {
                let old_wy = self.wy;
                self.wy = value;
                // The boundary-WY cross-line latch (see `Ppu::wy_xline_trig`):
                // a tail/head write matching the current line, window enabled
                // at the commit (DMG too — the DMG arm-7 twin reads the same
                // latch). Also enabled under `eager`: this arch commit runs
                // at the eager M-cycle END, so the boundary window catches the
                // tail-write class (`late_wy_FFto0/FFto1/10to0/1toFF`) that the
                // read-frame WY laws pair with — measured +8 CGB, 0 drop.
                // The write dot the tail/head boundary classifies against.
                // Under the eager DMG line-153 emission decouple the
                // shared LYC=153 ISR — and every WY write it times — fires one
                // M-cycle (4 dots SS) EARLIER than the stale dot-6/dot-8
                // recognition these 452/4 boundaries were tuned against, so a
                // boundary write that landed at `ly N dot 4` (base: past the
                // head → bare) now commits at `ly N dot 0` (inside the head →
                // spurious cross-line extend). Re-map by the +4 read-debt so the
                // moved write classifies on the calibrated frame: `FFto0_ly2_3`
                // ly1 dot0 → xdot 4 (NOT head → bare); its `_2` ly0 dot452 →
                // xdot 456 (still tail → extend). The SS twin of the DS lyfc
                // wake re-derivation. `eager && !is_cgb` (CGB emission
                // unmoved; tier2 + production byte-identical).
                let xdot = self.dot + if !self.model.is_cgb() { 4 } else { 0 };
                if self.enabled
                    && !(4..452).contains(&xdot)
                    && self.line < 144
                    && value == self.ly
                    && self.eff.lcdc & LCDC_WIN_ENABLE != 0
                {
                    self.wy_xline_trig = true;
                }
                // DMG — a HEAD write (dot < 4) matching the JUST-FINISHED
                // line: slopgb's deferred frame lands a line-boundary WY write
                // a full line late (SameBoy applies it at `ly N−1 cfl0` and its
                // continuous `wy_check` triggers on line N−1; slopgb commits at
                // `ly N dot0`, past that line's weMaster sample). If the value
                // matches the previous line (`value + 1 == line`), SameBoy
                // triggered there → the window is sticky-active for every later
                // line → set the cross-line latch. `late_wy_10to0/FFto0/FFto1`
                // `_2` (commit ly1/ly2 dot0) extend; the `_3` siblings commit
                // at dot 4 (past the head) → no trigger, bare. Also enabled
                // under `eager`: the eager arch commit lands at the
                // M-cycle END (same head-dot window), pairing with the DMG
                // read-frame WY laws already live under eager — L2 re-host of
                // the CGB slice-2 cross-line latch to DMG.
                if !self.model.is_cgb()
                    && self.enabled
                    && xdot < 4
                    && self.line >= 1
                    && self.line < 144
                    && u16::from(value) + 1 == u16::from(self.line)
                    && self.eff.lcdc & LCDC_WIN_ENABLE != 0
                {
                    self.wy_xline_trig = true;
                }
                // The DS trigger-line WY un-latch: SameBoy's per-line
                // `wy_check` for line N runs ~dot 2-5, but slopgb's
                // production `wy_latch` pre-latches at the PREVIOUS line's
                // dot-450/454 samples — so an un-matching WY write landing
                // before the hardware check (commit dot <= 4 of the fresh
                // trigger line) must release the latch the check would never
                // have acquired (`late_wy_1toFF_ds_1` renders bare on
                // hardware AND SameBoy; its `_2` sibling commits at dot 5
                // and keeps the trigger). Tier-2 + CGB + DS; also `eager`
                // (the eager DS `late_wy_ds`/`late_wy_1toFF_ds` pairs recover on
                // the same latch, part of the +8 CGB WY slice).
                if self.model.is_cgb()
                    && self.ds
                    && self.enabled
                    && self.wy_latch
                    && !self.render.win_active
                    && self.line < 144
                    // The un-latch deadline is PER-TRIGGER-LINE:
                    // lines >= 1 run the lyfc-path check at the mode-2 entry
                    // (~internal dot 3-4) → a commit <= 2 beats it
                    // (`late_wy_1toFF_ds_1` dot 2 bare / `_ds_2` dot 4 keeps
                    // the latch, extended); line 0's check sits ~4 dots later
                    // (lyfc becomes 0 only at dot 3) → commit <= 6
                    // (`late_wy_ds_1` dot 6 bare on HARDWARE — SameBoy
                    // mis-times the line-0 check and fails that row itself /
                    // `_ds_2` dot 8 keeps). The old single `<= 4` split both
                    // brackets wrong.
                    && self.dot <= if self.ly == 0 { 6 } else { 2 }
                    && old_wy == self.ly
                    && value != self.ly
                {
                    self.wy_latch = false;
                    // The shadow latched the same pre-check compare
                    // (wy2 still old at line start) — release it with the
                    // render latch so the read law's extend arm follows, and
                    // commit the wy2 copy immediately (the write BEAT the
                    // hardware check: every later compare this line reads the
                    // new value; the stale copy re-latched the shadow at the
                    // next dot, measured).
                    if self.wy_trig_sb && self.wy_trig_sb_line == self.ly {
                        self.wy_trig_sb = false;
                    }
                    self.wy2 = value;
                    self.wy2_delay = 0;
                }
                // The DMG SS trigger-line WY→(non-LY) un-latch: a
                // WY write that MATCHED at the line's mode-2 compare then
                // flips away by dot 4 un-triggers the window on SameBoy
                // (`wy_check` reads the settled WY) while slopgb's raw sticky
                // latch (`wy_trig_sb_raw`, set at dot ≥ 4) already caught the
                // brief match. Releasing it lets the D6 un-trigger arm fire.
                // `late_wy_1toFF_2`/`2toFF_2` (FF at dot 4 → bare) vs `_3`
                // (FF at dot 8, past the compare → the window drew, extends).
                // SS + DMG; the CGB path is the DS
                // block above (`wy_latch`/`wy_trig_sb`). Also enabled under
                // `eager`: pairs with the DMG arm-D6 un-trigger read law
                // already live under eager — L2 re-host of the DMG late-WY
                // un-trigger latch.
                if !self.model.is_cgb()
                    && !self.ds
                    && self.enabled
                    && self.line < 144
                    && self.dot <= 4
                    && old_wy == self.ly
                    && value != self.ly
                {
                    self.wy_trig_sb_raw = false;
                    // The dot-0/dot-<4 un-trigger write ALSO spuriously latched
                    // the wy2-lagged SHADOW (`wy_trig_sb`) at line start (wy2
                    // still = old_wy = ly before this write's delayed copy
                    // propagates). When the write lands BEFORE the render draws
                    // the window (the `_1` variant, WY→FF at dot 0 → `win_active`
                    // never rises, so the D6 arm cannot fire), the sticky shadow
                    // blocks the arm-8 emergent bare exit on every later line —
                    // over-holding mode 3. Release it (mirror of the DS
                    // `wy_latch` un-latch above) and commit wy2 immediately so
                    // the next dot's compare does not re-set it. `late_wy_1toFF_1`
                    // / `2toFF_1` recover; the `_2` siblings (render drew) keep
                    // D6. Byte-identical flag-OFF (gated tier2||eager).
                    if self.wy_trig_sb && self.wy_trig_sb_line == self.ly {
                        self.wy_trig_sb = false;
                    }
                    self.wy2 = value;
                    self.wy2_delay = 0;
                }
                // The live window-trigger comparison uses a delayed WY
                // copy — see `wy2`.
                if self.enabled {
                    // CGB: ~6 dots after the architectural commit (5 in
                    // double speed); DMG: 2 (gambatte wyChange: wy2 at
                    // cc+6-ds on CGB with the LCD on, cc+2 otherwise,
                    // one cycle later than the wx commit; calibrated
                    // against the gambatte window/arg/late_wy_* rounds).
                    self.wy2_delay = if !self.model.is_cgb() {
                        2
                    } else if self.staged_ds {
                        5
                    } else {
                        6
                    };
                } else {
                    self.wy2 = value;
                }
            }
            0xFF45 => {
                let old = self.lyc;
                self.lyc = value;
                // Fresh-write signal for the eager line-153 STAT-delivery retime
                // (see `l153_lyc_write_dot`). Eager+CGB only.
                if self.model.is_cgb() && self.line == 153 {
                    self.l153_lyc_write_dot = self.dot;
                }
                // The comparison retriggers immediately on LYC writes while
                // the comparison clock runs (`stat_lyc_onoff`).
                if self.enabled && old != value {
                    let before = self.pending_if;
                    if self.model.is_cgb() {
                        self.write_lyc_cgb(old, value);
                    } else {
                        self.write_lyc_dmg(old, value);
                    }
                    if (self.pending_if & !before) & IF_STAT != 0 {
                        // `& !before` keys on a NEWLY-set STAT bit (the trigger
                        // fired this call), not one already pending from an
                        // earlier tick this M-cycle — so the sync only fires for
                        // the double-fire case and never over-suppresses a
                        // legitimate dot-engine rise (the un-gated form dropped
                        // 15 SameBoy-passing rows).
                        // The FF45 analogue of the FF41 write-trigger re-sync
                        // above. The gambatte LYC-write trigger above just
                        // fired; re-derive
                        // `lyc_interrupt_line` for the NEW LYC (the engine's LYC
                        // input, normally latched in `stat_update_tick`) and
                        // re-sync the `StatUpdate` line so the next dot-clocked
                        // tick does NOT re-fire the same match as a 0→1 rise
                        // (`ff45_match_fires_once_flag_on`). Gated on the
                        // trigger having fired — a write that does not trigger
                        // here leaves the line for the dot engine to raise
                        // legitimately next tick. The edge is discarded.
                        // Read-frame-independent, flag-gated → byte-identical
                        // flag-OFF.
                        let ly = self.ly_for_comparison();
                        if ly != -1 {
                            self.lyc_interrupt_line = ly == i16::from(self.lyc);
                        }
                        let _ = self.stat_update.update(
                            self.mode_for_interrupt,
                            self.stat_en,
                            self.lyc_interrupt_line,
                        );
                    }
                } else {
                    self.lyc_event = value;
                    self.lyc_ev_m = value;
                    self.legacy_level_edge();
                }
            }
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4B => {
                // Latch a mid-line WX rewrite for the un-catch law (see
                // `Render::wx_write_dot`; DMG too) at the EAGER (cc+0) leading
                // edge here, NOT in `commit_eff`. The tier2 reclock defers the
                // render-VIEW `eff.wx` (a WX survive-stage, for the window
                // activation/reactivation comparator — `late_wx_ds`, m3_wx_*),
                // so `commit_eff` now runs at the deferred strobe dot; the
                // un-catch read law (`tier2_window_late_wx_uncatch_passes`) is
                // calibrated to the write's cc+0 dot, so its input stays here.
                // The SPLIT: length/read-law input eager, render view deferred.
                if self.render.active {
                    self.render.wx_write_dot = self.dot;
                }
                self.wx = value;
            }
            0xFF4F if self.model.is_cgb() => self.vbk = value & 1,
            0xFF68 if self.model.is_cgb() => self.bcps = value & 0xBF,
            0xFF69 if self.model.is_cgb() => {
                if !self.pal_ram_blocked() {
                    self.bg_pal_ram[usize::from(self.bcps & 0x3F)] = value;
                }
                // Auto-increment happens even when the write is blocked
                // (Pan Docs, "LCD Color Palettes (CGB only)").
                if self.bcps & 0x80 != 0 {
                    self.bcps = 0x80 | (self.bcps.wrapping_add(1) & 0x3F);
                }
            }
            0xFF6A if self.model.is_cgb() => self.ocps = value & 0xBF,
            0xFF6B if self.model.is_cgb() => {
                if !self.pal_ram_blocked() {
                    self.obj_pal_ram[usize::from(self.ocps & 0x3F)] = value;
                }
                if self.ocps & 0x80 != 0 {
                    self.ocps = 0x80 | (self.ocps.wrapping_add(1) & 0x3F);
                }
            }
            0xFF6C if self.model.is_cgb() => self.opri = value & 1,
            _ => {}
        }
        std::mem::take(&mut self.pending_if)
    }

    fn write_lcdc(&mut self, value: u8) {
        let was_on = self.lcdc & LCDC_ENABLE != 0;
        self.lcdc = value;
        let now_on = value & LCDC_ENABLE != 0;
        if was_on && !now_on {
            // LCD off: LY=0, mode 0, instantly; the comparison clock stops
            // with the flag frozen (`stat_lyc_onoff`); the displayed frame
            // goes white.
            self.enabled = false;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            self.glitch_line = false;
            // Invariant hygiene: frame_skip only matters while enabled and
            // every enable re-arms it; don't leave it stale across off.
            self.frame_skip = false;
            self.line_render_done = true;
            self.flip_dot = 0;
            self.vis_early = false;
            self.vis_hold_until = 0;
            self.render_finished = true;
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.hdma_lead = false;
            // An in-flight CGB FF45-write IRQ dies with the LCD
            // (gambatte: disabling cancels every scheduled memevent).
            self.lyc_if_delay = 0;
            self.flush_stat_copies();
            self.render.active = false;
            self.render.win_active = false;
            self.win_start_pending = false;
            // The post-switch exit-table latches die with the LCD
            // (the frame they classify is gone; SameBoy's freeze path is
            // LCD-bound). Inert flag-off: only tier2 STOPs set them.
            self.stop_anchor_set = false;
            self.stop_anchor_midframe = false;
            self.stop_leave_lcd_on = false;
            self.stop_leave_k = 2;
            let white = self.white();
            self.front.fill(white);
            self.legacy_level_edge();
        } else if !was_on && now_on {
            // LCD on: glitched first line (`lcdon_timing-GS`); the LYC
            // comparison restarts against LY=0 immediately and can raise
            // the STAT line in this very cycle (`stat_lyc_onoff` round 4).
            self.enabled = true;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            // The alignment shadow re-anchors at enable, like
            // SameBoy's `double_speed_alignment = 0` (memory.c:1510).
            self.sb_dsa8 = 0;
            // An enable re-anchors the PPU frame (the e-law: the DS
            // enable quantizes the phase to the 4-dot grid), so the
            // post-switch exit-table latches restart; record the enable's
            // speed (the lcdoff-dance −4 rp class term). Inert flag-off.
            self.stop_anchor_set = false;
            self.stop_anchor_midframe = false;
            self.stop_leave_lcd_on = false;
            self.stop_leave_k = 2;
            self.lcd_enable_in_ds = self.ds;
            // The event comparator's delayed FF45 copy restarts in sync
            // (gambatte lycIrq.lcdReset).
            self.lyc_event = self.lyc;
            self.glitch_line = true;
            // Hardware keeps the panel blank for the whole first frame
            // after enabling (see `frame_skip`).
            self.frame_skip = true;
            self.line_render_done = false;
            self.flip_dot = 0;
            self.vis_early = false;
            self.vis_hold_until = 0;
            self.render_finished = false;
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.hdma_lead = false;
            self.flush_stat_copies();
            self.render.active = false;
            self.wy_latch = false;
            self.win_line = 0xFF;
            self.win_start_pending = false;
            self.legacy_level_edge();
        }
    }
}
