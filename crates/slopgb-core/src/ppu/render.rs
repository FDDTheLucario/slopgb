//! Mode 3 pixel pipeline: BG/window fetcher, pixel FIFO, sprite fetcher.
//!
//! Timing model: the pipeline starts at line dot 84 (the glitched
//! LCD-enable line blocks from dot 78 but starts its pipe at 82) and
//! performs one step per dot. The fetcher needs 12 dots before the first
//! pixel ships (a discarded first tile fetch plus a real one), each
//! shipped pixel takes one dot, and SCX%8 leading pixels are popped and
//! discarded, so an unobstructed line's pipe ends after 172 + SCX%8 dots
//! at dot 256 + SCX%8 — with the externally visible mode-0 flip and the
//! mode-0 IRQ source leading the pipe end by 2 dots (`m0_flip_events`),
//! matching `hblank_ly_scx_timing-GS`.
//!
//! Sprite fetches stall the pipeline for 6 dots each (CGB-C discounts
//! the first fetch of the line to 5 — see `obj_fetch_base`), plus a
//! first-per-tile alignment penalty of max(0, 5 - (x + SCX) % 8) dots
//! (the BG fetcher finishing its current tile row in real time during
//! the stall). This reproduces every case table in
//! `intr_2_mode0_timing_sprites` exactly (Pan Docs "Mode 3 length" OBJ
//! penalty algorithm) — on DMG the mode-0 flip leads the
//! sprite-extended pipe end by 3 dots, keeping each case's flip on its
//! mooneye dot while the pop grid sits one dot later than the old
//! 5-dot model (the mealybug blob photographs pin the pixels).

use super::{
    LCDC_BG_ENABLE, LCDC_BG_MAP, LCDC_OBJ_ENABLE, LCDC_OBJ_SIZE, LCDC_TILE_DATA, LCDC_WIN_ENABLE,
    LCDC_WIN_MAP, Ppu,
};
use crate::SCREEN_W;
use crate::model::Model;

// Behavior-preserving submodules (each a second `impl Ppu` block). The Render
// struct, SpritePixel/FetchPhase types, consts, and the render_init/render_step
// driver stay here.
mod mode0;
mod sprite;
mod window;

#[derive(Clone, Copy, Default)]
pub(super) struct Sprite {
    y: u8,
    /// `pub(super)` for the #11bh Arm-3b sprite-at-window-X check.
    pub(super) x: u8,
    tile: u8,
    flags: u8,
    idx: u8,
}

/// One pending sprite pixel, aligned to upcoming output positions.
#[derive(Clone, Copy)]
struct SpritePixel {
    color: u8,
    /// DMG: OBP number (0/1); CGB: palette index 0-7.
    palette: u8,
    /// OAM attribute bit 7 (BG-over-OBJ).
    bg_priority: bool,
    /// OAM index, for CGB priority resolution.
    oam_idx: u8,
}

const EMPTY_SPRITE_PIXEL: SpritePixel = SpritePixel {
    color: 0,
    palette: 0,
    bg_priority: false,
    oam_idx: 0xFF,
};

/// OBJ fetch stall base cost. On the DMG blob every fetch pays the full
/// 6 dots; on CGB-C the first fetch of the line overlaps one dot of BG
/// pipeline work and costs 5. The mealybug blob photographs pin the DMG
/// pop grid one dot later than the old 5-dot first fetch
/// (m3_bgp_change_sprites/m3_obp0_change boundary columns land exactly
/// one pixel right of the 5-dot grid and are pixel-exact at 6), while
/// the CGB-C photographs (m3_lcdc_bg_en_change/obj_en_change) pin the
/// 5-dot first fetch. Every mooneye/gbmicrotest IRQ anchor stays on its
/// frozen dot because the DMG mode-0 flip leads the sprite-extended
/// pipe end by 3 dots instead of 2 (see `m0_flip_events`):
/// intr_2_mode0_timing_sprites' X=0 case still flips at 264.
fn obj_fetch_base(cgb: bool, fetched: u16) -> u16 {
    if cgb && fetched == 0 { 5 } else { 6 }
}

/// BG/window fetcher state. Each of the three VRAM reads (tile number, low
/// bitplane, high bitplane) takes 2 dots — the fetcher steps at half the dot
/// clock (Pan Docs "Pixel FIFO", "Get Tile"/"Get Tile Data Low"/"Get Tile
/// Data High" each lasting 2 dots) — modelled as an explicit wait state
/// before each read. The push into the FIFO retries every dot until the
/// FIFO drains (Pan Docs "Push": "this state is executed only if [the
/// FIFO] is empty").
#[derive(Clone, Copy, PartialEq, Eq)]
enum FetchPhase {
    /// First dot of the tile-number read.
    TileNoWait,
    /// Second dot: latch the tile number (+ CGB attributes).
    TileNo,
    /// First dot of the low-bitplane read.
    LoWait,
    /// Second dot: latch the low bitplane.
    Lo,
    /// First dot of the high-bitplane read.
    HiWait,
    /// Second dot: latch the high bitplane; push if the FIFO is empty.
    Hi,
    /// Tile row latched but the FIFO was full: retry the push each dot.
    Push,
}

pub(super) struct Render {
    pub(super) active: bool,
    /// Next output pixel x (0-159).
    lx: u8,
    /// Leading pixels still to discard (the fixed post-match fine-scroll
    /// schedule, or 7-WX window columns for WX<7).
    discard: u8,
    /// Mode-3 dots elapsed (render_step calls), anchoring the fine-scroll
    /// comparator hunt below.
    mode3_dot: u16,
    /// Pause-aware mode-3 dot: counts only dots the pipeline actually
    /// advances (frozen through sprite-fetch and window-start stalls),
    /// mirroring the hardware position counter the WX comparator runs
    /// against. Anchors the WX 0-7 window trigger: with no stalls it
    /// equals `mode3_dot`, and a prefill (OAM X < 8) sprite stall shifts
    /// the match later by the stall instead of skipping the comparison
    /// dot (m3_lcdc_win_map_change2: WX=7 with X=1/X=5 sprites on every
    /// line still draws the window).
    pos_dot: u16,
    /// SCX fine-scroll position comparator index (hardware positions
    /// -16..-9, cycling). From mode-3 dot 5 — where the first (thrown
    /// away) tile's pixels start popping on hardware — the comparator
    /// advances one step per dot (one per pop once real pops begin),
    /// comparing against SCX&7 *live* each step; on a match the discard
    /// schedule is fixed (SameBoy render_pixel_if_possible: `(position &
    /// 7) == (SCX & 7) -> position = -8`, with the -9 -> -16 wrap when
    /// the match was missed; gambatte scx_during_m3 sweeps).
    /// (`pub(super)` with `hunt_done` for the #11bh glitch re-open, `regs.rs`.)
    pub(super) hunt_idx: u8,
    /// The comparator matched: the fine-scroll discard is locked in.
    pub(super) hunt_done: bool,
    /// #11bh — the machine dot of the fine-scroll comparator match (0 =
    /// none yet). The glitch-line same-dot SCX-write hunt re-open keys on
    /// it (`regs.rs` FF43): a write landing on the match dot committed
    /// AFTER that dot's render tick, where hardware's comparator still
    /// sees the new value. Tier-2 only consumer; written unconditionally
    /// (inert flag-off).
    pub(super) hunt_match_dot: u16,
    /// Pipeline frozen for this many dots (sprite fetches).
    stall: u16,
    /// While a sprite-fetch stall runs, the BG fetcher keeps stepping in
    /// real time for this many dots — the alignment penalty *is* the
    /// fetcher finishing its in-flight tile row, after which it parks in
    /// `Push` (m3_lcdc_tile_sel_change bands 8-17 pin the mid-line read
    /// dots landing on consecutive stall dots; m3_scy_change line 0 pins
    /// the prefill X<8 path's refetch sampling the SCY written
    /// mid-stall). The first pixel still pops on the stall-shifted dot:
    /// see `push_allowed`.
    fetch_run: u16,

    // BG FIFO: 8 pixels as shift registers, all from one tile (pushes only
    // happen into an empty FIFO).
    bg_lo: u8,
    bg_hi: u8,
    bg_attr: u8,
    bg_count: u8,

    // Fetcher.
    phase: FetchPhase,
    /// Tile column counter (BG: added to SCX/8; window: from 0).
    fetch_x: u8,
    /// Fetching window tiles instead of BG.
    win_mode: bool,
    /// First fetch of the line is thrown away (12-dot mode 3 startup).
    first_discard: bool,
    t_no: u8,
    t_attr: u8,
    t_lo: u8,
    t_hi: u8,

    // Sprites (selected during OAM scan). `pub(super)` for the #11bh Arm-3b
    // sprite-at-window-X check (`stat_irq.rs`), like `n_sprites` below.
    pub(super) sprites: [Sprite; 10],
    /// `pub(super)` so the `vis_mode_read` window-length law (`stat_irq.rs`)
    /// can exclude sprite-extended lines from the off-screen-window range
    /// (the bare `259+SCX&7` exit does not carry the sprite penalty).
    pub(super) n_sprites: u8,
    fetched: u16,
    /// BG tiles that already paid the first-sprite alignment penalty,
    /// keyed by (x + SCX) / 8.
    penalty_tiles: u64,
    sp_fifo: [SpritePixel; 8],

    pub(super) win_active: bool,
    /// A window start (or aborted DMG WX=166 start) stalled this line:
    /// the restarted fetcher idles one dot later at the line tail, so
    /// the mode-0 flip/IRQ lead over the pipe end shrinks to 1 dot
    /// (gambatte window/late_* m3stat rows pin the later flip; the
    /// gbmicrotest win*_b line-1 grid prefers a 2-dot lead there and
    /// stays in the baseline — a documented one-dot conflict).
    /// `pub(super)` for the PORT 1 half-dot bare-exit law's clean-bare
    /// guard (`stat_irq.rs::vis_mode_read`).
    pub(super) win_stalled: bool,
    /// The window was aborted mid-line by an LCDC.5 clear: on DMG the
    /// resumed BG fetch trails at the line tail, dropping the flip lead
    /// to 0 (gambatte window/late_disable_* rows carry dmg08_out3 vs
    /// cgb04c_out0 split expectations for the same read). `pub(super)` for
    /// the C2 #11y window-length read law (`stat_irq.rs::vis_mode_read`).
    pub(super) win_aborted: bool,
    /// C2 #11at — a window enabled at line start was disabled by a mid-line
    /// LCDC.5 clear BEFORE it began drawing (`!win_mode` at the clear, set in
    /// `regs.rs::commit_eff`). SameBoy renders such a line BARE but with the
    /// SCX fine-scroll penalty DROPPED (mattcurrie §WIN_EN) → mode-3 exit
    /// cfl257, not 257+SCX&7; slopgb's whole-dot render over-extends it. The
    /// CGB-visible flag for the shadow bare-exit law (`stat_irq.rs::
    /// vis_mode_read`) — `win_aborted` is DMG-only. A POST-draw abort is NOT
    /// flagged (its exit extends by the tiles drawn, a per-config length the
    /// atomic render reclock owns). Reset per line. `pub(super)` for the law.
    pub(super) win_predraw_abort: bool,
    /// C2 #11at — the dot of a pre-draw window abort (see [`Self::
    /// win_predraw_abort`]). The bare exit still tracks the abort dot within
    /// the pre-draw class (an abort 1 M-cycle later catches the window's first
    /// tile → extends past the bare exit), so the read law thresholds on it.
    pub(super) win_predraw_abort_dot: u16,
    /// C2 #11au — the dot a mid-mode-3 LCDC.5 RE-enable landed (0 if none this
    /// line), for the shadow window-reenable mode-3 length law (`stat_irq.rs::
    /// vis_mode_read`). A `late_reenable` window redraws from the re-enable
    /// point; its mode-3 extends past the read iff the re-enable beat the WX
    /// match (`re-enable_dot <= wx_match_dot - 3` — the redraw-start deadline).
    /// slopgb's whole-dot render collapses the re-enable-dot dependence. Reset
    /// per line. `pub(super)` for the read law.
    pub(super) win_reenable_dot: u16,
    /// #11bf item 3a — the dot a mid-line LCDC.5 FIRST enable landed (0 if
    /// none this line; distinct from [`Self::win_reenable_dot`]: latched only
    /// when the window was neither active nor aborted, i.e. the enable IS the
    /// window trigger). A late-ENABLE-triggered first window line takes the
    /// STEADY mode-3 exit (`259+SCX&7+ds`) where a late-WY-triggered one
    /// extends later (#11y) — the trigger SOURCE is the arm-1 first-line
    /// discriminator (`late_enable_ly0_ds` want-pair straddles the steady
    /// exit). Reset per line; tier2+CGB only (read law input).
    pub(super) win_enable_dot: u16,
    /// #11bf item 3c — the dot a mid-line FF4B (WX) rewrite committed while
    /// the render was active (0 if none this line). A rewrite landing
    /// AT/BEFORE the WX match dot un-catches the window on SameBoy (the
    /// `late_wx_scx5` pair: write at the match dot 97 → bare, at 101 →
    /// extends) while slopgb's whole-dot render catches first. Reset per
    /// line; tier2+CGB law input only.
    pub(super) wx_write_dot: u16,
    /// WX comparator output on the previous dot: activations and
    /// reactivations fire on the rising edge only (the match holds while
    /// lx is frozen during the start stall and must not re-fire).
    win_match_prev: bool,
    /// Pause-aware prefill position (hardware positions -16..-9 as
    /// 0..=8): walks one step per *non-stalled* dot from mode-3 dot 5,
    /// driving both the SCX comparator hunt and the OBJ position
    /// comparator for sprites with OAM X 0-7 — an OBJ fetch freezes the
    /// walk (the SCX hunt pauses during an X<8 sprite fetch).
    prefill_pos: u8,
    /// **C2 #11af shadow WX-activation dot (tier2 + CGB only; 0 = no match
    /// yet).** The dot the raw WX comparator first matched this line,
    /// recorded *before* the `wy_ok`/`win_en` activation gate — so it is
    /// available even on a bare line the window never enters. The shadow
    /// WY-trigger ([`Ppu::wy_trig_sb`]) only extends mode 3 on a line where
    /// it was set at/before this dot (the SameBoy activation deadline).
    pub(super) wx_match_dot: u16,
}

impl Render {
    pub(super) fn new() -> Self {
        Self {
            active: false,
            lx: 0,
            discard: 0,
            mode3_dot: 0,
            pos_dot: 0,
            hunt_idx: 0,
            hunt_done: false,
            hunt_match_dot: 0,
            stall: 0,
            fetch_run: 0,
            bg_lo: 0,
            bg_hi: 0,
            bg_attr: 0,
            bg_count: 0,
            phase: FetchPhase::TileNoWait,
            fetch_x: 0,
            win_mode: false,
            first_discard: true,
            t_no: 0,
            t_attr: 0,
            t_lo: 0,
            t_hi: 0,
            sprites: [Sprite::default(); 10],
            n_sprites: 0,
            fetched: 0,
            penalty_tiles: 0,
            sp_fifo: [EMPTY_SPRITE_PIXEL; 8],
            win_active: false,
            win_stalled: false,
            win_aborted: false,
            win_predraw_abort: false,
            win_predraw_abort_dot: 0,
            win_reenable_dot: 0,
            win_enable_dot: 0,
            wx_write_dot: 0,
            win_match_prev: false,
            prefill_pos: 0,
            wx_match_dot: 0,
        }
    }
}

/// Line dot on which the serial mode-2 scan latches + evaluates OAM
/// entry `i` (gbctr "OAM scan": one entry per 2 dots across mode 2;
/// gambatte sprite_mapper.cpp `OamReader::update` latches (y,x) per
/// entry at the same 2-cycle rate; SameBoy display.c's mode-2 loop).
///
/// Anchoring (the one free parameter, like `oam_bug_row`'s): the
/// gambatte oamdma late_sp00/01/02/39 x/y `_1`/`_2` pairs each race an
/// OAM DMA start (x) or end (y) one M-cycle apart around a single
/// slot's latch dot and read the resulting mode-3 length. On both
/// model families they cohere on entry 0 latching on dot 3 or 4 — the
/// slot-39 rows (78-dot grid span vs 80-dot delay span) cut each
/// family's window to {3,4}, indistinguishable at the single-speed
/// M-cycle granularity; 3 is taken as the canonical anchor. The `_ds`
/// siblings would discriminate the dot but race at gambatte's half-dot
/// cc granularity, unrepresentable on this whole-dot lattice (they ride
/// the documented-swap list with the ds mode-0 flip lead). The last
/// entry's latch (dot 81) lands before mode 3 consumes the selection at
/// dot 84.
const SCAN_OFF: u16 = 3;

fn scan_latch_dot(i: u16) -> u16 {
    2 * i + SCAN_OFF
}

impl Ppu {
    pub(super) fn render_init(&mut self) {
        let r = &mut self.render;
        r.active = true;
        r.lx = 0;
        r.discard = 0;
        r.mode3_dot = 0;
        r.pos_dot = 0;
        r.hunt_idx = 0;
        r.hunt_done = false;
        r.hunt_match_dot = 0;
        r.stall = 0;
        r.fetch_run = 0;
        r.bg_count = 0;
        r.phase = FetchPhase::TileNoWait;
        r.fetch_x = 0;
        r.win_mode = false;
        r.first_discard = true;
        r.fetched = 0;
        r.penalty_tiles = 0;
        r.sp_fifo = [EMPTY_SPRITE_PIXEL; 8];
        r.win_active = false;
        r.win_stalled = false;
        r.win_aborted = false;
        r.win_predraw_abort = false;
        r.win_predraw_abort_dot = 0;
        r.win_reenable_dot = 0;
        r.win_enable_dot = 0;
        r.wx_write_dot = 0;
        r.win_match_prev = false;
        r.prefill_pos = 0;
        r.wx_match_dot = 0;
        if self.glitch_line {
            // No OAM scan ran on the glitched LCD-enable line: no sprites.
            r.n_sprites = 0;
            // The glitched line's pixel pipe starts 4 dots after its
            // blocking start: the lcdon_timing-GS OAM/VRAM tables pin the
            // unblock inside (248, 252] — the flip at 252 + SCX%8 sits at
            // the window top — and the gbmicrotest line-0 grids
            // (hblank_int_l0, int_hblank_nops/incs_scx0-7 dispatches,
            // int_hblank_halt_scx0-7 halt wakes) pin the IRQ rise at
            // 252 + SCX%8 for all eight SCX classes. Blocking begins at
            // dot 78, the pipe at 82, ending at 254 + SCX%8.
            r.stall = 4;
        }
        // A window-start request carried over from a DMG WX=166 match
        // (see `win_start_pending`): consumed at the next line's mode-3
        // start, which begins the line with the window drawing from the
        // left edge (gambatte M3Start::f0: a pending win_draw_start with
        // the window enabled becomes win_draw_started and increments
        // winYPos; otherwise the request drops). The line runs the
        // normal 12-dot startup whose thrown-away first fetch consumes
        // window column 0 — the on_screen/wxA6_wy00 reference pins the
        // diagonal marker tiles one column left of their map position —
        // and the SCX&7 fine-scroll hunt still applies (gambatte
        // M3Start::f1 discards scx % 8 regardless of the window state).
        if std::mem::take(&mut self.win_start_pending) && self.eff.lcdc & LCDC_WIN_ENABLE != 0 {
            self.win_line = self.win_line.wrapping_add(1);
            let r = &mut self.render;
            // Counts as this line's activation: the line's own WX match
            // must not increment the counter again.
            r.win_active = true;
            r.win_mode = true;
            r.fetch_x = 1;
        }
    }

    /// One mode-3 dot.
    pub(super) fn render_step(&mut self) {
        self.render.mode3_dot += 1;
        if self.render.stall > 0 {
            self.stall_tick();
            return;
        }
        self.render.fetch_run = 0;

        // SCX fine-scroll comparator hunt, dot-rate phase: on hardware the
        // first (thrown away) tile's pixels pop on mode-3 dots 5-12 while
        // the position comparator hunts for SCX&7 (see `hunt_idx`); our
        // pipeline never pushes that tile, so the comparator runs here as
        // a bare counter. A match at dot 5+n fixes the discard schedule
        // at n leading pixels (pixel 0 then ships at dot 13+n, matching
        // `hblank_ly_scx_timing-GS` for stable SCX). A line that *begins*
        // in window mode (DMG WX=166 carryover) still runs the hunt —
        // gambatte M3Start::f1 applies the scx % 8 discard regardless of
        // the window state; mid-line window starts set `hunt_done`.
        if self.render.mode3_dot >= 5 && self.render.prefill_pos < 8 {
            let pos = self.render.prefill_pos;
            if !self.render.hunt_done {
                if self.render.hunt_idx == self.eff.scx & 7 {
                    self.render.hunt_done = true;
                    self.render.discard = pos;
                    self.render.hunt_match_dot = self.dot;
                    // TEMP #11bb hunt tracer (`SLOPGB_S5DBG`; byte-identical
                    // unset): pin the fine-scroll match dot + discard against
                    // SameBoy's SCX-write straddle (`late_scx4`).
                    if crate::ppu::s5dbg_on() && (130..144).contains(&self.line) {
                        eprintln!(
                            "SLOPGB hunt ly={} dot={} scx={} discard={pos}",
                            self.line, self.dot, self.eff.scx
                        );
                    }
                } else {
                    self.render.hunt_idx = (self.render.hunt_idx + 1) & 7;
                }
            }
            // OBJ position comparator, prefill phase: sprites with OAM X
            // 0-7 are reached while the thrown-away first tile shifts
            // out — before any pixel pops — and their fetches freeze the
            // pipeline *including the SCX hunt* (gambatte runs
            // LoadSprites from xpos 0 with the M3Start scx discard
            // resuming afterwards). The stall arithmetic is unchanged
            // (mooneye intr_2_mode0_timing_sprites is the frozen
            // oracle). The BG fetcher free-runs through the stall like
            // the mid-line path, sampling eff (m3_scy_change line 0 pins
            // the refetch rows to the SCY written mid-stall) — the
            // 12-dot startup anchor is held by the `pos_dot` push gate
            // in `push_allowed`, not by freezing the fetch.
            self.render.prefill_pos += 1;
            if self.eff.lcdc & LCDC_OBJ_ENABLE != 0 {
                loop {
                    let mut pick: Option<usize> = None;
                    for i in 0..usize::from(self.render.n_sprites) {
                        if self.render.fetched & (1 << i) != 0 {
                            continue;
                        }
                        let s = self.render.sprites[i];
                        if s.x == pos && pick.is_none() {
                            pick = Some(i);
                        }
                    }
                    let Some(i) = pick else { break };
                    let s = self.render.sprites[i];
                    let base = obj_fetch_base(self.model.is_cgb(), self.render.fetched);
                    self.render.fetched |= 1 << i;
                    let wait = self.sprite_penalty(s.x);
                    self.fetch_sprite(i);
                    self.render.stall += base + wait;
                }
                if self.render.stall > 0 {
                    self.render.fetch_run = self.render.stall;
                    self.stall_tick();
                    return;
                }
            }
        }

        // Window trigger first: when the WX match and a sprite trigger
        // land on the same dot, the window start preempts the sprite
        // fetch — the restarted fetcher reads the window tile before the
        // sprite stall, and the sprite (FIFO now empty) defers to the
        // refill (m3_lcdc_win_map_change band 8: sprite X=8 with WX=7
        // shares dot 97 and the photo shows the window tile fetched
        // ahead of the toggle).
        if self.window_trigger_step() {
            return;
        }

        // Sprite fetch triggers at the current output position, but only
        // once the pipeline is actually about to ship that pixel (the FIFO
        // holds pixels and SCX discarding is done) — the alignment penalty
        // is the BG fetcher finishing the tile row it is mid-way through.
        // Multiple sprites can share a trigger only in the left-clipped
        // X <= 8 group (all trigger at lx == 0); they are fetched in
        // ascending (X, OAM index) order, not OAM order: on hardware the
        // OBJ position comparator also runs through the 8-pixel prefill,
        // so an X=3 sprite is reached before an X=8 one. That keeps the
        // first-fetched-wins FIFO merge equal to the DMG lower-X-wins rule
        // (Pan Docs "Drawing priority": smaller X = higher priority, OAM
        // order only breaks ties).
        let fine_scrolling = !self.render.win_mode && !self.render.hunt_done;
        if self.eff.lcdc & LCDC_OBJ_ENABLE != 0
            && self.render.bg_count > 0
            && self.render.discard == 0
            && !fine_scrolling
        {
            loop {
                let mut pick: Option<usize> = None;
                for i in 0..usize::from(self.render.n_sprites) {
                    if self.render.fetched & (1 << i) != 0 {
                        continue;
                    }
                    let s = self.render.sprites[i];
                    if s.x < 8 || s.x >= 168 || s.x - 8 != self.render.lx {
                        continue;
                    }
                    // Strict `<` keeps the earlier slot (= lower OAM
                    // index) on equal X.
                    if pick.is_none_or(|p| s.x < self.render.sprites[p].x) {
                        pick = Some(i);
                    }
                }
                let Some(i) = pick else { break };
                let s = self.render.sprites[i];
                let base = obj_fetch_base(self.model.is_cgb(), self.render.fetched);
                self.render.fetched |= 1 << i;
                let wait = self.sprite_penalty(s.x);
                self.fetch_sprite(i);
                self.render.stall += base + wait;
                self.m0_unflip();
            }
            if self.render.stall > 0 {
                // The BG fetcher free-runs through the stall (trigger dot
                // included): the alignment wait is the fetcher finishing
                // its tile row in real time, reads landing on consecutive
                // stall dots (see `fetch_run`).
                self.render.fetch_run = self.render.stall;
                self.stall_tick();
                return;
            }
        }

        // Pop one BG/window pixel.
        if self.render.bg_count > 0 {
            let fine_scx = self.eff.scx & 7;
            let r = &mut self.render;
            let c = ((r.bg_hi >> 7) << 1) | (r.bg_lo >> 7);
            r.bg_lo <<= 1;
            r.bg_hi <<= 1;
            r.bg_count -= 1;
            let attr = r.bg_attr;
            if r.discard > 0 {
                r.discard -= 1;
            } else if !r.hunt_done {
                // Comparator hunt, pop-rate phase: the match was missed
                // during dots 5-12 (an SCX write moved it), so the
                // counter wrapped (-9 -> -16) and keeps hunting through
                // the real pops, each one discarded; a match here leaves
                // the 7 remaining -8..-1 drops (see `hunt_idx`).
                if r.hunt_idx == fine_scx {
                    r.hunt_done = true;
                    r.discard = 7;
                } else {
                    r.hunt_idx = (r.hunt_idx + 1) & 7;
                }
            } else {
                self.output_pixel(c, attr);
                self.advance_lx();
                if !self.render.active {
                    return;
                }
            }
        }

        self.fetcher_step();
    }
}

/// "Magic enable" for the MGB frozen-OAM-DMA sprite glitch: sprites render
/// at all only if OAM holds at least one properly aligned 4-byte entry whose
/// bytes lie within `[$98-$9F, $00-$A7, $09-$9F, $00-$A7]`. "The position in
/// OAM does not matter, and there can be more than one. ... If any value is
/// out of range, the data will have no effect."
/// (madness/mgb_oam_dma_halt_sprites.s)
fn oam_glitch_magic_enable(oam: &[u8; 0xA0]) -> bool {
    oam.chunks_exact(4).any(|e| {
        (0x98..=0x9F).contains(&e[0])
            && e[1] <= 0xA7
            && (0x09..=0x9F).contains(&e[2])
            && e[3] <= 0xA7
    })
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
