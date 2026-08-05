// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

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
    /// match SEVERAL arms (e.g. a window line that is also sprite-laden); the
    /// source laws were ordered fall-through blocks, whose combined verdict
    /// folds to: `m == 3` arms (force-0 past their exit) take the MINIMUM
    /// matching exit, `m == 0` arms (hold-3 below their exit) the MAXIMUM. Each
    /// arm keeps its own guards.
    ///
    /// Every exit here is EMERGENT — `2 * flip + frame`, anchored to the
    /// render's own recorded or projected mode-0 flip. The render carries the
    /// line's whole mode-3 cost (fine-scroll discard, window start, sprite
    /// penalty), so all that is left per config is where the READ sits relative
    /// to that flip:
    ///
    /// | scope | read frame (half-dots) |
    /// |---|---|
    /// | CGB on-screen triggering window | −4 carried / +4 polled |
    /// | CGB off-screen window (WX >= 0xA0) | −2 SS / −4 DS |
    /// | DMG on-screen triggering window | −4 |
    /// | DMG off-screen window (WX == 0xA6) | −6 |
    /// | CGB DS window + sprites | +1 |
    /// | CGB DS pre-draw abort before the WX match | 0 |
    /// | bare line | +2 SS / −2 + 2*(SCX&1) DS |
    ///
    /// The behaviours that once needed their own config arms — window-Y
    /// triggering, the pre-draw and drawn aborts, the re-enable redraw
    /// deadline, the WX-rewrite un-catch — live in the window machine and the
    /// renderer now (`ppu/render/window.rs`), where they move pixels as well as
    /// this read.
    pub(in crate::ppu) fn vis_exit_hd(&self, m: u8) -> Option<i32> {
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
                // The off-screen (WX >= 0xA0) window renders nothing, and the
                // render's flip already lands where the line ends — so this is
                // the same emergent exit as the on-screen branch below, only the
                // read frame differs. Double speed reads a dot earlier than
                // single, its M-cycle being half as long.
                let flip = if self.line_render_done && self.flip_dot != 0 {
                    self.flip_dot
                } else {
                    self.projected_flip_dot()
                };
                fold(
                    &mut exit,
                    2 * i32::from(flip) + if self.ds { -4 } else { -2 },
                );
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
            } else {
                // WX == 0xA6: the off-screen window renders nothing, and the
                // render's flip already lands on the bare end it should
                // (`m2int_wxA6_scx2_m3stat`: flip 259 == 257 + SCX&7) — including
                // the extension an object at WX+1 adds. Only the read frame
                // separates it from the on-screen branch above, one dot earlier
                // at -6. Bracketed by the ROMs, not swept: -5 and -8 each drop
                // two rows, -7 and -6 both hold, and -6 is the whole dot.
                let flip = if self.line_render_done && self.flip_dot != 0 {
                    self.flip_dot
                } else {
                    self.projected_flip_dot()
                };
                fold(&mut exit, 2 * i32::from(flip) - 6);
            }
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
