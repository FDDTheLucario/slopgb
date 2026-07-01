//! STAT IRQ event engine: per-source predicates (m0/m1/m2/LYC) with delayed FF41/FF45 copies, mode readout, FF41-write trigger tables, edge/IF takers. Port of gambatte mstat_irq.h. Oracle: gbtr m2int/m0irq/lycm2int, gbmicrotest hblank_int/oam_int, mooneye intr_2_*/stat_irq_blocking.

use super::*;

impl Ppu {
    /// STAT mode bits as read through FF41. This is *not* the rendering
    /// state machine: mode reads 0 during the first 4 dots of every line
    /// (and during 144:0-3), and mode 3 appears 4 dots after VRAM read
    /// locking (`lcdon_timing-GS` tables).
    /// FF41-read STAT mode = [`Self::vis_mode`] + the C2 #11y/#11z window mode-3
    /// length law, applied ONLY to the FF41 register read (`regs.rs`), NOT the
    /// internal `vis_mode` consumers (`stat_line_level`/IRQ, `mode_bits`,
    /// `update_mode_for_interrupt`). A triggering window's SameBoy mode-3 exit is
    /// `SBex = 263 + SCX&7` (cfl), DECOUPLED from the counter-pinned
    /// `line_render_done`. The CPU-visible exit is `SBex − read_offset`: the
    /// deferred FF41 read samples the PPU `read_offset` dots BEFORE SameBoy reads
    /// the same `ldh a,(FF41)`. **#11z: that offset is +4 for the window m3stat
    /// reads (MEASURED — `m2int_wx03_scx5_m3stat_2` slopgb dot264 ↔ SameBoy
    /// cfl268 = SBex; `m2int_wx03_scx2_m3stat_1` dot260 ↔ cfl264), NOT the +3
    /// dispatch frame the #11y `260 + SCX&7` first used.** So the exit is
    /// `263 − 4 + SCX&7 = 259 + SCX&7`: it converts the scx5 `_2` over-extend
    /// rows (read dot264, want 0 — was mode 3 at exit 265) to mode 0, +2/−0
    /// full-CGB. The scx0 `_2` rows read robustly PAST the exit either way
    /// (dot260, SameBoy cfl265 > 263) so 259 vs 260 is invisible to them.
    /// LINE 0 EXCLUDED: the first window line (the WY-latch just matched) extends
    /// mode 3 later than the steady law and its `ly0` read frame differs (the
    /// `late_wy_*` rows; #11y). Tier2 + normal-trigger ly>=1 windows; production
    /// byte-identical OFF.
    pub(super) fn vis_mode_read(&self) -> u8 {
        let m = self.vis_mode();
        if self.tier2_reclock
            && self.render.win_active
            && self.model.is_cgb()
            && self.line >= 1
            // #11z extended to the off-screen window range (wx 0xA0..=0xA6,
            // wxA5/wxA6): SameBoy extends a triggering off-screen window's
            // mode-3 to the same `263 + SCX&7` exit, so the read-side law
            // applies. The off-screen extension carries NO sprite penalty in
            // the exit, so restrict the extended range to sprite-free lines
            // (a sprite pushes the real mode-3 end past `259+SCX&7`, and the
            // bare law would mis-shorten it — `m2int_wxA6_spxA7_m3stat_2`).
            && self.eff.wx <= 0xA6
            && (self.eff.wx < 0xA0 || self.render.n_sprites == 0)
            // #11ag DS: also exclude SPRITE-laden lines under double speed (SS
            // keeps allowing on-screen sprites — byte-identical). With sprites
            // the real mode-3 end extends PAST `260 + SCX&7`, and the DS read
            // frame straddles it so the bare exit mis-shortens the want-3 read
            // (`sprites/space/10spritesPrLine_wx*_m3stat_ds_1`, a SameBoy-pass).
            // The DS sprite-window exit is the #11t DS sprite read-grid, separate.
            && (!self.ds || self.render.n_sprites == 0)
            && !self.render.win_aborted
            // The first window line (wy2==ly) is excluded for ON-screen windows
            // (their mode-3 extends LATER than the steady 259+SCX&7 law on the
            // trigger line; #11y late_wy). But an OFF-SCREEN window (wx>=0xA0)
            // renders nothing, so its first line does NOT extend (SameBoy exit =
            // the bare 259+SCX&7) — the law applies there too (#11ac
            // `m2int_wxA6_firstline_m3stat_3`).
            && (self.wy2 != self.ly || self.eff.wx >= 0xA0)
            && self.wy2 <= 143
            && m == 3
            // #11ag DS: the FF41-read exit is `260 + SCX&7` in double speed (the
            // deferred cc+0 ISR read lands +3 dots before SameBoy's `SBex=263`,
            // vs the SS +4 → 259); MEASURED — `m2int_wxA6_scx5_m3stat_ds` reads
            // `_1` dot264 / `_2` dot266 so only exit 265 (=260+5) separates them
            // (the on-screen scx0 `_2` rows read dot260 robustly past either, so
            // 259 vs 260 is invisible to them — the SS legs stay byte-identical).
            && self.dot >= 259 + u16::from(self.eff.scx & 7) + u16::from(self.ds)
        {
            return 0;
        }
        // C2 #11af shadow late-WY extend (tier2 + CGB; polled reads). slopgb's
        // discrete `wy_latch` sampler misses the mid-line late-WY write that
        // SameBoy's continuous compare catches, so slopgb renders the line BARE
        // (native `m == 0` at the polled read dot) where SameBoy's window
        // triggered and extended mode 3 to `263 + SCX&7` (the POLLED exit, +0
        // ISR offset — these reads carry no mode-2 dispatch, #11z). The shadow
        // [`Self::win_extends_sb`] re-derives SameBoy's trigger decision; when
        // it holds, the FF41 read sees mode 3 to the polled exit. Disjoint from
        // the `win_active` law above (that gate requires the window already in
        // slopgb's render; this one fires only when it is NOT).
        if self.tier2_reclock
            && self.model.is_cgb()
            && self.line >= 1
            && self.line < 144
            && m == 0
            && !self.render.win_active
            // #11ag DS: exclude sprite-laden lines (same as the length law) — the
            // shadow's bare exit does not carry the sprite mode-3 penalty.
            && (!self.ds || self.render.n_sprites == 0)
            && self.win_extends_sb()
            // #11ag DS exit `264 + SCX&7` (the polled read lands +1 vs SS in DS:
            // `late_wy_FFto2_ly2_scx5_ds_1` reads dot268 / wants mode 3, so the
            // exit must clear 268 = 264+5; SS stays 263 — byte-identical).
            && self.dot < 263 + u16::from(self.eff.scx & 7) + u16::from(self.ds)
        {
            return 3;
        }
        // C2 #11at — the CGB PRE-DRAW window-abort BARE-exit shadow (SS). When
        // a window enabled at line start is disabled by an LCDC.5 clear BEFORE
        // it draws (`Render::win_predraw_abort`, set in `regs.rs::commit_eff`),
        // SameBoy renders the line BARE but DROPS the SCX fine-scroll penalty
        // (mattcurrie §WIN_EN: the BG resumes on a tile boundary, SCX&7 has no
        // effect) → mode-3 exit cfl257, NOT the normal bare 257+SCX&7. slopgb's
        // whole-dot render over-extends (mode3 at the read dot). The deferred
        // cc+0 read (read_offset 4 SS) reads mode0 iff dot + 4 >= 257.
        // MEASURED — `late_disable_early_scx03_wx{0f,10,11,12}_1`: LCDC.5 cleared
        // at dot104 while `!win_mode` (WX/WY not yet matched), SameBoy exit
        // cfl257, read at cfl260 → mode0 (want0); the `_2` siblings clear at
        // dot108 AFTER the window began drawing (`win_mode`, POST-draw, NOT
        // flagged), so their mode-3 EXTENDS by the drawn tiles (read mode3,
        // want3 — left to the atomic render-length reclock, since the extend is
        // a per-config length not a function of the abort dot: early_scx03
        // abort104→exit257 but non-early late_scx0 abort100→exit>260 at the same
        // read dot). Guarded to the currently-DISABLED window (excludes
        // late_reenable) + bare non-sprite non-glitch CGB lines. SS only (the DS
        // pre-draw abort = the S6 read grid). Tier2-gated; the flag is false
        // flag-off → byte-identical.
        if self.tier2_reclock
            && self.model.is_cgb()
            && !self.ds
            && self.render.win_predraw_abort
            // The pre-draw abort is fully BARE (exit cfl257) only when it lands
            // before the window's first tile ships (MEASURED ~dot106 for the
            // scx03 early setup: `_1` abort104 bare / `_2` abort108 extend, for
            // ALL wx0f-12 — the abort dot is wx-INDEPENDENT); a 1-M-cycle-later
            // abort catches the first tile and EXTENDS mode 3 (want mode3) — a
            // per-config window-tile-completion length the atomic render reclock
            // owns, NOT this bare law. `<= 105` scopes to the scx03 pre-first-tile
            // aborts. NOT `wx_match_dot`-relative (MEASURED REFUTED: the abort dot
            // is fixed 104/108 across wx while wx_match moves with WX, so
            // `wx_match+1` re-includes the higher-wx `_2` extends — +6/−4). The
            // late_scx_late_disable family (abort 124-132, boundary ~130) is a
            // distinct config constant left to the render reclock.
            && self.render.win_predraw_abort_dot <= 105
            && self.eff.lcdc & LCDC_WIN_ENABLE == 0
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.render.win_active
            && !self.glitch_line
            && self.render.n_sprites == 0
            && self.dot + 4 >= 257
        {
            return 0;
        }
        // C2 #11an UNIFIED bare-line read-frame law (EXPERIMENT, env-gated).
        // The dual-emulator trace (kernel m2int_m3stat_1/_2 + DS _2 +
        // late_disable) showed every FF41 mode read obeys:
        //   slopgb read at dot D  ⟺  SameBoy read at cfl D + read_offset
        // with read_offset = 4 (SS) / 3 (DS). So slopgb reproduces SameBoy's
        // verdict by comparing D against SameBoy's CONFIG render exit minus the
        // offset. The triggering-window law above is this with SBex = 263+SCX&7
        // (`259+SCX&7` SS). The BARE-line exit is SBex = 257 + SCX&7, so the
        // read-frame boundary is `257 + SCX&7 − read_offset = 253+SCX&7` (SS) /
        // `254+SCX&7` (DS). Anchored to SameBoy's CONFIG exit, NOT slopgb's own
        // render — so it also corrects late_disable, where slopgb's render
        // OVER-extends (window was active then disabled) but SameBoy renders the
        // line bare (exit 257). Bare non-sprite lines only for this first cut
        // (sprites push SBex past 257 — the penalty term is the next slice).
        if crate::ppu::barelaw_on()
            && self.tier2_reclock
            && self.model.is_cgb()
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.render.win_active
            && !self.glitch_line
            && self.render.n_sprites == 0
            && self.dot >= 253 + u16::from(self.eff.scx & 7) + u16::from(self.ds)
        {
            return 0;
        }
        // C2 #11ap HALF-DOT bare-line exit, RAISE-ODD (DS, env-gated
        // `SLOPGB_HDEXIT`; co-lands with `SLOPGB_DSM2DELAY`). The #11ao
        // scx-parity refutation localized the residual to a sub-dot exit.
        // The DSM2DELAY dispatch delay shifts the DS mode-2 FF41 read +2 dots,
        // separating it from the mode-0 (m0int) ISR read it otherwise collides
        // with (both land slopgb dot 254 at scx0, opposite wants — no exit
        // threshold can split them). With that read-position separation, EVEN
        // SCX resolves on the native exit `255 + SCX&7`; but the shifted ODD-SCX
        // `_ds_1` read lands EXACTLY on the native exit (read == exit → mode 0,
        // want 3). SameBoy's CPU-visible exit rounds UP to the even read grid:
        // `255 + SCX&7 + (SCX&1)`, so odd SCX stays mode 3 one dot longer. This
        // RAISE-odd override returns mode 3 in the `[native_exit, raised_exit)`
        // gap — a no-op for even SCX (whose gap is empty: the even reads never
        // hit the odd `255+SCX&7` boundary). The half-dot resolution expressed
        // on slopgb's whole (even) read grid as a `+(SCX&1)` parity term.
        // MEASURED m2int_m3stat DS scx0-7 both legs (DSM2DELAY=1). DS-only: SS
        // reads already resolve. Bare non-sprite CGB lines.
        if crate::ppu::hdexit_on()
            && self.tier2_reclock
            && self.model.is_cgb()
            && self.ds
            && self.line >= 1
            && self.line < 144
            && m == 0
            && self.dot >= 250
            && self.dot < 255 + u16::from(self.eff.scx & 7) + u16::from(self.eff.scx & 1)
            && !self.render.win_active
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            return 3;
        }
        // C2 #11ar SCOPED carried-read exit OVERRIDE (env-gated `SLOPGB_CARRYOVR`,
        // co-lands with `SLOPGB_M2CARRY`). The carry moved THIS read (an OAM/
        // HBlank STAT-ISR handler read, flagged by `read_carried`) to SameBoy's
        // absolute cfl, so the read-frame offset is 0 and the verdict is
        // SameBoy's mode AT that cfl: `mode 3 iff cfl < SBex`, a FULL 3↔0
        // override of slopgb's native mode. This unifies the two prior
        // half-laws — `M2HOLD` (holds 3 for a native-0 read past slopgb's low
        // exit, e.g. m2int_ds_1) and `BARELAW` (forces 0 for a native-3
        // over-extended render, e.g. late_disable_ds_1) — which at the SAME
        // carried dot want OPPOSITE directions. Crucially SCOPED to `read_carried`
        // (the #11aq −50 fix): the blanket exit hold mis-framed non-carried
        // polled reads whose native frame was already right. `SBex = 257 + SCX&7
        // + ds` (+ the #11ap `SCX&1` half-dot parity on the even DS read grid).
        // DS-only (the carry is DS-only); bare non-sprite non-glitch CGB lines.
        // C2 #11ar-full — the FULL per-read carry + ONE SBex exit (the goal's
        // "carry EVERY deferred read to SameBoy's cfl + one render-length exit,
        // globally consistent"). Applies the SBex verdict to EVERY bare mode-3
        // FF41 read — carried STAT-ISR reads AND polled reads alike — via a
        // transient read-frame offset (a PEEK, no machine advance, so the
        // counter-pinned dispatch dot + IF delivery stay put, mooneye 91/91):
        //
        //   off = (read_carried && HBlank-source) ? 2 : 4      // the deferred
        //         cc+0 read lands 4 dots before SameBoy's cc+4 frame (the leading-
        //         edge default); only the mode-0 (HBlank) STAT-ISR read is +2
        //         (MEASURED — #11aq: OAM ISR +4, HBlank ISR +2; polled = +4).
        //   verdict = (dot + off) < SBex ? 3 : 0                // SBex = SameBoy's
        //         bare mode-3 exit 257 + SCX&7 + ds (+ the #11ap SCX&1 half-dot
        //         parity on the even DS read grid).
        //
        // This UNIFIES the two shipped scoped peeks (m2int_m3stat +6, the carried
        // read; the dma polled reads +2) into one global law: the POLLED_OFF
        // sweep is +7/−0 at off 0-3, **+9/−0 at off 4-5** (the clean plateau —
        // gdma_cycles_long_ds_2 + hdma_cycles_ds_2 land at SameBoy's frame),
        // A/B-swapping only at off ≥ 6 (the co-temporal dma `_ds_1` siblings). The
        // guards keep it exact: bare (`!win_active`/`!win_aborted`/`!wy_trig_sb`
        // excludes the co-temporal late_disable + window render-length A/B pairs),
        // non-glitch, non-sprite, DS, CGB, `m == 3` (mode-3 reads only — the
        // m0stat/m2stat reads probe a different boundary and keep native). The
        // residual 123 blockers are NOT bare-mode-3 FF41 reads (FF0F IF-delivery /
        // VRAM/OAM/palette accessibility / co-temporal wake) — structurally
        // unreachable by any FF41 verdict law, needing the S4/S6/IF-lifecycle
        // ports. Tier2-unconditional; byte-identical flag-OFF.
        if self.tier2_reclock
            && self.model.is_cgb()
            && self.ds
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.render.win_active
            && !self.render.win_aborted
            && !self.wy_trig_sb
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            let off = if self.read_carried && self.stat_rise_m0 {
                2
            } else {
                4
            };
            let sbex = 257
                + u16::from(self.eff.scx & 7)
                + u16::from(self.ds)
                + u16::from(self.eff.scx & 1);
            return if self.dot + off < sbex { 3 } else { 0 };
        }
        // C2 #11ar-wake ATTEMPT (WAKE-CLOCK class, env-gated `SLOPGB_WAKEPEEK`).
        // The mode-0-source (HBlank) STAT-ISR halt-wake FF41 read resumes at a
        // line-start dot ~4 reading mode 2 (OAM), where SameBoy — reading at cfl0
        // (its mode bits lag the OAM flip by 4 T) — sees mode 0 (the HBlank tail).
        // Force line-start mode-2→0 for the carried mode-0-source wake read.
        // Scoped to `stat_rise_m0` (excludes the m2int OAM reads that legitimately
        // want mode 2) + native `m == 2` (excludes the want-2 `scx3_3b` which reads
        // mode 0 already, and the `dec` want-6 which reads mode 3). Two-binned.
        if crate::ppu::wakepeek_on()
            && self.read_carried
            && self.stat_rise_m0
            && self.tier2_reclock
            && self.model.is_cgb()
            && m == 2
            && self.line >= 2
            && self.line < 144
            && self.dot >= 1
            && self.dot <= 4
        {
            return 0;
        }
        // C2 #11ar-m0stat READ-FRAME slice (the second clean read-position slice,
        // +1/−0). The m2int mode-2 OAM ISR reads FF41 at line-start checking the
        // mode0→2 (HBlank→OAM) flip: slopgb's native flip lags SameBoy's, which
        // flips at 8 MHz pos 4 = dot 2 (the DS mode-bits lag). Peek the line-start
        // verdict at SameBoy's dot-2 flip. Scoped: carried mode-2 ISR read
        // (`stat_rise_oam`), native mode 0, line-start dot < 4 — the exhaustive
        // per-class characterization flagged the shared mode0→2 boundary as A/B
        // risk, but the two-bin is +1/−0 (the scope confines it to `m2int_m0stat`).
        if self.read_carried
            && self.stat_rise_oam
            && self.tier2_reclock
            && self.model.is_cgb()
            && self.ds
            && self.line >= 1
            && self.line < 144
            && m == 0
            && self.dot < 4
        {
            return if self.dot >= 2 { 2 } else { 0 };
        }
        // C2 #11aq CARRY-FRAME bare-line exit HOLD (env-gated `SLOPGB_M2HOLD`;
        // co-lands with the `SLOPGB_M2CARRY` +4-dot read-position carry). The
        // carry moves the DS mode-2 OAM-ISR FF41 read to SameBoy's *absolute*
        // cfl (measured: `m2int_m3stat_ds_1/_2` reads land slopgb dot 256/258 =
        // SameBoy cfl 256/258 at carry_t 8), so the read FRAME offset is now 0
        // and the verdict is SameBoy's mode AT that cfl: mode 3 iff cfl < SameBoy
        // bare exit `SBex = 257 + SCX&7` (+1 DS = 258 at scx0, MEASURED `SBMODE
        // cfl257 dc2`). slopgb's NATIVE `vis_mode` flips to 0 at its own exit
        // `255 + SCX&7` — 2-3 dots BELOW SameBoy's — so for the carried read it
        // reports mode 0 too early. HOLD mode 3 in the `[native_exit, SBex)` gap.
        // This is the BARELAW (#11an) with read_offset 0 (the carry replaces the
        // −4): the render-length (a) half of the (a)+(b) co-land — anchored to
        // SameBoy's exit, NOT slopgb's render. DS-only (paired with the DS-only
        // carry); bare non-sprite CGB lines.
        if crate::ppu::m2hold_on()
            && self.tier2_reclock
            && self.model.is_cgb()
            && self.ds
            && self.line >= 1
            && self.line < 144
            && m == 0
            && self.dot >= 250
            && self.dot < 257 + u16::from(self.eff.scx & 7) + u16::from(self.ds)
            && !self.render.win_active
            && !self.glitch_line
            && self.render.n_sprites == 0
        {
            return 3;
        }
        m
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
            // #11ag DS: the slack is +4 (the DS wy2-copy lands the shadow trigdot
            // 2 dots later relative to the WX match — `late_wy_FFto2_ly2_ds` `_1`
            // trigdot 101 / `_2` 103 vs wxmatch 97, so 4 separates them).
            && self.wy_trig_sb_dot <= self.render.wx_match_dot + 2 + 2 * u16::from(self.ds)
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
        let cmp_cgb = if self.glitch_line {
            0
        } else {
            match (self.line, self.dot) {
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
        let lyc_fire = lyc_high && data & STAT_SRC_LYC != 0;
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
            && (1..=143).contains(&self.line)
            && self.dot < 4
            && old & STAT_SRC_LYC == 0
            && data & STAT_SRC_LYC != 0
            && self.lyc == self.line - 1;
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
        let lyc_wrap_153 = self.leading_edge_reads
            && self.line == 153
            && self.lyc_interrupt_line
            && !lyc_high
            && old & STAT_SRC_LYC == 0
            && data & STAT_SRC_LYC != 0;
        // m2 sub-trigger window (kept from the pre-port calibration;
        // gambatte's ly==143 and ly==153 branches are empty at single
        // speed, so the (144,0) and (0,0) cells never fire it).
        let m2 = old & STAT_SRC_OAM == 0
            && data & (STAT_SRC_OAM | STAT_SRC_HBLANK) == STAT_SRC_OAM
            && (1..=143).contains(&self.line)
            && self.dot < 2 + 2 * u16::from(self.ds);
        // gambatte's ly-region split on our grid: dots 0-3 still belong
        // to the previous line, so (0, 0-3) is mode 1's tail and
        // (144, 0-3) line 143's hblank (an m0 enable written there still
        // fires: m1/ly143_late_m0enable_ds_1 cgb04c_out3).
        let vis = (self.line <= 143 && !(self.line == 0 && self.dot < 4))
            || (self.line == 144 && self.dot < 4);
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
            let tail = self.dot < 4;
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
            let carryover_tail = self.dot < if self.ds { 2 } else { 4 };
            if self.leading_edge_reads
                && carryover_tail
                && !self.glitch_line
                && self.vis_mode() == 0
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
                data & STAT_SRC_HBLANK != 0 || lyc_fire
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
            let m1_tail = self.line == 0 && self.dot < 4 && !self.leading_edge_reads;
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
