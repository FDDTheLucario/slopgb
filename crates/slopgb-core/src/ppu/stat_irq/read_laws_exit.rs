//! FF41 read-law engine, part 2: the per-config CPU-visible mode-3→0 exit
//! table `vis_exit_hd` (window length/shadow arms · pre-draw/reenable aborts ·
//! the post-switch exit table · the unified bare exit) + the shadow
//! two bare-line precondition
//! helpers. Split out of `read_laws.rs` for the CLAUDE.md <1000-line cap
//! (a second `impl Ppu` block via `use super::*`, like `reclock.rs`);
//! verdict-only — consumed by `Ppu::vis_mode_read` in `read_laws.rs`.

use super::*;

impl Ppu {
    /// The post-switch mode-3 exit offset, in half-dots, off the render's own
    /// flip. A mid-frame-anchored speed dance leaves the projection long by a
    /// fixed amount per (speed, leave-advance) class — derived per class over
    /// all 50 rows the old 4-variable table covered: every class is feasible,
    /// `leave_k = 6` classes sit 4 half-dots above `leave_k = 2` ones, and
    /// within a `leave_k` the dances that end in double speed sit 4 below those
    /// that end in single. The table's `SCX&7` term is gone — the render's flip
    /// carries it — and so is `lcd_enable_in_ds`.
    fn post_switch_exit_hd(&self) -> i32 {
        let speed = if self.ds { -6 } else { -2 };
        speed + i32::from(self.stop_leave_k) - 2
    }

    /// A visible (line 1–143), non-glitch, sprite-free line currently in
    /// mode 3 — the bare window-exit precondition shared by the DMG/CGB
    /// window arms of [`Self::vis_exit_hd`].
    fn bare_m3_visible(&self, m: u8) -> bool {
        self.line >= 1
            && self.line < 144
            && m == 3
            && !self.glitch_line
            && self.render.n_sprites == 0
    }

    /// A non-glitch, sprite-free line — the [`Self::bare_m3_visible`] sub-pair
    /// reused where the surrounding arm supplies its own mode/line guards.
    fn bare_sprite_free(&self) -> bool {
        !self.glitch_line && self.render.n_sprites == 0
    }
    /// The per-config CPU-visible mode-3→0 exit for the current FF41
    /// read, in 8 MHz half-dots on slopgb's line frame, with the read's own
    /// per-ISR carry ([`Ppu::isr_read_carry_hd`]) and the carried LCD phase
    /// (`lcd_phase_hd`, SS) already FOLDED (subtracted) so the caller compares
    /// plain [`Ppu::read_pos_hd`] `< exit`. `None` = no half-dot exit model
    /// for this config (the read returns the native [`Self::vis_mode`]).
    ///
    /// slopgb-frame constants relate to SameBoy's by the uniform +8 hd frame
    /// offset (slopgb dot D ↔ SameBoy cfl·2+dc = 2D+8, both speeds). A read can
    /// match SEVERAL arms (e.g. a re-enabled triggering window matches the
    /// length arm AND the reenable arm); the source laws were ordered
    /// fall-through blocks, whose combined verdict folds to: `m == 3` arms
    /// (force-0 past their exit) take the MINIMUM matching exit, `m == 0` arms
    /// (hold-3 below their exit) the MAXIMUM. Each arm keeps its own guards:
    ///
    /// | arm | config | exit (slopgb dots) |
    /// |---|---|---|
    /// | 1 | active triggering window | `259 + SCX&7 + ds` (SameBoy `SBex = 263 + SCX&7`, read offset +4) |
    /// | 2 | shadow late-WY extend (render bare, SameBoy window) | `263 + SCX&7 + ds` (polled) |
    /// | 3 | CGB pre-draw window-abort, SS | `253` (SCX penalty DROPPED, mattcurrie §WIN_EN) |
    /// | 4 | CGB pre-draw window-abort, DS | `254`; abort boundary `(89+WX)&!1` |
    /// | 5 | CGB window re-enable too late to redraw | `253` |
    /// | 6 | CGB late-WY UN-trigger (SameBoy bare, slopgb window) | `253 + SCX&7` |
    /// | 7 | boundary-WY cross-line extend | `263 + SCX&7 + ds` polled / `259 …` carried |
    /// | 8 | bare line | SS: emergent `2*flip + 2` hd − carry − phase; DS: `508 + 2*(SCX&7) + 2*(SCX&1)` hd − carry |
    pub(in crate::ppu) fn vis_exit_hd(&self, m: u8) -> Option<i32> {
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
        // Arm 1 — the triggering-window mode-3 length law.
        // A triggering window's SameBoy exit is `SBex = 263 + SCX&7`; the
        // deferred read samples the PPU +4 dots before SameBoy reads the same
        // `ldh a,(FF41)` (`m2int_wx03_scx5_m3stat_2` slopgb dot264
        // ↔ SameBoy cfl268 = SBex), so the CPU-visible exit is `259 + SCX&7`
        // (+1 in DS: the deferred cc+0 ISR read lands +1 dot vs SS). A POLLED
        // read sits at +0 of SameBoy's exit instead, so it takes the raw 263 —
        // the one read-frame offset, the same ISR carry every window read
        // takes. Off-screen windows
        // (wx A0-A6) extend with NO sprite penalty → sprite-free lines only
        // there; DS excludes sprite-laden lines entirely (the real mode-3 end
        // extends past the bare exit; `10spritesPrLine_wx*_m3stat_ds_1`
        // SameBoy-passes).
        if (self.render.win_active || self.eager_offscreen_win_arming())
            && self.model.is_cgb()
            && self.line >= 1
            && self.eff.wx <= 0xA6
            && (self.eff.wx < 0xA0 || self.render.n_sprites == 0)
            && (!self.ds || self.render.n_sprites == 0)
            && !self.render.win_aborted
            && self.wy <= 143
            && m == 3
        {
            // The off-screen (WX >= 0xA0) window renders nothing and is read
            // before it HBlank-activates, so its arming exit stays the closed
            // form the read lands on an M-cycle later either way.
            if self.eff.wx >= 0xA0 {
                fold(&mut exit, 2 * (259 + scx7 + ds1));
            } else {
                // EMERGENT: an on-screen active window's whole mode-3 cost —
                // including the fine-scroll discard it activates into — is in
                // the render's own flip, so the exit is that flip plus one
                // read-frame offset. The offset is the ISR frame: a polled read
                // sits +4 dots of the flip where a carried mode-2-ISR read sits
                // -4. Derived from the ROMs rather than swept: the polled rows
                // (`late_wy_FFto2_ly2_scx{2,3,5}`) bound it above 2 half-dots
                // and the carried DS rows (`m2int_wx{03,07,0C,57}_m3stat_ds_2`
                // against `..._scx5_m3stat_ds_1`) bracket it to exactly -4.
                let flip = if self.line_render_done && self.flip_dot != 0 {
                    self.flip_dot
                } else {
                    self.projected_flip_dot()
                };
                let frame = if self.read_carried { -4 } else { 4 };
                fold(&mut exit, 2 * i32::from(flip) + frame);
            }
        }
        // Arm D1 — the DMG triggering-window exit family, the arm-1
        // port. The deferred read samples +4 dots
        // before SameBoy reads the same `ldh a,(FF41)` (slopgb dot D ↔ SameBoy
        // cfl D+4 across the m2int family, same as CGB SS), and SameBoy's DMG
        // window exits split by WX class:
        //   wx <= 0xA5:  SBex = 263 + SCX&7 (the CGB length law verbatim —
        //                slopgb's native effective exit already matches, only
        //                the read frame differs) → exit 259 + SCX&7;
        //   wx == 0xA6, no sprites: the off-screen window renders NOTHING on
        //                DMG — SameBoy exits BARE (257 + SCX&7), while
        //                slopgb's render still activates and over-extends
        //                (`m2int_wxA6_*_m3stat` want-0 legs) → exit 253+SCX&7;
        //   wx == 0xA6 + object at WX+1 (`spxA7`): the sprite fetch extends
        //                mode 3 to SBex 263 → exit 259.
        // First-window-line EXCLUDED for on-screen WX (trigger-line mode 3
        // extends later, the CGB rule holds on DMG: `late_wy_*_1`
        // trigger-line reads at 260 stay 3) but INCLUDED for wx >= 0xA0
        // (`m2int_wxA6_firstline` fits the same 253+SCX&7).
        if !self.model.is_cgb()
            && (self.render.win_active || self.eager_offscreen_win_arming())
            && self.line >= 1
            && self.eff.wx <= 0xA6
            && !self.render.win_aborted
            && (self.wy != self.ly || self.eff.wx >= 0xA0)
            && self.wy <= 143
            && m == 3
        {
            if self.eff.wx < 0xA6 {
                // EMERGENT, as arm 1: the render's flip already carries the
                // window's mode-3 cost including its fine-scroll discard, so
                // only the read frame remains a constant. DMG takes a flat -4
                // with no polled/carried split (the closed form it replaces had
                // none either): derived from `gbmicrotest/win{0,10}_b` +
                // `win0_scx3_b`, whose polled read wants mode 0 at rphd 520 /
                // 528 against a flip of 522 / 530.
                let flip = if self.line_render_done && self.flip_dot != 0 {
                    self.flip_dot
                } else {
                    self.projected_flip_dot()
                };
                fold(&mut exit, 2 * i32::from(flip) - 4);
            } else if self.render.n_sprites == 0 {
                fold(&mut exit, 2 * (253 + scx7));
            } else if self.render.sprites[..usize::from(self.render.n_sprites)]
                .iter()
                .any(|s| u16::from(s.x) == u16::from(self.eff.wx) + 1)
            {
                fold(&mut exit, 2 * 259);
            }
        }
        // Arm D3 — the DMG PRE-DRAW window-abort exit, the arm-3/4
        // port. An LCDC.5 clear before the window's first fetch
        // (`win_predraw_abort`, `!win_mode`) leaves the line's mode-3 length
        // decided by WHERE the clear landed vs the window's WX-fetch ship
        // deadline (`wx_match_dot − 3 + min(fetch_scx, 2)`):
        //   clear before the ship deadline: the window ships NOTHING →
        //     SameBoy renders BARE, the SCX penalty KEPT (unlike CGB arm-3
        //     which drops it) → SBex `257 + SCX&7`; slopgb's whole-dot render
        //     over-extends → force 0. `early_scx03_wx0f/10/11/12_1`+`wx12_2`
        //     (clear 103, wx_match 108, scx3); `late_disable_scx2/3/5_0`
        //     (clear 95, wx_match 97 — the fetch SCX pushes the deadline past
        //     95 where scx0 catches it).
        //   clear at/after the deadline: the first tile shipped and the full
        //     mode-3 cost bakes in → SameBoy extends `263 + SCX&7`; slopgb's
        //     render aborted to bare → hold 3. `late_disable_1`/`wx0f_1`
        //     (clear 95, wx_match 97, scx0); `late_scx03_wx0f/10/11_2`.
        // Fetch SCX (`wx_match_scx`), NOT the read-time SCX, sets BOTH the
        // deadline and the exit fine-scroll (`late_scx_late_disable` rewrites
        // SCX 0→4 mid-line AFTER the window fetched). The −4 polled read frame
        // folds into both exits: bare `253 + fetch_scx`, extend `259 + fetch_scx`;
        // the `min(fetch_scx, 2)` deadline cap is the fetch-latency saturation.
        let fscx = i32::from(self.render.wx_match_scx);
        let wxm = self.render.wx_match_dot;
        let abd = self.render.win_predraw_abort_dot;
        // Extend once the clear lands within 3 dots of the WX match (the
        // first tile has shipped) — 4 on the cc+0 read frame, which records
        // `abd` one M-cycle earlier than a cc+4 read would, so the threshold is
        // 4 where a cc+4 read would use 3 (`wx11_2`
        // abd106 EXTEND vs `_1` abd102 BARE). EXCEPT a low-WX window whose
        // SCX fine-scroll pushes the fetch well past the match: there a clear
        // BEFORE the match (`abd < wxm`) definitively kills it → bare
        // (`late_disable_scx2/3/5_0`, wxm 97, clear 95, fetch SCX ≥ 1; the
        // scx0 sibling `late_disable_1` fetches immediately at the match and
        // still extends). The `wxm <= 100` bound is the near-left window
        // where the fine-scroll delay dominates (WX ≳ 0x10 windows extend a
        // pre-match clear regardless — `wx0f/10/11_2`, wxm ≥ 108).
        let scx_kills_early = fscx >= 1 && wxm <= 100 && abd < wxm;
        if !self.model.is_cgb()
            && self.render.win_predraw_abort
            && wxm != 0
            // A mid-line SCX rewrite (`scx_write_dot != 0`) is admitted:
            // `late_scx_late_disable` rewrites SCX 0→4 AFTER the
            // window fetched, so its fetch-time `wx_match_scx` (=4) still drives
            // the exit fine-scroll and the fetch-ship deadline.
            && self.eff.lcdc & LCDC_WIN_ENABLE == 0
            && self.line >= 1
            && self.line < 144
            && !self.render.win_active
            && !self.glitch_line
        {
            // The fetch-ship deadline `abd + K >= wxm` and the bare exit take a
            // wider K and a back-dated base on the scx-rewrite frame: the
            // fine-scroll (fscx=4) pushes the window's first-tile ship, so extend
            // needs K = 8 (measured: `late_scx_late_disable` abd 122 bare / 126
            // extend, wxm 133), and the cc+0 bare exit back-dates one dot
            // (253→252, the +1 read-debt) so the early-abort `_0` (read rp 512)
            // reads mode 0. Non-scx keeps K=4 / base 253.
            let eager_scx = self.render.scx_write_dot != 0;
            let ek = if eager_scx { 8 } else { 4 };
            let extend = i32::from(abd) + ek >= i32::from(wxm) && !scx_kills_early;
            let bare = if eager_scx { 252 } else { 253 };
            if self.render.n_sprites == 0 {
                fold(&mut exit, 2 * (if extend { 259 } else { bare } + fscx));
            } else if extend {
                // Arm D3-spr — a pre-draw abort with an object on the window
                // line (`late_disable_spx10_wx0f_2`, ns=1): the sprite fetch
                // extends mode 3 past the bare exit → SBex 274 (`263 + 11`
                // one-object penalty); the early-abort sprite sibling (`_1`)
                // genuinely aborts (native bare, rebaselined). −4 read frame
                // → 270.
                fold(&mut exit, 2 * 270);
            }
        }
        // A mid-line WX rewrite committing AT/BEFORE the WX
        // match dot un-catches the window on SameBoy (`late_wx_scx5_1`: the
        // FF4B:=FF write and the match both at dot 97 → SameBoy bare; `_2`
        // at 101 → caught, extends) while slopgb's whole-dot render catches
        // first and extends both. SS, bare-sprite-free; the SS bare exit.
        // SCX&7 == 5 ONLY: at scx0/2/3 SameBoy still catches the
        // same write≤match race — `late_wx_2`/`_scx2_2`/`_scx3_2`/`_ff_*_1`
        // all want 3; the un-scoped arm dropped all 8. The scx5 fine-scroll
        // phase is what pushes the effective catch past the write.
        // Double speed shifts the un-catch boundary one dot later: its M-cycle
        // is 2 dots, so the write that still beats the fetch lands at
        // `wx_match + 1` (`late_wx_scx5_ds_1` writes at 98 against a match at 97
        // and un-catches; its `_2` sibling writes at 100 and does not).
        let wx_slack = u16::from(self.ds);
        if scx7 == 5
            && self.render.wx_write_dot != 0
            && self.render.wx_match_dot != 0
            && self.render.wx_write_dot <= self.render.wx_match_dot + wx_slack
            && self.render.win_active
            && self.model.is_cgb()
            && self.render.n_sprites == 0
            && !self.render.win_aborted
            && m == 3
        {
            fold(&mut exit, 2 * (253 + scx7));
        }
        // Arm D-wx — the DMG WX-rewrite un-catch. Same mechanism as
        // the CGB arm above, but the un-catch boundary sits LOWER on DMG:
        // `scx&7 >= 3` un-catches (`late_wx_scx3_2`/`scx5_1`, write ≤ match →
        // SameBoy bare), where CGB only un-catches at scx5 (the DMG fetch
        // phase is 1 fine-scroll step ahead — the same ±1-dot re-derivation
        // the DS port needed). scx0/2 still catch on DMG (`late_wx_2`
        // want 3).
        if !self.ds
            && !self.model.is_cgb()
            && scx7 >= 3
            && self.render.wx_write_dot != 0
            && self.render.wx_match_dot != 0
            && self.render.wx_write_dot <= self.render.wx_match_dot
            && self.render.win_active
            && self.render.n_sprites == 0
            && !self.render.win_aborted
            && m == 3
        {
            fold(&mut exit, 2 * (253 + scx7));
        }
        // A late-ENABLE-triggered window (the mid-line
        // LCDC.5 write IS the trigger, `Render::win_enable_dot`) whose
        // enable lands past the line's fetch-catch deadline renders BARE on
        // SameBoy — the window misses this line entirely — while slopgb's
        // whole-dot render still activates and extends (`late_enable_ly0_ds`
        // want-pair: enable dot 94 → native extend holds (want 3, no arm);
        // dot 96 → SameBoy bare (want 0), both legs reading the identical
        // dot 260 — the enable dot is the only discriminator). DS-scoped,
        // bare-sprite-free lines; the DS bare exit form.
        if self.ds
            && self.render.win_enable_dot > 94
            && self.render.win_active
            && self.model.is_cgb()
            && self.render.n_sprites == 0
            && !self.render.win_aborted
            && self.wy <= 143
            && m == 3
        {
            fold(&mut exit, 508 + 2 * scx7 + 2 * i32::from(self.scx & 1));
        }
        // Arm 3b — the sprite-at-window-X abort-slot removal, SS CGB
        // (asm_window_gdma Row 4). With an object at the window's screen X
        // (OAM X = WX+1) the window activation precedes the object fetch and
        // the sprite fetch then OCCUPIES the fetcher's next GET_TILE_T1 —
        // removing the late CGB abort slot, so an LCDC.5 clear landing in
        // that last slot (commit ≥ wx_match−4; `late_disable_spx10_wx0f_2`
        // clear 104, match 105) leaves the window+sprite line fully extended
        // (SameBoy flip 272 → slopgb-frame exit 270). slopgb's whole-dot
        // en-sample at the match suppressed the start → native bare+sprite
        // abort exit 264, read 264 → 0 (want 3). The `_1` clear (100) lands
        // a slot earlier and genuinely aborts (native, stays 0).
        if self.model.is_cgb()
            && !self.ds
            && self.render.win_predraw_abort
            && self.render.wx_match_dot != 0
            && self.render.win_predraw_abort_dot + 4 >= self.render.wx_match_dot
            && self.render.win_predraw_abort_dot < self.render.wx_match_dot
            && self.eff.lcdc & LCDC_WIN_ENABLE == 0
            && self.line >= 1
            && self.line < 144
            && m == 0
            && !self.render.win_active
            && !self.glitch_line
            && self.render.n_sprites > 0
            && self.render.sprites[..usize::from(self.render.n_sprites)]
                .iter()
                .any(|s| u16::from(s.x) == u16::from(self.eff.wx) + 1)
        {
            fold(&mut exit, 2 * 270);
        }
        // Arm 4 — the DS pre-draw abort twin. SameBoy renders the
        // early aborts bare with the penalty dropped, exit `cfl257 dc2` (the
        // DS half-dot bare exit) = slopgb 254. The DS abort boundary is
        // wx-DEPENDENT: `(89 + WX) & !1` — the window's first-fetch M-cycle
        // start on the DS 2-dot grid.
        if self.model.is_cgb()
            && self.ds
            && self.render.win_predraw_abort
            && self.render.win_predraw_abort_dot < (89 + u16::from(self.wx)) & !1
            && self.eff.lcdc & LCDC_WIN_ENABLE == 0
            && self.line >= 1
            && self.line < 144
            && m == 3
            && !self.render.win_active
            && self.bare_sprite_free()
        {
            fold(&mut exit, 2 * 254);
        }
        // Arm 5 — the CGB window-REENABLE length, SS. A window
        // disabled then RE-enabled mid-mode-3 redraws from the re-enable
        // point; mode 3 extends past the read iff the re-enable beat the WX
        // redraw start (`reen <= wx_match − 3`, uniform — base wxmatch97:
        // reen92 extend / reen96 bare; wx0f wxmatch105: 100/104). The LATE
        // re-enable renders the tail BARE (exit 253); slopgb collapses both
        // to mode 3. SCX&7 <= 3 only (the fine-scroll shifts the redraw
        // deadline at high SCX — scx5 boundary 98 not 94; scx5+
        // pass natively).
        if self.model.is_cgb()
            && !self.ds
            && self.render.win_reenable_dot != 0
            && self.render.wx_match_dot != 0
            && self.render.win_reenable_dot + 3 > self.render.wx_match_dot
            && self.scx & 7 <= 3
            && self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && self.render.win_active
            && self.bare_m3_visible(m)
        {
            fold(&mut exit, 2 * 253);
        }
        // Arm D5 — the DMG window-REENABLE-too-late bare exit, the
        // arm-5 port. The redraw deadline carries an SCX term absent on CGB:
        // bare iff `reen + K > wx_match + SCX&7` (the fine-scroll delays the
        // redraw start, so a higher-SCX re-enable at the same dot still
        // catches the tile); K = 4 on the cc+0 read frame, which records
        // `win_reenable_dot` one M-cycle earlier than the frame the CGB arm's
        // +3 above was calibrated against (`late_reenable_2` reen 94 —
        // mirroring the arm-D3 +4). `late_reenable_2` reen 94 /
        // match 97 / scx0 → bare (94+4 > 97); `scx2_2` reen 94 / scx2 → extend
        // (98 ≯ 99); `wx0f_2` reen 102 / match 105 → bare. The CGB arm above
        // keeps +3 — it is SCX-flat there (scx ≤ 3), the ±1 fetch phase again.
        if !self.model.is_cgb()
            && !self.ds
            && self.render.win_reenable_dot != 0
            && self.render.wx_match_dot != 0
            && i32::from(self.render.win_reenable_dot) + 4
                > i32::from(self.render.wx_match_dot) + scx7
            && self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && self.render.win_active
            && self.bare_m3_visible(m)
        {
            fold(&mut exit, 2 * 253);
        }
        // Arm 8-spr — the DS WINDOW+SPRITE mode-3 exit (CGB). Arm 1 (the
        // triggering-window length law) EXCLUDES sprite-laden DS lines
        // (`!ds || n_sprites == 0`) because its closed-form `259 + SCX&7` exit
        // cannot carry the per-line sprite penalty; a NON-window sprite DS line
        // falls to arm 8's own sprite-free scope but its raw native mode already
        // verdicts correctly, so ONLY the window+sprite DS line (arm-1-excluded,
        // no other arm) mis-verdicts the `_2` sibling that reads one M-cycle
        // PAST the render's flip (`10spritesPrLine_wx0..7_m3stat_ds_2`: eager
        // read dot 370 < the render's flip 371, raw mode still 3, want 0). The
        // render's OWN recorded/projected flip bakes in the exact window+sprite
        // cost, so the EMERGENT exit `2*flip` resolves the pair on the DS
        // read frame with NO closed form: `_1` reads rp 740 < 743 → mode 3 (want
        // 3); `_2` reads rp 744 ≥ 743 → mode 0 (want 0). The `+1` (over the bare
        // arm's `−2` DS lead) is the projected-flip lead for a window+sprite
        // line. Only `10spritesPrLine_wx7` has the
        // render's flip 371 MATCHING SameBoy's mode-3 end; `wx0..6` share the
        // same render flip 371 but SameBoy ends mode 3 wx-dependently earlier
        // (~321..361) — those are a RENDER-length mismatch, not a read-frame miss
        // (the render's projected flip is itself wrong there), so this read arm
        // cannot reach them. Scoped to an ACTIVE, non-aborted window with sprites
        // on a visible DS line where no earlier arm matched (`exit.is_none()`);
        // CGB + DS only.
        if self.model.is_cgb()
            && self.ds
            && exit.is_none()
            && m == 3
            && self.render.n_sprites > 0
            && self.render.win_active
            && !self.render.win_aborted
            && !self.glitch_line
            && self.line >= 1
            && self.line < 144
            && (self.line_render_done || self.render.active)
        {
            let flip = if self.line_render_done && self.flip_dot != 0 {
                self.flip_dot
            } else {
                self.projected_flip_dot()
            };
            fold(&mut exit, 2 * i32::from(flip) + 1);
        }
        // Arm 8 — the unified half-dot BARE-line mode-3 exit.
        // The read position is `read_pos_hd + isr_read_carry_hd + lcd_phase`
        // (folded into the returned exit); the exit is a per-speed half-dot
        // line constant:
        //
        //   SS: exit_hd = 2*flip + 2, EMERGENT from the render's own recorded
        //       flip (`flip_dot`) or its projection — NOT a live-`scx` closed
        //       form: a mid-line SCX write moves the exit exactly as the
        //       fine-scroll hunt resolved it (late_scx4 / scx_m3_extend; a
        //       closed form broke them). For a clean steady line
        //       this equals `510 + 2*(SCX&7)` (flip 254+SCX&7).
        //   DS: exit_hd = 508 + 2*(SCX&7) + 2*(SCX&1) — the full-carry
        //       law rewritten exactly on the half-dot grid.
        //
        // SS fires on native m ∈ {3, 0} — the true exit sits ±1 dot around
        // the whole-dot flip, BOTH directions needed (the HOLD
        // direction is derivable only on the STOPADV-advanced frame;
        // speedchange4 scx2_1 reads AT the native flip dot and must still
        // read 3); DS keeps the `m == 3` gate. Bare non-sprite non-window
        // non-glitch lines, ARCH `self.scx` (the write-strobe rule).
        // SS reads add the carried LCD phase (the per-leave m3stat read-frame
        // surplus over the machine epoch; 0 for never-switched ROMs); DS
        // keeps 0 — the DS post-leave segments are epoch-only.
        // The DS branch includes LINE 0: the gdma_cycles post-stall
        // polls land at ly0 (the corrected DS line-153 wake moved them −2
        // onto the flip straddle: `_1` dot252 want3 / `_2` dot254 want0 —
        // exactly the emergent exit 508 hd). SS keeps `line >= 1`.
        if (self.line >= 1 || self.ds)
            && self.line < 144
            && !self.render.win_active
            && !self.render.win_aborted
            && !self.wy_triggered
            && self.bare_sprite_free()
        {
            let carry = self.isr_read_carry_hd();
            if self.ds {
                if self.model.is_cgb() && m == 3 {
                    // The DS exit re-expressed EMERGENT (like SS):
                    // `2*flip − 2 + 2*(SCX&1)`, anchored to the render's own
                    // recorded/projected flip. For a steady bare DS line the
                    // flip is `255 + SCX&7` (DS lead 1), so this equals the
                    // closed form `508 + 2*(SCX&7) + 2*(SCX&1)`
                    // exactly — byte-identical there — while a mid-line SCX
                    // rewrite that re-arms the fine-scroll hunt EXTENDS the
                    // exit with the render (`scx_m3_extend_ds`: SameBoy reads
                    // hd 660 want 3 / 664 want 0, slopgb frame — the closed
                    // form forced both to 0).
                    let flip = if self.line_render_done && self.flip_dot != 0 {
                        self.flip_dot
                    } else if self.render.active {
                        self.projected_flip_dot()
                    } else {
                        255 + u16::from(self.scx & 7)
                    };
                    // The DS post-switch bare exit (the 4-variable
                    // table's DS arm): a mid-frame-anchored speed dance
                    // (speedchange v1/3/5 ly44) lands the true post-switch
                    // frame the emergent exit's absorbed calibration
                    // misses; in scope the law REPLACES the emergent exit.
                    // `E = 502 + leave_k + 2*(SCX&7)` rp, LINEAR in scx
                    // (the (SCX&1) parity term drops out for these
                    // dances), leave_k = 2 when never left (v1). The
                    // VBlank/boot-anchored suite (kernel `_ds`, offset1,
                    // gdma — all first-STOP at ly144) and the DS-enable
                    // dances (lcdoffds — `lcd_enable_in_ds`, sits exactly
                    // on the emergent exit) are excluded.
                    if self.stop_anchor_midframe && !self.lcd_enable_in_ds {
                        fold(
                            &mut exit,
                            2 * i32::from(flip) + self.post_switch_exit_hd() - carry,
                        );
                    } else {
                        fold(
                            &mut exit,
                            2 * i32::from(flip) - 2 + 2 * i32::from(self.scx & 1) - carry,
                        );
                    }
                }
            } else if !self.wy_triggered
                && !self.render.win_stalled
                && (m == 3 || m == 0)
                && (self.line_render_done || self.render.active)
            {
                let mut flip = if self.line_render_done && self.flip_dot != 0 {
                    self.flip_dot
                } else {
                    self.projected_flip_dot()
                };
                // Back out the render's spurious mid-mode-3 SCX
                // extension for the BARE-line exit verdict. A mid-mode-3 SCX
                // rewrite (`scx_write_dot != 0`) commits `eff.scx` at the
                // cc+0 write frame — 4 dots (8hd) before its true cc+4 landing
                // — so it reaches the render's fine-scroll hunt (`render.rs`
                // ~dot 89) BEFORE the hunt latches and the render over-discards
                // the NEW fine-scroll, flipping `eff.scx&7` dots late (258 vs the
                // production 254 on `late_scx4_2`: the write's true cc+4
                // landing is PAST the hunt → the current line keeps the fetch-
                // start length). The FF43 write-commit debt that would fix this
                // in the render is REFUTED (`eff.scx` IS the length — it breaks
                // the `late_scx_late_disable` window siblings; see `regs.rs`
                // `stage_write`). This is the verdict-only READ analogue: undo the
                // extension in the bare exit ONLY (window aborts own the
                // `scx_write_dot` arm above). DMG + bare only.
                //
                // The spurious part is only what the render added BEYOND the
                // fine scroll its comparator actually resolved, so back out
                // `SCX&7 - hunt_fine`, not the whole live `SCX&7`. On
                // `late_scx4_2` the hunt latched 0 while the write left
                // `eff.scx&7 == 4`, so both forms back out 4. On
                // `scx_m3_extend_1` the hunt latched the same 5 the write left
                // — the render's length is legitimate and nothing is backed
                // out; subtracting the full 5 there double-counted the fine
                // scroll and read mode 0 one M-cycle early.
                if !self.model.is_cgb() && self.render.scx_write_dot != 0 {
                    let spurious = (self.scx & 7).saturating_sub(self.render.hunt_fine);
                    flip = flip.saturating_sub(u16::from(spurious));
                }
                // The SS post-switch bare exit: a
                // 4-variable table. `E = 504 + leave_k −
                // 4*[lcd_enable_in_ds] + 2*(SCX&7)` rp — the leave k
                // (dsa7-branched, 2/6) and the enable-in-DS re-anchor are
                // the two class variables; ISR carry drops out (the
                // carried m2int and polled ly44 legs share constants).
                // Scoped to mid-frame-anchored dances post-LCD-on-leave
                // (`stop_anchor_midframe`): the VBlank/boot-anchored classes
                // (base/frame1/nop m2int + offset2/3 counts) fall outside this
                // anchor and are served by the emergent arm. In scope
                // the law REPLACES the emergent exit for BOTH directions —
                // the emergent `2*flip + 2` m==0 hold over-holds the
                // post-switch frame by up to 6 rp
                // (`speedchange4_ly44_m3_nop_m3stat_scx3_2` reads rp 512
                // native-0, true exit 512, emergent hold 518 — a fold
                // cannot override a max-hold). The one out-of-scope
                // hold-direction row (`speedchange2_nop_m2int_m3stat_
                // scx1_1`, VBlank-anchored) stays the pre-seeded
                // rebaseline joiner.
                if self.stop_anchor_midframe && self.stop_leave_lcd_on {
                    fold(
                        &mut exit,
                        2 * i32::from(flip) + self.post_switch_exit_hd() - carry,
                    );
                } else {
                    let phase = i32::from(self.lcd_phase_hd);
                    // The emergent bare exit's `+2` over-holds a POLLED read
                    // that lands EXACTLY on the flip boundary. Production reads
                    // mode 0 AT `flip_dot` (the flip is inclusive), so the true
                    // CPU-visible mode-0 boundary sits at rphd `2*flip`, not
                    // `2*flip + 2`. sprite0's polled measurement read is the one
                    // ROM that reads at exactly rphd `2*flip` (its whole point is
                    // to bracket the flip): `ppu_sprite0_scx{2,6}_b` reads
                    // rphd 512/520 = `2*flip` and want mode 0, but `+2` (514/522)
                    // forces mode 3. The carried m2int/scx weld-partners
                    // (`late_scx4_1`, `m2int_m3stat_1`) read the SAME rphd 512 yet
                    // carry `= 4` — their `- carry` already lands exit `2*flip - 2`
                    // — and want mode 3, so the split is `read_carried`, NOT a
                    // uniform read-frame bias (which would shift both and
                    // shuffle). Drop the `+2` only for the DMG
                    // polled read; every other case still keeps `+2`, carried
                    // reads untouched (`- carry` owns them).
                    let over = if !self.model.is_cgb() && !self.read_carried {
                        0
                    } else {
                        2
                    };
                    fold(&mut exit, 2 * i32::from(flip) + over - carry - phase);
                }
            }
        }
        exit
    }
}
