//! BG fetcher + FIFO + the mode-0/IRQ-flip end-of-line grid (pipe-end projection, SCX hunt, flip/IRQ dot leads). Oracle: gbtr bgtile*/m0enable, gbmicrotest hblank_int/int_hblank, mooneye intr_2_mode0_timing.

use super::*;

impl Ppu {
    /// Consume one stall dot. While a sprite fetch holds the pipeline
    /// (prefill or mid-line), the BG fetcher keeps stepping in real time
    /// (`fetch_run`) until it parks with a completed row — see the field
    /// docs.
    pub(super) fn stall_tick(&mut self) {
        self.render.stall -= 1;
        if self.render.fetch_run > 0 {
            self.render.fetch_run -= 1;
            self.fetcher_step();
        }
    }

    /// Advance the output position and fire the pipe-end anchors:
    ///
    /// * lx 159 (gambatte xpos 167): the HBlank DMA trigger leads the
    ///   pipe end by one dot (see [`Ppu::hdma_lead`]).
    /// * lx 160 (xpos 168): the pipeline ends; `render_finished` anchors
    ///   the HBlank-DMA window and CGB palette-RAM blocking (gambatte
    ///   hdma_start/cgbpAccessible calibration — must not move with the
    ///   visible flip, which leads it: see [`Ppu::m0_flip_events`]).
    pub(super) fn advance_lx(&mut self) {
        self.render.lx += 1;
        match self.render.lx {
            159 => self.hdma_lead = true,
            160 => {
                self.render.active = false;
                self.render_finished = true;
                // Record the pipe-end dot for the tier2 palette
                // unblock law (see `Ppu::pal_open_dot`).
                self.pal_open_dot = self.dot;
                // The CGB palette-RAM unblock (this `render_finished` rise)
                // is half-classified by the interconnect for the cc+2
                // MID-phase FF69/FF6B read (sub-dot event-phase model);
                // bare steady-state lines only (see `m0_access_flip`). The
                // `lead_eighths` carried here is 0 (net-zero whole-M-cycle
                // commit) until the reclock sets a per-SCX palette offset.
                self.pal_access_flip =
                    (self.render.fetched == 0 && !self.render.win_active && !self.glitch_line)
                        .then_some(0i8);
                if !self.m0_src {
                    // Zero-lead lines (DMG aborted windows) flip at the
                    // pipe end itself; also the safety net for any
                    // projection miss.
                    self.m0_src = true;
                    self.m0_rise_dot = true;
                    self.line_render_done = true;
                    self.flip_dot = self.dot;
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
    pub(in crate::ppu) fn m0_flip_events(&mut self) {
        if self.m0_src || !self.render.active {
            return;
        }
        let (proj, lead) = self.flip_projection();
        let r = &self.render;
        // The sub-dot event-phase model calibrates the
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
        // Back-date the CPU-visible mode→0 boundary
        // (`vis_mode`) AHEAD of the dispatch flip on the flag-on path (single
        // speed): `vis_early` rises here while `m0_src`/`line_render_done` (the
        // IRQ dispatch) stay at `proj <= lead` below, so the visible mode reads 0
        // at SameBoy's `visible_mode0_dot` without moving the dispatch — the
        // instrumented kernel-pair separator (`m0int_m3stat_2` read reads mode 0,
        // `m2int_m3stat_1` stays mode 3). Bare lines use `lead + 3` (our-line
        // dot 251, the kernel −1 net shift); sprite/window lines use `lead + 4`:
        // their sprite-extended mode-3 geometry shifts the boundary, and +4
        // lands it at SameBoy's frame there — measured to lift the gambatte
        // sprite `m3stat_2` reads (the sprite analogs of the kernel m0int, +40
        // single-speed flag-on / 0 regress) + mooneye
        // `intr_2_mode0_timing_sprites`, window reads neutral. The LCD-enable
        // glitch line also takes `lead + 4`: there +4 is the *full*
        // single-speed read offset, so the visible mode→0 EXIT is observationally
        // neutral — `vis_early` anticipates `line_render_done` by the same 4 dots
        // the cc+0 read samples early, and `vis_mode`'s glitch branch (paired with
        // its 78→74 entry back-date) reproduces the flag-off cc+4 view
        // (`lcdon_timing-GS` STAT tables; gambatte enable_display / post-enable
        // m3stat). `bare_flip` is false on the glitch line, so it lands in the +4
        // arm. DS excluded (the DS read offset is 2, deferred). `leading_edge_reads`
        // is off in production, so `vis_early` is never set there (byte-identical).
        // Re-derive the BARE-line `vis_early` lead for the −2
        // dispatch reclock (the kernel separator). The Tier-1 lead was calibrated
        // against the cc+4 dispatch (our dot 254): vis_early fires ~dot 251 so the
        // kernel m2int read (cc+0 dot 248) sees mode 3 and m0int (dot 252) sees
        // mode 0. The deferred Tier-2 frame samples those reads at dots
        // 252 / 256, so the dot-251 vis_early makes m2int@252 read mode 0 (wrong,
        // wants 3). Lowering the bare lead by 2 (`lead + 3` → `lead + 1`) fires
        // vis_early 2 dots LATER, landing the visible mode→0 boundary in (252,
        // 256] so m2int@252 reads mode 3 and m0int@256 reads mode 0. Sprite lines
        // take the separate grid-snap below (their finer mode-3 geometry needs
        // a per-config re-grid, not a uniform −2). Gated on `tier2_reclock`.
        // Sprite-line visible mode→0 RE-GRID for the
        // deferred Tier-2 frame. The `intr_2_mode0_timing_sprites` test resolves
        // the mode-3 length to whole M-cycles (a NOP-count delay then an FF41
        // poll): hardware buckets configs that share an `extra` to the same
        // value (e.g. 10 sprites at X=0 and X=1 both extend by 16), but our
        // `proj` formula tracks a finer per-X staircase (X=0→dispatch dot 318,
        // X=1→317, …). At cc+4 the CPU read's own M-cycle quantization snaps that
        // staircase back onto the right buckets, so production passes every
        // config; at cc+0 the leading-edge read no longer hides the sub-M-cycle
        // dispatch phase, so configs whose dispatch dot straddles a read-grid
        // boundary mis-bucket (e.g. X=1's dot 317 reads mode 0 one poll early).
        // Fix: on sprite lines (`has_sprites`, including OAM sprites pushed fully
        // off-screen at X≥168 that take the bare `lead`), snap BOTH the dispatch
        // and the coincident `vis_early` to the CPU read grid — the next dot
        // ≡ 0 (mod 4), one dot below the read dots (≡ 1) — so all configs in a
        // bucket land on the same grid dot and reproduce the cc+4 quantization.
        // `early_lead = 0` makes `vis_early` coincide with the snapped dispatch
        // (a negative sprite lead is structurally dead — the dispatch sets
        // `m0_src` and early-returns). Bare lines keep `lead + 1` (the kernel /
        // int_hblank −1 shift, no snap); window/glitch lines keep `lead + 2`.
        // All gated on `tier2_reclock`; production (`!leading_edge_reads`) never
        // sets `vis_early` and the snap is inert, so it is byte-identical OFF.
        let has_sprites = r.n_sprites > 0;
        // The tier2 `early_lead` case-tower is
        // COLLAPSED to its two physical residues; everything else is 0 (the
        // visible flip coincides with the dispatch, and the FF41 read's finer
        // placement is the ONE half-dot comparison in `vis_exit_hd`):
        //
        // 1. The LCD-enable GLITCH line keeps +2 — the glitch frame's visible
        //    mode→0 offset (452-dot line, dot-82 pipe); `lcdon_timing-GS`'s
        //    post-glitch reads are calibrated against it.
        // 2. The bare-line cc2 READ-GRID parity (+1 iff the dispatch dot lands
        //    ≡ 1 mod 4 on a clean bare line): a leading-edge access in the
        //    dispatch's own M-cycle observes the flip at the cc+2 phase, which
        //    the whole-dot `line_render_done` misses only for the cc2 parity
        //    (`m2int_scx3`/`nobg_scx7` m3stat_2). Its FF41 side is
        //    subsumed by the carry law (`vis_exit_hd` arm 8 — the
        //    m2-ISR +4 hd carry reproduces the same verdict); the
        //    residue survives for the OAM/VRAM ACCESSIBILITY release + the
        //    fallback reads the carry law doesn't cover (line 0, CGB
        //    `wy_trig_sb`-bare). Scoped to clean bare lines — on a
        //    late-window-DISABLE line the anticipation is an A/B swap that
        //    drops the SameBoy-passing `_2` sibling (measured).
        //
        // The IRQ side (`mode_for_interrupt`/`prev_done`, reclock.rs) keys on
        // `line_render_done`, never `vis_early` — the counter-pinned dispatch
        // dot is untouched. `vis_early` is never set in production
        // (`leading_edge_reads` off) → byte-identical OFF.
        let early_lead = if self.tier2_reclock {
            if self.glitch_line {
                2
            } else if bare_flip && !has_sprites {
                let dispatch_dot = self.dot + proj.saturating_sub(lead);
                let clean_bare =
                    !self.wy_latch && self.wy2 != self.ly && !r.win_stalled && !r.win_aborted;
                u16::from((dispatch_dot & 3) == 1 && clean_bare)
            } else {
                // Sprite lines (the grid-snap below) and window lines (their
                // mode-3 extension is already in `proj`/`lead`) take 0.
                0
            }
        } else if bare_flip {
            3
        } else {
            4
        };
        // Single-speed ONLY (`!self.ds`). In DOUBLE speed the sprite-line FF41
        // mode-bit read rides the production `stat_mode_edge` override
        // (`interconnect/memory.rs`: a DS sprite m3→m0
        // flip holds the FF41 bits at 3 for the read M-cycle), armed by the
        // `m0_stat_flip` stamp that only `m0_flip_events` sets. Snapping the DS
        // dispatch to the `% 4` grid pushed it past the pipe end, where
        // `advance_lx`'s fallback flips `m0_src` first and `m0_flip_events`
        // early-returns — so the stamp never armed and the deferred cc+0 read
        // saw the already-flipped mode 0. Gating the snap to single speed lets DS
        // sprite lines flip at the natural dot, arm the stamp, and the deferred
        // read straddle the override (gambatte sprites `*_m3stat_ds_1` want the
        // lagging 3). `vis_early` stays `!self.ds`-gated (it anticipates mode 0,
        // the wrong direction for these reads). See
        // `tier2_sprite_m3stat_ds_passes`.
        let snap_ok = !(self.tier2_reclock && has_sprites && !self.ds) || self.dot % 4 == 0;
        if self.leading_edge_reads
            && !self.ds
            && !self.vis_early
            && proj <= lead + early_lead
            && snap_ok
        {
            self.vis_early = true;
        }
        if proj <= lead && snap_ok {
            self.m0_src = true;
            self.m0_rise_dot = true;
            self.line_render_done = true;
            self.flip_dot = self.dot;
            // The window vis-HOLD foundation. SameBoy
            // extends a TRIGGERING window's CPU-visible mode-3 to ≈ `263 + SCX&7`
            // (the measured window-length law), PAST this counter-pinned dispatch
            // dot; slopgb's window flip is flat at ~261. Record the hold target so
            // `vis_mode` keeps reading mode 3 until it, WITHOUT moving the dispatch
            // (`line_render_done`). Win-active lines only (`bare_flip` lines keep
            // the eighth-grid lever); tier2-gated, so `vis_hold_until` stays 0
            // in production (byte-identical OFF). Currently inert on its own (the
            // want=3 rows render bare via the WY-latch); it is the
            // visible-mode half of the parallel window-length model. See the
            // `vis_hold_until` field docs.
            if self.tier2_reclock && self.render.win_active {
                self.vis_hold_until = 263 + u16::from(self.scx & 7);
            }
            // The accessibility unblock (this `line_render_done` rise) is
            // half-classified by the interconnect for the cc+2 MID-phase
            // OAM read (sub-dot event-phase model).
            //
            // Boundary-coincident accessibility
            // release. The production stamp blocks an OAM/VRAM access landing in
            // the unblock M-cycle's SECOND HALF (`event_phase` commit eighth >
            // ACCESS_PHASE) — the cc+2 MID-frame model. But under the cc+0
            // deferred read SameBoy unblocks AT the boundary: `vram_m3`/
            // `oam_access` `postread_scx2/scx5_2` read ACCESSIBLE on the dot
            // `line_render_done` fires, not the trailing mode 3. A read 4 dots
            // earlier (`_1`, a different M-cycle) sees no stamp and stays blocked,
            // so releasing the boundary M-cycle's stamp is a clean separation
            // (full-CGB two-bin +7/−0 single speed). Push the M0Access edge to
            // phase 0 (`lead = -8` clamps there) so the leading-edge access is
            // never pre-empted. SINGLE SPEED only: the same release in double
            // speed unblocks the DS VRAM-WRITE path too (the stamp gates writes
            // at `memory.rs` `0x8000..=0x9FFF if stamp_blocks`), regressing the
            // `vramw_m3end_scx5_ds_{2,4}` write-end floors — the DS read grid is
            // its own reclock. Tier2 + bare lines + `!ds`; `bare_flip` is
            // false in production → byte-identical OFF.
            let access_lead = if self.tier2_reclock && !self.ds {
                -8i8
            } else {
                0i8
            };
            self.m0_access_flip = bare_flip.then_some(access_lead);
            // The STAT mode-bit flip routes the double-speed FF41 mode-bit
            // read at the cc+2 MID phase (sub-dot event-phase model; gambatte
            // sprites m3stat_ds). Gated to
            // sprite-extended lines (`r.fetched != 0`): bare-line DS reads
            // that reach FF41 through the DMA-cycle / lcd-offset chains
            // (dma/gdma/hdma_cycles_scx5_ds_2, lcd_offset m0stat_count) sit at
            // a different sub-cycle offset within the same M-cycle half, so a
            // bare-line override regresses them — the parked multi-chain
            // problem. Sprite lines are the clean, hold-floor-safe subset.
            self.m0_stat_flip = (r.fetched != 0).then_some(0i8);
        }
    }


    /// The pure flip projection `(proj, lead)` exactly as
    /// [`Self::m0_flip_events`] evaluates it each mode-3 dot: the dispatch
    /// fires when `proj <= lead`, so the PROJECTED dispatch dot from any
    /// mid-render read position is `dot + proj - lead`. Split out so the
    /// half-dot bare-exit law (`stat_irq.rs::vis_mode_read`, tier2-only) can
    /// anchor the CPU-visible mode-3→0 exit to the render's own projected
    /// flip BEFORE it fires — which tracks mid-line SCX writes through the
    /// fine-scroll hunt (late_scx4 / scx_m3_extend) where a live-`scx`
    /// closed form cannot. Pure (`&self`); byte-identical refactor for the
    /// production caller.
    pub(in crate::ppu) fn flip_projection(&self) -> (u16, u16) {
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
        (proj, lead)
    }

    /// A stall source engaged after the mode-0 flip already fired (a
    /// late WY/WX/LCDC write armed a window start or sprite fetch inside
    /// the final tile): the flip is a combinational level on hardware —
    /// the fetcher waking back up drops STAT back to mode 3, re-blocks
    /// OAM/VRAM and lowers the IRQ source until the projection re-fires
    /// (gambatte window/late_wy_* and late_disable_* m3stat rows pin the
    /// drop; an IF bit already latched stays latched, matching the
    /// edge-pulse the hardware line produced).
    pub(super) fn m0_unflip(&mut self) {
        if self.m0_src && self.render.active {
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.line_render_done = false;
            self.flip_dot = 0;
            // The visible back-date drops with the dispatch (flag-on only;
            // always false in production). See the `vis_early` field docs.
            self.vis_early = false;
            self.vis_hold_until = 0;
        }
    }

    pub(super) fn fetcher_step(&mut self) {
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
        // The BG/window fetcher samples the DEFERRED addressing
        // view (`render_lcdc`, bit3 BG map / bit4 tile-data / bit6 win map): a
        // mid-mode-3 bgtilemap/bgtiledata toggle reaches the fetch grid at the
        // production dot under tier2. OBJ-enable / mode-3-length reads keep the
        // eager `eff.lcdc` (they must not move the length). `render_lcdc` ==
        // `eff.lcdc` in production (byte-identical).
        let lcdc = self.eff.render_lcdc;
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
}
