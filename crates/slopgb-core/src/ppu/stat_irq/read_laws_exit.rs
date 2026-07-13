//! FF41 read-law engine, part 2: the per-config CPU-visible mode-3→0 exit
//! table `vis_exit_hd` (window length/shadow arms · pre-draw/reenable aborts ·
//! the post-switch exit table · the unified bare exit) + the shadow
//! window-extend predicate `win_extends_sb` and the two bare-line precondition
//! helpers. Split out of `read_laws.rs` for the CLAUDE.md <1000-line cap
//! (a second `impl Ppu` block via `use super::*`, like `reclock.rs`);
//! verdict-only — consumed by `Ppu::vis_mode_read` in `read_laws.rs`.

use super::*;

impl Ppu {
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
        // (+1 in DS: the deferred cc+0 ISR read lands +1 dot vs SS). LINE-0 /
        // first-window-line (wy2 == ly) excluded for ON-screen windows (their
        // trigger-line mode-3 extends LATER than the steady law) but
        // NOT for off-screen wx >= 0xA0 (renders nothing, no extend).
        // Off-screen windows (wx A0-A6) extend with NO sprite penalty →
        // sprite-free lines only there; DS excludes sprite-laden lines
        // entirely (the real mode-3 end extends past the bare exit;
        // `10spritesPrLine_wx*_m3stat_ds_1` SameBoy-passes).
        if (self.render.win_active || self.eager_offscreen_win_arming())
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
            && (self.wy2 != self.ly || self.eff.wx >= 0xA0)
            && self.wy2 <= 143
            && m == 3
        {
            if self.eff.wx < 0xA6 {
                // The boundary-WY cross-line trigger line (`wy_xline_trig`, set
                // by the regs.rs tail/head seam writes) extends the SAME +4 dots
                // past the steady-state exit that the normal first-window line
                // does (SameBoy's trigger line ends mode 3 later — the exclusion
                // above only skips the `wy2 == ly` first line). On the DMG eager
                // clock the wy2-lagged render OVER-triggers this seam line
                // (`win_active` rises where the tier2 render misses it and arm 7
                // compensates at `m == 0`), so arm D1 fires with the steady 259
                // and the read under-holds. Give it the cross-line 263 exit,
                // matching arm 7's polled extend (`late_wy_10to0_ly1_1`,
                // `FFto0/FFto1/FFto2_ly2_scx*_1`, want extend). `eager_value`-gated
                // → tier2 byte-identical (its render never triggers the seam, so
                // arm D1 does not fire there).
                let base = if self.eager_value && self.wy_xline_trig {
                    263
                } else {
                    259
                };
                fold(&mut exit, 2 * (base + scx7));
            } else if self.render.n_sprites == 0 {
                fold(&mut exit, 2 * (253 + scx7));
            } else if self.render.sprites[..usize::from(self.render.n_sprites)]
                .iter()
                .any(|s| u16::from(s.x) == u16::from(self.eff.wx) + 1)
            {
                fold(&mut exit, 2 * 259);
            }
        }
        // Arm D6 — the DMG late-WY UN-trigger bare exit, the arm-6
        // port. SameBoy's continuous `wy_check` reads the IMMEDIATE WY: a
        // WY→FF write landing before the line's compare window un-triggers
        // the window (line renders BARE, SBex 257 + SCX&7) while slopgb's
        // wy2-lagged render still draws it (`late_wy_1toFF_1/_2`,
        // `late_wy_2toFF_1/_2`, `late_scx_late_wy_FFto4_ly4_wx20_3` — the
        // `_3` keep-siblings latch `wy_trig_sb_raw` at dot 4 before the
        // write commits, the discriminator). The polled read sits
        // at +0 of SameBoy's exit; a carried STAT-ISR read at +4 → 253.
        if !self.model.is_cgb()
            && self.render.win_active
            && !self.wy_trig_sb_raw
            && self.bare_m3_visible(m)
        {
            let base = if self.read_carried { 253 } else { 257 };
            fold(&mut exit, 2 * (base + scx7));
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
        // first tile has shipped) — 4 on the eager cc+0 read frame, which records
        // `abd` an M-cycle before the tier2 cc+4 read the +3 targets (`wx11_2`
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
            // A mid-line SCX rewrite (`scx_write_dot != 0`) is admitted ONLY on
            // the eager clock: `late_scx_late_disable` rewrites SCX 0→4 AFTER the
            // window fetched, so its fetch-time `wx_match_scx` (=4) still drives
            // the exit fine-scroll and the fetch-ship deadline. Tier2
            // keeps the `== 0` scope (byte-identical).
            && (self.render.scx_write_dot == 0 || self.eager_value)
            && self.eff.lcdc & LCDC_WIN_ENABLE == 0
            && self.line >= 1
            && self.line < 144
            && !self.render.win_active
            && !self.glitch_line
        {
            // The fetch-ship deadline `abd + K >= wxm` and the bare exit take a
            // wider K and a back-dated base on the eager scx-rewrite frame: the
            // fine-scroll (fscx=4) pushes the window's first-tile ship, so extend
            // needs K = 8 (measured: `late_scx_late_disable` abd 122 bare / 126
            // extend, wxm 133), and the eager cc+0 bare exit back-dates one dot
            // (253→252, the +1 read-debt) so the early-abort `_0` (read rp 512)
            // reads mode 0. Non-scx eager keeps K=4 / base 253.
            let eager_scx = self.eager_value && self.render.scx_write_dot != 0;
            let ek = if eager_scx {
                8
            } else if self.eager_value {
                4
            } else {
                3
            };
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
            && self.wy2 <= 143
            && m == 3
        {
            fold(&mut exit, 508 + 2 * scx7 + 2 * i32::from(self.scx & 1));
        }
        // Arm 2 — the shadow late-WY extend (line 0 included).
        // slopgb's discrete `wy_latch` sampler misses the mid-line late-WY
        // write SameBoy's continuous `wy_check` catches, so slopgb renders the
        // line BARE (native m == 0) where SameBoy's window triggered and
        // extended mode 3 to the POLLED exit `263 + SCX&7` (+0 ISR offset —
        // these reads carry no mode-2 dispatch). The shadow
        // [`Self::win_extends_sb`] re-derives SameBoy's trigger decision.
        // Sprite-laden DS lines excluded (the shadow's bare exit carries no
        // sprite penalty).
        //
        // DMG shares this arm verbatim: the mid-line late-WY family
        // (`FFto2_ly2_2`/`_scx*`/`_wx0f_2`, `10to1_ly1_2`, `FFto0_ly0_2`)
        // extends on DMG where CGB stays bare — the SAME `wx_match_dot + 2`
        // deadline, the model-dependent `wy2` lag alone splitting the two
        // (DMG shadow latches +2 dots after the WY write, CGB +6, so a write
        // at wx_match−1 clears the DMG deadline but misses the CGB one):
        // FFto2_ly2 `_2` latch 98 ≤ 99 (extend) /
        // `_3` latch 102 > 99 (bare), wx0f `_2` 106 ≤ 107 / `_3` 110 > 107.
        // The eager DMG FIRST-window-line (`wy2 == ly`) trigger is admitted even
        // when slopgb's render DID activate (`win_active`): arm D1 excludes the
        // trigger line (mode 3 extends later than the steady 259 law) and native
        // mode has already flipped, so the late-WY-triggered first line falls to
        // native 0 where SameBoy still extends (`late_wy_FFto2_ly2_scx{2,3}_1`,
        // win_active, wy_trig 94 ≤ wxm 97, read dot 260 → want 3). On-screen WX
        // only (`eff.wx < 0xA0`): the off-screen `m2int_wxA6_firstline` renders
        // nothing → bare, must NOT extend. eager DMG only → byte-identical off.
        if self.line < 144
            && m == 0
            && (!self.render.win_active
                || (self.eager_value
                    && !self.model.is_cgb()
                    && self.wy2 == self.ly
                    && self.eff.wx < 0xA0))
            && (!self.ds || self.render.n_sprites == 0)
            && self.win_extends_sb()
        {
            fold(&mut exit, 2 * (263 + scx7 + ds1));
        }
        // Arm D-wx0 — the eager DMG low-WX co-incident-trigger BARE exit
        // (the Arm-2 complement). On a low-WX window's OWN trigger line
        // the WX comparator matches during the 8-dot prefill, so slopgb's
        // whole-dot render activates (`win_active`) the instant `wy2 == ly` is
        // caught — even when that catch lands AT the match dot. SameBoy's mode-2
        // `wy_check` samples ~2 dots BEFORE the match, so a wy2 that becomes
        // valid at/after it (the `win_extends_sb` deadline FALSE, `wy_trig_sb_dot
        // > wx_match_dot − 2`) does NOT trigger → SameBoy renders BARE while
        // slopgb drew the window and over-extended (`vis_hold_until` / native
        // flip 257). Force the bare exit so the trigger-line read verdicts mode 0
        // (`late_wy_FFto2_ly2_wx00_3` wytrig 90 == wxmatch 90 → bare; its `_2`
        // wytrig 86 ≤ 88 rides Arm 2's extend). WX < 7 (the prefill-match class;
        // WX ≥ 7 goes bare in the render and rides Arm 2). eager DMG only →
        // tier2 / CGB / production byte-identical.
        if self.eager_value
            && !self.model.is_cgb()
            && self.render.win_active
            && self.wy2 == self.ly
            && self.wy_trig_sb_line == self.ly
            && self.eff.wx < 7
            && self.render.wx_match_dot != 0
            // NO fine scroll only. A nonzero SCX&7 (incl. a `late_scx_*` rewrite
            // to 4) shifts the window fetch onto its own fine-scroll frame where
            // the window LEGITIMATELY extends: `late_scx_late_wy_*_wx00_2` has
            // the IDENTICAL render state (wytrig 90 == wxmatch 90) but scx7=4 and
            // wants EXTEND. Only the scx7=0 co-incident trigger cleanly fails
            // SameBoy's mode-2 `wy_check` → bare.
            && self.scx & 7 == 0
            && !self.win_extends_sb()
            && (m == 0 || m == 3)
        {
            fold(&mut exit, 2 * 251);
        }
        // Arm 3 — the CGB PRE-DRAW window-abort bare exit, SS. A
        // window disabled by an LCDC.5 clear BEFORE its first fetch renders
        // BARE on SameBoy with the SCX fine-scroll penalty DROPPED
        // (mattcurrie §WIN_EN) → exit cfl257 = slopgb 253, NOT 257+SCX&7;
        // slopgb's whole-dot render over-extends. Boundary: the abort must
        // land before the window's first tile ships (~dot 106 for the scx03
        // early setup — `_1` abort104 bare / `_2` abort108 extend, ALL
        // wx0f-12; wx-INDEPENDENT, a `wx_match+1`-relative form REFUTED). A
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
            && self.bare_sprite_free()
        {
            fold(&mut exit, 2 * 253);
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
        // start on the DS 2-dot grid (three candidates built + refuted first).
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
        // catches the tile); K = 4 on the eager cc+0 read frame, which records
        // `win_reenable_dot` one M-cycle before the tier2 cc+4 read the +3 was
        // calibrated against (`late_reenable_2` eager reen 94 vs tier2 95 —
        // mirroring the arm-D3 +4). `late_reenable_2` reen 94 /
        // match 97 / scx0 → bare (94+4 > 97); `scx2_2` reen 94 / scx2 → extend
        // (98 ≯ 99); `wx0f_2` reen 102 / match 105 → bare. Tier2 keeps +3 (CGB
        // arm-5 above is SCX-flat, scx ≤ 3 — the ±1 fetch phase again.)
        if !self.model.is_cgb()
            && !self.ds
            && self.render.win_reenable_dot != 0
            && self.render.wx_match_dot != 0
            && i32::from(self.render.win_reenable_dot) + if self.eager_value { 4 } else { 3 }
                > i32::from(self.render.wx_match_dot) + scx7
            && self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && self.render.win_active
            && self.bare_m3_visible(m)
        {
            fold(&mut exit, 2 * 253);
        }
        // Arm 6 — the CGB late-WY UN-trigger bare exit, SS. SameBoy's
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
            && self.bare_m3_visible(m)
        {
            fold(&mut exit, 2 * (253 + scx7));
        }
        // Arm 7 — the boundary-WY cross-line extend. A WY
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
        // read-class-dependent: POLLED reads sit at +0 of SameBoy's
        // `263 + SCX&7` exit; a carried STAT-ISR read at +4 → 259.
        //
        // The DMG twin shares this arm verbatim: the boundary-WY
        // family (`late_wy_10to0_ly1`, `FFto0_ly2`, `FFto1_ly2` `_1`/`_2`)
        // fits the identical polled 263 + SCX&7 exit (SameBoy extends every
        // later line; slopgb's discrete sampler misses the seam write). The
        // DMG latch adds the tail-write next-line case in `regs.rs`
        // (SameBoy's continuous check vs the 450/454 old-value samples).
        if self.line >= 1
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
            && crate::probe::noxline_fires()
        {
            let base = if self.read_carried { 259 } else { 263 };
            fold(&mut exit, 2 * (base + scx7 + ds1));
        }
        // Arm 8-spr — the DS WINDOW+SPRITE mode-3 exit (eager CGB). Arm 1 (the
        // triggering-window length law) EXCLUDES sprite-laden DS lines
        // (`!ds || n_sprites == 0`) because its closed-form `259 + SCX&7` exit
        // cannot carry the per-line sprite penalty; a NON-window sprite DS line
        // falls to arm 8's own sprite-free scope but its raw native mode already
        // verdicts correctly, so ONLY the window+sprite DS line (arm-1-excluded,
        // no other arm) mis-verdicts the `_2` sibling that reads one M-cycle
        // PAST the render's flip (`10spritesPrLine_wx0..7_m3stat_ds_2`: eager
        // read dot 370 < the render's flip 371, raw mode still 3, want 0). The
        // render's OWN recorded/projected flip bakes in the exact window+sprite
        // cost, so the EMERGENT exit `2*flip` resolves the pair on the eager DS
        // read frame with NO closed form: `_1` reads rp 740 < 743 → mode 3 (want
        // 3); `_2` reads rp 744 ≥ 743 → mode 0 (want 0). The `+1` (over the bare
        // arm's `−2` DS lead) is the projected-flip lead for a window+sprite line
        // (swept unique-optimal `2*flip + 1` on the DS window+sprite set: `+0`
        // drops 7 sibling rows, `+1`/`+2` recover `wx7` clean, `+3` loses it —
        // the `+1`/`+2` plateau centre). Only `10spritesPrLine_wx7` has the
        // render's flip 371 MATCHING SameBoy's mode-3 end; `wx0..6` share the
        // same render flip 371 but SameBoy ends mode 3 wx-dependently earlier
        // (~321..361) — those are a RENDER-length mismatch, not a read-frame miss
        // (the render's projected flip is itself wrong there), so this read arm
        // cannot reach them. Scoped to an ACTIVE, non-aborted window with sprites
        // on a visible DS line where no earlier arm matched (`exit.is_none()`);
        // `eager_value` + CGB → production/tier2 (which advance `self.dot`
        // natively) + SS + non-window-sprite lines (raw-mode-correct)
        // byte-identical.
        if self.eager_value
            && self.model.is_cgb()
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
            && !self.wy_trig_sb
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
                        fold(&mut exit, 502 + i32::from(self.stop_leave_k) + 2 * scx7);
                    } else {
                        fold(
                            &mut exit,
                            2 * i32::from(flip) - 2 + 2 * i32::from(self.scx & 1) - carry,
                        );
                    }
                }
            } else if !self.wy_latch
                && self.wy2 != self.ly
                && !self.render.win_stalled
                && (m == 3 || m == 0)
                && (self.line_render_done || self.render.active)
            {
                let mut flip = if self.line_render_done && self.flip_dot != 0 {
                    self.flip_dot
                } else {
                    self.projected_flip_dot()
                };
                // Back out the eager render's spurious mid-mode-3 SCX
                // extension for the BARE-line exit verdict. A mid-mode-3 SCX
                // rewrite (`scx_write_dot != 0`) commits `eff.scx` at the eager
                // cc+0 write frame — 4 dots (8hd) before its true cc+4 landing
                // — so it reaches the render's fine-scroll hunt (`render.rs`
                // ~dot 89) BEFORE the hunt latches and the render over-discards
                // the NEW fine-scroll, flipping `eff.scx&7` dots late (258 vs the
                // production/tier2 254 on `late_scx4_2`: the write's true cc+4
                // landing is PAST the hunt → the current line keeps the fetch-
                // start length). The FF43 write-commit debt that would fix this
                // in the render is REFUTED (`eff.scx` IS the length — it breaks
                // the `late_scx_late_disable` window siblings; see `regs.rs`
                // `stage_write`). This is the verdict-only READ analogue: undo the
                // extension in the bare exit ONLY (window aborts own the
                // `scx_write_dot` arm above). `eager_value`+DMG+bare-scoped →
                // byte-identical flag-off.
                if self.eager_value && !self.model.is_cgb() && self.render.scx_write_dot != 0 {
                    flip = flip.saturating_sub(u16::from(self.scx & 7));
                }
                // The SS post-switch bare exit: a
                // 4-variable table. `E = 504 + leave_k −
                // 4*[lcd_enable_in_ds] + 2*(SCX&7)` rp — the leave k
                // (dsa7-branched, 2/6) and the enable-in-DS re-anchor are
                // the two class variables; ISR carry drops out (the
                // carried m2int and polled ly44 legs share constants).
                // Scoped to mid-frame-anchored dances post-LCD-on-leave
                // (`stop_anchor_midframe`): a blanket arm's 14
                // SameBoy-pass drops were the VBlank/boot-anchored classes
                // (base/frame1/nop m2int + offset2/3 counts) this anchor
                // excludes; the emergent arm still serves those. In scope
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
                    let en = if self.lcd_enable_in_ds { 4 } else { 0 };
                    fold(
                        &mut exit,
                        504 + i32::from(self.stop_leave_k) - en + 2 * scx7,
                    );
                } else {
                    let phase = i32::from(self.lcd_phase_hd);
                    // The emergent bare exit's `+2` over-holds a POLLED read
                    // that lands EXACTLY on the flip boundary. Production reads
                    // mode 0 AT `flip_dot` (the flip is inclusive), so the true
                    // CPU-visible mode-0 boundary sits at rphd `2*flip`, not
                    // `2*flip + 2`. sprite0's polled measurement read is the one
                    // ROM that reads at exactly rphd `2*flip` (its whole point is
                    // to bracket the flip): `ppu_sprite0_scx{2,6}_b` eager reads
                    // rphd 512/520 = `2*flip` and want mode 0, but `+2` (514/522)
                    // forces mode 3. The carried m2int/scx weld-partners
                    // (`late_scx4_1`, `m2int_m3stat_1`) read the SAME rphd 512 yet
                    // carry `= 4` — their `- carry` already lands exit `2*flip - 2`
                    // — and want mode 3, so the split is `read_carried`, NOT the
                    // uniform read-frame bias swept (`ARM8BIAS`, which shifts
                    // both and shuffles). Drop the `+2` only for the eager-DMG
                    // polled read → tier2 + production byte-identical (both keep
                    // `+2`), carried reads untouched (`- carry` owns them).
                    let over = if self.eager_value && !self.model.is_cgb() && !self.read_carried {
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

    /// Shadow window-extend predicate (tier2 + CGB only). Fires ONLY
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
            // In DS the slack was +4 (`_1` trigdot 101 / `_2` 103 vs
            // wxmatch 97). The corrected DS line-153 lyfc table moves
            // the LYC=153 wake — and with it every ISR-timed WY write in this
            // family — 2 dots earlier (`_1` 99 / `_2` 101), so the DS slack
            // re-derives to the SS value (+2); the same shift is what fixes
            // the `late_wy_ds` blocker trio outright.
            //
            // SS DMG eager: the same LYC=153-wake re-host. The dot-4
            // line-153 LYC STAT emission decouple (`reclock.rs`) fires the
            // shared LYC=153 ISR one M-cycle (4 dots SS) EARLIER than the stale
            // dot-6/dot-8 recognition the `+2` slack was tuned against, so every
            // ISR-timed `late_wy` WY write — and its `wy_trig_sb_dot` — moves 4
            // dots earlier (`FFto2_ly2_2` trigdot 98→94, `_3` 102→98). The `+2`
            // deadline (wxmatch 97 → 99) then extends BOTH (`_3` 98 ≤ 99), where
            // SameBoy renders `_3` bare. Re-derive to `−2` (wxmatch → 95) so
            // `_2` (94 ≤ 95, extend) and `_3` (98 > 95, bare) re-split — the
            // exact −4 read-debt of the emission move, the SS twin of the DS
            // lyfc re-derivation above. `eager_value && !is_cgb` only (the CGB
            // emission is unmoved; production + tier2 byte-identical).
            && i32::from(self.wy_trig_sb_dot)
                <= i32::from(self.render.wx_match_dot)
                    + if self.eager_value && !self.model.is_cgb() { -2 } else { 2 }
    }
}
