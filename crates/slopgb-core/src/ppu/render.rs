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
mod tests {
    use super::super::Ppu;
    use super::oam_glitch_magic_enable;
    use crate::Model;

    const WHITE: u32 = 0xFF_FFFF;
    const LIGHT: u32 = 0xAA_AAAA;
    const DARK: u32 = 0x55_5555;
    const BLACK: u32 = 0x00_0000;

    fn run_to(p: &mut Ppu, line: u8, dot: u16) {
        let mut guard = 0u32;
        while !(p.line == line && p.dot == dot) {
            p.tick();
            guard += 1;
            assert!(guard < 200_000, "run_to({line},{dot}) never reached");
        }
    }

    /// Render the given line to completion; returns the dot at which mode 3
    /// ended (V0).
    fn render_line(p: &mut Ppu, line: u8) -> u16 {
        run_to(p, line, 84);
        finish_line(p)
    }

    fn px(p: &Ppu, line: usize, x: usize) -> u32 {
        p.back[line * crate::SCREEN_W + x]
    }

    fn dmg_on(lcdc: u8) -> Ppu {
        let mut p = Ppu::new(Model::Dmg);
        p.write(0xFF47, 0xE4); // identity BGP
        p.write(0xFF48, 0xE4);
        p.write(0xFF49, 0xE4);
        p.write(0xFF40, lcdc);
        p
    }

    fn set_tile_row(p: &mut Ppu, bank: usize, tile: usize, row: usize, lo: u8, hi: u8) {
        p.vram[bank * 0x2000 + tile * 16 + row * 2] = lo;
        p.vram[bank * 0x2000 + tile * 16 + row * 2 + 1] = hi;
    }

    fn set_map(p: &mut Ppu, base: usize, row: usize, col: usize, tile: u8) {
        p.vram[base + row * 32 + col] = tile;
    }

    fn sprite(p: &mut Ppu, i: u8, y: u8, x: u8, tile: u8, flags: u8) {
        p.oam_dma_write(i * 4, y);
        p.oam_dma_write(i * 4 + 1, x);
        p.oam_dma_write(i * 4 + 2, tile);
        p.oam_dma_write(i * 4 + 3, flags);
    }

    // --- BG rendering ---

    #[test]
    fn bg_tile_pixels_and_bgp() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), LIGHT);
        assert_eq!(px(&p, 2, 3), LIGHT);
        assert_eq!(px(&p, 2, 4), DARK);
        assert_eq!(px(&p, 2, 7), DARK);
        assert_eq!(px(&p, 2, 8), WHITE); // tile 0 = color 0

        // Remap shades through BGP.
        let mut p = dmg_on(0x91);
        p.write(0xFF47, 0x1B); // 0->3, 1->2, 2->1, 3->0
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), DARK);
        assert_eq!(px(&p, 2, 4), LIGHT);
        assert_eq!(px(&p, 2, 8), BLACK);
    }

    #[test]
    fn bg_scx_fine_scroll_shifts_pixels() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_map(&mut p, 0x1800, 0, 1, 1);
        p.write(0xFF43, 3);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), LIGHT); // bg col 3
        assert_eq!(px(&p, 2, 1), DARK); // bg col 4
        assert_eq!(px(&p, 2, 4), DARK); // bg col 7
        assert_eq!(px(&p, 2, 5), LIGHT); // bg col 8 = next tile col 0
    }

    #[test]
    fn bg_scy_selects_row() {
        let mut p = dmg_on(0x91);
        p.write(0xFF42, 5);
        set_tile_row(&mut p, 0, 1, 7, 0xFF, 0xFF); // line 2 + scy 5 = row 7
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLACK);
    }

    #[test]
    fn bg_signed_tile_addressing() {
        let mut p = dmg_on(0x81); // LCDC bit 4 clear: 0x8800 signed mode
        // Tile 0x80 lives at 0x9000 + (-128)*16 = 0x8800.
        p.vram[0x0800 + 2 * 2] = 0xFF;
        p.vram[0x0800 + 2 * 2 + 1] = 0xFF;
        set_map(&mut p, 0x1800, 0, 0, 0x80);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLACK);
    }

    #[test]
    fn bg_map_select_bit3() {
        let mut p = dmg_on(0x99); // bit 3: map at 0x9C00
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0xFF);
        set_map(&mut p, 0x1C00, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLACK);
    }

    #[test]
    fn dmg_lcdc0_blanks_bg_to_white() {
        let mut p = dmg_on(0x90); // BG disabled
        set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), WHITE);
        assert_eq!(px(&p, 2, 159), WHITE);
    }

    // --- Mode-3 IO write strobe ---
    //
    // The CPU drives the data bus during the second half of a write M-cycle
    // (gbctr "Memory access timing": the store lands around T3, not after
    // T4), so the dot-clocked pixel pipeline observes a rendering-register
    // write 2 dots before the tick-then-access commit point. Decoded from
    // the mealybug m3_bgp_change references: of the write M-cycle's four
    // dots, the pipeline pops dot 1 with the old value, dot 2 with old|new
    // on pre-CGB models (mealybug README: "BGP takes the value old OR new
    // for one cycle"; CGB-C switches cleanly and still reads old), and
    // dots 3-4 with the new value.

    /// Mimic the interconnect's write path: stage, tick one M-cycle (4 dots
    /// at normal speed), then commit architecturally.
    fn mcycle_write(p: &mut Ppu, addr: u16, value: u8) {
        p.stage_write(addr, value, 2);
        for _ in 0..4 {
            p.tick();
        }
        p.write(addr, value);
    }

    /// Finish the current line's mode 3; returns the dot it ended on (V0).
    fn finish_line(p: &mut Ppu) -> u16 {
        let mut flip = None;
        let mut guard = 0u32;
        while !p.line_render_done || p.render.active {
            p.tick();
            if p.line_render_done && flip.is_none() {
                flip = Some(p.dot);
            }
            guard += 1;
            assert!(guard < 2_000, "mode 3 never finished");
        }
        flip.expect("flip dot recorded")
    }

    #[test]
    fn strobe_bgp_write_two_dots_early_with_dmg_blend_pixel() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // solid color 1
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
        }
        // Pixel x pops at dot 97 + x (no SCX/sprites/window): after dot 130
        // pixels 0..=33 have shipped through the old palette.
        run_to(&mut p, 2, 130);
        mcycle_write(&mut p, 0xFF47, 0xE8); // color 1: shade 1 -> shade 2
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 254, "a palette strobe must not move mode-3 end");
        assert_eq!(px(&p, 2, 33), LIGHT, "well before the write: old");
        assert_eq!(px(&p, 2, 34), LIGHT, "write M-cycle dot 1: still old");
        assert_eq!(
            px(&p, 2, 35),
            BLACK,
            "dot 2 transition pixel: BGP reads old|new = 0xEC (color 1 -> 3)"
        );
        assert_eq!(px(&p, 2, 36), DARK, "dot 3: new value, 2 dots early");
        assert_eq!(px(&p, 2, 37), DARK, "dot 4: new");
        assert_eq!(px(&p, 2, 40), DARK, "after the commit: new");
    }

    #[test]
    fn strobe_bgp_write_clean_switch_on_cgb() {
        let mut p = cgb_on(0x91);
        p.set_dmg_compat(true); // BGP remaps into compat palette 0
        p.write(0xFF47, 0xE4);
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // solid color 1
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
        }
        run_to(&mut p, 2, 130);
        mcycle_write(&mut p, 0xFF47, 0xE8);
        finish_line(&mut p);
        let old = p.cgb_color(&p.bg_pal_ram, 0, 1);
        let new = p.cgb_color(&p.bg_pal_ram, 0, 2);
        let blend = p.cgb_color(&p.bg_pal_ram, 0, 3);
        assert_eq!(px(&p, 2, 34), old, "write M-cycle dot 1: old");
        assert_eq!(px(&p, 2, 35), old, "dot 2: still old — no blend on CGB");
        assert_ne!(px(&p, 2, 35), blend, "CGB never blends");
        assert_eq!(px(&p, 2, 36), new, "dot 3: new value, 2 dots early");
        assert_eq!(px(&p, 2, 37), new, "dot 4: new");
    }

    #[test]
    fn strobe_obp0_write_blend_pixel_dmg() {
        let mut p = dmg_on(0x93);
        p.write(0xFF48, 0xE4); // identity OBP0
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // sprite solid color 1
        sprite(&mut p, 0, 18, 8, 4, 0x00); // line 2, screen 0-7, OBP0
        // The X=8 sprite stalls the pipeline 6+5 dots at dot 97, so its
        // pixels 0-7 pop at dots 108-115; dots 110-113 cover x=2..=5.
        run_to(&mut p, 2, 108);
        mcycle_write(&mut p, 0xFF48, 0xE8);
        finish_line(&mut p);
        assert_eq!(px(&p, 2, 0), LIGHT, "before the write: old");
        assert_eq!(px(&p, 2, 1), LIGHT, "write M-cycle dot 1: old");
        assert_eq!(px(&p, 2, 2), BLACK, "dot 2: OBP0 reads old|new");
        assert_eq!(px(&p, 2, 3), DARK, "dot 3: new, 2 dots early");
        assert_eq!(px(&p, 2, 4), DARK, "dot 4: new");
    }

    /// Double speed: the M-cycle is 2 dots, the strobe lands 1 dot before
    /// the commit (second half of the M-cycle, same as normal speed).
    #[test]
    fn strobe_double_speed_one_dot_early() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00);
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
        }
        run_to(&mut p, 2, 130);
        p.stage_write(0xFF47, 0xE8, 1);
        for _ in 0..2 {
            p.tick();
        }
        p.write(0xFF47, 0xE8);
        finish_line(&mut p);
        assert_eq!(px(&p, 2, 34), BLACK, "ds dot 1: transition (old|new)");
        assert_eq!(px(&p, 2, 35), DARK, "ds dot 2: new, 1 dot early");
    }

    /// The SCX fine scroll is a live position comparator, not a latched
    /// discard count: the comparator hunts through positions 0..7
    /// (hardware positions -16..-9) one per dot from mode-3 dot 5,
    /// re-reading SCX&7 each step, and the discard schedule is fixed only
    /// once it matches (SameBoy render_pixel_if_possible: `(position &
    /// 7) == (SCX & 7) -> position = -8`; gambatte scx_during_m3 offset
    /// sweeps). A write landing during the hunt changes how many pixels
    /// drop and thereby the line's phase and mode-3 length.
    #[test]
    fn strobe_scx_write_during_hunt_changes_discard_count() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_map(&mut p, 0x1800, 0, 1, 1);
        p.write(0xFF43, 7); // hunt would match at dot 96 (index 7)
        // Stage SCX=2 at state(88): the pipeline view commits at dot 91,
        // where the hunt is at index 2 -> match: 2 pixels discard, pixel
        // 0 ships at dot 99 showing bg column 2.
        run_to(&mut p, 2, 88);
        mcycle_write(&mut p, 0xFF43, 2);
        let v0 = finish_line(&mut p);
        assert_eq!(px(&p, 2, 0), LIGHT, "pixel 0 is bg column 2 (color 1)");
        assert_eq!(px(&p, 2, 1), LIGHT, "bg column 3");
        assert_eq!(px(&p, 2, 2), DARK, "bg column 4");
        assert_eq!(v0, 256, "2 discarded pixels: V0 = 254 + 2");
    }

    /// If an SCX write makes the comparator miss its match (the new value
    /// points at an index the hunt already passed), the position counter
    /// wraps (-9 -> -16) and re-hunts: the discard grows by 8 and mode 3
    /// extends with it (SameBoy: `position_in_line == -9 -> position =
    /// -16`; gambatte scx_during_m3 encodes the +8 in its offset sweeps).
    #[test]
    fn strobe_scx_write_missing_the_match_wraps_the_hunt() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
        }
        p.write(0xFF43, 7);
        // Commit SCX=5 at dot 95: index 5 (dot 94) and earlier compared
        // against 7, indices 6 (dot 95) and 7 (dot 96) miss 5, the
        // counter wraps and re-hunts through the first real tile's pops,
        // matching at index 5 = dot 102 (the 6th pop): 13 pixels discard
        // in total.
        run_to(&mut p, 2, 92);
        mcycle_write(&mut p, 0xFF43, 5);
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 267, "13 discarded pixels: V0 = 254 + 13");
        assert_eq!(
            px(&p, 2, 0),
            DARK,
            "pixel 0 is bg column 13 (col 5: color 2)"
        );
    }

    /// The BG row is re-evaluated from SCY at each fetcher VRAM access,
    /// not latched per fetch: an SCY write landing between a tile's
    /// tile-number read and its data reads keeps the old tile number but
    /// fetches the new scroll's data rows (mealybug m3_scy_change;
    /// gambatte scy/).
    #[test]
    fn strobe_scy_write_between_tileno_and_data_reads_uses_new_row() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // tile 1 row 2: color 1
        set_tile_row(&mut p, 0, 1, 5, 0xFF, 0xFF); // tile 1 row 5: color 3
        set_tile_row(&mut p, 0, 2, 2, 0x00, 0xFF); // tile 2 (map row 1): color 2
        set_tile_row(&mut p, 0, 2, 5, 0x00, 0xFF);
        set_map(&mut p, 0x1800, 0, 1, 1); // old scroll: map row 0 -> tile 1
        set_map(&mut p, 0x1800, 1, 1, 2); // new scroll: map row 1 -> tile 2
        // Tile column 1 is fetched at dots 97-102: tile number at 98, data
        // low at 100, data high at 102. Staging SCY=11 at state(97) commits
        // the pipeline view at dot 100: the tile number was read with SCY=0
        // (map row 0 -> tile 1), the data reads use the new row
        // (2 + 11) & 7 = 5.
        run_to(&mut p, 2, 97);
        mcycle_write(&mut p, 0xFF42, 11);
        finish_line(&mut p);
        assert_eq!(px(&p, 2, 0), WHITE, "tile col 0 fetched before the write");
        assert_eq!(
            px(&p, 2, 8),
            BLACK,
            "old tile number (tile 1), new data row (5): color 3"
        );
    }

    /// A staged LCDC value must not enable/disable the LCD early: bit 7 is
    /// only honored at the architectural commit (`lcdon_*` mooneye tables
    /// were calibrated there).
    #[test]
    fn strobe_lcdc_bit7_only_at_commit() {
        let mut p = dmg_on(0x91);
        run_to(&mut p, 2, 130);
        p.stage_write(0xFF40, 0x11, 2); // LCD off staged
        for _ in 0..4 {
            p.tick();
        }
        assert!(p.enabled, "staged LCDC.7 must not act before the commit");
        p.write(0xFF40, 0x11);
        assert!(!p.enabled, "the architectural commit disables");
    }

    // --- Mode 3 length ---

    #[test]
    fn mode3_length_scx() {
        for scx in 0u8..=8 {
            let mut p = dmg_on(0x91);
            p.write(0xFF43, scx);
            let v0 = render_line(&mut p, 1);
            assert_eq!(v0, 254 + u16::from(scx & 7), "scx {scx}");
        }
    }

    /// The end-of-line event grid: the mode-0 STAT IRQ source and the
    /// externally visible mode-0 flip land together 2 dots before the
    /// pipe end — 254+SCX%8 on a bare line (see `m0_flip_events`); the
    /// pipe end (the HDMA/palette-blocking anchor, `render_finished`)
    /// stays at 256+SCX%8.
    #[test]
    fn mode0_irq_flip_pipe_end_split() {
        for scx in [0u8, 1, 5, 7] {
            let s = u16::from(scx & 7);
            let mut p = dmg_on(0x91);
            p.write(0xFF41, 0x08); // mode-0 STAT IRQ source enabled
            p.write(0xFF43, scx);
            run_to(&mut p, 2, 84);
            let mut flip = None;
            let mut if_dot = None;
            let mut finished = None;
            while finished.is_none() {
                let iff = p.tick();
                if p.line_render_done && flip.is_none() {
                    flip = Some(p.dot);
                }
                if iff & 0x02 != 0 && if_dot.is_none() {
                    if_dot = Some(p.dot);
                }
                if p.render_finished && finished.is_none() {
                    finished = Some(p.dot);
                }
                assert!(p.dot < 400, "mode 3 never finished (scx {scx})");
            }
            assert_eq!(if_dot, Some(254 + s), "mode-0 STAT IF (scx {scx})");
            assert_eq!(flip, Some(254 + s), "visible flip (scx {scx})");
            assert_eq!(finished, Some(256 + s), "pipe end (scx {scx})");
        }
    }

    /// Sprite stalls shift the whole event grid: on DMG one sprite at
    /// X=0 costs 6 (fetch) + 5 (alignment) dots and the flip leads the
    /// pipe end by 3 (see `obj_fetch_base`), so the flip lands at 264 —
    /// the top of mooneye intr_2_mode0_timing_sprites' "2 extra cycles"
    /// window (260, 264] — and the pipe ends at 267.
    #[test]
    fn sprite_stall_shifts_event_grid() {
        let mut p = dmg_on(0x93);
        sprite(&mut p, 0, 19, 0, 0, 0); // row 0 on line 3
        run_to(&mut p, 3, 84);
        let mut flip = None;
        let mut finished = None;
        while finished.is_none() {
            p.tick();
            if p.line_render_done && flip.is_none() {
                flip = Some(p.dot);
            }
            if p.render_finished {
                finished = Some(p.dot);
            }
            assert!(p.dot < 400, "mode 3 never finished");
        }
        assert_eq!(flip, Some(264), "flip: 256 + 6 + 5 - 3 (sprite lead)");
        assert_eq!(finished, Some(267), "pipe end: flip + 3");
    }

    fn penalty(xs: &[u8]) -> i32 {
        let mut p = dmg_on(0x93);
        for (i, &x) in xs.iter().enumerate() {
            sprite(&mut p, i as u8, 19, x, 0, 0); // row 0 on line 3
        }
        i32::from(render_line(&mut p, 3)) - 256
    }

    /// Mooneye intr_2_mode0_timing_sprites pins each case's flip to the
    /// 4-dot window (4e-4, 4e] past its poll anchor at dot 256, where e
    /// is the "extra cycles" value — so e = ceil((flip - 256)/4). With
    /// the flip at pipe end - 2 and the first fetch costing 5 dots (see
    /// `obj_fetch_base`), every sprite case's flip sits exactly where
    /// the old end-anchored model put it (the +2 cost and the -2 flip
    /// lead cancel), while a sprite-free line flips 2 dots earlier —
    /// still inside its e = 0 window.
    #[test]
    fn sprite_penalty_table() {
        fn e(p: i32) -> i32 {
            assert!(p >= 0, "e() is only defined for real penalties");
            (p + 3) / 4
        }
        // 1-N sprites at X=0 -> extra cycles 2,4,5,7,8,10,11,13,14,16.
        let expect = [2, 4, 5, 7, 8, 10, 11, 13, 14, 16];
        for n in 1..=10usize {
            let dots = penalty(&vec![0u8; n]);
            assert_eq!(dots, 6 * n as i32 + 2, "{n} sprites at x=0");
            assert_eq!(e(dots), expect[n - 1], "{n} sprites at x=0");
        }
        // 10 sprites at X=N.
        for (x, cycles) in [
            (1u8, 16),
            (2, 15),
            (5, 15),
            (7, 15),
            (8, 16),
            (16, 16),
            (160, 16),
            (167, 15),
        ] {
            assert_eq!(e(penalty(&[x; 10])), cycles, "10 sprites at x={x}");
        }
        // Off-screen X >= 168: selected but never fetched — the line
        // flips at the bare 254, i.e. -2 against the poll anchor, inside
        // the e = 0 window (mooneye lists these cases at 0 extra cycles).
        assert_eq!(penalty(&[168; 10]), -2);
        assert_eq!(penalty(&[169; 10]), -2);
        // Two groups on different BG tiles both pay the alignment penalty.
        assert_eq!(e(penalty(&[0, 0, 0, 0, 0, 160, 160, 160, 160, 160])), 17);
        assert_eq!(e(penalty(&[4, 4, 4, 4, 4, 164, 164, 164, 164, 164])), 15);
        // Single sprite at X=N.
        for (x, cycles) in [(0u8, 2), (3, 2), (4, 1), (7, 1), (8, 2), (164, 1)] {
            assert_eq!(e(penalty(&[x])), cycles, "1 sprite at x={x}");
        }
        // Two sprites 8 apart.
        assert_eq!(e(penalty(&[0, 8])), 5);
        assert_eq!(e(penalty(&[4, 12])), 3);
        // 10 sprites 8 apart.
        assert_eq!(e(penalty(&[0, 8, 16, 24, 32, 40, 48, 56, 64, 72])), 27);
        assert_eq!(e(penalty(&[1, 9, 17, 25, 33, 41, 49, 57, 65, 73])), 25);
        assert_eq!(e(penalty(&[4, 12, 20, 28, 36, 44, 52, 60, 68, 76])), 17);
        assert_eq!(e(penalty(&[5, 13, 21, 29, 37, 45, 53, 61, 69, 77])), 15);
        // Reverse OAM order: identical timing.
        assert_eq!(e(penalty(&[72, 64, 56, 48, 40, 32, 24, 16, 8, 0])), 27);
    }

    #[test]
    fn sprites_disabled_no_penalty() {
        let mut p = dmg_on(0x91); // OBJ off
        for i in 0..10 {
            sprite(&mut p, i, 19, 0, 0, 0);
        }
        assert_eq!(render_line(&mut p, 3), 254);
    }

    #[test]
    fn window_costs_6_dots() {
        let mut p = dmg_on(0xB1); // window on, map 0x9800 for both
        p.write(0xFF4A, 0); // WY=0
        p.write(0xFF4B, 87); // WX: window from pixel 80
        let v0 = render_line(&mut p, 2);
        // Window-stalled lines flip 1 dot before the pipe end (262).
        assert_eq!(v0, 261);
    }

    // --- Window rendering ---

    #[test]
    fn window_pixels_and_line_counter() {
        let mut p = dmg_on(0xF1); // win map 0x9C00, win on, bg map 0x9800
        p.write(0xFF4A, 1);
        p.write(0xFF4B, 15); // window from pixel 8
        set_map(&mut p, 0x1C00, 0, 0, 2);
        set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF); // window line 0: color 3
        set_tile_row(&mut p, 0, 2, 1, 0x00, 0xFF); // window line 1: color 2
        render_line(&mut p, 1);
        assert_eq!(px(&p, 1, 7), WHITE);
        assert_eq!(px(&p, 1, 8), BLACK, "first window line uses row 0");
        render_line(&mut p, 2);
        assert_eq!(
            px(&p, 2, 8),
            DARK,
            "window line counter advances independently of LY/SCY"
        );
    }

    #[test]
    fn window_wx0_starts_at_left_edge() {
        let mut p = dmg_on(0xB1);
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 0);
        set_map(&mut p, 0x1800, 0, 0, 0); // bg tile 0 (white)
        set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF);
        for col in 0..21 {
            set_map(&mut p, 0x1800, 0, col, 2); // window map = bg map here
        }
        render_line(&mut p, 0);
        // WX=0: the leading 7 window pixels fall off the left edge but the
        // window occupies the whole line.
        assert_eq!(px(&p, 0, 0), BLACK);
    }

    #[test]
    fn window_disabled_by_lcdc5() {
        let mut p = dmg_on(0x91); // bit 5 clear
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 7);
        set_map(&mut p, 0x1C00, 0, 0, 2);
        set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF);
        let v0 = render_line(&mut p, 2);
        assert_eq!(v0, 254, "no window penalty");
        assert_eq!(px(&p, 2, 0), WHITE);
    }

    /// A WX<=7 value written before mode 3 triggers at its prefill dot
    /// even when WX is rewritten twice more mid-line (the m3_wx_5_change
    /// per-line pattern): the prefill match wins and the later rewrites
    /// find the window already active.
    #[test]
    fn wx_prefill_trigger_survives_midline_wx_rewrites() {
        let mut p = dmg_on(0xF3);
        p.write(0xFF4A, 4); // WY=4
        p.write(0xFF4B, 80);
        for r in 0..8 {
            set_tile_row(&mut p, 0, 1, r, 0xFF, 0x00); // BG LIGHT
            set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // window BLACK
        }
        for row in 0..32 {
            for col in 0..32 {
                set_map(&mut p, 0x1800, row, col, 1);
                set_map(&mut p, 0x1C00, row, col, 2);
            }
        }
        // Line 10: WX=5 early (dot 58), WX=10 at dot 100, WX=80 at dot 196.
        run_to(&mut p, 10, 56);
        mcycle_write(&mut p, 0xFF4B, 5);
        run_to(&mut p, 10, 98);
        mcycle_write(&mut p, 0xFF4B, 10);
        run_to(&mut p, 10, 194);
        mcycle_write(&mut p, 0xFF4B, 80);
        finish_line(&mut p);
        assert_eq!(px(&p, 10, 0), BLACK, "WX=5 prefill trigger: window from 0");
        assert_eq!(px(&p, 10, 80), BLACK, "window continues");
    }
    // --- Window machine: LCDC.5 mid-line disable / re-enable ---
    //
    // mattcurrie's comprehensive-ppu-doc §WIN_EN: "WIN_EN can be disabled
    // during mode 3. The disabling will take effect at the end of the
    // current window tile being drawn. When the current window tile has
    // finished being drawn, the PPU will start drawing background tiles
    // again. When the background resumes drawing it is on a tile boundary.
    // The low 3 bits of SCX have no effect. [...] If WX has been updated
    // correctly and WIN_EN is set again then [...] it will start drawing
    // the next row of the window, on the same scanline."

    /// Window at WX=15 (pixel 8); WIN_EN staged off so the pipeline view
    /// commits at dot 127, mid-way through the window tile covering pixels
    /// 24-31. That tile (and the fetch already in flight) finishes; the BG
    /// resumes at pixel 32 on a tile boundary at the live map column
    /// (gambatte ppu.cpp Tile::f0: `(scx + xpos + 1 - cgb) / 8`), without
    /// re-showing the columns the window covered.
    #[test]
    fn win_en_disable_mid_line_finishes_window_tile_then_bg_resumes() {
        let mut p = dmg_on(0xF1); // win map 9C00, win on, data 8000, bg map 9800
        p.write(0xFF4A, 0); // WY=0
        p.write(0xFF4B, 15); // window from pixel 8
        for r in 0..8 {
            set_tile_row(&mut p, 0, 1, r, 0xFF, 0x00); // tile 1: solid LIGHT
            set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // tile 2: solid BLACK
            set_tile_row(&mut p, 0, 3, r, 0x00, 0xFF); // tile 3: solid DARK
        }
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1); // BG: LIGHT everywhere...
            set_map(&mut p, 0x1C00, 0, col, 2); // window: BLACK everywhere
        }
        set_map(&mut p, 0x1800, 0, 5, 3); // ...except BG col 5: DARK
        // Window triggers at dot 105 (lx==8); window pixel x pops at dot
        // 103+x. Stage the disable at state(124): eff commits at dot 127,
        // while the window tile covering 24-31 pops.
        run_to(&mut p, 2, 124);
        mcycle_write(&mut p, 0xFF40, 0xD1);
        let v0 = finish_line(&mut p);
        assert_eq!(px(&p, 2, 7), LIGHT, "BG before the window");
        assert_eq!(px(&p, 2, 8), BLACK, "window from pixel 8");
        assert_eq!(px(&p, 2, 31), BLACK, "current window tile finishes");
        assert_eq!(
            px(&p, 2, 32),
            LIGHT,
            "BG resumes on the tile boundary at the live column (col 4)"
        );
        assert_eq!(px(&p, 2, 39), LIGHT);
        assert_eq!(px(&p, 2, 40), DARK, "BG col 5 follows: columns 0-3 skipped");
        // DMG aborted-window line: the flip lead drops to 0 (end 262).
        assert_eq!(v0, 262, "the 6-dot window penalty is not refunded");
    }

    /// After a mid-line disable, re-enabling WIN_EN with WX pointing at a
    /// not-yet-drawn pixel retriggers the window — drawing the *next*
    /// window row on the same scanline (doc §WIN_EN; gambatte plotPixel
    /// increments winYPos on every activation).
    #[test]
    fn win_en_reenable_same_line_draws_next_window_row() {
        let mut p = dmg_on(0xF1);
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 15);
        for r in 0..8 {
            set_tile_row(&mut p, 0, 1, r, 0xFF, 0x00); // BG tile: LIGHT
        }
        set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // win row 2: BLACK
        set_tile_row(&mut p, 0, 2, 3, 0x00, 0xFF); // win row 3: DARK
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
            set_map(&mut p, 0x1C00, 0, col, 2);
        }
        run_to(&mut p, 2, 124);
        mcycle_write(&mut p, 0xFF40, 0xD1); // window off mid-tile
        p.write(0xFF4B, 107); // new WX: pixel 100, not yet drawn
        mcycle_write(&mut p, 0xFF40, 0xF1); // window back on
        let v0 = finish_line(&mut p);
        assert_eq!(px(&p, 2, 8), BLACK, "first segment: window row 2");
        assert_eq!(px(&p, 2, 99), LIGHT, "BG between the segments");
        assert_eq!(
            px(&p, 2, 100),
            DARK,
            "second segment retriggers at the new WX with the next row (3)"
        );
        assert_eq!(px(&p, 2, 108), DARK, "window column advances normally");
        assert_eq!(p.win_line, 3, "retrigger advanced the line counter");
        // Re-enabled same line: aborted + restarted, DMG lead 0 (end 268).
        assert_eq!(v0, 268, "two window starts: 256 + 6 + 6");
    }

    /// The window line counter increments at each activation (gambatte
    /// plotPixel ++winYPos, init 0xFF at frame start), not at line end:
    /// WX=166 activates every line — advancing the counter — even though
    /// at most the last pixel can show window output.
    #[test]
    fn wx_166_advances_window_line_counter_every_line() {
        let mut p = cgb_on(0xB1); // native CGB: no DMG carryover quirk
        p.write(0xFF4A, 0); // WY=0
        p.write(0xFF4B, 166);
        render_line(&mut p, 0);
        assert_eq!(p.win_line, 0, "line 0 activation: 0xFF + 1");
        let v0 = render_line(&mut p, 1);
        assert_eq!(p.win_line, 1);
        assert_eq!(v0, 261, "CGB: the WX=166 start stalls the line end 6 dots");
        p.write(0xFF4B, 15); // normal WX on line 2
        render_line(&mut p, 2);
        assert_eq!(p.win_line, 2, "line 2 draws window row 2: rows 0-1 skipped");
    }

    /// DMG WX=166 quirk: the start request raised at the match cannot be
    /// consumed before the pipeline ends (gambatte handleWinDrawStartReq
    /// honors requests at xpos >= 167 only on CGB), so the match line
    /// shows no window pixel and only pays a short freeze for the
    /// aborted start (m2int_wxA6_m3stat_1/_2 bracket the DMG end between
    /// 1 and 4 dots past the unextended end). The request survives to
    /// the next line, which starts with the window drawing from the
    /// left edge (gambatte M3Start::f0; on_screen/wxA6_wy00), re-arms
    /// itself at its own match, and the chain repeats — one window row
    /// per line.
    #[test]
    fn dmg_wx_166_no_window_pixels_counter_advances() {
        let mut p = dmg_on(0xF1);
        p.write(0xFF4A, 1); // WY=1: line 0 (the LCD-enable glitch line) is clean
        p.write(0xFF4B, 166);
        for r in 0..8 {
            set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // window: BLACK
        }
        for col in 0..32 {
            set_map(&mut p, 0x1C00, 0, col, 2);
        }
        let v0 = render_line(&mut p, 1);
        assert_eq!(v0, 257, "DMG: only the aborted-start stall extends mode 3");
        assert_eq!(px(&p, 1, 159), WHITE, "no window pixel on the match line");
        assert_eq!(p.win_line, 0, "the activation still counted a row");
        let v0 = render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLACK, "carryover: window from the left edge");
        assert_eq!(px(&p, 2, 159), BLACK);
        assert_eq!(p.win_line, 1, "mode-3 start consumed the request: ++row");
        assert_eq!(v0, 257, "the re-armed match pays the same freeze");
        // The carried-over activation suppresses the line's own match
        // increment but re-arms the request: one row per line.
        render_line(&mut p, 3);
        assert_eq!(px(&p, 3, 0), BLACK);
        assert_eq!(p.win_line, 2);
    }

    /// The WY condition is sampled at discrete dots (gambatte weMaster
    /// checks at line cycles 450/454 and line 0 dot 2), not compared
    /// continuously: a WY value that matches LY only *between* the
    /// sample points and is gone again by the window's WX match dot
    /// must not arm the frame latch. The live comparison against the
    /// delayed wy2 copy covers same-line writes instead.
    #[test]
    fn wy_latch_samples_discretely() {
        let mut p = dmg_on(0xB1);
        p.write(0xFF4B, 87); // window at pixel 80
        p.write(0xFF4A, 200); // WY: no match anywhere
        // Mid-line on line 2 (after dot 2, before dot 451), set WY=2 and
        // move it away again before the dot-451 sample: with continuous
        // latching this would arm the window for the rest of the frame.
        run_to(&mut p, 2, 100);
        p.write(0xFF4A, 2);
        run_to(&mut p, 2, 300);
        p.write(0xFF4A, 200);
        let v0 = render_line(&mut p, 3);
        assert_eq!(v0, 254, "no window: WY matched only between samples");
        // A WY write that holds through the dot-451 sample arms the
        // latch for the rest of the frame.
        run_to(&mut p, 4, 100);
        p.write(0xFF4A, 4);
        run_to(&mut p, 5, 0);
        p.write(0xFF4A, 200);
        let v0 = render_line(&mut p, 6);
        assert_eq!(v0, 261, "the dot-451 sample armed the frame latch");
    }

    /// On CGB the live WY comparison uses a copy that lags the
    /// architectural write by ~6 dots (gambatte video.cpp wyChange:
    /// wy2 at cc+6 vs the wx-style commit at cc+2): a WY write landing
    /// within 6 dots before the WX match dot is not seen by the
    /// comparator on that line.
    #[test]
    fn cgb_wy2_lags_architectural_wy() {
        let mut p = cgb_on(0xB1);
        p.write(0xFF4B, 87); // window at pixel 80: match dot 170
        p.write(0xFF4A, 200);
        // Commit WY=2 at dot 173 of line 2: arch wy == ly at the match
        // dot 177 (lx == 80), but wy2 catches up only at dot 179.
        run_to(&mut p, 2, 173);
        p.write(0xFF4A, 2);
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 254, "wy2 still held the old value at the match");
        // Same write 5 dots earlier: wy2 caught up before the match.
        let mut p = cgb_on(0xB1);
        p.write(0xFF4B, 87);
        p.write(0xFF4A, 200);
        run_to(&mut p, 3, 168);
        p.write(0xFF4A, 3);
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 261, "wy2 caught up: the live comparison triggers");
    }

    /// Sprites with OAM X 0-7 are fetched during the 8-dot prefill walk
    /// (positions 0-7, before any pixel pops), and the fetch pauses the
    /// SCX comparator hunt: an SCX rewrite landing inside the sprite
    /// stall is seen by the *paused* comparator when it resumes, not
    /// missed (gambatte scx_during_m3 spx0/spx1; the mode-3 length
    /// tables of intr_2_mode0_timing_sprites are unchanged because the
    /// stall and discard counts are additive either way).
    #[test]
    fn prefill_sprite_fetch_pauses_scx_hunt() {
        // Baseline: scx=3 + one sprite at X=0 -> discard 3 + stall
        // 3 + (5 - (0+3)) = 5 (Pan Docs OBJ penalty with the
        // first-fetch discount).
        let mut p = dmg_on(0x93);
        p.write(0xFF43, 3);
        sprite(&mut p, 0, 19, 0, 0, 0);
        let v0 = render_line(&mut p, 3);
        assert_eq!(v0, 264, "discard 3 + first-sprite stall 7, flip at end - 2");
        // SCX rewritten from 7 to 2 during the sprite stall (X=0 with
        // SCX=7: stall 3 over dots 89-91, the hunt frozen at position
        // 0): the resumed hunt walks positions 1, 2 against the
        // committed SCX=2 and matches at position 2 -> discard 2. An
        // unpaused hunt would have walked past index 2 before the
        // commit, wrapped, and re-hunted through the pops.
        let mut p = dmg_on(0x93);
        p.write(0xFF43, 7);
        sprite(&mut p, 0, 19, 0, 0, 0);
        run_to(&mut p, 3, 88);
        mcycle_write(&mut p, 0xFF43, 2);
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 261, "paused hunt: discard 2 + stall 5, flip at end - 2");
    }

    /// WX reaches the pipeline one dot later than the palette strobe
    /// (see `stage_write`): a WX=LY rewrite committing at the WX=6
    /// prefill comparator dot beats the wx=6 match but not the wx=5 one
    /// (mealybug m3_wx_4/5/6_change).
    #[test]
    fn wx_commit_is_one_dot_later_than_palettes() {
        for (early_wx, hits) in [(5u8, true), (6, false)] {
            let mut p = dmg_on(0xB1);
            p.write(0xFF4A, 0);
            p.write(0xFF4B, early_wx);
            // Stage WX=200 at state(92): the +1 commit lands at dot 96 =
            // prefill position dot for WX=6 (mode-3 dot 12), one past
            // the WX=5 dot (11).
            run_to(&mut p, 2, 92);
            mcycle_write(&mut p, 0xFF4B, 200);
            let v0 = finish_line(&mut p);
            if hits {
                assert_eq!(v0, 261, "wx=5 matched at dot 95, before the commit");
            } else {
                assert_eq!(v0, 254, "wx=6's match dot 96 already saw the rewrite");
            }
        }
    }

    /// A WX match while the window is already drawing ("reactivation"),
    /// landing on the dot that ships the first pixel of a window tile,
    /// emits one color-0 pixel and pushes the rest of the line out by a
    /// dot; off-boundary matches do nothing (mealybug m3_wx_5_change
    /// asm note + reference photos).
    #[test]
    fn window_reactivation_zero_pixel_on_tile_boundary() {
        let mut p = dmg_on(0xF1);
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 15); // window from pixel 8
        for r in 0..8 {
            set_tile_row(&mut p, 0, 2, r, 0xFF, 0xFF); // window: BLACK
        }
        for col in 0..32 {
            set_map(&mut p, 0x1C00, 0, col, 2);
        }
        // Window tile boundaries at pixels 8, 16, 24...; pixel 16 pops
        // at dot 119 with bg_count == 8. Stage WX=23 so the comparator
        // matches lx==16 exactly there.
        run_to(&mut p, 2, 112);
        mcycle_write(&mut p, 0xFF4B, 23);
        let v0 = finish_line(&mut p);
        assert_eq!(px(&p, 2, 15), BLACK, "window before the reactivation");
        assert_eq!(px(&p, 2, 16), WHITE, "the inserted zero pixel");
        assert_eq!(px(&p, 2, 17), BLACK, "window resumes, shifted one dot");
        // The injected pixel replaces a FIFO pixel at the line's tail:
        // mode-3 length is unchanged.
        assert_eq!(v0, 261, "zero pixel does not extend mode 3");
    }

    /// LCDC.0 does not gate the window *machine* on DMG: with BG/window
    /// display disabled the pixels blank, but the fetch stall and the
    /// line-counter advance still happen (gambatte ppu.cpp lcdcWinEn
    /// checks only LCDC.5; the bgen bit masks pixels at output).
    #[test]
    fn dmg_lcdc0_off_window_still_stalls_and_counts() {
        let mut p = dmg_on(0xB0); // window on, BG/window display off
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 87); // window from pixel 80
        let v0 = render_line(&mut p, 2);
        assert_eq!(v0, 261, "window penalty applies with LCDC.0 clear");
        assert_eq!(p.win_line, 2, "line counter advances (lines 0-2)");
        assert_eq!(px(&p, 2, 80), WHITE, "pixels blank through LCDC.0");
    }

    #[test]
    fn cgb_dmg_compat_lcdc0_gates_window() {
        // DMG compatibility mode: LCDC.0 clear blanks BG *and* window
        // pixels (Pan Docs "LCDC.0 — BG and Window enable/priority"), but
        // the window *machine* — trigger, 6-dot stall, line counter — only
        // looks at LCDC.5, exactly as on DMG (gambatte lcdcWinEn).
        let mut p = cgb_on(0xB0); // LCD on, window on, LCDC.0 = 0
        p.set_dmg_compat(true);
        p.write(0xFF4A, 0); // WY = 0
        p.write(0xFF4B, 87); // WX: window from pixel 80
        let v0 = render_line(&mut p, 2);
        assert_eq!(v0, 261, "window stall applies in compat mode, LCDC.0=0");
        assert_eq!(p.win_line, 2, "line counter advances (lines 0-2)");
        assert_eq!(px(&p, 2, 80), CGB_WHITE, "pixels blank through LCDC.0");

        // Native CGB mode: LCDC.0 is only priority — window unaffected.
        let mut p = cgb_on(0xB0);
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 87);
        let v0 = render_line(&mut p, 2);
        assert_eq!(v0, 261, "native CGB: window triggers despite LCDC.0=0");
        assert_eq!(p.win_line, 2, "lines 0, 1 and 2 advanced the counter");
    }

    // --- Sprite rendering ---

    #[test]
    fn sprite_pixels_palettes_transparency() {
        let mut p = dmg_on(0x93);
        p.write(0xFF48, 0xE4);
        p.write(0xFF49, 0x1B);
        set_tile_row(&mut p, 0, 4, 0, 0x0F, 0x00); // right half color 1
        sprite(&mut p, 0, 18, 16, 4, 0x00); // line 2, screen 8-15, OBP0
        sprite(&mut p, 1, 18, 40, 4, 0x10); // screen 32-39, OBP1
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), WHITE, "transparent sprite pixel shows BG");
        assert_eq!(px(&p, 2, 12), LIGHT, "OBP0 color 1");
        assert_eq!(px(&p, 2, 15), LIGHT);
        assert_eq!(px(&p, 2, 16), WHITE);
        assert_eq!(px(&p, 2, 36), DARK, "OBP1 maps 1 -> 2");
    }

    #[test]
    fn sprite_bg_priority_flag() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00); // bg: cols 0-3 color 1
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0xFF); // sprite solid color 3
        sprite(&mut p, 0, 18, 8, 4, 0x80); // behind BG, screen 0-7
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), LIGHT, "BG color 1-3 beats OBJ-behind-BG");
        assert_eq!(px(&p, 2, 4), BLACK, "BG color 0 shows the sprite");
    }

    #[test]
    fn sprite_x_flip() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0x80, 0x00); // only leftmost pixel
        sprite(&mut p, 0, 18, 16, 4, 0x00);
        sprite(&mut p, 1, 18, 40, 4, 0x20); // X-flipped
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), LIGHT);
        assert_eq!(px(&p, 2, 9), WHITE);
        assert_eq!(px(&p, 2, 32), WHITE);
        assert_eq!(px(&p, 2, 39), LIGHT);
    }

    #[test]
    fn sprite_y_flip() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // row 0: color 1
        set_tile_row(&mut p, 0, 4, 7, 0xFF, 0xFF); // row 7: color 3
        sprite(&mut p, 0, 18, 16, 4, 0x40); // Y-flipped: line 2 = row 7
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), BLACK);
    }

    #[test]
    fn sprite_8x16_tile_masking() {
        let mut p = dmg_on(0x97); // 8x16
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // top tile row 0: color 1
        set_tile_row(&mut p, 0, 5, 0, 0xFF, 0xFF); // bottom tile row 0: color 3
        // Line 2 hits row 8 of a sprite at y=10 -> bottom tile.
        sprite(&mut p, 0, 10, 16, 5, 0x00); // tile 5: bit 0 ignored -> 4/5
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), BLACK, "row 8 comes from tile|1");

        let mut p = dmg_on(0x97);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        set_tile_row(&mut p, 0, 5, 0, 0xFF, 0xFF);
        sprite(&mut p, 0, 18, 16, 5, 0x00); // line 2 = row 0 -> top tile 4
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 8), LIGHT, "row 0 comes from tile&0xFE");
    }

    /// Sprite selection happens at OAM-scan time (mode 2) with the height
    /// LCDC.2 holds *then*; the fetch re-reads LCDC.2. A game clearing
    /// LCDC.2 (16 -> 8) mid-mode-3 can hand the Y-flip a scan-time row
    /// (>= 8) that exceeds the fetch-time height — `h - 1 - row` must not
    /// underflow (panic in debug builds).
    #[test]
    fn sprite_height_shrunk_between_scan_and_fetch_no_panic() {
        let mut p = dmg_on(0x97); // 8x16 sprites
        sprite(&mut p, 0, 10, 88, 4, 0x40); // line 2 = row 8, Y-flipped
        run_to(&mut p, 2, 90); // scanned during mode 2 (h=16); mode 3 running
        p.write(0xFF40, 0x93); // clear LCDC.2 before the sprite's fetch
        let mut guard = 0u32;
        while !p.line_render_done {
            p.tick();
            guard += 1;
            assert!(guard < 2_000, "mode 3 never finished");
        }
    }

    #[test]
    fn sprite_priority_dmg_lower_x_wins() {
        let mut p = dmg_on(0x93);
        p.write(0xFF49, 0x1B);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
        sprite(&mut p, 0, 18, 20, 4, 0x00); // idx 0, screen 12-19, OBP0
        sprite(&mut p, 1, 18, 18, 4, 0x10); // idx 1, screen 10-17, OBP1
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 10), DARK, "lower-X sprite only");
        assert_eq!(px(&p, 2, 14), DARK, "lower X wins overlap on DMG");
        assert_eq!(px(&p, 2, 18), LIGHT, "higher-X sprite tail");
    }

    #[test]
    fn sprite_priority_clipped_left_edge_lower_x_wins() {
        // Sprites with X <= 8 all trigger at lx == 0, but hardware still
        // fetches them in ascending X order (the OBJ position comparator
        // also runs through the 8-pixel prefill), so the DMG lower-X-wins
        // rule (Pan Docs "Drawing priority") holds even when the OAM order
        // is reversed.
        let mut p = dmg_on(0x93);
        p.write(0xFF49, 0x1B);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
        sprite(&mut p, 0, 18, 8, 4, 0x00); // idx 0, X=8: screen 0-7, OBP0
        sprite(&mut p, 1, 18, 3, 4, 0x10); // idx 1, X=3: screen 0-2, OBP1
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), DARK, "X=3 sprite wins the overlap");
        assert_eq!(px(&p, 2, 2), DARK, "X=3 sprite covers pixels 0-2");
        assert_eq!(px(&p, 2, 3), LIGHT, "X=8 sprite resumes at pixel 3");
        assert_eq!(px(&p, 2, 7), LIGHT);
    }

    #[test]
    fn sprite_penalty_clipped_group_pays_in_x_order() {
        // X=0 and X=4 share the trigger (lx == 0) *and* the BG tile: the
        // leftmost sprite pays the first-per-tile alignment penalty
        // (5 - 0 = 5 dots) whichever OAM slot it sits in, so OAM order
        // [4, 0] costs the same as [0, 4]: 3 + 5 + 6 + 0 dots.
        assert_eq!(penalty(&[0, 4]), 14);
        assert_eq!(penalty(&[4, 0]), 14, "OAM order must not change timing");
    }

    #[test]
    fn sprite_priority_same_x_oam_order() {
        let mut p = dmg_on(0x93);
        p.write(0xFF49, 0x1B);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 20, 4, 0x00); // idx 0, OBP0
        sprite(&mut p, 1, 18, 20, 4, 0x10); // idx 1, OBP1, same X
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 14), LIGHT, "lower OAM index wins at equal X");
    }

    // --- MGB frozen-OAM-DMA sprite glitch (madness/mgb_oam_dma_halt_sprites.s) ---

    fn mgb_on(lcdc: u8) -> Ppu {
        let mut p = Ppu::new(Model::Mgb);
        p.write(0xFF47, 0xE4);
        p.write(0xFF40, lcdc);
        p
    }

    /// The exact scenario of the test ROM: old=$30/next=$40 in OAM, in-flight
    /// byte $1A, magic-enable entry present. The glitch sprite must render at
    /// Y=$38/X=$5A, tile $38, flags $5A (OBP1, Y flip, above BG, no X flip).
    #[test]
    fn mgb_frozen_dma_glitch_sprite_renders() {
        let mut p = mgb_on(0x93);
        p.write(0xFF48, 0x00); // OBP0 all white: proves OBP1 is selected
        p.write(0xFF49, 0xE4); // identity OBP1
        p.oam_dma_write(2, 0x30); // old
        p.oam_dma_write(3, 0x40); // next
        sprite(&mut p, 1, 0x9F, 0xA7, 0x9F, 0xA7); // magic enable entry
        set_tile_row(&mut p, 0, 0x38, 0, 0xFF, 0xFF); // solid color 3
        set_tile_row(&mut p, 0, 0x38, 7, 0x80, 0x80); // leftmost pixel only
        p.set_oam_dma_freeze(Some((2, 0x1A)));
        // Sprite Y=$38=56: first line 40. Flags Y flip: line 40 = tile row 7.
        render_line(&mut p, 40);
        assert_eq!(p.render.n_sprites, 10, "all slots hold the glitch sprite");
        assert_eq!(px(&p, 40, 81), WHITE);
        assert_eq!(px(&p, 40, 82), BLACK, "X=$5A: left edge at 82, OBP1");
        assert_eq!(px(&p, 40, 83), WHITE, "flags $5A: no X flip");
        // Last line 47 = tile row 0 (flipped): solid 8 pixels.
        render_line(&mut p, 47);
        for x in 82..90 {
            assert_eq!(px(&p, 47, x), BLACK, "x={x}");
        }
        assert_eq!(px(&p, 47, 90), WHITE);
        // Off the glitch sprite's Y range: nothing renders.
        render_line(&mut p, 48);
        assert_eq!(p.render.n_sprites, 0);
        assert_eq!(px(&p, 48, 82), WHITE);
    }

    /// The glitched entry formulas: Y = C = (old | new) & $FC,
    /// X = F = next | new; selection by the glitched Y as usual.
    #[test]
    fn mgb_glitch_formulas_and_selection() {
        let mut p = mgb_on(0x93);
        sprite(&mut p, 1, 0x98, 0x00, 0x09, 0x00); // minimal magic entry
        p.oam[8] = 0x21; // old
        p.oam[9] = 0x05; // next
        p.set_oam_dma_freeze(Some((8, 0x18)));
        // (0x21|0x18) & 0xFC = 0x38; 0x05|0x18 = 0x1D.
        p.ly = 40; // row 56 = Y exactly
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 10);
        for (i, s) in p.render.sprites.iter().enumerate() {
            assert_eq!(s.y, 0x38, "slot {i}");
            assert_eq!(s.x, 0x1D, "slot {i}");
            assert_eq!(s.tile, 0x38, "slot {i}");
            assert_eq!(s.flags, 0x1D, "slot {i}");
            assert_eq!(s.idx, i as u8, "slot {i}");
        }
        p.ly = 39; // row 55: above the sprite
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 0);
        p.ly = 47; // row 63: last 8x8 line
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 10);
        p.ly = 48; // row 64: below
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 0);
        // 8x16 mode extends the match window like a normal sprite.
        p.write(0xFF40, 0x97);
        p.ly = 55; // row 71 < 56+16
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 10);
        // Clearing the freeze restores the normal scan (real OAM: nothing
        // on this line).
        p.set_oam_dma_freeze(None);
        p.ly = 40;
        p.write(0xFF40, 0x93);
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 0);
    }

    /// Magic-enable ranges [$98-$9F, $00-$A7, $09-$9F, $00-$A7]: each byte
    /// position checked just inside and just outside its range; position in
    /// OAM is irrelevant but 4-byte alignment is required.
    #[test]
    fn mgb_glitch_magic_enable_ranges() {
        let mut oam = [0u8; 0xA0];
        assert!(!oam_glitch_magic_enable(&oam), "all-zero OAM: no enable");
        for (entry, ok) in [
            ([0x98, 0x00, 0x09, 0x00], true),  // every byte at its low bound
            ([0x9F, 0xA7, 0x9F, 0xA7], true),  // every byte at its high bound
            ([0x97, 0x00, 0x09, 0x00], false), // byte 0 below $98
            ([0xA0, 0x00, 0x09, 0x00], false), // byte 0 above $9F
            ([0x98, 0xA8, 0x09, 0x00], false), // byte 1 above $A7
            ([0x98, 0x00, 0x08, 0x00], false), // byte 2 below $09
            ([0x98, 0x00, 0xA0, 0x00], false), // byte 2 above $9F
            ([0x98, 0x00, 0x09, 0xA8], false), // byte 3 above $A7
        ] {
            let mut oam = [0u8; 0xA0];
            oam[12..16].copy_from_slice(&entry);
            assert_eq!(oam_glitch_magic_enable(&oam), ok, "{entry:02X?}");
        }
        // "The position in OAM does not matter": last entry works too.
        oam[156..160].copy_from_slice(&[0x9F, 0xA7, 0x9F, 0xA7]);
        assert!(oam_glitch_magic_enable(&oam));
        // Misaligned in-range bytes straddling two entries do not count.
        let mut oam = [0u8; 0xA0];
        oam[14..18].copy_from_slice(&[0x98, 0x00, 0x09, 0x00]);
        assert!(!oam_glitch_magic_enable(&oam));
    }

    /// Without a magic-enable entry the MGB scan selects nothing at all
    /// while frozen, even on a line the glitched Y would match.
    #[test]
    fn mgb_glitch_needs_magic_enable() {
        let mut p = mgb_on(0x93);
        p.oam[2] = 0x30;
        p.oam[3] = 0x40;
        p.set_oam_dma_freeze(Some((2, 0x1A)));
        p.ly = 40;
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 0);
        // Adding the magic entry enables it.
        sprite(&mut p, 5, 0x9F, 0xA7, 0x9F, 0xA7);
        p.oam_scan();
        assert_eq!(p.render.n_sprites, 10);
    }

    /// The interconnect caps the in-flight DMA index at 159, but the pub
    /// `set_oam_dma_freeze` API accepts any u8: an out-of-range index must
    /// degrade like the no-successor case (undriven bus reads 0xFF), not
    /// panic during the next scan.
    #[test]
    fn mgb_glitch_freeze_index_out_of_range_no_panic() {
        let mut p = mgb_on(0x93);
        sprite(&mut p, 1, 0x9F, 0xA7, 0x9F, 0xA7); // magic enable entry
        p.set_oam_dma_freeze(Some((0xA0, 0x1A)));
        p.ly = 40;
        p.oam_scan();
        // old = next = 0xFF -> glitched Y = 0xFC: matches no visible line.
        assert_eq!(p.render.n_sprites, 0);
    }

    /// The glitch is MGB-only: the asm documents different (unreferenced)
    /// results for DMG/CGB/AGB. With no disconnect level set those models
    /// fall back to the plain scan of the frozen OAM contents (in the
    /// integrated machine a freeze always coincides with the DMA owning
    /// OAM, so their scans latch $FF instead — the dmg08-verified
    /// gambatte oamdma_late_halt_stat rows pin that selection).
    #[test]
    fn frozen_dma_glitch_is_mgb_only() {
        for model in [Model::Dmg, Model::Cgb, Model::Agb] {
            let mut p = Ppu::new(model);
            p.write(0xFF40, 0x93);
            p.oam_dma_write(2, 0x30);
            p.oam_dma_write(3, 0x40);
            sprite(&mut p, 1, 0x9F, 0xA7, 0x9F, 0xA7); // magic entry
            p.set_oam_dma_freeze(Some((2, 0x1A)));
            p.ly = 40; // glitched Y would match here on MGB
            p.oam_scan();
            assert_eq!(p.render.n_sprites, 0, "{model:?}");
            // Plain scan still sees the real (frozen) OAM: the $9F entry
            // covers rows 159-166, i.e. visible line 143 only.
            p.ly = 143;
            p.oam_scan();
            assert_eq!(p.render.n_sprites, 1, "{model:?}");
            assert_eq!(p.render.sprites[0].y, 0x9F, "{model:?}");
        }
    }

    // --- dot-serial OAM scan (gbctr "OAM scan": one entry per 2 dots;
    // --- gambatte sprite_mapper.cpp OamReader; SameBoy display.c mode-2
    // --- loop) ---

    /// The scan consumes one OAM entry per 2 dots across mode 2: an OAM
    /// mutation landing mid-scan must not affect entries the scan already
    /// consumed, and must reach entries it has not.
    #[test]
    fn oam_scan_consumes_entries_serially() {
        let mut p = dmg_on(0x93);
        sprite(&mut p, 0, 18, 20, 4, 0x00); // covers line 2
        sprite(&mut p, 30, 18, 40, 4, 0x00); // covers line 2
        run_to(&mut p, 2, 40); // mid-scan: entry 0 consumed, entry 30 not
        p.oam[0] = 0; // move both entries off every line
        p.oam[120] = 0;
        run_to(&mut p, 2, 83);
        assert_eq!(
            p.render.n_sprites, 1,
            "entry 0 was latched before the write, entry 30 after"
        );
        assert_eq!(p.render.sprites[0].idx, 0);
        // An undisturbed line selects both again (and in OAM order).
        run_to(&mut p, 3, 83);
        assert_eq!(p.render.n_sprites, 0, "post-write contents: none match");
        p.oam[0] = 18;
        p.oam[120] = 18;
        run_to(&mut p, 4, 83);
        assert_eq!(p.render.n_sprites, 2);
        assert_eq!(p.render.sprites[0].idx, 0);
        assert_eq!(p.render.sprites[1].idx, 30);
    }

    /// While the OAM DMA controller owns OAM, the scan's reads are
    /// disconnected from it and latch $FF — a disabled sprite (gambatte
    /// memory.cpp startOamDma: the OamReader's source switches to
    /// rdisabledRam, all $FF, until endOamDma).
    #[test]
    fn oam_scan_reads_disabled_while_dma_owns_oam() {
        let mut p = dmg_on(0x93);
        sprite(&mut p, 0, 18, 20, 4, 0x00);
        sprite(&mut p, 30, 18, 40, 4, 0x00);
        run_to(&mut p, 2, 40); // entry 0 latched, entry 30 not yet
        p.set_oam_dma_active(true);
        run_to(&mut p, 2, 83);
        assert_eq!(p.render.n_sprites, 1, "entry 30's slot read $FF");
        assert_eq!(p.render.sprites[0].idx, 0);
        // A fully covered scan selects nothing.
        run_to(&mut p, 3, 83);
        assert_eq!(p.render.n_sprites, 0);
        // Reconnect mid-scan: entries scanned after it read real OAM.
        run_to(&mut p, 4, 40);
        p.set_oam_dma_active(false);
        run_to(&mut p, 4, 83);
        assert_eq!(p.render.n_sprites, 1);
        assert_eq!(p.render.sprites[0].idx, 30, "entry 0's slot read $FF");
        // Fully reconnected: both select again.
        run_to(&mut p, 5, 83);
        assert_eq!(p.render.n_sprites, 2);
    }

    #[test]
    fn ten_sprite_limit_by_oam_order() {
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        // 11 sprites on the line; the 11th (highest OAM index) is dropped.
        for i in 0..11u8 {
            sprite(&mut p, i, 18, 8 + i * 12, 4, 0);
        }
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 9 * 12), LIGHT, "10th sprite renders");
        assert_eq!(px(&p, 2, 10 * 12), WHITE, "11th sprite dropped");
    }

    // --- CGB ---

    fn cgb_on(lcdc: u8) -> Ppu {
        let mut p = Ppu::new(Model::Cgb);
        // BG palette 0 color 0 = white, identity-ish grayscale for colors.
        for pal in 0..2usize {
            for (c, raw) in [(0usize, 0x7FFFu16), (1, 0x294A), (2, 0x14A5), (3, 0x0000)] {
                p.bg_pal_ram[pal * 8 + c * 2] = raw as u8;
                p.bg_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
                p.obj_pal_ram[pal * 8 + c * 2] = raw as u8;
                p.obj_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
            }
        }
        // Make palette 1 color 1 pure red, obj palette 1 color 1 pure blue.
        p.bg_pal_ram[8 + 2] = 0x1F;
        p.bg_pal_ram[8 + 3] = 0x00;
        p.obj_pal_ram[8 + 2] = 0x00;
        p.obj_pal_ram[8 + 3] = 0x7C;
        p.write(0xFF40, lcdc);
        p
    }

    const CGB_WHITE: u32 = 0xFF_FFFF;
    const RED: u32 = 0xFF_0000;
    const BLUE: u32 = 0x00_00FF;

    #[test]
    fn cgb_color_expansion() {
        let p = cgb_on(0x91);
        assert_eq!(p.cgb_color(&p.bg_pal_ram, 0, 0), CGB_WHITE);
        assert_eq!(p.cgb_color(&p.bg_pal_ram, 1, 1), RED);
        // 5->8 bit expansion: (c << 3) | (c >> 2).
        let mut q = cgb_on(0x91);
        q.bg_pal_ram[0] = 0x10; // red = 16
        q.bg_pal_ram[1] = 0x00;
        assert_eq!(q.cgb_color(&q.bg_pal_ram, 0, 0), 0x84_0000);
    }

    #[test]
    fn cgb_bg_attributes_palette_bank_flips() {
        let mut p = cgb_on(0x91);
        // Tile 1 data in bank 1 only; bank 0 left zero.
        set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00); // leftmost pixel color 1
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x09; // palette 1, bank 1
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), RED, "bank 1 data, palette 1");
        assert_eq!(px(&p, 2, 1), CGB_WHITE);

        // X flip.
        let mut p = cgb_on(0x91);
        set_tile_row(&mut p, 1, 1, 2, 0x80, 0x00);
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x29; // + X flip
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), CGB_WHITE);
        assert_eq!(px(&p, 2, 7), RED);

        // Y flip: line 2 fetches tile row 5.
        let mut p = cgb_on(0x91);
        set_tile_row(&mut p, 1, 1, 5, 0x80, 0x00);
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x49; // + Y flip
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), RED);
    }

    #[test]
    fn cgb_sprite_priority_by_oam_index() {
        let mut p = cgb_on(0x93);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00); // solid color 1
        sprite(&mut p, 0, 18, 20, 4, 0x01); // idx 0, obj palette 1 (blue)
        sprite(&mut p, 1, 18, 18, 4, 0x00); // idx 1, palette 0, lower X
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 14), BLUE, "CGB: lower OAM index wins overlap");
        // OPRI bit 0 set: DMG-style X priority.
        let mut p = cgb_on(0x93);
        p.write(0xFF6C, 1);
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 20, 4, 0x01);
        sprite(&mut p, 1, 18, 18, 4, 0x00);
        render_line(&mut p, 2);
        assert_ne!(px(&p, 2, 14), BLUE, "OPRI=1: lower X wins");
    }

    #[test]
    fn cgb_bg_priority_and_master_priority() {
        // BG attr bit 7 set, BG color nonzero: BG wins...
        let mut p = cgb_on(0x93);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00); // bg cols 0-3 color 1
        set_map(&mut p, 0x1800, 0, 0, 1);
        p.vram[0x2000 + 0x1800] = 0x81; // priority + palette 1
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 8, 4, 0x01); // obj palette 1 (blue)
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), RED, "BG attr priority beats sprite");
        assert_eq!(px(&p, 2, 4), BLUE, "BG color 0 always loses");

        // ...unless LCDC bit 0 is clear: master priority off.
        let mut p = cgb_on(0x92);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x00);
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_map(&mut p, 0x1800, 0, 2, 1);
        p.vram[0x2000 + 0x1800] = 0x81;
        p.vram[0x2000 + 0x1802] = 0x81;
        set_tile_row(&mut p, 0, 4, 0, 0xFF, 0x00);
        sprite(&mut p, 0, 18, 8, 4, 0x81); // even OAM bit 7 set
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), BLUE, "LCDC0=0 strips all BG priority");
        // And the BG itself still renders (not blanked like DMG).
        assert_eq!(px(&p, 2, 9), CGB_WHITE);
        assert_eq!(px(&p, 2, 16), RED, "BG drawn where no sprite covers it");
    }

    #[test]
    fn cgb_vbk_banks() {
        let mut p = cgb_on(0x91);
        run_to(&mut p, 145, 0); // vblank: VRAM accessible
        assert_eq!(p.read(0xFF4F), 0xFE);
        p.write(0x8000, 0x11);
        p.write(0xFF4F, 1);
        assert_eq!(p.read(0xFF4F), 0xFF);
        assert_eq!(p.read(0x8000), 0);
        p.write(0x8000, 0x22);
        assert_eq!(p.read(0x8000), 0x22);
        assert_eq!(p.vram_read_raw(0x8000), 0x22);
        p.vram_write_raw(0x9FFF, 0x33);
        assert_eq!(p.vram[0x3FFF], 0x33);
        p.write(0xFF4F, 0xFE); // only bit 0 counts
        assert_eq!(p.read(0x8000), 0x11);
        assert_eq!(p.vram_read_raw(0x8000), 0x11);
    }

    #[test]
    fn cgb_palette_registers() {
        let mut p = cgb_on(0x91);
        run_to(&mut p, 145, 0);
        p.write(0xFF68, 0x80); // index 0, auto-increment
        p.write(0xFF69, 0x1F);
        p.write(0xFF69, 0x00);
        assert_eq!(p.read(0xFF68), 0x40 | 0x82);
        assert_eq!(p.bg_pal_ram[0], 0x1F);
        assert_eq!(p.bg_pal_ram[1], 0x00);
        p.write(0xFF68, 0x00);
        assert_eq!(p.read(0xFF69), 0x1F, "read back without increment");
        assert_eq!(p.read(0xFF68), 0x40, "reads have bit 6 set");

        p.write(0xFF6A, 0x80 | 0x10);
        p.write(0xFF6B, 0xAA);
        assert_eq!(p.obj_pal_ram[0x10], 0xAA);
        assert_eq!(p.read(0xFF6A), 0x40 | 0x91);
    }

    #[test]
    fn cgb_palette_ram_blocked_in_mode3() {
        let mut p = cgb_on(0x91);
        p.bg_pal_ram[0] = 0x12;
        run_to(&mut p, 1, 100); // mode 3
        assert_eq!(p.read(0xFF41) & 3, 3);
        p.write(0xFF68, 0x80);
        assert_eq!(p.read(0xFF69), 0xFF, "reads blocked during mode 3");
        p.write(0xFF69, 0x77);
        assert_eq!(p.bg_pal_ram[0], 0x12, "write dropped during mode 3");
        assert_eq!(
            p.read(0xFF68) & 0x3F,
            1,
            "auto-increment still happens on a blocked write (Pan Docs)"
        );
    }

    #[test]
    fn dmg_cgb_registers_unmapped() {
        let mut p = dmg_on(0x91);
        assert_eq!(p.read(0xFF4F), 0xFF);
        assert_eq!(p.read(0xFF68), 0xFF);
        assert_eq!(p.read(0xFF69), 0xFF);
        assert_eq!(p.read(0xFF6C), 0xFF);
        p.write(0xFF4F, 1); // ignored
        p.write(0x9000, 0x55);
        run_to(&mut p, 150, 0);
        assert_eq!(p.read(0x9000), 0x55);
    }

    #[test]
    fn set_dmg_palette_applies() {
        let mut p = dmg_on(0x91);
        p.set_dmg_palette([0x11, 0x22, 0x33, 0x44]);
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F);
        set_map(&mut p, 0x1800, 0, 0, 1);
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), 0x22);
        assert_eq!(px(&p, 2, 4), 0x33);
        assert_eq!(px(&p, 2, 8), 0x11);
    }

    /// End-to-end DMG-compat rendering through the CGB boot ROM's *default*
    /// compatibility palettes (Pan Docs "Compatibility palettes"; SameBoy
    /// cgb_boot.asm combination OBJ0=4, OBJ1=4, BG=29): BG pixels remap
    /// through BGP into the BG table, OBJ pixels through OBP0/OBP1 into the
    /// distinct OBJ table. Expected XRGB values follow the c-sp collection's
    /// `(X << 3) | (X >> 2)` channel expansion (dmg-acid2 README).
    #[test]
    fn cgb_compat_default_palette_render() {
        let mut p = Ppu::new(Model::Cgb);
        p.set_dmg_compat(true);
        // Install the boot defaults through the palette ports (LCD off — no
        // mode-3 blocking), exactly as `apply_post_boot_state` does.
        p.write(0xFF68, 0x80);
        for c in [0x7FFFu16, 0x1BEF, 0x6180, 0x0000] {
            p.write(0xFF69, c as u8);
            p.write(0xFF69, (c >> 8) as u8);
        }
        p.write(0xFF6A, 0x80);
        for _ in 0..2 {
            for c in [0x7FFFu16, 0x421F, 0x1CF2, 0x0000] {
                p.write(0xFF6B, c as u8);
                p.write(0xFF6B, (c >> 8) as u8);
            }
        }
        p.write(0xFF47, 0xE4); // identity BGP
        p.write(0xFF48, 0xE4); // identity OBP0
        set_tile_row(&mut p, 0, 1, 2, 0xF0, 0x0F); // cols 0-3 = 1, 4-7 = 2
        set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // shade 3
        set_map(&mut p, 0x1800, 0, 0, 1);
        set_map(&mut p, 0x1800, 0, 1, 2);
        set_tile_row(&mut p, 0, 3, 0, 0xF0, 0x0F); // sprite: 1s then 2s
        sprite(&mut p, 0, 18, 48, 3, 0); // line 2 row 0, screen x 40-47, OBP0
        p.write(0xFF40, 0x93); // LCD + BG + OBJ on
        render_line(&mut p, 2);
        assert_eq!(px(&p, 2, 0), 0x7BFF31, "BG shade 1");
        assert_eq!(px(&p, 2, 4), 0x0063C6, "BG shade 2");
        assert_eq!(px(&p, 2, 8), 0x00_0000, "BG shade 3");
        assert_eq!(px(&p, 2, 16), 0xFF_FFFF, "BG shade 0");
        assert_eq!(px(&p, 2, 40), 0xFF8484, "OBJ shade 1");
        assert_eq!(px(&p, 2, 44), 0x943939, "OBJ shade 2");
    }

    #[test]
    fn frame_buffer_double_buffering() {
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 0, 0xFF, 0xFF);
        set_map(&mut p, 0x1800, 0, 0, 1);
        // The frame right after the LCD enable is presented blank (see
        // `first_frame_after_lcd_enable_is_blank`); double buffering is
        // observable from the second frame on.
        run_to(&mut p, 144, 0);
        run_to(&mut p, 143, 455);
        assert_eq!(p.frame()[0], WHITE, "frame() is the completed frame");
        p.tick(); // 144:0 -> swap
        assert_eq!(p.frame()[0], BLACK);
    }

    // --- Fetch-grid register sampling (mealybug mode-3 fetch cluster) ---
    //
    // On the DMG blob, every BG fetch VRAM access samples the plain eff
    // view at its read dot — the same strobe the pop-anchored palette
    // photographs pin (write visible from the dot after the transition
    // dot). Decoded from m3_lcdc_tile_sel_change/bg_map_change blob bands,
    // whose sprite-stepped stalls bracket each stage's sampling dot.
    //
    // Steady no-sprite grid on a bare line: tile col c's NO/LO/HI reads
    // sit at dots 98/100/102 + 8*(c-1); pixel x pops at dot 97 + x.

    #[test]
    fn fetch_lo_read_samples_eff_at_the_read_dot() {
        let mut p = dmg_on(0x91); // bit 4: unsigned $8000 tile data
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // $8000 tile 1: color 1
        // The $8800-region alias of tile 1 ($9010) keeps row 2 = 00/00.
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
        }
        // Stage at dot 105: eff commits for reads from dot 108 = tile
        // col 2's LO read.
        run_to(&mut p, 2, 105);
        mcycle_write(&mut p, 0xFF40, 0x81); // clear LCDC.4 mid-line
        finish_line(&mut p);
        assert_eq!(px(&p, 2, 8), LIGHT, "tile 1: reads done before the write");
        assert_eq!(
            px(&p, 2, 16),
            WHITE,
            "tile 2: the LO read at dot 108 sees the committed LCDC.4"
        );
        assert_eq!(px(&p, 2, 24), WHITE, "tile 3: fully new");
    }

    #[test]
    fn fetch_lo_read_one_dot_before_commit_keeps_old_value() {
        // Same shape staged one dot later: the LO read at dot 108 now
        // sits one dot before the eff commit (109) and must keep the old
        // data (only the HI read sees the new bank, whose row is 00/00
        // either way, so tile 2 stays color 1).
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00);
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
        }
        run_to(&mut p, 2, 106);
        mcycle_write(&mut p, 0xFF40, 0x81);
        finish_line(&mut p);
        assert_eq!(px(&p, 2, 16), LIGHT, "LO one dot before the commit: old");
    }

    #[test]
    fn fetch_tile_no_read_samples_eff_at_the_read_dot() {
        // The tile-number read samples the same eff view
        // (m3_lcdc_bg_map_change blob bands 2/3 bracket it): a write
        // committing at dot 106 is seen by tile col 2's NO read at 106.
        let mut p = dmg_on(0x91);
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0x00); // color 1 (LIGHT)
        set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // color 3 (BLACK)
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1); // $9800: tile 1
            set_map(&mut p, 0x1C00, 0, col, 2); // $9C00: tile 2
        }
        run_to(&mut p, 2, 103);
        mcycle_write(&mut p, 0xFF40, 0x99); // BG map -> $9C00
        finish_line(&mut p);
        assert_eq!(px(&p, 2, 8), LIGHT, "tile 1: NO read at 98, old map");
        assert_eq!(
            px(&p, 2, 16),
            BLACK,
            "tile 2: NO read at the commit dot 106 reads $9C00"
        );
    }

    #[test]
    fn bg_fetcher_free_runs_during_sprite_stall() {
        let mut p = dmg_on(0x83); // BG + OBJ on, $8800-signed tile data
        set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF); // $8000 tile 0: black
        // $9000 tile 0 row 2 stays 00/00 (white); sprite tile 2 stays
        // all-zero = transparent, the stall is what matters.
        sprite(&mut p, 0, 18, 17, 2, 0);
        // LCDC.4 set for dots [106, 113] of the fetch view: stage at 104,
        // restore staged 8 dots later (the mealybug ld [hl],c / ld [hl],b
        // cadence).
        run_to(&mut p, 2, 104);
        mcycle_write(&mut p, 0xFF40, 0x93);
        for _ in 0..4 {
            p.tick();
        }
        mcycle_write(&mut p, 0xFF40, 0x83);
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 263, "10-dot stall, flip at pipe end - 3: mooneye dot");
        assert_eq!(
            px(&p, 2, 16),
            BLACK,
            "in-flight tile col 2: NO/LO/HI on stall dots 107/109/111 all \
             see the toggled tile-data bank"
        );
        assert_eq!(px(&p, 2, 24), WHITE, "tile col 3 fetched after restore");
    }

    #[test]
    fn bg_fetcher_stall_reads_before_window_stay_old() {
        // Band-8 bracket: sprite X=8 triggers at dot 97; the free-running
        // reads (98/100/102) all precede the write window, so the
        // in-flight tile keeps the old bank even though the stall overlaps
        // the write.
        let mut p = dmg_on(0x83);
        set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF);
        sprite(&mut p, 0, 18, 8, 2, 0);
        run_to(&mut p, 2, 104);
        mcycle_write(&mut p, 0xFF40, 0x93);
        for _ in 0..4 {
            p.tick();
        }
        mcycle_write(&mut p, 0xFF40, 0x83);
        finish_line(&mut p);
        assert_eq!(
            px(&p, 2, 8),
            WHITE,
            "tile col 1 in flight at the trigger: reads 98/100/102 are old"
        );
    }

    #[test]
    fn prefill_stall_refetch_reads_complete_before_the_walk() {
        // Prefill (X=0) sprite stall: the free-running refetch completes
        // its LO/HI reads on stall dots 94/96, well before a write
        // landing around the old frozen-refetch dots — the tile keeps
        // the old bank (m3_lcdc_tile_sel_change blob band 0).
        let mut p = dmg_on(0x83);
        set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF); // $8000 tile 0: black
        sprite(&mut p, 0, 18, 0, 2, 0); // X=0 prefill sprite, transparent
        run_to(&mut p, 2, 104);
        mcycle_write(&mut p, 0xFF40, 0x93);
        for _ in 0..4 {
            p.tick();
        }
        mcycle_write(&mut p, 0xFF40, 0x83);
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 264, "X=0 sprite: 11-dot stall, flip on its mooneye dot");
        assert_eq!(
            px(&p, 2, 0),
            WHITE,
            "tile 0 refetch: LO at 104 (before the lead) and HI on the \
             transition dot 106 (eff still old) both fetch $9000"
        );
    }

    #[test]
    fn fetch_during_stall_samples_eff_at_the_read_dot() {
        // In-stall (free-running) fetch reads sample eff exactly like the
        // steady grid. m3_lcdc_bg_map_change blob bands 16/17: the
        // in-flight tile's NO read lands one dot before the eff commit
        // during the stall and reads the old map.
        let mut p = dmg_on(0x93); // BG + OBJ on, $8000 tiles, map $9800
        set_tile_row(&mut p, 0, 1, 2, 0x00, 0x00); // tile 1: white
        set_tile_row(&mut p, 0, 2, 2, 0xFF, 0xFF); // tile 2: black
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
            set_map(&mut p, 0x1C00, 0, col, 2);
        }
        sprite(&mut p, 0, 18, 16, 3, 0); // X=16: trigger dot 105, stall 11
        // BG map -> $9C00 for eff dots [107, 114].
        run_to(&mut p, 2, 104);
        mcycle_write(&mut p, 0xFF40, 0x9B);
        for _ in 0..4 {
            p.tick();
        }
        mcycle_write(&mut p, 0xFF40, 0x93);
        finish_line(&mut p);
        assert_eq!(
            px(&p, 2, 16),
            WHITE,
            "in-stall NO read on the transition dot samples eff: old map"
        );
    }

    #[test]
    fn window_start_preempts_same_dot_sprite_trigger() {
        // m3_lcdc_win_map_change band 8 (sprite X=8, WX=7): the WX match
        // and the sprite trigger land on the same dot (97), and the
        // reference shows the window's first tile fetched *before* the
        // sprite stall — its NO read sits at dot 99, ahead of a write
        // whose eff commit lands at 107 — so the window start preempts
        // the sprite fetch on the shared dot.
        let mut p = dmg_on(0xF3); // LCD + WIN ($9C00) + $8000 tiles + OBJ + BG
        set_tile_row(&mut p, 0, 0, 2, 0x00, 0x00); // tile 0: white
        set_tile_row(&mut p, 0, 1, 2, 0xFF, 0xFF); // tile 1: black
        // Window map $9C00: tile 0 (white); the toggled map $9800: tile 1
        // (black).
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1);
            set_map(&mut p, 0x1C00, 0, col, 0);
        }
        p.write(0xFF4A, 0);
        p.write(0xFF4B, 7);
        sprite(&mut p, 0, 18, 8, 2, 0); // X=8: triggers at lx 0 (dot 97)
        run_to(&mut p, 0, 2); // latch WY
        run_to(&mut p, 2, 104);
        mcycle_write(&mut p, 0xFF40, 0xB3); // win map -> $9800 (black)
        for _ in 0..4 {
            p.tick();
        }
        mcycle_write(&mut p, 0xFF40, 0xF3);
        finish_line(&mut p);
        assert_eq!(
            px(&p, 2, 0),
            WHITE,
            "window col 0 NO read at dot 99 precedes the toggle: old map"
        );
    }

    #[test]
    fn prefill_sprite_stall_free_runs_fetcher_with_eff_sampling() {
        // m3_scy_change line 0 (sprite X=0): the refetched first tile's
        // LO/HI reads land on stall dots 94/96 sampling the live eff SCY
        // (the row written ~dot 91), while the push waits for the
        // pause-aware startup walk (first pixel stays at dot 107 — the
        // mooneye X=0 cost-10 anchor).
        let mut p = dmg_on(0x93);
        set_tile_row(&mut p, 0, 0, 2, 0xFF, 0x00); // ly2+scy0: color 1
        set_tile_row(&mut p, 0, 0, 5, 0xFF, 0xFF); // ly2+scy3: color 3
        sprite(&mut p, 0, 18, 0, 2, 0); // X=0 prefill sprite
        // SCY=3 drives eff reads from dot 93 and SCY=0 again from dot 97:
        // the in-stall LO/HI reads (dots 94/96) see 3 and fetch row 5,
        // while a frozen-prefill refetch (reads at 104/106) would see the
        // restored 0 and fetch row 2.
        run_to(&mut p, 2, 90);
        mcycle_write(&mut p, 0xFF42, 3);
        for _ in 0..4 {
            p.tick();
        }
        mcycle_write(&mut p, 0xFF42, 0);
        let v0 = finish_line(&mut p);
        assert_eq!(v0, 264, "X=0 sprite: 11-dot stall, flip on its mooneye dot");
        assert_eq!(
            px(&p, 2, 0),
            BLACK,
            "first tile fetched during the stall with the live SCY row"
        );
        assert_eq!(px(&p, 2, 8), LIGHT, "steady tiles back on SCY=0");
    }

    #[test]
    fn obj_disable_suppresses_sprite_pixels_at_the_mix() {
        // LCDC.1 gates sprite pixels at the pixel mix, not just the
        // fetch trigger: a sprite fetched while enabled stops showing on
        // the dots where the eff view reads OBJ off
        // (m3_lcdc_obj_en_change: each band's sprite is fetched during
        // the prefill, yet the columns shipping inside the disable
        // window show background).
        let mut p = dmg_on(0x93);
        p.write(0xFF48, 0xFF); // OBP0: all black
        set_tile_row(&mut p, 0, 2, 0, 0xFF, 0xFF); // sprite tile: solid c3
        sprite(&mut p, 0, 18, 10, 2, 0); // screen x 2-9, fetched at lx 2
        // Disable OBJ with eff commit at dot 109: pixels x2..3 (dots
        // 107/108 after the 9-dot stall) still show, x4+ (dots 109+) are
        // suppressed mid-sprite.
        run_to(&mut p, 2, 106);
        mcycle_write(&mut p, 0xFF40, 0x91);
        finish_line(&mut p);
        assert_eq!(px(&p, 2, 2), BLACK, "shipped before the disable");
        assert_eq!(
            px(&p, 2, 7),
            WHITE,
            "sprite pixel mixed while eff OBJ-enable is low: background"
        );
    }

    #[test]
    fn dmg_sprite_stall_shifts_palette_boundary_one_pixel() {
        // The blob's 6-dot first OBJ fetch (see `obj_fetch_base`) puts a
        // sprite-stalled line's pop grid one dot later than the old
        // 5-dot model: the same BGP write boundary lands one pixel left
        // (m3_lcdc_obj_en_change_variant's late BGP pulse and the
        // m3_bgp_change_sprites photos pin these columns exactly).
        let mut p = dmg_on(0x93);
        sprite(&mut p, 0, 18, 2, 2, 0); // X=2 prefill, stall 6+3
        run_to(&mut p, 2, 252);
        mcycle_write(&mut p, 0xFF47, 0xFF);
        finish_line(&mut p);
        // Pop start 106: px148 pops at 254 (the blend dot), px149 at 255.
        assert_eq!(px(&p, 2, 147), WHITE, "px147 pops 253: old bgp");
        assert_eq!(px(&p, 2, 148), BLACK, "px148 pops 254: blend dot");
        assert_eq!(px(&p, 2, 149), BLACK, "px149 pops 255: committed");
    }

    // --- WX 0-7 trigger is pause-aware (m3_lcdc_win_map_change family) ---
    //
    // The WX comparator runs against the position counter, which freezes
    // during sprite fetch stalls: a prefill (OAM X < 8) sprite stall
    // shifts a WX<=7 match later by the stall length instead of skipping
    // it. The m3_lcdc_win_map_change2 reference (WX=7 with X=1/X=5
    // sprites on every line) shows the window drawn on all sprite lines.

    #[test]
    fn wx7_window_trigger_survives_prefill_sprite_stall() {
        // LCD + WIN (map $9C00) + unsigned tiles + OBJ + BG.
        let mut p = dmg_on(0xF3);
        // Window line counter reaches 2 on line 2 (one activation per
        // line from line 0): the window fetch reads tile row 2.
        set_tile_row(&mut p, 0, 0, 2, 0xFF, 0xFF); // tile 0: black (window)
        for col in 0..32 {
            set_map(&mut p, 0x1800, 0, col, 1); // BG: tile 1 = white
            set_map(&mut p, 0x1C00, 0, col, 0); // window: tile 0 = black
        }
        p.write(0xFF4A, 0); // WY = 0
        p.write(0xFF4B, 7); // WX = 7: window from lx 0
        sprite(&mut p, 0, 18, 1, 2, 0); // X=1 prefill sprite, transparent
        run_to(&mut p, 0, 2); // latch WY at line 0 dot 2
        render_line(&mut p, 2);
        assert_eq!(
            px(&p, 2, 0),
            BLACK,
            "window starts: the WX=7 match shifted by the 10-dot stall"
        );
        assert_eq!(px(&p, 2, 100), BLACK, "window holds to the right edge");
    }
}
