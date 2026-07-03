//! STAT IRQ event engine: per-source predicates (m0/m1/m2/LYC) with delayed FF41/FF45 copies, mode readout, FF41-write trigger tables, edge/IF takers. Port of gambatte mstat_irq.h. Oracle: gbtr m2int/m0irq/lycm2int, gbmicrotest hblank_int/oam_int, mooneye intr_2_*/stat_irq_blocking.

use super::*;

impl Ppu {
    /// STAT mode bits as read through FF41 — the CPU-visible side of the
    /// two-latch model (HALFDOT-BUILD-PLAN Part C). This is *not* the rendering
    /// state machine: mode reads 0 during the first 4 dots of every line
    /// (and during 144:0-3), and mode 3 appears 4 dots after VRAM read
    /// locking (`lcdon_timing-GS` tables).
    ///
    /// **Part C (the law collapse):** under `tier2_reclock` the FF41 read's
    /// mode-3→0 verdict is ONE comparison — the read's exact half-dot position
    /// ([`Ppu::read_pos_hd`]) against the per-config CPU-visible mode-3 exit
    /// ([`Ppu::vis_exit_hd`]) — replacing the seven accreted shadow laws
    /// (#11z/#11ag window length · #11af/#11bd late-WY + boundary-WY ·
    /// #11at/#11bb pre-draw aborts · #11au reenable · #11aw un-trigger ·
    /// #11bc/#11ar unified bare exit + carries). The exit is DECOUPLED from
    /// the counter-pinned IRQ dispatch (`line_render_done` /
    /// `mode_for_interrupt`), which never moves (SameBoy `GB_STAT_update`
    /// two-latch model, display.c:523-574). Production is byte-identical
    /// (`tier2_reclock` off → native [`Self::vis_mode`]).
    pub(super) fn vis_mode_read(&self) -> u8 {
        let m = self.vis_mode();
        if !self.tier2_reclock {
            return m;
        }
        // C2 #11ar — the DS mode-2 ISR line-start read probes the mode0→2
        // (HBlank→OAM) LINE-START boundary, not the mode-3 exit: slopgb's
        // native flip lags SameBoy's, which flips at 8 MHz pos 4 = dot 2 (the
        // DS mode-bits lag). Scoped to the carried mode-2 ISR read
        // (`stat_rise_oam`), native mode 0, line-start dot < 4 (+1/−0
        // measured; the exhaustive per-class characterization flagged the
        // shared mode0→2 boundary as A/B risk, the scope confines it to
        // `m2int_m0stat`). Checked first: no mode-3-exit arm can match at
        // dot < 4 (the window arms need a same-line WX match ≥ ~dot 89; the
        // bare DS arm needs m == 3).
        if self.read_carried
            && self.stat_rise_oam
            && self.model.is_cgb()
            && self.ds
            && self.line >= 1
            && self.line < 144
            && m == 0
            && self.dot < 4
        {
            return if self.dot >= 2 { 2 } else { 0 };
        }
        // #11bg — the DS mode-2 ISR read at the mode2→3 ENTRY boundary: the
        // same +2 carried-read frame as the line-start arm above, applied to
        // the visible mode-3 entry (slopgb 84; the carried read's SameBoy
        // instant is dot+2). `m2int_m2stat_ds_1/_2` straddle it at dots
        // 80/82 (want 2/3); the entry is SCX-independent (`m2int_scx4_
        // m2stat_ds` — asm-pinned).
        if self.read_carried
            && self.stat_rise_oam
            && self.model.is_cgb()
            && self.ds
            && self.line >= 1
            && self.line < 144
            && m == 2
            && (80..84).contains(&self.dot)
        {
            return if self.dot + 2 >= 84 { 3 } else { 2 };
        }
        let Some(exit_adj) = self.vis_exit_hd(m) else {
            return m;
        };
        if self.read_pos_hd() < exit_adj { 3 } else { 0 }
    }

    /// Part C — the per-config CPU-visible mode-3→0 exit for the current FF41
    /// read, in 8 MHz half-dots on slopgb's line frame, with the read's own
    /// per-ISR carry ([`Ppu::isr_read_carry_hd`]) and the carried LCD phase
    /// (`lcd_phase_hd`, SS) already FOLDED (subtracted) so the caller compares
    /// plain [`Ppu::read_pos_hd`] `< exit`. `None` = no half-dot exit model
    /// for this config (the read returns the native [`Self::vis_mode`]).
    ///
    /// slopgb-frame constants relate to SameBoy's by the uniform +8 hd frame
    /// offset (`b-readsync-dualtrace-2026-07-02.md`: slopgb dot D ↔ SameBoy
    /// cfl·2+dc = 2D+8, both speeds). A read can match SEVERAL arms (e.g. a
    /// re-enabled triggering window matches the length arm AND the reenable
    /// arm); the source laws were ordered fall-through blocks, whose combined
    /// verdict folds to: `m == 3` arms (force-0 past their exit) take the
    /// MINIMUM matching exit, `m == 0` arms (hold-3 below their exit) the
    /// MAXIMUM. Each arm keeps the guards its source law was measured under:
    ///
    /// | arm | config | exit (slopgb dots) | source |
    /// |---|---|---|---|
    /// | 1 | active triggering window | `259 + SCX&7 + ds` | #11z/#11ag (SameBoy `SBex = 263 + SCX&7`, read offset +4) |
    /// | 2 | shadow late-WY extend (render bare, SameBoy window) | `263 + SCX&7 + ds` (polled) | #11af/#11ag/#11bd |
    /// | 3 | CGB pre-draw window-abort, SS | `253` (SCX penalty DROPPED, mattcurrie §WIN_EN) | #11at |
    /// | 4 | CGB pre-draw window-abort, DS | `254`; abort boundary `(89+WX)&!1` | #11bb |
    /// | 5 | CGB window re-enable too late to redraw | `253` | #11au |
    /// | 6 | CGB late-WY UN-trigger (SameBoy bare, slopgb window) | `253 + SCX&7` | #11aw |
    /// | 7 | boundary-WY cross-line extend | `263 + SCX&7 + ds` polled / `259 …` carried | #11bd item 4 |
    /// | 8 | bare line | SS: emergent `2*flip + 2` hd − carry − phase; DS: `508 + 2*(SCX&7) + 2*(SCX&1)` hd − carry | PORT 1 #11bc/#11ar |
    fn vis_exit_hd(&self, m: u8) -> Option<i32> {
        let scx7 = i32::from(self.scx & 7);
        let ds1 = i32::from(self.ds);
        let mut exit: Option<i32> = None;
        // Fold a matching arm's exit: min for the m==3 (force-0) class, max
        // for the m==0 (hold-3) class — the source laws' fall-through order.
        let fold = |exit: &mut Option<i32>, e: i32| {
            *exit = Some(match *exit {
                Some(cur) if m == 3 => cur.min(e),
                Some(cur) => cur.max(e),
                None => e,
            });
        };
        // Arm 1 — the triggering-window mode-3 length law (#11z, DS #11ag).
        // A triggering window's SameBoy exit is `SBex = 263 + SCX&7`; the
        // deferred read samples the PPU +4 dots before SameBoy reads the same
        // `ldh a,(FF41)` (MEASURED — `m2int_wx03_scx5_m3stat_2` slopgb dot264
        // ↔ SameBoy cfl268 = SBex), so the CPU-visible exit is `259 + SCX&7`
        // (+1 in DS: the deferred cc+0 ISR read lands +1 dot vs SS). LINE-0 /
        // first-window-line (wy2 == ly) excluded for ON-screen windows (their
        // trigger-line mode-3 extends LATER than the steady law; #11y) but
        // NOT for off-screen wx >= 0xA0 (renders nothing, no extend; #11ac).
        // Off-screen windows (wx A0-A6) extend with NO sprite penalty →
        // sprite-free lines only there; DS excludes sprite-laden lines
        // entirely (the real mode-3 end extends past the bare exit;
        // `10spritesPrLine_wx*_m3stat_ds_1` SameBoy-passes).
        if self.render.win_active
            && self.model.is_cgb()
            && self.line >= 1
            && self.eff.wx <= 0xA6
            && (self.eff.wx < 0xA0 || self.render.n_sprites == 0)
            && (!self.ds || self.render.n_sprites == 0)
            && !self.render.win_aborted
            && (self.wy2 != self.ly || self.eff.wx >= 0xA0)
            && self.wy2 <= 143
            && m == 3
        {
            fold(&mut exit, 2 * (259 + scx7 + ds1));
        }
        // #11bf item 3c — a mid-line WX rewrite committing AT/BEFORE the WX
        // match dot un-catches the window on SameBoy (`late_wx_scx5_1`: the
        // FF4B:=FF write and the match both at dot 97 → SameBoy bare; `_2`
        // at 101 → caught, extends) while slopgb's whole-dot render catches
        // first and extends both. SS, bare-sprite-free; the SS bare exit.
        // SCX&7 == 5 ONLY (measured: at scx0/2/3 SameBoy still catches the
        // same write≤match race — `late_wx_2`/`_scx2_2`/`_scx3_2`/`_ff_*_1`
        // all want 3; the un-scoped arm dropped all 8. The scx5 fine-scroll
        // phase is what pushes the effective catch past the write).
        if !self.ds
            && scx7 == 5
            && self.render.wx_write_dot != 0
            && self.render.wx_match_dot != 0
            && self.render.wx_write_dot <= self.render.wx_match_dot
            && self.render.win_active
            && self.model.is_cgb()
            && self.render.n_sprites == 0
            && !self.render.win_aborted
            && m == 3
        {
            fold(&mut exit, 2 * (253 + scx7));
        }
        // #11bf item 3a — a late-ENABLE-triggered window (the mid-line
        // LCDC.5 write IS the trigger, `Render::win_enable_dot`) whose
        // enable lands past the line's fetch-catch deadline renders BARE on
        // SameBoy — the window misses this line entirely — while slopgb's
        // whole-dot render still activates and extends (`late_enable_ly0_ds`
        // want-pair: enable dot 94 → native extend holds (want 3, no arm);
        // dot 96 → SameBoy bare (want 0), both legs reading the identical
        // dot 260 — the enable dot is the only discriminator). DS-scoped,
        // bare-sprite-free lines; the DS bare exit form (PORT 1).
        if self.ds
            && self.render.win_enable_dot > 94
            && self.render.win_active
            && self.model.is_cgb()
            && self.render.n_sprites == 0
            && !self.render.win_aborted
            && self.wy2 <= 143
            && m == 3
        {
            fold(&mut exit, 508 + 2 * scx7 + 2 * i32::from(self.scx & 1));
        }
        // Arm 2 — the shadow late-WY extend (#11af; line 0 included #11bd).
        // slopgb's discrete `wy_latch` sampler misses the mid-line late-WY
        // write SameBoy's continuous `wy_check` catches, so slopgb renders the
        // line BARE (native m == 0) where SameBoy's window triggered and
        // extended mode 3 to the POLLED exit `263 + SCX&7` (+0 ISR offset —
        // these reads carry no mode-2 dispatch; #11z). The shadow
        // [`Self::win_extends_sb`] re-derives SameBoy's trigger decision.
        // Sprite-laden DS lines excluded (the shadow's bare exit carries no
        // sprite penalty).
        if self.model.is_cgb()
            && self.line < 144
            && m == 0
            && !self.render.win_active
            && (!self.ds || self.render.n_sprites == 0)
            && self.win_extends_sb()
        {
            fold(&mut exit, 2 * (263 + scx7 + ds1));
        }
        // Arm 3 — the CGB PRE-DRAW window-abort bare exit, SS (#11at). A
        // window disabled by an LCDC.5 clear BEFORE its first fetch renders
        // BARE on SameBoy with the SCX fine-scroll penalty DROPPED
        // (mattcurrie §WIN_EN) → exit cfl257 = slopgb 253, NOT 257+SCX&7;
        // slopgb's whole-dot render over-extends. Boundary: the abort must
        // land before the window's first tile ships (~dot 106 for the scx03
        // early setup — `_1` abort104 bare / `_2` abort108 extend, ALL
        // wx0f-12; wx-INDEPENDENT, `wx_match+1`-relative REFUTED +6/−4). A
        // later abort catches the first tile and EXTENDS (per-config length —
        // the atomic render reclock's). Currently-DISABLED window only
        // (excludes late_reenable); bare non-sprite non-glitch CGB lines.
        if self.model.is_cgb()
            && !self.ds
            && self.render.win_predraw_abort
            && self.render.win_predraw_abort_dot <= 105
            && self.eff.lcdc & LCDC_WIN_ENABLE == 0
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.render.win_active
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            fold(&mut exit, 2 * 253);
        }
        // Arm 4 — the DS pre-draw abort twin (#11bb). SameBoy renders the
        // early aborts bare with the penalty dropped, exit `cfl257 dc2` (the
        // DS half-dot bare exit) = slopgb 254. The DS abort boundary is
        // wx-DEPENDENT: `(89 + WX) & !1` — the window's first-fetch M-cycle
        // start on the DS 2-dot grid (measured across all 8 wx0f-12 legs;
        // three candidates built + refuted first).
        if self.model.is_cgb()
            && self.ds
            && self.render.win_predraw_abort
            && self.render.win_predraw_abort_dot < (89 + u16::from(self.wx)) & !1
            && self.eff.lcdc & LCDC_WIN_ENABLE == 0
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.render.win_active
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            fold(&mut exit, 2 * 254);
        }
        // Arm 5 — the CGB window-REENABLE length, SS (#11au). A window
        // disabled then RE-enabled mid-mode-3 redraws from the re-enable
        // point; mode 3 extends past the read iff the re-enable beat the WX
        // redraw start (`reen <= wx_match − 3`, uniform — base wxmatch97:
        // reen92 extend / reen96 bare; wx0f wxmatch105: 100/104). The LATE
        // re-enable renders the tail BARE (exit 253); slopgb collapses both
        // to mode 3. SCX&7 <= 3 only (the fine-scroll shifts the redraw
        // deadline at high SCX — scx5 boundary 98 not 94, measured; scx5+
        // pass natively).
        if self.model.is_cgb()
            && !self.ds
            && self.render.win_reenable_dot != 0
            && self.render.wx_match_dot != 0
            && self.render.win_reenable_dot + 3 > self.render.wx_match_dot
            && self.scx & 7 <= 3
            && self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && self.render.win_active
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            fold(&mut exit, 2 * 253);
        }
        // Arm 6 — the CGB late-WY UN-trigger bare exit, SS (#11aw). SameBoy's
        // `wy_check` compares the IMMEDIATE WY; a late WY→(non-LY) write
        // un-triggers its window (line renders BARE) while slopgb — its
        // render + `wy_trig_sb` reading the 6-dot-lagged `wy2` — triggers and
        // over-extends. When slopgb's render triggered (`win_active`) but the
        // raw compare did NOT (`!wy_trig_sb_raw`), the line is SameBoy-bare:
        // exit `253 + SCX&7`.
        if self.model.is_cgb()
            && !self.ds
            && self.render.win_active
            && !self.wy_trig_sb_raw
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            fold(&mut exit, 2 * (253 + scx7));
        }
        // Arm 7 — the boundary-WY cross-line extend (#11bd item 4). A WY
        // write committing in a line's tail (dot >= 452) or head (dot < 4)
        // matching the CURRENT (old) line latches SameBoy's `wy_triggered`
        // (its scheduled `wy_check` compares the old `current_line`); every
        // later line renders the window where slopgb's render + wy2-lagged
        // shadow both miss it. Fires on the RAW sticky latch for lines the
        // render did NOT trigger, window still enabled + on-screen WX + a
        // same-line WX match (a late off-screen WX write or an enable that
        // missed the match window renders SameBoy-bare — `late_wx_ff_*_2`,
        // `late_enable_afterVblank_2`; the LCDC-enable latch is deliberately
        // NOT taken, 7 want-0 legs SameBoy-PASS bare). The exit is
        // read-class-dependent (#11z): POLLED reads sit at +0 of SameBoy's
        // `263 + SCX&7` exit; a carried STAT-ISR read at +4 → 259.
        if self.model.is_cgb()
            && self.line >= 1
            && self.line < 144
            && m == 0
            && !self.render.win_active
            && !self.render.win_aborted
            && self.wy_xline_trig
            && self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && self.eff.wx <= 0xA6
            && self.render.wx_match_dot != 0
            && !self.glitch_line
            && (!self.ds || self.render.n_sprites == 0)
            && std::env::var("SLOPGB_NOXLINE").is_err()
        {
            let base = if self.read_carried { 259 } else { 263 };
            fold(&mut exit, 2 * (base + scx7 + ds1));
        }
        // Arm 8 — PORT 1 (#11bc): the unified half-dot BARE-line mode-3 exit.
        // The read position is `read_pos_hd + isr_read_carry_hd + lcd_phase`
        // (folded into the returned exit); the exit is a per-speed half-dot
        // line constant:
        //
        //   SS: exit_hd = 2*flip + 2, EMERGENT from the render's own recorded
        //       flip (`flip_dot`) or its projection — NOT a live-`scx` closed
        //       form: a mid-line SCX write moves the exit exactly as the
        //       fine-scroll hunt resolved it (late_scx4 / scx_m3_extend; a
        //       closed form broke them, measured). For a clean steady line
        //       this equals `510 + 2*(SCX&7)` (flip 254+SCX&7), the constant
        //       the #11bc six-pin algebra derived (kernel pair + lcdon@253 +
        //       #11n m2int_scx3 + the speedchange4 fp dual-trace).
        //   DS: exit_hd = 508 + 2*(SCX&7) + 2*(SCX&1) — the #11ar full-carry
        //       law rewritten exactly on the half-dot grid.
        //
        // SS fires on native m ∈ {3, 0} — the true exit sits ±1 dot around
        // the whole-dot flip, BOTH directions needed (#11bd: the HOLD
        // direction is derivable only on the STOPADV-advanced frame;
        // speedchange4 scx2_1 reads AT the native flip dot and must still
        // read 3); DS keeps the `m == 3` gate. Bare non-sprite non-window
        // non-glitch lines, ARCH `self.scx` (the #11bb write-strobe rule).
        // SS reads add the carried LCD phase (the per-leave m3stat read-frame
        // surplus over the machine epoch; 0 for never-switched ROMs); DS
        // keeps 0 — the DS post-leave segments are epoch-only (measured).
        // #11bg — the DS branch includes LINE 0: the gdma_cycles post-stall
        // polls land at ly0 (the corrected DS line-153 wake moved them −2
        // onto the flip straddle: `_1` dot252 want3 / `_2` dot254 want0 —
        // exactly the emergent exit 508 hd). SS keeps `line >= 1`.
        if (self.line >= 1 || self.ds)
            && self.line < 144
            && !self.render.win_active
            && !self.render.win_aborted
            && !self.wy_trig_sb
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            let carry = self.isr_read_carry_hd();
            if self.ds {
                if self.model.is_cgb() && m == 3 {
                    // Part C — the DS exit re-expressed EMERGENT (like SS):
                    // `2*flip − 2 + 2*(SCX&1)`, anchored to the render's own
                    // recorded/projected flip. For a steady bare DS line the
                    // flip is `255 + SCX&7` (DS lead 1), so this equals the
                    // shipped #11ar closed form `508 + 2*(SCX&7) + 2*(SCX&1)`
                    // exactly — byte-identical there — while a mid-line SCX
                    // rewrite that re-arms the fine-scroll hunt EXTENDS the
                    // exit with the render (`scx_m3_extend_ds`: SameBoy reads
                    // hd 660 want 3 / 664 want 0, slopgb frame — the closed
                    // form forced both to 0).
                    let flip = if self.line_render_done && self.flip_dot != 0 {
                        self.flip_dot
                    } else if self.render.active {
                        let (proj, lead) = self.flip_projection();
                        self.dot + proj.saturating_sub(lead)
                    } else {
                        255 + u16::from(self.scx & 7)
                    };
                    fold(
                        &mut exit,
                        2 * i32::from(flip) - 2 + 2 * i32::from(self.scx & 1) - carry,
                    );
                }
            } else if !self.wy_latch
                && self.wy2 != self.ly
                && !self.render.win_stalled
                && (m == 3 || m == 0)
                && (self.line_render_done || self.render.active)
            {
                let flip = if self.line_render_done && self.flip_dot != 0 {
                    self.flip_dot
                } else {
                    let (proj, lead) = self.flip_projection();
                    self.dot + proj.saturating_sub(lead)
                };
                let phase = i32::from(self.lcd_phase_hd);
                fold(&mut exit, 2 * i32::from(flip) + 2 - carry - phase);
            }
        }
        exit
    }

    /// C2 #11af shadow window-extend predicate (tier2 + CGB only). Fires ONLY
    /// for the mid-line late-WY trigger that slopgb's discrete `wy_latch`
    /// sampler missed: the WY-trigger ([`Self::wy_trig_sb`]) latched on THIS
    /// line, at/before the WX-activation dot ([`Render::wx_match_dot`]).
    ///
    /// The cross-line case (`trig_line < line`) is deliberately NOT handled:
    /// (a) the late-WY rows that trigger on an earlier line (`10to0`/`FFto0`/
    /// `FFto1` — WY written at the line boundary) land their write a line later
    /// in slopgb's deferred frame, so the shadow never latches them anyway; and
    /// (b) a window that genuinely latched earlier and draws every line is
    /// already `win_active` in slopgb's render (excluded by the caller), so a
    /// `!win_active` cross-line latch means the window was aborted / its WX or
    /// LCDC.5 toggled late (`late_wx`/`late_reenable`/`late_enable`) — SameBoy
    /// renders THOSE bare (`cfl 257`), so extending them is wrong.
    pub(super) fn win_extends_sb(&self) -> bool {
        self.wy_trig_sb
            && self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && self.wy_trig_sb_line == self.ly
            && self.render.wx_match_dot != 0
            // The trigger must beat the WX-activation dot. The +2 slack is the
            // wy2-copy phase difference: slopgb's `wy2` lags the WY write by 6
            // dots (CGB), SameBoy's `wy_check` catches it at write + ~4, so the
            // shadow `trigdot` runs 2 dots behind SameBoy's detection — the
            // late-WY `_1` (extend) vs `_2`/`_3` (miss) split sits exactly on
            // this 2-dot phase (`_1` trigdot = wxmatch + 1, `_2` = wxmatch + 5).
            // #11ag DS: the slack was +4 (`_1` trigdot 101 / `_2` 103 vs
            // wxmatch 97). #11bg: the corrected DS line-153 lyfc table moves
            // the LYC=153 wake — and with it every ISR-timed WY write in this
            // family — 2 dots earlier (`_1` 99 / `_2` 101), so the DS slack
            // re-derives to the SS value (+2); the same shift is what fixes
            // the `late_wy_ds` blocker trio outright.
            && self.wy_trig_sb_dot <= self.render.wx_match_dot + 2
    }

    pub(super) fn vis_mode(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.line >= 144 {
            if self.line == 144 && self.dot < 4 {
                0
            } else {
                1
            }
        } else if self.glitch_line {
            // Leading-edge (cc+0) reads sample the PPU 4 dots before the
            // trailing cc+4 view, so the glitch line's mode-3 window must be
            // back-dated the same 4 dots (single speed) to stay
            // observationally neutral: the ENTRY boundary (mode 0→3) moves
            // 78→74 and the EXIT (`line_render_done`, the visible 3→0 flip)
            // is anticipated by `vis_early` rising 4 dots early (the glitch
            // line takes the +4 `early_lead` in `m0_flip_events`). Both are
            // exactly the A7 read-offset back-date, restricted to `vis_mode`
            // — the OAM/VRAM/palette accessibility reads keep the raw
            // `GLITCH_MODE3_START` (they are byte-identical flag-on,
            // `lcdon_timing-GS` OAM/VRAM legs). Always 78 / never-`vis_early`
            // flag-OFF, so production is byte-identical.
            // C1.2: the −4 glitch back-date is LEADING-EDGE-ONLY (like
            // `mode3_entry_dot`). The Tier-2 deferred read samples the glitch
            // 0→3 entry at the trailing frame, so it takes no back-date — dot 74
            // made the deferred `lcdon_timing-GS` STAT read see mode 3 a full
            // M-cycle early (round-1 STAT $87 vs $84). 78 restores it.
            let start = if self.leading_edge_reads && !self.tier2_reclock && !self.ds {
                GLITCH_MODE3_START - 4
            } else {
                GLITCH_MODE3_START
            };
            if self.dot < start || self.line_render_done || self.vis_early {
                0
            } else {
                3
            }
        } else if self.dot < 4 {
            // CGB line 0: the vblank's mode 1 persists through dots 0-3
            // — there is no mode-0 gap before the OAM scan (wilbertpol
            // ly00_mode1_2-C vs ly00_mode1_0-GS; SameBoy display.c only
            // clears the mode bits at the line-0 LY-write dot on DMG;
            // gambatte getStat's mode-1 window runs to 3 cycles before
            // line 0's mode 2).
            u8::from(self.model.is_cgb() && self.line == 0)
        } else if self.dot < self.mode3_entry_dot() {
            2
        } else if !(self.line_render_done || self.vis_early) {
            // `vis_early` back-dates the CPU-visible mode→0 boundary to
            // SameBoy's frame on the flag-on path (3 dots before the dispatch
            // flip, bare single-speed lines); always false in production, so
            // this reads `line_render_done` exactly. See the field docs.
            3
        } else if self.dot < self.vis_hold_until {
            // Window vis-HOLD: a triggering window extends the CPU-visible
            // mode-3 PAST the dispatch flip to SameBoy's `263 + SCX&7` exit
            // (`vis_hold_until`, set in `m0_flip_events`). 0 (no hold) in
            // production, so byte-identical OFF. See the `vis_hold_until` docs.
            3
        } else {
            0
        }
    }

    /// The dot the CPU-visible STAT mode flips 2→3 (the mode-2 OAM scan end).
    ///
    /// Port Stage A7 — on the **leading-edge-only** (cc+0 read, eager machine)
    /// flag-on path the boundary is back-dated by the read offset (4 dots,
    /// single speed) to dot 80, so the cc+0 FF41 read reproduces the flag-off
    /// cc+4 mode-3 detection timing: the leading-edge read latches the PPU 4
    /// dots before the trailing view, and moving the boundary the same 4 dots
    /// makes that read **observationally neutral** for the mode-2→3 entry
    /// (mooneye `intr_2_mode3_timing` passes LE-only).
    ///
    /// Port Stage B C1 — the **Tier-2 deferred-commit** frame does NOT take
    /// that back-date (84, the flag-off value). The deferred read pays the
    /// previous M-cycle's parked debt — advancing the PPU to this cycle's
    /// leading edge — then samples; for the 2→3 ENTRY that lands the read at
    /// the trailing (cc+4) frame, not LE-only's 4-dots-early peek, so dot 80
    /// makes the deferred read see mode 3 a full M-cycle early (`test_iter 2`
    /// counts one poll, want two). 84 restores it (`intr_2_mode3_timing` passes
    /// flag-on, both models). The mode-0 *exit* differs (`early_lead`, gated on
    /// `tier2_reclock` in `m0_flip_events`): it keeps a back-date because the
    /// kernel separation needs the −1 net shift. Single speed only (the DS read
    /// offset is deferred with the rest of the DS back-dating); always 84
    /// flag-OFF, so production is byte-identical.
    fn mode3_entry_dot(&self) -> u16 {
        if self.leading_edge_reads && !self.tier2_reclock && !self.ds {
            80
        } else {
            84
        }
    }

    /// STAT mode bits (FF41 bits 0-1) as currently visible to the CPU, for
    /// the interconnect (FEA0-FEFF prohibited-area reads key on OAM locking).
    pub(crate) fn mode_bits(&self) -> u8 {
        self.vis_mode()
    }

    /// Current (line, dot) — the rendering-FSM position, for the interconnect's
    /// C1.3 post-halt-wake LY read-phase carry (`Interconnect::halt_ly_phase`).
    pub(crate) fn line_dot(&self) -> (u8, u16) {
        (self.line, self.dot)
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] came
    /// from the line-0 OAM rise and must miss the CPU's interrupt sample
    /// for the current M-cycle (see `stat_events_tick`).
    pub(crate) fn take_stat_late(&mut self) -> bool {
        std::mem::take(&mut self.stat_late)
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] was a
    /// second-half commit that the halt-exit sampler must miss for one
    /// M-cycle (see the `stat_halt_late` field docs). Drained by the
    /// interconnect into its `if_late` halt-wake mask.
    pub(crate) fn take_stat_halt_late(&mut self) -> bool {
        std::mem::take(&mut self.stat_halt_late)
    }

    /// #11aq (C2 read-position carry): whether the currently-pending STAT IRQ
    /// was raised by the mode-2 OAM line-start rise (vs mode-0/LYC). A sticky
    /// level read (not drained) by the interconnect's `dispatch_retime` to key
    /// the per-ISR deferred-read carry (see the [`Ppu::stat_rise_oam`] field +
    /// [`crate::ppu::m2carry_on`]).
    pub(crate) fn stat_rise_oam(&self) -> bool {
        self.stat_rise_oam
    }

    /// #11aq: whether the currently-pending STAT IRQ was the mode-0 HBlank rise
    /// (the +2-dot ISR read carry). See [`Ppu::stat_rise_m0`].
    pub(crate) fn stat_rise_m0(&self) -> bool {
        self.stat_rise_m0
    }

    /// #11bf (`SLOPGB_P2GRID`): whether the current line is the LCD-enable
    /// glitch line — its mode-0 engine rise is emitted at a different offset
    /// from the true (SameBoy) commit than normal lines' (rise == visexit vs
    /// visexit − 3, dual-trace measured), so the halt-wake visibility
    /// deadline carries a per-shape correction.
    pub(crate) fn glitch_line_now(&self) -> bool {
        self.glitch_line
    }

    /// #11ar: arm/disarm the SCOPED carried-read exit override (see the
    /// [`Ppu::read_carried`] field). `dispatch_retime` sets it after a STAT-ISR
    /// read carry; the interconnect clears it once the handler's FF41 read has
    /// resolved (one-shot).
    pub(crate) fn set_read_carried(&mut self, v: bool) {
        self.read_carried = v;
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] came
    /// from the mode-0 source rise (`m0_flip_events`). The interconnect
    /// drains this and, when the rise landed in the second half of the
    /// CPU's M-cycle, masks it from the halt-exit sampler for one
    /// M-cycle (see the `m0_rise` field docs).
    pub(crate) fn take_m0_rise(&mut self) -> bool {
        std::mem::take(&mut self.m0_rise)
    }

    /// The mode-3→mode-0 OAM/VRAM accessibility unblock's `lead_eighths`
    /// `Some(_)` if it fired on the dot just stepped, else `None` (see the
    /// `m0_access_flip` field docs). The interconnect stamps the edge at
    /// `event_phase(M0Access, cc, lead)` so a cc+2 MID-phase OAM read still
    /// sees mode 3 when the unblock lands in the cycle's second half.
    pub(crate) fn take_m0_access_flip(&mut self) -> Option<i8> {
        self.m0_access_flip.take()
    }

    /// The CGB palette-RAM unblock's `lead_eighths` `Some(_)` if it fired on
    /// the dot just stepped, else `None` (see the `pal_access_flip` field
    /// docs). The interconnect stamps the edge at
    /// `event_phase(PalAccess, cc, lead)` so a cc+2 MID-phase FF69/FF6B read
    /// still reads $FF while the palette is blocked.
    pub(crate) fn take_pal_access_flip(&mut self) -> Option<i8> {
        self.pal_access_flip.take()
    }

    /// The mode-3→mode-0 STAT mode-bit flip's `lead_eighths` `Some(_)` if it
    /// fired on the dot just stepped, else `None` (see the `m0_stat_flip`
    /// field docs). The interconnect stamps the edge at
    /// `event_phase(StatMode, cc, lead)` so a cc+2 MID-phase FF41 read in
    /// double speed still reads mode 3 when the flip straddles the M-cycle.
    pub(crate) fn take_m0_stat_flip(&mut self) -> Option<i8> {
        self.m0_stat_flip.take()
    }

    /// Level of the shared STAT interrupt line for the given enable bits.
    /// The LYC source uses the IRQ-side comparison (`cmp_irq` — the
    /// delayed `lyc_event` copy on CGB); FF41 reads show the live `cmp`.
    pub(super) fn stat_line_level(&self, en: u8) -> bool {
        let mut high = en & STAT_SRC_LYC != 0 && self.cmp_irq;
        if !self.enabled {
            // With the LCD off only the (frozen) LYC source persists
            // (`stat_lyc_onoff` round 2: no edge across off/on with cmp=1).
            return high;
        }
        let vm = self.vis_mode();
        // HBlank source: rises at the mode-0 IRQ event (`m0_src`, one
        // dot *before* the visible flip — gambatte memevent_m0irq one
        // xpos ahead of its m0 anchor) and holds through the hblank and
        // the next line's dots 0-3 (and 144:0-3) so consecutive sources
        // overlap and block each other (`stat_irq_blocking`). The
        // glitched post-enable prefix is not a real hblank.
        high |= en & STAT_SRC_HBLANK != 0
            && ((self.line <= 143 && self.m0_src) || (vm == 0 && self.dot < 4))
            && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
        // Vblank source. On CGB the level extends through line 0 dots
        // 0-3 together with the persisting visible mode 1 (gambatte
        // getStat's mode-1 window + the lycEnable lyc0_m1disable
        // cgb04c_outE0 rows: edges under it stay blocked).
        high |= en & STAT_SRC_VBLANK != 0
            && (self.line >= 145
                || (self.line == 144 && self.dot >= 4)
                || (self.model.is_cgb() && self.line == 0 && !self.glitch_line && self.dot < 4));
        if en & STAT_SRC_OAM != 0 {
            // The OAM *blocking level* spans the whole scan+render of every
            // visible line, dots 0..the mode-0 source rise — one dot past
            // the visible flip, so the hblank source takes over without a
            // gap (gambatte mstat_irq.h doM0Event: the m2 enable blocks
            // the m0 IRQ — m2int_m0irq_*_out0; the level also blocks the
            // LYC dot-4 edge, lycm2int). The IRQ itself is an *event* at
            // the line-start dots — see `stat_events_tick` (SameBoy display.c
            // mode_for_interrupt pulse). Line 0's level starts at dot 4
            // with the LY/LYC validity.
            let oam_window = self.line <= 143
                && !self.glitch_line
                && if self.dot < 4 {
                    // Line-start dots 0-3: the previous line's `m0_src`
                    // is still set; the level is high here exactly as
                    // before (line 0's starts at dot 4).
                    self.line != 0
                } else {
                    !self.m0_src
                };
            let cgb = self.model.is_cgb();
            // OAM pulse at vblank entry: one M-cycle before the vblank IF
            // on *both* families (wilbertpol intr_2_timing rounds 5-7 pin
            // MGB and CGB alike; gbmicrotest line_144_oam_int_b/c/d pin
            // DMG — `vblank_stat_intr-GS` sees it together with the
            // vblank IF through the DMG halt-late commit, see
            // `stat_events_tick`).
            let pulse144 = self.line == 144 && self.dot == 0;
            // DMG: the OAM source also pulses on every later vblank line
            // (`intr_1_2_timing-GS`: mode1→mode2 IRQ distance is 464 dots —
            // one line + 8 dots).
            let vblank_pulse = !cgb && (145..=153).contains(&self.line) && self.dot == 12;
            high |= oam_window || pulse144 || vblank_pulse;
        }
        high
    }

    /// Recompute the readable comparison flag (`cmp`), the IRQ-side
    /// comparison (`cmp_irq`) and the legacy line level (`stat_line` —
    /// kept for the LCD-off edge path and the CGB FF45 trigger's level
    /// check; IF emission no longer hangs on it).
    pub(super) fn refresh_cmp(&mut self, from_tick: bool) {
        if self.enabled {
            self.cmp = self.compare_ly() == Some(self.lyc);
            if !self.model.is_cgb() {
                self.cmp_irq = self.cmp;
            } else if !from_tick
                || self.glitch_line
                || self.dot == 0
                || self.dot == 4
                || (self.line == 153 && (self.dot == 8 || self.dot == 12))
            {
                // The IRQ-side comparison is event-clocked on CGB: it
                // re-evaluates at the window-boundary dots and on
                // register writes, against the delayed `lyc_event` copy
                // — a copy that caught up *between* events changes the
                // line level only at the next event (no IRQ for an FF45
                // write that lands inside its line's protected window:
                // wilbertpol ly_lyc_write-C round 4).
                self.cmp_irq = self.compare_ly_irq() == Some(self.lyc_event);
            }
        }
        self.stat_line = self.stat_line_level(self.stat_en);
    }

    /// Register-write edge for the LCD-off state and LCDC transitions:
    /// with the LCD off only the frozen LYC source contributes
    /// (`stat_lyc_onoff`), and the enable transition can raise the line
    /// in its own cycle (round 4).
    pub(super) fn legacy_level_edge(&mut self) {
        let was = self.stat_line;
        self.refresh_cmp(false);
        if self.stat_line && !was {
            self.pending_if |= IF_STAT;
        }
    }

    /// Per-source STAT IRQ events, fired from the dot clock (gambatte
    /// mstat_irq.h `MStatIrqEvent` + lyc_irq.cpp `LycIrq`, ported
    /// function by function). There is no wired-OR STAT line on the IRQ
    /// side: each source is an *event* whose rise is allowed or
    /// suppressed by a predicate over the *other* sources' enables —
    /// sampled through delayed register copies — and the delayed LYC
    /// value. Truth table (live = `stat_en` at the event tick; ev/evl =
    /// the delayed [`Self::stat_ev`]/[`Self::stat_lyc_ev`] FF41 copies;
    /// lycm/lyce = the delayed [`Self::lyc_ev_m`]/[`Self::lyc_event`]
    /// FF45 copies):
    ///
    /// | event (line, dot) | exists iff | fires iff (provenance) |
    /// |---|---|---|
    /// | m2 pulse (N∈1-144, 0) | live m2en ∧ ¬live m0en | ¬(ev lycen ∧ lycm = N−1) — `doM2Event` blockedByLycIrq compares the *previous* line (its compare is still held at the pulse dot); the ¬m0en exists-gate is `mode2IrqSchedule` routing every per-line event to the line-0 slot while m0en is set |
    /// | m2 pulse (0, 4) | live m2en | ¬(ev m1en) ∧ ¬(ev lycen ∧ lycm = 0) — `doM2Event` blockedByM1Irq + blockedByLycIrq |
    /// | m2 pulse, DMG only (N∈145-153, 12) | live m2en | ¬(live m1en) ∧ ¬(live lycen ∧ cmp_irq) — no gambatte equivalent (mooneye `intr_1_2_timing-GS`); keeps the pre-port level blocking |
    /// | m0 rise (`m0_flip_events`) | (live ∨ ev) m0en | ¬(ev lycen ∧ lycm = N) — `doM0Event`: *not* blocked by m2en (lcdirq_precedence/m0irq_ly44_lcdstat28) |
    /// | m1 (144, 4) | live m1en | ¬(ev ∧ (m2en ∨ m0en)) — `doM1Event` |
    /// | LYC (N∈1-153, 4), lyce = N | (live ∨ evl) lycen | N ∈ 1-144: ¬(evl m2en); else ¬(evl m1en) — `LycIrq::doEvent` + `lycIrqBlockedByM2OrM1StatIrq` (keyed on the LYC *value*, so LYC=144 is m2-blocked and never m1-blocked) |
    /// | LYC=0 (153, 12), lyce = 0 | (live ∨ evl) lycen | ¬(evl m1en) |
    ///
    /// Emission masks: the (N,0) pulses are second-half commits
    /// (`stat_late` + `stat_halt_late`; the CGB 144 entry is exempt —
    /// misc/ppu/vblank_stat_intr-C), the (0,4) pulse is dispatch-late
    /// (`stat_late`; SameBoy "except on line 0", mealybug's "line 0
    /// timing is different by 4 cycles" handlers), the m0 rise carries
    /// the half-cycle halt law (`m0_rise`); LYC and m1 events commit
    /// plain.
    pub(super) fn stat_events_tick(&mut self) {
        self.refresh_cmp(true);
        let cgb = self.model.is_cgb();
        let live = self.stat_en;
        let ev = self.stat_ev;
        let evl = self.stat_lyc_ev;
        let mut fired = 0u8;

        // m2 line-start pulse (a CGB STAT write committing in this same
        // M-cycle still reaches the pulse — handled retroactively in the
        // FF41 write path, see `m2_pulse_fires`).
        if !self.glitch_line
            && self.dot == 0
            && (1..=144).contains(&self.line)
            && self.m2_pulse_fires(live)
        {
            fired |= IF_STAT;
            if !(cgb && self.line == 144) {
                self.stat_late = true;
                self.stat_halt_late = true;
            }
        }
        if !self.glitch_line && self.line == 0 && self.dot == 4 {
            // m2 line-0 pulse (the one slot that survives the m0en
            // schedule routing).
            if live & STAT_SRC_OAM != 0
                && ev & STAT_SRC_VBLANK == 0
                && !(ev & STAT_SRC_LYC != 0 && self.lyc_ev_m == 0)
            {
                fired |= IF_STAT;
                self.stat_late = true;
            }
        }
        if !cgb && (145..=153).contains(&self.line) && self.dot == 12 {
            // DMG vblank-line OAM pulses.
            if live & STAT_SRC_OAM != 0
                && live & STAT_SRC_VBLANK == 0
                && !(live & STAT_SRC_LYC != 0 && self.cmp_irq)
            {
                fired |= IF_STAT;
            }
        }
        if self.line == 144 && self.dot == 4 {
            // m1 event, one M-cycle after the 144:0 pulse, together with
            // the vblank IF.
            if live & STAT_SRC_VBLANK != 0 && ev & (STAT_SRC_OAM | STAT_SRC_HBLANK) == 0 {
                fired |= IF_STAT;
            }
        }
        if std::mem::take(&mut self.m0_rise_dot) {
            // m0 event on the visible flip's dot (incl. un-flip refires).
            // The m0 event's delayed view is one M-cycle *fresher* than
            // the m1/m2 events': the mstat_irq guards are uniform in
            // gambatte cc, but the m0 event dot carries a smaller
            // calibration skew on our grid, so a write in the preceding
            // M-cycle already lands (m0enable disable_1 out0 vs
            // disable_2 out2 pin the 2-dot cell) — take pending staged
            // values that are within their last 3 dots.
            let ev_m0 = self.stat_ev_fresh();
            let lyc_m0 = self.lyc_ev_m_fresh();
            if (live | ev_m0) & STAT_SRC_HBLANK != 0
                && !(ev_m0 & STAT_SRC_LYC != 0 && lyc_m0 == self.line)
            {
                fired |= IF_STAT;
                self.m0_rise = true;
            }
        }
        // LYC events: once per frame at the (delayed) LYC value's line.
        let lyc_val = if self.glitch_line {
            None
        } else if self.line >= 1 && self.dot == 4 && self.lyc_event == self.line {
            Some(self.line)
        } else if self.line == 153 && self.dot == 12 && self.lyc_event == 0 {
            Some(0)
        } else {
            None
        };
        if let Some(value) = lyc_val {
            let blocker = if (1..=144).contains(&value) {
                STAT_SRC_OAM
            } else {
                STAT_SRC_VBLANK
            };
            // The enable side ORs the live registers with the delayed
            // copy (gambatte's `(statReg_ | statRegSrc_) & lycirqen`):
            // both a just-enabled and a just-disabled source fire.
            if (live | evl) & STAT_SRC_LYC != 0 && evl & blocker == 0 {
                fired |= IF_STAT;
            }
        }
        self.pending_if |= fired;
    }

    /// gambatte `statChangeTriggersStatIrqDmg`: the DMG STAT-write glitch
    /// — the write momentarily enables every source (Pan Docs "STAT
    /// bug"), raising IF from the hblank/vblank levels and the held LYC
    /// match (never from the mode-2 condition), suppressed per source
    /// when the corresponding *old* enable already held the line high.
    /// Independent of the written value. gbmicrotest
    /// stat_write_glitch_l0/l1/l143/l154 pin the position grid.
    pub(super) fn stat_write_trigger_dmg(&self, old: u8) -> bool {
        let lyc_high = self.lyc_period();
        // Visible-line region (gambatte ly < 144: our dots 0-3 still
        // belong to the previous line on their grid, so line 144's first
        // M-cycle is still "line 143, hblank").
        if self.line <= 143 || (self.line == 144 && self.dot < 4) {
            // This line's mode-0 time passed = a real hblank (the
            // LCD-enable glitch prefix is not one).
            let hblank = (self.m0_src || self.dot < 4)
                && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
            if hblank {
                old & STAT_SRC_HBLANK == 0 && !(lyc_high && old & STAT_SRC_LYC != 0)
            } else {
                // Mode 2/3: only the LYC condition fires the glitch.
                lyc_high && old & STAT_SRC_LYC == 0
            }
        } else {
            old & STAT_SRC_VBLANK == 0 && !(lyc_high && old & STAT_SRC_LYC != 0)
        }
    }

    /// gambatte `statChangeTriggersStatIrqCgb` (+ the M2/M0LycOrM1
    /// helpers): CGB STAT writes raise IF only for newly-enabled
    /// sources —
    /// * lyc: enabling while the held compare matches fires anywhere
    ///   (an old lyc enable suppresses everything);
    /// * m0: enabling during mode 2/3 of a visible line fires at the
    ///   write; in the hblank it raises nothing;
    /// * m1: enabling during vblank fires, except in mode 1's last
    ///   M-cycle (line 0 dots 0-3, where only the lyc condition can
    ///   fire — the old `m1_tail_veto`);
    /// * m2: only in the last M-cycle before a visible line's pulse
    ///   (`statChangeTriggersM2IrqCgb`; the m2enable late_enable
    ///   ladders pin the window).
    pub(super) fn stat_write_trigger_cgb(&self, old: u8, data: u8) -> bool {
        if data & !old & STAT_SRC_ALL == 0 {
            return false;
        }
        // The CGB write's compare view: the trigger-side compare has
        // already switched to the new line at our dot 0 (gambatte's CGB
        // write cc sits later against getLycCmpLy's −2 switch:
        // miscmstatirq m1statwirq_trigger_ly94 round 2 fires its m1
        // enable at the line boundary because the LYC=148 period has
        // ended, while lycEnable lyc_ff41_enable_3's same-cell enable
        // still matches its own line and fires).
        // #11bd: classify the write on the un-shifted calibrated frame (the
        // machine STOPADV advance moved post-leave writes deeper into the
        // frame; identity for never-switched ROMs).
        let (ll, ld) = self.law_pos();
        let cmp_cgb = if self.glitch_line {
            0
        } else {
            match (ll, ld) {
                (0, _) => 0,
                (153, 0..=7) => 153,
                (153, _) => 0,
                (line, _) => line,
            }
        };
        let lyc_high = self.lyc == cmp_cgb;
        if lyc_high && old & STAT_SRC_LYC != 0 {
            return false;
        }
        // #11bg — unshifted CGB single-speed Tier-2: the engine's two-phase
        // FF41 view (`eng_stat_pending`) owns the LYC-source write fires (the
        // bit6-late continuity fire at commit+4 + external edges against the
        // armed old bit6), replacing the write-instant lyc arms below. The
        // shifted (lcd-offset) frames keep the calibrated arms — their write
        // law positions are one poll quantum ambiguous (#11bd).
        let eng_lyc = self.leading_edge_reads && !self.ds && self.lcd_shift_dots == 0;
        // The engine owns the LINE-BOUNDARY region (where the staged view +
        // the lyfc schedule decide); a MID-LINE enable keeps gambatte's
        // write-instant fire — the `lyc_ff41_trigger_delay` pair collapses to
        // one deferred commit dot (both legs dot 77, measured), so only the
        // calibrated write-instant arm can satisfy it.
        let eng_boundary = eng_lyc && !(16..448).contains(&ld);
        let lyc_fire = !eng_boundary && lyc_high && data & STAT_SRC_LYC != 0;
        // Port Stage C / S5 (mech 3 — the dispatch-class write-trigger, the LYC
        // sub-family). The lcd-offset shifts `late_ff41_enable_lcdoffset1_1`'s
        // LYC-source enable into the line-start carryover (dots 0-3 of lines
        // 1-143, which on the gambatte grid still belong to the previous line):
        // it sets LYC = ly-1 and enables LYC at `ly7 dot3`, where SameBoy fires
        // via the carryover compare (LYC matched the previous line) while
        // `cmp_cgb` has already switched to the new line (so `lyc_high` is false).
        // Under Tier-2 a fresh LYC enable matching the PREVIOUS line fires in the
        // carryover; `cmp_cgb` (pinning the new-line compare for the calibrated
        // m1statwirq/lyc_ff41_enable_3 rows) is untouched. Byte-identical OFF.
        let lyc_carryover = self.leading_edge_reads
            && !eng_lyc
            && (1..=143).contains(&ll)
            && ld < 4
            && old & STAT_SRC_LYC == 0
            && data & STAT_SRC_LYC != 0
            && self.lyc == ll - 1;
        // Port Stage C / S5 (mech 3 — the dispatch-class write-trigger, the ly153
        // LYC-WRAP sub-family). The lcd-offset shifts
        // `lyc153_late_ff41_enable_lcdoffset1_1`'s LYC enable into the ly153 LY=0
        // wrap window (dots 8-11), where `cmp_cgb`'s `(153, _) => 0` arm has
        // already wrapped to 0 (≠ lyc=153) so `lyc_high` is false — yet the held
        // `lyc_interrupt_line` latch is still TRUE there (SameBoy holds it across
        // the line-153 `ly_for_comparison == -1` gaps, `display.c:534`; the latch
        // matched 153 at the dot-6 step and only drops at the dot-12 LY=0 step).
        // SameBoy fires the fresh enable at `ly153 cfl0 lyc_line=1` (measured
        // `SBLEVEL 0->1 stat=c5`); slopgb's `cmp_cgb`-snapshot `lyc_fire` missed
        // it. Under Tier-2 a fresh LYC enable at line 153 with the held latch high
        // (the real-state discriminator, not the offset) fires; `cmp_cgb` (pinning
        // the dot-6 base `lyc153_late_ff41_enable_1` compare) is untouched.
        // Byte-identical OFF.
        // #11bd: on a shifted ROM the held latch is engine state at the REAL
        // instant — the write that law-lands inside the dots-6..11 latch window
        // arrives after the real latch dropped, so re-derive the latch at the
        // law position (LYC=153 matched at the dot-6 step, drops at the dot-12
        // LY=0 step).
        let latch_at_law = if self.lcd_shift_dots == 0 {
            self.lyc_interrupt_line
        } else {
            self.lyc_interrupt_line || (self.lyc == 153 && (6..12).contains(&ld))
        };
        let lyc_wrap_153 = self.leading_edge_reads
            && !eng_lyc
            && ll == 153
            && latch_at_law
            && !lyc_high
            && old & STAT_SRC_LYC == 0
            && data & STAT_SRC_LYC != 0;
        // m2 sub-trigger window (kept from the pre-port calibration;
        // gambatte's ly==143 and ly==153 branches are empty at single
        // speed, so the (144,0) and (0,0) cells never fire it).
        let m2 = old & STAT_SRC_OAM == 0
            && data & (STAT_SRC_OAM | STAT_SRC_HBLANK) == STAT_SRC_OAM
            && (1..=143).contains(&ll)
            && ld < 2 + 2 * u16::from(self.ds);
        // gambatte's ly-region split on our grid: dots 0-3 still belong
        // to the previous line, so (0, 0-3) is mode 1's tail and
        // (144, 0-3) line 143's hblank (an m0 enable written there still
        // fires: m1/ly143_late_m0enable_ds_1 cgb04c_out3).
        let vis = (ll <= 143 && !(ll == 0 && ld < 4)) || (ll == 144 && ld < 4);
        let main = if vis {
            // A scheduled mode-0 event still ahead within this line
            // (gambatte `eventTimes_(memevent_m0irq) <
            // lyCounter.time()`): the write trigger defers to it. The
            // m0irq event is (re)scheduled with the *new* enables before
            // the trigger check, so a fresh m0 enable during mode 2/3
            // stays silent (its event fires instead: m0enable
            // late_enable_1), while the same enable in the hblank — the
            // prediction then points at the next line, beyond the LY
            // increment — raises IF at the write (m1/m1irq_m0enable_1).
            let crossed = self.m0_src && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
            let m0_pending = !crossed && (old | data) & STAT_SRC_HBLANK != 0;
            // Line-boundary tail (`timeToNextLy <= 4 + 4*ds`).
            let tail = ld < 4;
            // Port Stage C / S5 (mech 3 — the dispatch-class write-trigger, the
            // HBlank sub-family). At the dots-0-3 line-start tail the gambatte
            // logic defers a fresh m0 enable to the scheduled m0irq event; but
            // when the PREVIOUS line's mode-0 has already passed (`vis_mode==0`
            // hblank carryover, not the pre-mode-0 tail), that scheduled event
            // points at the NEXT line's mode-0 — beyond the LY increment — so
            // the deferral loses the IRQ before the cc+0 read samples it. The
            // lcd-offset shifts these late enables into exactly this carryover
            // tail (`late_enable_lcdoffset1_1` writes FF41 at `ly dot3`), where
            // SameBoy raises IF at the write. Under Tier-2 (`leading_edge_reads`)
            // a fresh m0 enable in the carryover tail raises IF immediately;
            // glitch lines excluded (the LCD-enable prefix is not a real
            // hblank). Never set in production / LE-only → byte-identical OFF.
            //
            // Double speed HALVES the carryover window: the deferred cc+0 write
            // lands 2 dots earlier in the dot grid (the CPU runs at 2×), so the
            // `_ds_lcdoffset1_1` enable that fires lands `dot0` while its `_2`
            // sibling lands `dot2` — and at `dot2` SameBoy's fire is *early*
            // (cleared by the test's IF-clear) so it must NOT be delivered. A
            // `dot < 4` window would over-fire the `_2` enable; halve to `< 2`
            // (mirrors the `m2`/`stage_stat_copies` DS halving). LE-only.
            let carryover_tail = ld < if self.ds { 2 } else { 4 };
            // At a shifted position the vis check evaluates on the law frame
            // (ld < 4 at line start is always the mode-0 hold on non-glitch
            // lines); un-shifted keeps the live view.
            let vis0 = if self.lcd_shift_dots == 0 {
                self.vis_mode() == 0
            } else {
                true
            };
            if self.leading_edge_reads
                && carryover_tail
                && !self.glitch_line
                && vis0
                && old & STAT_SRC_HBLANK == 0
                && data & STAT_SRC_HBLANK != 0
            {
                return true;
            }
            if m0_pending || tail {
                lyc_fire
            } else if old & STAT_SRC_HBLANK != 0 {
                false
            } else {
                // #11bg -- under the two-phase engine view (unshifted CGB
                // SS Tier-2) the fresh-m0-enable fire moves to the ENGINE
                // (the phase-1 rise at its effective instant, where the
                // line-end OAM carryover in `update_mode_for_interrupt`
                // provides the hardware cutoff); the write-instant fire
                // would double up or fire enables the carryover blocks.
                (data & STAT_SRC_HBLANK != 0 && !eng_lyc) || lyc_fire
            }
        } else {
            // Vblank region. Mode 1's last M-cycle (line 0 dots 0-3)
            // doesn't fire a written m1 enable, and an old m1 enable
            // still suppresses a written lyc condition there (gambatte's
            // `old & m1irqen` arm; miscmstatirq
            // lycstatwirq_trigger_ly00_10_50_1 reads E0).
            // Port Stage C / S5 (mech 3 — the dispatch-class write-trigger, the
            // VBlank sub-family). The gambatte `m1_tail` (line 0 dots 0-3 = mode
            // 1's last M-cycle) suppresses a freshly-written m1 enable; but the
            // lcd-offset shifts `m1irq_late_enable_lcdoffset1_1`'s FF41 enable
            // into exactly that tail (`ly0 dot3`), where SameBoy fires the fresh
            // VBlank enable (out2) — slopgb delivered `if=00`. Under Tier-2
            // (`leading_edge_reads`) drop the `m1_tail` suppression so the fresh
            // VBlank enable raises IF; the `old & STAT_SRC_VBLANK` suppression and
            // the lyc arm (the #11k `lycstatwirq_trigger_ly00` E0 rows) are
            // untouched. Never set in production / LE-only → byte-identical OFF.
            let m1_tail = ll == 0 && ld < 4 && !self.leading_edge_reads;
            if old & STAT_SRC_VBLANK != 0 {
                false
            } else {
                (data & STAT_SRC_VBLANK != 0 && !m1_tail) || lyc_fire
            }
        };
        main || m2 || lyc_carryover || lyc_wrap_153
    }

    /// Stage the delayed event-register FF41 copies after a write
    /// (gambatte statRegChange guards): CGB copies land 6 dots after the
    /// architectural commit — an event in the following M-cycle still
    /// sees the old enables — DMG copies update immediately.
    pub(super) fn stage_stat_copies(&mut self) {
        if self.model.is_cgb() {
            // The guard windows are in machine cycles (`cc + 2*cgb <
            // nextEventTime`), so the dot spans halve in double speed.
            let k = if self.ds { 2 } else { 6 };
            self.stat_ev_staged = Some((self.stat_en, k));
            self.stat_lyc_ev_staged = Some((self.stat_en, k));
        } else {
            self.flush_stat_copies();
        }
    }

    /// The m0 event's (and the CGB line-start pulses') *fresher* view of
    /// the delayed copies: those events carry a smaller calibration skew
    /// on our dot grid, so a staged write within its last few dots
    /// already counts for them (m0enable disable_1/2 and
    /// lyc1_m2irq_late_lycdisable_1 pin the cells).
    fn stat_ev_fresh(&self) -> u8 {
        match self.stat_ev_staged {
            Some((v, d)) if d <= 3 => v,
            _ => self.stat_ev,
        }
    }

    fn lyc_ev_m_fresh(&self) -> u8 {
        match self.lyc_ev_m_staged {
            Some((v, d)) if d <= 1 => v,
            _ => self.lyc_ev_m,
        }
    }

    /// Predicate of the line-start m2 pulse (lines 1-144 dot 0) for the
    /// given live enables: exists iff m2 enabled and m0 not (gambatte
    /// mode2IrqSchedule routes every per-line event to the line-0 slot
    /// while m0en is set), blocked by the previous line's still-held LYC
    /// compare through the delayed copies (doM2Event blockedByLycIrq).
    /// Also consulted retroactively by the CGB FF41 write path: a write
    /// committing at the pulse's own M-cycle reaches it on CGB
    /// (m2enable lyc1_late_m2enable_lycdisable_1 cgb04c_out2 vs the same
    /// row's dmg08_out0).
    pub(super) fn m2_pulse_fires(&self, en: u8) -> bool {
        let (evp, lycp) = if self.model.is_cgb() {
            (self.stat_ev_fresh(), self.lyc_ev_m_fresh())
        } else {
            (self.stat_ev, self.lyc_ev_m)
        };
        en & STAT_SRC_OAM != 0
            && en & STAT_SRC_HBLANK == 0
            && !(evp & STAT_SRC_LYC != 0 && lycp == self.line - 1)
    }

    /// Synchronise every delayed event copy with the live registers
    /// (LCD transitions: gambatte lcdReset / LycIrq::lcdReset).
    pub(super) fn flush_stat_copies(&mut self) {
        self.stat_ev = self.stat_en;
        self.stat_ev_staged = None;
        self.stat_lyc_ev = self.stat_en;
        self.stat_lyc_ev_staged = None;
        self.lyc_ev_m = self.lyc;
        self.lyc_ev_m_staged = None;
    }
}
