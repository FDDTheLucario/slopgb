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

#[derive(Clone, Copy, Default)]
pub(super) struct Sprite {
    y: u8,
    x: u8,
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
    hunt_idx: u8,
    /// The comparator matched: the fine-scroll discard is locked in.
    hunt_done: bool,
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

    // Sprites (selected during OAM scan).
    sprites: [Sprite; 10],
    n_sprites: u8,
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
    win_stalled: bool,
    /// The window was aborted mid-line by an LCDC.5 clear: on DMG the
    /// resumed BG fetch trails at the line tail, dropping the flip lead
    /// to 0 (gambatte window/late_disable_* rows carry dmg08_out3 vs
    /// cgb04c_out0 split expectations for the same read).
    win_aborted: bool,
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
            win_match_prev: false,
            prefill_pos: 0,
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
    /// One dot of the serial OAM scan: latch + evaluate the entry whose
    /// slot this dot is, if any. Called for every dot below mode-3 start
    /// (84) of a visible non-glitch line.
    pub(super) fn oam_scan_step(&mut self) {
        let off = scan_latch_dot(0);
        if self.dot < off || (self.dot - off) % 2 != 0 {
            return;
        }
        let i = (self.dot - off) / 2;
        if i >= 40 {
            return;
        }
        if i == 0 {
            self.render.n_sprites = 0;
        }
        self.oam_scan_entry(i as u8);
    }

    /// Run a whole line's scan at once: selection equivalent of the serial
    /// grid on an undisturbed line. Test/diagnostic helper only — the dot
    /// path goes through [`Self::oam_scan_step`].
    #[cfg(test)]
    pub(super) fn oam_scan(&mut self) {
        self.render.n_sprites = 0;
        for i in 0..40 {
            self.oam_scan_entry(i);
        }
    }

    /// Latch OAM entry `i` from the PPU's OAM view and select it for this
    /// line if its Y matches, in OAM order, by Y only (X — even 0 or ≥168
    /// — does not affect selection; it only affects fetching: see
    /// `intr_2_mode0_timing_sprites`), capped at 10.
    ///
    /// The view is not always real OAM:
    ///
    /// * While an OAM DMA transfer sits frozen mid-byte on MGB (HALT gates
    ///   the core clock the DMA controller runs on), every entry reads as
    ///   the same glitched sprite — fully characterized by
    ///   madness/mgb_oam_dma_halt_sprites.s, hardware-verified by its
    ///   author (see [`Self::mgb_dma_freeze_glitch_entry`]).
    /// * While the OAM DMA controller owns OAM — running *or* frozen by
    ///   HALT/STOP on the other models ("DMG: A different sprite ... CGB:
    ///   Checkerboard without sprites" — the asm gives no reference data
    ///   for their glitches) — the scan is disconnected and latches $FF, a
    ///   disabled sprite (gambatte memory.cpp startOamDma/endOamDma switch
    ///   the OamReader source to rdisabledRam; the dmg08-verified
    ///   oamdma/late_sp* and oamdma_late_halt_stat families pin the
    ///   selection loss per slot).
    fn oam_scan_entry(&mut self, i: u8) {
        let (y, x, tile, flags) = if self.model == Model::Mgb && self.dma_freeze.is_some() {
            match self.mgb_dma_freeze_glitch_entry() {
                Some(e) => e,
                None => return,
            }
        } else if self.oam_dma_active {
            (0xFF, 0xFF, 0xFF, 0xFF)
        } else {
            let b = usize::from(i) * 4;
            (
                self.oam[b],
                self.oam[b + 1],
                self.oam[b + 2],
                self.oam[b + 3],
            )
        };
        let h = if self.eff.lcdc & LCDC_OBJ_SIZE != 0 {
            16u16
        } else {
            8
        };
        let row = u16::from(self.ly) + 16;
        if self.render.n_sprites < 10 && row >= u16::from(y) && row < u16::from(y) + h {
            let n = usize::from(self.render.n_sprites);
            self.render.sprites[n] = Sprite {
                y,
                x,
                tile,
                flags,
                idx: i,
            };
            self.render.n_sprites += 1;
        }
    }

    /// The glitched entry every OAM slot reads as while an OAM DMA
    /// transfer sits frozen mid-byte on MGB, or `None` when the magic
    /// enable is absent. Everything here implements the hardware behavior
    /// documented in madness/mgb_oam_dma_halt_sprites.s:
    ///
    /// With `new` = the in-flight DMA source byte, `old` = the OAM byte it
    /// was about to replace and `next` = the OAM byte after that one, every
    /// OAM entry is seen as the same glitched sprite
    ///
    /// ```text
    /// Y: (old | new) & $FC      C: (old | new) & $FC
    /// X:  next | new            F:  next | new
    /// ```
    ///
    /// ("Why & $FC? I have no idea, but it seems that the low two bits are
    /// always 0"). Selection then proceeds as normal — Y match, 10-sprite
    /// cap — so a matching line gets its sprite slots filled with identical
    /// copies, which render as a single sprite shape (the asm's expected
    /// image shows exactly one).
    fn mgb_dma_freeze_glitch_entry(&self) -> Option<(u8, u8, u8, u8)> {
        let (index, new) = self.dma_freeze?;
        // "This is the data that somehow enables sprite rendering": without
        // at least one aligned magic entry in OAM, no sprite renders.
        if !oam_glitch_magic_enable(&self.oam) {
            return None;
        }
        // The interconnect caps the in-flight index at 159, but the pub
        // freeze API accepts any u8: out-of-range degrades to the
        // undriven-bus value like `next` below (matching `oam_dma_write`'s
        // bounds check) instead of panicking.
        let old = self.oam.get(usize::from(index)).copied().unwrap_or(0xFF);
        // The byte after the in-flight one. A freeze on the final byte
        // (index 159) has no successor; the asm does not pin that case and
        // $FF is the usual undriven-bus value.
        let next = self
            .oam
            .get(usize::from(index) + 1)
            .copied()
            .unwrap_or(0xFF);
        let y = (old | new) & 0xFC;
        let x = next | new;
        Some((y, x, y, x))
    }

    pub(super) fn render_init(&mut self) {
        let r = &mut self.render;
        r.active = true;
        r.lx = 0;
        r.discard = 0;
        r.mode3_dot = 0;
        r.pos_dot = 0;
        r.hunt_idx = 0;
        r.hunt_done = false;
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
        r.win_match_prev = false;
        r.prefill_pos = 0;
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

    /// Consume one stall dot. While a sprite fetch holds the pipeline
    /// (prefill or mid-line), the BG fetcher keeps stepping in real time
    /// (`fetch_run`) until it parks with a completed row — see the field
    /// docs.
    fn stall_tick(&mut self) {
        self.render.stall -= 1;
        if self.render.fetch_run > 0 {
            self.render.fetch_run -= 1;
            self.fetcher_step();
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

    /// The window trigger: the WX position comparator runs every dot
    /// (gambatte ppu.cpp plotPixel: `wx == xpos`, xpos < 168), checked
    /// *before* the same-dot sprite trigger (see the call site). Returns
    /// true when the caller's render_step must end (a start stall or a
    /// reactivation pixel consumed the dot). The comparator also runs
    /// through the 8-dot prefill — so WX 0-7 match before any pixel
    /// pops; from the first pop on, a match at WX >= 8 lands the first
    /// window pixel at lx = WX-7. The wx+6 prefill anchor is pinned by
    /// the m3_window_timing reference photographs: every WX 0-7 line
    /// pops pixel 0 at dot 103 — the same 6-dot-delayed schedule as
    /// WX 8-10 — so trigger + 6-dot restart + (7-WX)-pixel discard must
    /// sum to 19 prefill dots. The machine is gated on LCDC.5 + the WY
    /// latch only: LCDC.0 blanks pixels at output but does not stop the
    /// window fetch (gambatte lcdcWinEn).
    fn window_trigger_step(&mut self) -> bool {
        // The position counter the WX comparator runs against advances
        // only on dots the pipeline advances: sprite-fetch stalls freeze
        // it (and the stall returns in render_step skip this increment
        // on the trigger dot itself), so a WX 0-7 match shifts later by
        // the stall instead of skipping its comparison dot
        // (m3_lcdc_win_map_change2's per-line X<8 sprites).
        self.render.pos_dot += 1;
        let wx = self.eff.wx;
        let win_match = if wx <= 7 {
            self.render.pos_dot == u16::from(wx) + 6
        } else {
            wx <= 166 && self.render.lx == wx - 7
        };
        // The WY condition: the frame-sticky latch OR a live match
        // against the delayed WY copy (gambatte plotPixel:
        // `weMaster || (wy2 == ly && lcdcWinEn)`).
        let wy_ok = self.wy_latch || self.wy2 == self.ly;
        // Rising edge only: the match level holds while lx is frozen
        // through the start stall and must not re-fire.
        let prev_match = std::mem::replace(&mut self.render.win_match_prev, win_match);
        let win_match = win_match && !prev_match;
        let win_en_now = self.eff.lcdc & LCDC_WIN_ENABLE != 0;
        if win_match
            && !win_en_now
            && self.wy_latch
            && !self.model.is_cgb()
            && wx == 166
            && !self.win_start_pending
        {
            // DMG: a WX=166 match with the window *disabled* still
            // latches the start request when the frame's WY latch holds
            // (gambatte plotPixel's `!cgb` branch runs without lcdcWinEn
            // when weMaster is set; requests at any other WX are
            // consumed and dropped one dot later, but the xpos >= 167
            // bound leaves the 166 one pending into the next line --
            // on_screen/wxA6_weoff_at_xposA6). Honored at the next
            // mode-3 start only if the window is enabled by then.
            self.win_start_pending = true;
        }
        if win_match && wy_ok && win_en_now {
            if !self.render.win_active {
                // Activation: the window line counter advances *here*
                // (gambatte plotPixel: ++winYPos), which is what makes a
                // same-line retrigger draw the next row (mattcurrie
                // comprehensive-ppu-doc §WIN_EN).
                self.win_line = self.win_line.wrapping_add(1);
                if !self.model.is_cgb() && wx == 166 {
                    // DMG: the start request raised at a WX=166 match is
                    // never consumed in-line (gambatte
                    // handleWinDrawStartReq honors requests at
                    // xpos >= 167 only on CGB): no window pixel ships —
                    // the pipeline only freezes briefly for the aborted
                    // start (m2int_wxA6_m3stat_1/_2 bracket the DMG
                    // mode-3 end 1-4 dots past the unextended end) —
                    // and the request survives to the next line's
                    // mode-3 start (see `win_start_pending`). The line
                    // still counts as started (gambatte keeps
                    // win_draw_started set) — the comparator must not
                    // re-fire while lx sits at 159 through the stall.
                    self.win_start_pending = true;
                    self.render.win_active = true;
                    self.render.win_stalled = true;
                    // Freeze from the match dot: 2 dots total.
                    self.render.stall += 1;
                    self.m0_unflip();
                    return true;
                } else {
                    self.m0_unflip();
                    let r = &mut self.render;
                    r.win_active = true;
                    r.win_stalled = true;
                    r.win_mode = true;
                    r.bg_count = 0;
                    r.phase = FetchPhase::TileNoWait;
                    r.fetch_x = 0;
                    r.first_discard = false;
                    // Window pixels are not subject to SCX fine scroll;
                    // WX<7 cuts the leading 7-WX window columns instead,
                    // and the BG fine-scroll comparator hunt ends with
                    // the BG fetching.
                    r.hunt_done = true;
                    r.discard = 7u8.saturating_sub(wx);
                    if wx == 0 {
                        // WX=0 with a fine scroll: the start eats the
                        // SCX&7 discard plus one extra dot (SameBoy
                        // display.c WX=0/SCX&7 extra cycle; the mealybug
                        // m3_window_timing_wx_0 photos pin pixel 0 at
                        // dot 103 + SCX&7 + 1 on both DMG and CGB-C).
                        let fine = self.eff.scx & 7;
                        if fine > 0 {
                            r.discard += fine + 1;
                        }
                    }
                }
            } else if !self.model.is_cgb() && wx == 166 && !self.win_start_pending {
                // DMG: a WX=166 match with the window already drawing
                // re-arms the carryover without counting a new activation
                // (gambatte plotPixel else-branch: `xpos == lcd_hres + 6`
                // sets win_draw_start; M3Start::f0 increments winYPos
                // when it consumes the request), with the same short
                // aborted-start freeze. `win_start_pending` doubles as
                // the once-per-line guard while lx sits at 159.
                self.win_start_pending = true;
                self.render.win_stalled = true;
                self.render.stall += 1;
                self.m0_unflip();
                return true;
            } else if self.render.win_mode && self.render.bg_count == 8 {
                // Window *reactivation*: a WX match while the window is
                // already drawing, landing exactly on the dot that ships
                // the first pixel of a window tile, emits one color-0
                // pixel and delays the rest of the line by one dot
                // (mealybug m3_wx_5_change.asm: "Window reactivation
                // zero pixels should be present when window is already
                // activated and the pixel that the window reactivates on
                // is on the same cycle as the window tile nametable
                // read" -- its reference photos pin the inserted zero
                // pixel on exactly the rows where WX-7 falls on a window
                // tile boundary, and pin that off-boundary matches have
                // no visible effect).
                self.output_pixel(0, 0);
                self.advance_lx();
                return true;
            }
        }
        false
    }

    /// Advance the output position and fire the pipe-end anchors:
    ///
    /// * lx 159 (gambatte xpos 167): the HBlank DMA trigger leads the
    ///   pipe end by one dot (see [`Ppu::hdma_lead`]).
    /// * lx 160 (xpos 168): the pipeline ends; `render_finished` anchors
    ///   the HBlank-DMA window and CGB palette-RAM blocking (gambatte
    ///   hdma_start/cgbpAccessible calibration — must not move with the
    ///   visible flip, which leads it: see [`Ppu::m0_flip_events`]).
    fn advance_lx(&mut self) {
        self.render.lx += 1;
        match self.render.lx {
            159 => self.hdma_lead = true,
            160 => {
                self.render.active = false;
                self.render_finished = true;
                // The CGB palette-RAM unblock (this `render_finished` rise)
                // is half-classified by the interconnect for the cc+2
                // MID-phase FF69/FF6B read (sub-dot event-phase model);
                // bare steady-state lines only (see `m0_access_flip`).
                self.pal_access_flip =
                    self.render.fetched == 0 && !self.render.win_active && !self.glitch_line;
                if !self.m0_src {
                    // Zero-lead lines (DMG aborted windows) flip at the
                    // pipe end itself; also the safety net for any
                    // projection miss.
                    self.m0_src = true;
                    self.m0_rise_dot = true;
                    self.line_render_done = true;
                }
            }
            _ => {}
        }
    }

    /// The mode-0 STAT IRQ rise and the externally visible mode-0 flip
    /// (STAT mode bits read 0, OAM/VRAM unblock), evaluated once per
    /// mode-3 dot (after the dot's render step). Both land on the same
    /// dot, leading the pipe end *including every late stall* by 2 dots
    /// on a bare line — 254 + SCX%8 + penalties — 1 dot in double speed
    /// or after a window start, 0 after a DMG window abort (the `lead`
    /// computation below; gambatte's xpos-166/167 event pair and its
    /// cc+2 access offset fold to one dot in our end-sampled lattice).
    /// The pins: the gbmicrotest hblank_int_scx0-7 dispatch
    /// grid and hblank_int_scx*_if_b/c FF0F-vs-dispatch races (IRQ),
    /// the wilbertpol intr_2_mode0_scx*_nops STAT polls and mooneye's
    /// intr_2_mode0_timing/_sprites windows (flip), and mooneye
    /// hblank_ly_scx_timing-GS plus gbmicrotest int_hblank_halt_scx0-7
    /// (the halt-wake view of the same rise).
    ///
    /// On hardware this is the fetcher/sprite machinery going idle
    /// while the FIFO drains its last pixels, a combinational condition
    /// — modelled here as a projection over the renderer's committed
    /// state: the line ends in `stall + refill + pixels-left` dots once
    /// no sprite fetch or window start can intervene. The projection is
    /// exact for every pinned case; window starts inside the final tile
    /// (WX 164-166) can land it ±1 dot (the gambatte wx_166 rows judge
    /// those).
    ///
    /// `m0_src` also takes over the OAM blocking level gaplessly so an
    /// enabled mode-2 source still blocks the edge (gambatte
    /// m2int_m0irq).
    pub(super) fn m0_flip_events(&mut self) {
        if self.m0_src || !self.render.active {
            return;
        }
        let r = &self.render;
        let mut proj = r.stall + (160 - u16::from(r.lx));
        // Dots until the FIFO can pop again (it refills mid-fetch only
        // after a window start in the final tile).
        if r.bg_count == 0 {
            proj += match r.phase {
                FetchPhase::TileNoWait => 6,
                FetchPhase::TileNo => 5,
                FetchPhase::LoWait => 4,
                FetchPhase::Lo => 3,
                FetchPhase::HiWait => 2,
                FetchPhase::Hi | FetchPhase::Push => 1,
            };
        }
        // Sprite fetches still ahead of the output position: the base
        // cost plus the first-per-tile alignment penalty (mirrors the
        // fetch path without committing the tile set).
        if self.eff.lcdc & LCDC_OBJ_ENABLE != 0 {
            let mut tiles = r.penalty_tiles;
            let mut fetched = r.fetched;
            for i in 0..usize::from(r.n_sprites) {
                if r.fetched & (1 << i) != 0 {
                    continue;
                }
                let x = r.sprites[i].x;
                if (8..168).contains(&x) && x - 8 >= r.lx {
                    proj += obj_fetch_base(self.model.is_cgb(), fetched);
                    fetched |= 1 << i;
                    let v = u16::from(x) + u16::from(self.eff.scx);
                    if tiles & (1u64 << (v >> 3)) == 0 {
                        tiles |= 1u64 << (v >> 3);
                        proj += 5u16.saturating_sub(v & 7);
                    }
                }
            }
        }
        // A window start still ahead: 6 dots (FIFO restart), or the 2-dot
        // DMG WX=166 aborted-start freeze. A reactivation zero pixel
        // (±1 dot, boundary-dependent) is not projected.
        let wx = self.eff.wx;
        if self.eff.lcdc & LCDC_WIN_ENABLE != 0
            && (self.wy_latch || self.wy2 == self.ly)
            && (7..=166).contains(&wx)
            && wx - 7 >= r.lx
        {
            let dmg_166 = !self.model.is_cgb() && wx == 166;
            if !r.win_active {
                proj += if dmg_166 { 2 } else { 6 };
            } else if dmg_166 && !self.win_start_pending {
                proj += 2;
            }
        }
        // Sprite-laden DMG lines flip 3 dots before the pipe end: the
        // blob's 6-dot first OBJ fetch (see `obj_fetch_base`) extends the
        // pipe by one dot more than the old 5-dot model, and the flip
        // stays on its mooneye/gbmicrotest-frozen dot (the photographs
        // move the pixels, the IRQ grids hold the flip).
        let lead = (2 + u16::from(r.fetched != 0 && !self.model.is_cgb()) - u16::from(self.ds))
            .saturating_sub(u16::from(r.win_stalled) + u16::from(r.win_aborted));
        // Increment 1 of the sub-dot event-phase model calibrates the
        // accessibility-unblock sub-dot phase for STEADY-STATE BARE-line
        // flips only. Sprite/window mode-3 extensions push the visible
        // flip onto its mooneye/gbmicrotest-frozen dot (above), and the
        // LCD-enable glitch line runs a 452-dot/dot-82-pipe geometry — both
        // carry a different cc+2 accessibility phase (gambatte
        // oam_access/10spritesprline_postread_2 reads unblocked; gbmicrotest
        // lcdon_to_oam_unlock/oam_read_l0 + mooneye lcdon_timing-GS unlock
        // earlier). Gate the OAM-read MID signal to those lines; the
        // sprite/window/glitch phases are later increments.
        let bare_flip = r.fetched == 0 && !r.win_active && !self.glitch_line;
        if proj <= lead {
            self.m0_src = true;
            self.m0_rise_dot = true;
            self.line_render_done = true;
            // The accessibility unblock (this `line_render_done` rise) is
            // half-classified by the interconnect for the cc+2 MID-phase
            // OAM read (sub-dot event-phase model, increment 1).
            self.m0_access_flip = bare_flip;
            // The STAT mode-bit flip routes the double-speed FF41 mode-bit
            // read at the cc+2 MID phase (sub-dot event-phase model,
            // increment INC-DS-1 — gambatte sprites m3stat_ds). Gated to
            // sprite-extended lines (`r.fetched != 0`): bare-line DS reads
            // that reach FF41 through the DMA-cycle / lcd-offset chains
            // (dma/gdma/hdma_cycles_scx5_ds_2, lcd_offset m0stat_count) sit at
            // a different sub-cycle offset within the same M-cycle half, so a
            // bare-line override regresses them — the parked multi-chain
            // problem. Sprite lines are the clean, hold-floor-safe subset.
            self.m0_stat_flip = r.fetched != 0;
        }
    }

    /// A stall source engaged after the mode-0 flip already fired (a
    /// late WY/WX/LCDC write armed a window start or sprite fetch inside
    /// the final tile): the flip is a combinational level on hardware —
    /// the fetcher waking back up drops STAT back to mode 3, re-blocks
    /// OAM/VRAM and lowers the IRQ source until the projection re-fires
    /// (gambatte window/late_wy_* and late_disable_* m3stat rows pin the
    /// drop; an IF bit already latched stays latched, matching the
    /// edge-pulse the hardware line produced).
    fn m0_unflip(&mut self) {
        if self.m0_src && self.render.active {
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.line_render_done = false;
        }
    }

    /// First-per-BG-tile sprite alignment penalty (Pan Docs OBJ penalty
    /// algorithm; verified against intr_2_mode0_timing_sprites).
    fn sprite_penalty(&mut self, x: u8) -> u16 {
        let v = u16::from(x) + u16::from(self.eff.scx);
        let key = v >> 3;
        if self.render.penalty_tiles & (1u64 << key) != 0 {
            0
        } else {
            self.render.penalty_tiles |= 1u64 << key;
            5u16.saturating_sub(v & 7)
        }
    }

    /// LCDC.5 cleared mid-line while the window is drawing. The disable
    /// "takes effect at the end of the current window tile being drawn"
    /// and the BG then resumes "on a tile boundary — the low 3 bits of
    /// SCX have no effect" (mattcurrie comprehensive-ppu-doc §WIN_EN).
    /// Mechanically (gambatte ppu.cpp setLcdc + Tile::f0): the started
    /// flag clears immediately, the FIFO/latched window tile row still
    /// ships, remaining reads of the in-flight fetch revert to BG
    /// addressing, and the next BG map read uses the live column
    /// `(scx + xpos + 1 - cgb) / 8` — re-anchoring the tile grid to the
    /// output position rather than re-showing skipped columns.
    pub(super) fn window_abort(&mut self) {
        if !self.render.win_mode {
            return;
        }
        let cgb = self.model.is_cgb();
        let r = &mut self.render;
        if !cgb {
            r.win_aborted = true;
        }
        r.win_mode = false;
        // Re-arms the trigger: re-enabling with WX pointing at a pixel
        // not yet drawn retriggers the window (doc §WIN_EN).
        r.win_active = false;
        // First screen pixel of the tile the *next* tile-number read
        // belongs to: the FIFO drains bg_count more pops (minus pending
        // discards), and a fetch already past its tile-number read ships
        // one more full row first.
        let tileno_pending = matches!(r.phase, FetchPhase::TileNoWait | FetchPhase::TileNo);
        let x = i32::from(r.lx) + i32::from(r.bg_count) - i32::from(r.discard)
            + if tileno_pending { 0 } else { 8 };
        let col = (i32::from(self.eff.scx) + x.max(0) + 1 - i32::from(cgb)) >> 3;
        r.fetch_x = (col as u8).wrapping_sub(self.eff.scx >> 3) & 31;
    }

    fn fetcher_step(&mut self) {
        // Every fetch read samples the pipeline view (eff) at its read
        // dot — the m3_lcdc_tile_sel/bg_map blob bands bracket each
        // stage's sampling to the eff commit exactly, and the gambatte
        // bgtiledata spx cgb04c rows pin the same clean commit on CGB-C.
        // (A CGB rising-bits-one-late LCDC view fits most of the
        // tile_sel/bg_map/win_map _cgb_c photo columns but contradicts
        // the hardware-captured bgtiledata_spx0B_2/_4 rows — the CGB
        // fetch residue stays documented in baselines/mealybug.txt.)
        // The fine-scroll comparator hunt and the pop side have their
        // own anchors and never read these.
        let lcdc = self.eff.lcdc;
        let (scy, scx) = (self.eff.scy, self.eff.scx);
        match self.render.phase {
            FetchPhase::TileNoWait => self.render.phase = FetchPhase::TileNo,
            FetchPhase::TileNo => {
                // Tile number (+ attributes on CGB) from the tile map. The
                // row is sampled from SCY here for the *map* address only;
                // the data reads re-sample it (see `bg_tile_addr`).
                let (map_bit, row, col) = if self.render.win_mode {
                    (LCDC_WIN_MAP, self.win_line >> 3, self.render.fetch_x & 31)
                } else {
                    (
                        LCDC_BG_MAP,
                        self.ly.wrapping_add(scy) >> 3,
                        (scx / 8).wrapping_add(self.render.fetch_x) & 31,
                    )
                };
                let base = if lcdc & map_bit != 0 { 0x1C00 } else { 0x1800 };
                let map = base + usize::from(row) * 32 + usize::from(col);
                self.render.t_no = self.vram[map];
                self.render.t_attr = if self.model.is_cgb() {
                    self.vram[0x2000 + map]
                } else {
                    0
                };
                self.render.phase = FetchPhase::LoWait;
            }
            FetchPhase::LoWait => self.render.phase = FetchPhase::Lo,
            FetchPhase::Lo => {
                let addr = self.bg_tile_addr(lcdc, scy);
                self.render.t_lo = self.vram[addr];
                self.render.phase = FetchPhase::HiWait;
            }
            FetchPhase::HiWait => self.render.phase = FetchPhase::Hi,
            FetchPhase::Hi => {
                let addr = self.bg_tile_addr(lcdc, scy) + 1;
                self.render.t_hi = self.vram[addr];
                if self.render.first_discard {
                    // The first tile fetch of the line is thrown away and
                    // restarted: 12 dots of mode 3 before the first pixel.
                    self.render.first_discard = false;
                    self.render.phase = FetchPhase::TileNoWait;
                } else if self.render.bg_count == 0 && self.push_allowed() {
                    self.push_bg_row();
                } else {
                    self.render.phase = FetchPhase::Push;
                }
            }
            FetchPhase::Push => {
                if self.render.bg_count == 0 && self.push_allowed() {
                    self.push_bg_row();
                }
            }
        }
    }

    /// The first push of a line waits for the pause-aware startup walk:
    /// the FIFO ships nothing before pause-aware dot 13 (pos_dot 12 is
    /// the push dot of the bare 12-dot startup), so a prefill sprite
    /// stall whose free-running fetch completes early still pops pixel 0
    /// exactly `stall` dots late (the mooneye X=0 cost-10 anchor and the
    /// hblank_ly_scx grids). Mid-line pushes are never gated (pos_dot is
    /// past 12 from the first shipped pixel on), and a window start
    /// replaces the walk with its own 6-dot restart — its push ships at
    /// trigger+6 even when the trigger sits inside the startup window
    /// (m3_window_timing/m3_window_timing_wx_0: pixel 0 at dot 103).
    fn push_allowed(&self) -> bool {
        self.render.win_mode || self.render.pos_dot >= 12
    }

    fn push_bg_row(&mut self) {
        let r = &mut self.render;
        let (lo, hi) = if r.t_attr & 0x20 != 0 {
            // X flip (CGB BG attribute bit 5).
            (r.t_lo.reverse_bits(), r.t_hi.reverse_bits())
        } else {
            (r.t_lo, r.t_hi)
        };
        r.bg_lo = lo;
        r.bg_hi = hi;
        r.bg_attr = r.t_attr;
        r.bg_count = 8;
        r.fetch_x = r.fetch_x.wrapping_add(1);
        r.phase = FetchPhase::TileNoWait;
    }

    /// Tile-data address for the current fetch's Lo/Hi read. The fine row
    /// is re-derived from SCY (or the window line counter) at *each* data
    /// access rather than latched at the tile-number read: an SCY write
    /// landing between the accesses fetches the new scroll's rows under
    /// the old tile number (mealybug m3_scy_change; gambatte scy/). The
    /// CGB Y-flip applies to whatever row the access samples.
    ///
    /// `lcdc`/`scy` carry the caller's sampling view (see
    /// `fetcher_step`).
    fn bg_tile_addr(&self, lcdc: u8, scy: u8) -> usize {
        let r = &self.render;
        let fine = if r.win_mode {
            self.win_line & 7
        } else {
            self.ly.wrapping_add(scy) & 7
        };
        let fine = if r.t_attr & 0x40 != 0 {
            7 - fine // Y flip (CGB BG attribute bit 6).
        } else {
            fine
        };
        let bank = if self.model.is_cgb() && r.t_attr & 0x08 != 0 {
            0x2000
        } else {
            0
        };
        let base = if lcdc & LCDC_TILE_DATA != 0 {
            usize::from(r.t_no) * 16
        } else {
            (0x1000i32 + i32::from(r.t_no as i8) * 16) as usize
        };
        bank + base + usize::from(fine) * 2
    }

    /// Fetch sprite `i`'s row and merge it into the sprite FIFO.
    fn fetch_sprite(&mut self, i: usize) {
        let s = self.render.sprites[i];
        let tall = self.eff.lcdc & LCDC_OBJ_SIZE != 0;
        let h: u8 = if tall { 16 } else { 8 };
        // Selection bounded the row by the height LCDC.2 held at OAM-scan
        // time (dot 80), but LCDC.2 is re-read here at fetch time: a game
        // clearing it (16 -> 8) mid-mode-3 can leave row >= h. Mask into the
        // current height (h is a power of two) — the hardware row counter
        // feeds the tile-data address through these low bits either way —
        // so the Y flip below cannot underflow.
        let mut row = self.ly.wrapping_add(16).wrapping_sub(s.y) & (h - 1);
        if s.flags & 0x40 != 0 {
            row = h - 1 - row; // Y flip.
        }
        let tile = if tall {
            // 8x16: bit 0 of the tile index is ignored (Pan Docs).
            (s.tile & 0xFE) + (row >> 3)
        } else {
            s.tile
        };
        let bank = if self.model.is_cgb() && s.flags & 0x08 != 0 {
            0x2000
        } else {
            0
        };
        let addr = bank + usize::from(tile) * 16 + usize::from(row & 7) * 2;
        let mut lo = self.vram[addr];
        let mut hi = self.vram[addr + 1];
        if s.flags & 0x20 != 0 {
            lo = lo.reverse_bits();
            hi = hi.reverse_bits();
        }
        let cgb = self.model.is_cgb();
        // CGB with OPRI bit 0 clear: lower OAM index wins regardless of X;
        // otherwise (DMG, or OPRI=1) earlier-fetched (= leftmost, then
        // lowest OAM index) sprites keep their pixels.
        let index_priority = cgb && self.opri & 1 == 0;
        for px in 0..8u8 {
            let screen = i16::from(s.x) - 8 + i16::from(px);
            let slot = screen - i16::from(self.render.lx);
            if !(0..8).contains(&slot) {
                continue;
            }
            let bit = 7 - px;
            let c = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1);
            let entry = &mut self.render.sp_fifo[slot as usize];
            let replace = if entry.color == 0 {
                true
            } else {
                index_priority && c != 0 && s.idx < entry.oam_idx
            };
            if replace {
                *entry = SpritePixel {
                    // Integration addition: in DMG compatibility mode the
                    // CGB PPU uses the DMG palette bit (OAM flag bit 4,
                    // selecting OBP0/OBP1 -> obj palette 0/1), not the CGB
                    // palette bits (Pan Docs "DMG compatibility mode").
                    palette: if cgb && !self.dmg_compat {
                        s.flags & 0x07
                    } else {
                        (s.flags >> 4) & 1
                    },
                    color: c,
                    bg_priority: s.flags & 0x80 != 0,
                    oam_idx: s.idx,
                };
            }
        }
    }

    fn output_pixel(&mut self, bg_c: u8, bg_attr: u8) {
        // Shift the sprite FIFO in step with shipped pixels.
        let mut sp = self.render.sp_fifo[0];
        self.render.sp_fifo.copy_within(1.., 0);
        self.render.sp_fifo[7] = EMPTY_SPRITE_PIXEL;
        // LCDC.1 also gates sprite pixels at the mix: pixels already in
        // the FIFO stop showing on dots where the OBJ enable reads low
        // (mealybug m3_lcdc_obj_en_change: sprites fetched during the
        // prefill turn into background mid-glyph at the disable commit).
        // The DMG mixer samples the bit one dot ahead of the eff view
        // (the fetch-lead timing — the blob photos put each band's
        // suppression boundary one column left of the eff commit);
        // CGB-C samples eff (its leg is pixel-exact on eff).
        if self.eff.lcdc & LCDC_OBJ_ENABLE == 0 {
            sp = EMPTY_SPRITE_PIXEL;
        }

        let cgb = self.model.is_cgb();
        // DMG LCDC bit 0: BG and window disabled — they show as white
        // (color 0 for sprite priority purposes). DMG compatibility mode on
        // CGB behaves the same way (integration addition).
        let bg_off = (!cgb || self.dmg_compat) && self.eff.lcdc & LCDC_BG_ENABLE == 0;
        let bg_c = if bg_off { 0 } else { bg_c };

        let sprite_wins = sp.color != 0
            && if cgb {
                // CGB: BG color 0 always loses; LCDC bit 0 clear strips all
                // BG priority; else BG attribute bit 7 or OAM bit 7 wins.
                bg_c == 0
                    || self.eff.lcdc & LCDC_BG_ENABLE == 0
                    || !(bg_attr & 0x80 != 0 || sp.bg_priority)
            } else {
                !(sp.bg_priority && bg_c != 0)
            };

        let color = if sprite_wins {
            if cgb {
                // Integration addition: DMG compatibility mode remaps the
                // pixel through OBP0/OBP1 before the (boot-installed)
                // compat palette (Pan Docs "DMG compatibility mode").
                let c = if self.dmg_compat {
                    let obp = if sp.palette == 1 {
                        self.eff.obp1
                    } else {
                        self.eff.obp0
                    };
                    (obp >> (sp.color * 2)) & 3
                } else {
                    sp.color
                };
                self.cgb_color(&self.obj_pal_ram, sp.palette, c)
            } else {
                let obp = if sp.palette == 1 {
                    self.eff.obp1
                } else {
                    self.eff.obp0
                };
                self.dmg_palette[usize::from((obp >> (sp.color * 2)) & 3)]
            }
        } else if cgb {
            // Integration addition: compat mode remaps BG pixels through
            // BGP; BG attributes are all zero (VRAM bank 1 is locked), so
            // palette 0 is used either way.
            let c = if self.dmg_compat && !bg_off {
                (self.eff.bgp >> (bg_c * 2)) & 3
            } else {
                bg_c
            };
            self.cgb_color(&self.bg_pal_ram, bg_attr & 0x07, c)
        } else if bg_off {
            self.dmg_palette[0]
        } else {
            self.dmg_palette[usize::from((self.eff.bgp >> (bg_c * 2)) & 3)]
        };

        let idx = usize::from(self.ly) * SCREEN_W + usize::from(self.render.lx);
        self.back[idx] = color;
    }

    /// RGB555 palette RAM entry to XRGB8888: straight 5→8 bit expansion
    /// ((c << 3) | (c >> 2)), no color correction in the core.
    fn cgb_color(&self, ram: &[u8; 64], palette: u8, color: u8) -> u32 {
        let i = usize::from(palette) * 8 + usize::from(color) * 2;
        let raw = u16::from(ram[i]) | (u16::from(ram[i + 1]) << 8);
        let expand = |c: u16| -> u32 { u32::from(((c << 3) | (c >> 2)) & 0xFF) };
        let r = expand(raw & 0x1F);
        let g = expand((raw >> 5) & 0x1F);
        let b = expand((raw >> 10) & 0x1F);
        (r << 16) | (g << 8) | b
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
